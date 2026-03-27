use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use ib_data_collector::database::{ParquetDatabase, DatabaseRepository};
use polars::prelude::*;
use rust_decimal::Decimal;
use std::path::Path;

use crate::datetime::TradingTimestamp;
use crate::repositories::{EquityDataRepository, RepositoryError};
use crate::value_objects::SpotPrice;

pub struct IbEquityRepository {
    db: ParquetDatabase,
}

impl IbEquityRepository {
    pub fn new(data_dir: &Path) -> Result<Self, RepositoryError> {
        let db = ParquetDatabase::open(data_dir)
            .map_err(|e| RepositoryError::NotFound(format!("Failed to open IB database at {:?}: {}", data_dir, e)))?;
        Ok(Self { db })
    }
}

#[async_trait]
impl EquityDataRepository for IbEquityRepository {
    async fn get_spot_price(
        &self,
        symbol: &str,
        target_time: DateTime<Utc>,
    ) -> Result<SpotPrice, RepositoryError> {
        // Read equity bars from {symbol}/spot.parquet
        let bars = self.db.read_equity_bars(symbol)
            .map_err(|e| RepositoryError::NotFound(format!("Failed to read equity bars for {}: {}", symbol, e)))?;

        if bars.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No equity data available for {}",
                symbol
            )));
        }

        // Find latest bar where timestamp <= target_time
        let bar = bars.iter()
            .filter(|b| b.timestamp <= target_time)
            .max_by_key(|b| b.timestamp)
            .ok_or_else(|| RepositoryError::NotFound(format!(
                "No equity bar found for {} at or before {}",
                symbol, target_time
            )))?;

        Ok(SpotPrice {
            value: Decimal::try_from(bar.close)
                .map_err(|e| RepositoryError::Parse(format!("Invalid decimal: {}", e)))?,
            timestamp: bar.timestamp,
        })
    }

    async fn get_bars(
        &self,
        symbol: &str,
        date: NaiveDate,
    ) -> Result<DataFrame, RepositoryError> {
        // Read equity bars
        let bars = self.db.read_equity_bars(symbol)
            .map_err(|e| RepositoryError::NotFound(format!("Failed to read equity bars for {}: {}", symbol, e)))?;

        // Filter to specific date
        let filtered_bars: Vec<_> = bars.into_iter()
            .filter(|b| b.timestamp.date_naive() == date)
            .collect();

        if filtered_bars.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No equity bars found for {} on {}",
                symbol, date
            )));
        }

        // Build DataFrame
        let timestamps: Vec<i64> = filtered_bars.iter()
            .map(|b| TradingTimestamp::from_datetime_utc(b.timestamp).to_nanos())
            .collect();
        let opens: Vec<f64> = filtered_bars.iter().map(|b| b.open).collect();
        let highs: Vec<f64> = filtered_bars.iter().map(|b| b.high).collect();
        let lows: Vec<f64> = filtered_bars.iter().map(|b| b.low).collect();
        let closes: Vec<f64> = filtered_bars.iter().map(|b| b.close).collect();
        let volumes: Vec<i64> = filtered_bars.iter().map(|b| b.volume).collect();

        DataFrame::new(vec![
            Series::new("timestamp", timestamps),
            Series::new("open", opens),
            Series::new("high", highs),
            Series::new("low", lows),
            Series::new("close", closes),
            Series::new("volume", volumes),
        ])
        .map_err(|e| RepositoryError::Polars(e.to_string()))
    }
}
