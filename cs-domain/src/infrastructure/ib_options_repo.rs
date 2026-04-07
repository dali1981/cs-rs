use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use ib_data_collector::database::{ParquetDatabase, DatabaseRepository};
use ib_data_collector::domain::{Contract, OptionType as IbOptionType, BarType};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::repositories::{OptionsDataRepository, RepositoryError};
use crate::value_objects::{OptionBar, Strike};

pub struct IbOptionsRepository {
    db: ParquetDatabase,
}

impl IbOptionsRepository {
    pub fn new(data_dir: &Path) -> Result<Self, RepositoryError> {
        let db = ParquetDatabase::open(data_dir)
            .map_err(|e| RepositoryError::NotFound(format!("Failed to open IB database at {:?}: {}", data_dir, e)))?;
        Ok(Self { db })
    }

    /// Group contracts by (expiration, strike, option_type)
    fn group_contracts(&self, symbol: &str) -> Result<HashMap<(NaiveDate, Decimal, IbOptionType), Vec<Contract>>, RepositoryError> {
        let contracts = self.db.get_symbol_contracts(symbol)
            .map_err(|e| RepositoryError::NotFound(format!("Failed to get contracts for {}: {}", symbol, e)))?;

        let mut grouped: HashMap<(NaiveDate, Decimal, IbOptionType), Vec<Contract>> = HashMap::new();

        for state in contracts {
            let key = (
                state.contract.expiration,
                state.contract.strike,
                state.contract.option_type,
            );
            grouped.entry(key).or_default().push(state.contract);
        }

        Ok(grouped)
    }

    /// Find latest bar at or before target time
    fn find_latest_bar(bars: &[ib_data_collector::database::repository::Bar], target_time: DateTime<Utc>) -> Option<&ib_data_collector::database::repository::Bar> {
        bars.iter()
            .filter(|b| b.timestamp <= target_time)
            .max_by_key(|b| b.timestamp)
    }
}

/// Intermediate snapshot structure for assembling option chain data
struct OptionSnapshot {
    expiration: NaiveDate,
    strike: Decimal,
    option_type: IbOptionType,
    timestamp: DateTime<Utc>,
    close: f64,
}

fn snapshots_to_option_bars(snapshots: Vec<OptionSnapshot>) -> Vec<OptionBar> {
    snapshots.into_iter()
        .filter(|s| s.close > 0.0)
        .map(|s| OptionBar {
            strike: s.strike.to_f64().unwrap_or(0.0),
            expiration: s.expiration,
            option_type: match s.option_type {
                IbOptionType::Call => finq_core::OptionType::Call,
                IbOptionType::Put => finq_core::OptionType::Put,
            },
            close: Some(s.close),
            timestamp: Some(s.timestamp),
        })
        .collect()
}

#[async_trait]
impl OptionsDataRepository for IbOptionsRepository {
    async fn get_option_bars(
        &self,
        _underlying: &str,
        _date: NaiveDate,
    ) -> Result<Vec<OptionBar>, RepositoryError> {
        // Not implemented for IB - daily bars not typically used in backtests
        Err(RepositoryError::NotFound(
            "Daily option bars not implemented for IB data source".to_string()
        ))
    }

    async fn get_option_minute_bars(
        &self,
        _underlying: &str,
        _date: NaiveDate,
    ) -> Result<Vec<OptionBar>, RepositoryError> {
        // Not implemented for IB - use get_option_bars_at_time instead
        Err(RepositoryError::NotFound(
            "Minute option bars not implemented for IB data source".to_string()
        ))
    }

