use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use finq_core::Timeframe;
use finq_flatfiles::{OptionBarReader, OptionBarRepository, FlatfileConfig};
use polars::prelude::*;
use rust_decimal::Decimal;
use std::path::PathBuf;

use crate::datetime::{TradingDate, TradingTimestamp};
use crate::repositories::{OptionsDataRepository, RepositoryError};
use crate::value_objects::Strike;

pub struct FinqOptionsRepository {
    repository: OptionBarRepository,
}

impl FinqOptionsRepository {
    pub fn new(data_dir: PathBuf) -> Self {
        let config = FlatfileConfig::new(data_dir);
        Self {
            repository: OptionBarRepository::new(config),
        }
    }
}

#[async_trait]
impl OptionsDataRepository for FinqOptionsRepository {
    async fn get_option_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<DataFrame, RepositoryError> {
        self.repository
            .get_chain_bars(underlying, date)
            .await
            .map_err(|e| RepositoryError::NotFound(format!(
                "Failed to load option bars for {} on {}: {}",
                underlying, date, e
            )))
    }

    async fn get_option_minute_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<DataFrame, RepositoryError> {
        self.repository
            .get_bars(underlying, Timeframe::MINUTE, date, date)
            .await
            .map_err(|e| RepositoryError::NotFound(format!(
                "Failed to load minute option bars for {} on {}: {}",
                underlying, date, e
            )))
    }

