use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use finq_core::Timeframe;
use finq_flatfiles::{OptionBarReader, OptionBarRepository, FlatfileConfig};
use polars::prelude::*;
use rust_decimal::Decimal;
use std::path::PathBuf;

use crate::datetime::{TradingDate, TradingTimestamp};
use crate::repositories::{OptionsDataRepository, RepositoryError};
use crate::value_objects::{OptionBar, Strike};
use super::option_bar_conversions::dataframe_to_option_bars;

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
    ) -> Result<Vec<OptionBar>, RepositoryError> {
        let df = self.repository
            .get_chain_bars(underlying, date)
            .await
            .map_err(|e| RepositoryError::NotFound(format!(
                "Failed to load option bars for {} on {}: {}",
                underlying, date, e
            )))?;
        dataframe_to_option_bars(&df)
    }

    async fn get_option_minute_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<Vec<OptionBar>, RepositoryError> {
        let df = self.repository
            .get_bars(underlying, Timeframe::MINUTE, date, date)
            .await
            .map_err(|e| RepositoryError::NotFound(format!(
                "Failed to load minute option bars for {} on {}: {}",
                underlying, date, e
            )))?;
        dataframe_to_option_bars(&df)
    }

    async fn get_option_bars_at_time(
        &self,
        underlying: &str,
        target_time: DateTime<Utc>,
    ) -> Result<Vec<OptionBar>, RepositoryError> {
        let date = target_time.date_naive();

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

        let filtered = df
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
            ])
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if filtered.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No option bars at or before {} for {} on {}",
                target_time, underlying, date
            )));
        }

        dataframe_to_option_bars(&filtered)
    }

    async fn get_option_bars_at_or_after_time(
        &self,
        underlying: &str,
        target_time: DateTime<Utc>,
        max_forward_minutes: u32,
    ) -> Result<(Vec<OptionBar>, DateTime<Utc>), RepositoryError> {
        let date = target_time.date_naive();

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
            ])
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if !backward.is_empty() {
            let bars = dataframe_to_option_bars(&backward)?;
            return Ok((bars, target_time));
        }

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
                    .with_order_descending_multi(vec![false, false, false, false])
            )
            .group_by([col("strike"), col("expiration"), col("option_type")])
            .agg([
                col("close").first().alias("close"),
                col("timestamp").first().alias("timestamp"),
            ])
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if forward.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No option bars within {} minutes of {} for {} on {}",
                max_forward_minutes, target_time, underlying, date
            )));
        }

        let timestamps = forward
            .column("timestamp")
            .map_err(|e| RepositoryError::Polars(e.to_string()))?
            .i64()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        let max_ts = timestamps
            .max()
            .ok_or_else(|| RepositoryError::NotFound("No timestamp in forward data".to_string()))?;

        let actual_snapshot_time = TradingTimestamp::from_nanos(max_ts).to_datetime_utc();
        let bars = dataframe_to_option_bars(&forward)?;
        Ok((bars, actual_snapshot_time))
    }

    async fn get_available_expirations(
        &self,
        underlying: &str,
        as_of_date: NaiveDate,
    ) -> Result<Vec<NaiveDate>, RepositoryError> {
        let bars = self.get_option_bars(underlying, as_of_date).await?;
        let mut result: Vec<NaiveDate> = bars.iter()
            .map(|b| b.expiration)
            .filter(|&exp| exp > as_of_date)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
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
        let mut result: Vec<Strike> = bars.iter()
            .filter(|b| b.expiration == expiration)
            .filter_map(|b| {
                Decimal::try_from(b.strike).ok().and_then(|d| Strike::new(d).ok())
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
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

        if result.is_ok() {
            let bars = result.unwrap();
            assert!(!bars.is_empty());
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
            for i in 1..expirations.len() {
                assert!(expirations[i] > expirations[i - 1]);
            }
        }
    }
}