    async fn get_option_bars_at_time(
        &self,
        underlying: &str,
        target_time: DateTime<Utc>,
    ) -> Result<Vec<OptionBar>, RepositoryError> {
        let grouped = self.group_contracts(underlying)?;
        let mut snapshots = Vec::new();

        for ((expiration, strike, option_type), contracts) in grouped {
            // Find bid and ask contracts
            let bid_contract = contracts.iter().find(|c| c.bar_type == BarType::Bids);
            let ask_contract = contracts.iter().find(|c| c.bar_type == BarType::Asks);

            if let (Some(bid_c), Some(ask_c)) = (bid_contract, ask_contract) {
                // Read bars
                let bid_bars = self.db.read_bars(bid_c)
                    .map_err(|e| RepositoryError::NotFound(format!("Failed to read bid bars: {}", e)))?;
                let ask_bars = self.db.read_bars(ask_c)
                    .map_err(|e| RepositoryError::NotFound(format!("Failed to read ask bars: {}", e)))?;

                // Find latest bars at or before target_time
                if let (Some(bid_bar), Some(ask_bar)) = (
                    Self::find_latest_bar(&bid_bars, target_time),
                    Self::find_latest_bar(&ask_bars, target_time),
                ) {
                    // Compute mid-price
                    let mid = (bid_bar.close + ask_bar.close) / 2.0;
                    let timestamp = bid_bar.timestamp.max(ask_bar.timestamp);

                    snapshots.push(OptionSnapshot {
                        expiration,
                        strike,
                        option_type,
                        timestamp,
                        close: mid,
                    });
                }
            }
        }

        if snapshots.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No option data found for {} at {}",
                underlying, target_time
            )));
        }

        Ok(snapshots_to_option_bars(snapshots))
    }

    async fn get_option_bars_at_or_after_time(
        &self,
        underlying: &str,
        target_time: DateTime<Utc>,
        max_forward_minutes: u32,
    ) -> Result<(Vec<OptionBar>, DateTime<Utc>), RepositoryError> {
        // Try backward lookup first
        let backward_result = self.get_option_bars_at_time(underlying, target_time).await;

        if backward_result.is_ok() {
            return Ok((backward_result.unwrap(), target_time));
        }

        // Forward lookup - try each minute up to max_forward_minutes
        let max_forward = chrono::Duration::minutes(max_forward_minutes as i64);
        let max_time = target_time + max_forward;

        let mut current_time = target_time;
        while current_time <= max_time {
            current_time = current_time + chrono::Duration::minutes(1);

            if let Ok(bars) = self.get_option_bars_at_time(underlying, current_time).await {
                return Ok((bars, current_time));
            }
        }

        Err(RepositoryError::NotFound(format!(
            "No option data found for {} within {} minutes of {}",
            underlying, max_forward_minutes, target_time
        )))
    }

    async fn get_available_expirations(
        &self,
        underlying: &str,
        as_of_date: NaiveDate,
    ) -> Result<Vec<NaiveDate>, RepositoryError> {
        let contracts = self.db.get_symbol_contracts(underlying)
            .map_err(|e| RepositoryError::NotFound(format!("Failed to get contracts for {}: {}", underlying, e)))?;

        let expirations: HashSet<NaiveDate> = contracts
            .iter()
            .map(|c| c.contract.expiration)
            .filter(|&exp| exp > as_of_date)
            .collect();

        let mut result: Vec<NaiveDate> = expirations.into_iter().collect();
        result.sort();
        Ok(result)
    }

    async fn get_available_strikes(
        &self,
        underlying: &str,
        expiration: NaiveDate,
        _as_of_date: NaiveDate,
    ) -> Result<Vec<Strike>, RepositoryError> {
        let contracts = self.db.get_symbol_contracts(underlying)
            .map_err(|e| RepositoryError::NotFound(format!("Failed to get contracts for {}: {}", underlying, e)))?;

        let strikes: HashSet<Decimal> = contracts
            .iter()
            .filter(|c| c.contract.expiration == expiration)
            .map(|c| c.contract.strike)
            .collect();

        let mut result: Vec<Strike> = strikes
            .into_iter()
            .filter_map(|d| Strike::new(d).ok())
            .collect();

        result.sort();
        Ok(result)
    }
}