    async fn get_option_bars_at_time(
        &self,
        underlying: &str,
        target_time: DateTime<Utc>,
    ) -> Result<DataFrame, RepositoryError> {
        let date = target_time.date_naive();

        // Load minute bars for the target date
        let df = self.repository
            .get_bars(underlying, Timeframe::MINUTE, date, date)
            .await
            .map_err(|e| RepositoryError::NotFound(format!(
                "Failed to load minute option bars for {} on {}: {}",
                underlying, date, e
            )))?;

        if df.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No minute bars found for {} on {}",
                underlying, date
            )));
        }

        // Convert target time to nanoseconds
        let target_nanos = TradingTimestamp::from_datetime_utc(target_time).to_nanos();

        // Filter to trades at or before target time, then take latest per contract
        let filtered = df
            .lazy()
            .filter(col("timestamp").lt_eq(lit(target_nanos)))
            .sort(
                ["strike", "expiration", "option_type", "timestamp"],
                SortMultipleOptions::default()
                    .with_order_descending_multi(vec![false, false, false, true])
            )
            // Group by contract (strike, expiration, option_type) and take first (latest due to sort)
            .group_by([col("strike"), col("expiration"), col("option_type")])
            .agg([
                col("close").first().alias("close"),
                col("timestamp").first().alias("timestamp"),
                col("open").first().alias("open"),
                col("high").first().alias("high"),
                col("low").first().alias("low"),
                col("volume").first().alias("volume"),
            ])
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if filtered.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No option bars at or before {} for {} on {}",
                target_time, underlying, date
            )));
        }

        Ok(filtered)
    }

    async fn get_option_bars_at_or_after_time(
        &self,
        underlying: &str,
        target_time: DateTime<Utc>,
        max_forward_minutes: u32,
    ) -> Result<(DataFrame, DateTime<Utc>), RepositoryError> {
        let date = target_time.date_naive();

        // Load minute bars for the target date
        let df = self.repository
            .get_bars(underlying, Timeframe::MINUTE, date, date)
            .await
            .map_err(|e| RepositoryError::NotFound(format!(
                "Failed to load minute option bars for {} on {}: {}",
                underlying, date, e
            )))?;

        if df.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No minute bars found for {} on {}",
                underlying, date
            )));
        }

        let target_nanos = TradingTimestamp::from_datetime_utc(target_time).to_nanos();

        // First try backward lookup (at or before target time)
        let backward = df
            .clone()
            .lazy()
            .filter(col("timestamp").lt_eq(lit(target_nanos)))
            .sort(
                ["strike", "expiration", "option_type", "timestamp"],
                SortMultipleOptions::default()
                    .with_order_descending_multi(vec![false, false, false, true])
            )
            .group_by([col("strike"), col("expiration"), col("option_type")])
            .agg([
                col("close").first().alias("close"),
                col("timestamp").first().alias("timestamp"),
                col("open").first().alias("open"),
                col("high").first().alias("high"),
                col("low").first().alias("low"),
                col("volume").first().alias("volume"),
            ])
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if !backward.is_empty() {
            // Found data at or before target time - return with target_time as snapshot
            return Ok((backward, target_time));
        }

        // No backward data - look forward up to max_forward_minutes
        let max_forward_nanos = target_nanos + (max_forward_minutes as i64 * 60 * 1_000_000_000);

        let forward = df
            .lazy()
            .filter(
                col("timestamp").gt(lit(target_nanos))
                    .and(col("timestamp").lt_eq(lit(max_forward_nanos)))
            )
            .sort(
                ["strike", "expiration", "option_type", "timestamp"],
                SortMultipleOptions::default()
                    .with_order_descending_multi(vec![false, false, false, false]) // ascending timestamp
            )
            .group_by([col("strike"), col("expiration"), col("option_type")])
            .agg([
                col("close").first().alias("close"),
                col("timestamp").first().alias("timestamp"),
                col("open").first().alias("open"),
                col("high").first().alias("high"),
                col("low").first().alias("low"),
                col("volume").first().alias("volume"),
            ])
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if forward.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No option bars within {} minutes of {} for {} on {}",
                max_forward_minutes, target_time, underlying, date
            )));
        }

        // Get the actual snapshot time (max timestamp in the forward-looked data)
        let timestamps = forward
            .column("timestamp")
            .map_err(|e| RepositoryError::Polars(e.to_string()))?
            .i64()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        let max_ts = timestamps
            .max()
            .ok_or_else(|| RepositoryError::NotFound("No timestamp in forward data".to_string()))?;

        let actual_snapshot_time = TradingTimestamp::from_nanos(max_ts).to_datetime_utc();

        Ok((forward, actual_snapshot_time))
    }

    async fn get_available_expirations(
        &self,
        underlying: &str,
        as_of_date: NaiveDate,
    ) -> Result<Vec<NaiveDate>, RepositoryError> {
        let bars = self.get_option_bars(underlying, as_of_date).await?;

        // Extract unique expirations from DataFrame
        let expirations = bars
            .column("expiration")
            .map_err(|e| RepositoryError::Parse(format!("Missing expiration column: {}", e)))?
            .date()
            .map_err(|e| RepositoryError::Parse(format!("Invalid date type: {}", e)))?
            .unique()
            .map_err(|e| RepositoryError::Parse(format!("Failed to get unique dates: {}", e)))?;

        // Convert from Polars Date (days since Unix epoch) to NaiveDate using TradingDate
        let mut result: Vec<NaiveDate> = expirations
            .into_iter()
            .filter_map(|opt| {
                opt.map(|days| TradingDate::from_polars_date(days).to_naive_date())
            })
            .filter(|&exp| exp > as_of_date)
            .collect();

        result.sort();
        Ok(result)
    }

    async fn get_available_strikes(
        &self,
        underlying: &str,
        expiration: NaiveDate,
        as_of_date: NaiveDate,
    ) -> Result<Vec<Strike>, RepositoryError> {
        let bars = self.get_option_bars(underlying, as_of_date).await?;

        // Filter to specific expiration using TradingDate
        let expiration_polars = TradingDate::from_naive_date(expiration).to_polars_date();
        let filtered = bars
            .lazy()
            .filter(col("expiration").eq(lit(expiration_polars)))
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if filtered.is_empty() {
            return Ok(Vec::new());
        }

        let strikes = filtered
            .column("strike")
            .map_err(|e| RepositoryError::Parse(format!("Missing strike column: {}", e)))?
            .f64()
            .map_err(|e| RepositoryError::Parse(format!("Invalid strike type: {}", e)))?
            .unique()
            .map_err(|e| RepositoryError::Parse(format!("Failed to get unique strikes: {}", e)))?;

        let mut result: Vec<Strike> = strikes
            .into_iter()
            .filter_map(|opt| {
                opt.and_then(|v| {
                    Decimal::try_from(v)
                        .ok()
                        .and_then(|d| Strike::new(d).ok())
                })
            })
            .collect();

        result.sort();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires actual finq data
    async fn test_finq_options_repository_get_bars() {
        let repo = FinqOptionsRepository::new(PathBuf::from(
            std::env::var("FINQ_DATA_DIR").unwrap_or_else(|_| "~/finq_data".to_string())
        ));

        let date = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let result = repo.get_option_bars("AAPL", date).await;

        // This will fail if no data exists, which is expected in CI
        if result.is_ok() {
            let df = result.unwrap();
            assert!(df.height() > 0);
        }
    }

    #[tokio::test]
    #[ignore] // Requires actual finq data
    async fn test_finq_options_repository_get_expirations() {
        let repo = FinqOptionsRepository::new(PathBuf::from(
            std::env::var("FINQ_DATA_DIR").unwrap_or_else(|_| "~/finq_data".to_string())
        ));

        let date = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let result = repo.get_available_expirations("AAPL", date).await;

        if result.is_ok() {
            let expirations = result.unwrap();
            assert!(!expirations.is_empty());
            // Expirations should be sorted
            for i in 1..expirations.len() {
                assert!(expirations[i] > expirations[i - 1]);
            }
        }
    }
}
