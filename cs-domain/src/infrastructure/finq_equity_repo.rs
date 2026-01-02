use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use finq_flatfiles::{StockBarReader, StockBarRepository, FlatfileConfig};
use finq_core::Timeframe;
use polars::prelude::*;
use rust_decimal::Decimal;
use std::path::PathBuf;

use crate::datetime::TradingTimestamp;
use crate::repositories::{EquityDataRepository, RepositoryError};
use crate::value_objects::SpotPrice;

pub struct FinqEquityRepository {
    repository: StockBarRepository,
}

impl FinqEquityRepository {
    pub fn new(data_dir: PathBuf) -> Self {
        let config = FlatfileConfig::new(data_dir);
        Self {
            repository: StockBarRepository::new(config),
        }
    }
}

#[async_trait]
impl EquityDataRepository for FinqEquityRepository {
    async fn get_spot_price(
        &self,
        symbol: &str,
        target_time: DateTime<Utc>,
    ) -> Result<SpotPrice, RepositoryError> {
        let date = target_time.date_naive();
        let df = self.get_bars(symbol, date).await?;

        if df.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No bars found for {} on {}",
                symbol, date
            )));
        }

        // Convert target time to nanoseconds using TradingTimestamp
        let target_ts = TradingTimestamp::from_datetime_utc(target_time);
        let target_nanos = target_ts.to_nanos();

        // Note: DataFrame timestamps are in milliseconds (Datetime[ms])
        // Convert target to milliseconds for comparison
        let target_millis = target_nanos / 1_000_000;

        // Filter to bars at or before target time
        let filtered = df
            .lazy()
            .filter(col("timestamp").lt_eq(lit(target_millis)))
            .sort(
                ["timestamp"],
                SortMultipleOptions::default().with_order_descending(true),
            )
            .limit(1)
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if filtered.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No spot price for {} at {} (no bars before this time)",
                symbol, target_time
            )));
        }

        let close = filtered
            .column("close")
            .map_err(|e| RepositoryError::Parse(format!("Missing close column: {}", e)))?
            .f64()
            .map_err(|e| RepositoryError::Parse(format!("Invalid close type: {}", e)))?
            .get(0)
            .ok_or_else(|| RepositoryError::NotFound("Empty close column".into()))?;

        let timestamp_col = filtered
            .column("timestamp")
            .map_err(|e| RepositoryError::Parse(format!("Missing timestamp column: {}", e)))?;

        // Extract timestamp based on the column type
        let timestamp_nanos = if let Ok(i64_series) = timestamp_col.i64() {
            i64_series.get(0)
                .ok_or_else(|| RepositoryError::NotFound("Empty timestamp column".into()))?
        } else if let Ok(datetime_series) = timestamp_col.datetime() {
            // Datetime column stores values in its time_unit (milliseconds in this case)
            // Convert to nanoseconds: ms * 1_000_000 = ns
            let timestamp_ms = datetime_series.get(0)
                .ok_or_else(|| RepositoryError::NotFound("Empty timestamp column".into()))?;
            timestamp_ms * 1_000_000
        } else {
            return Err(RepositoryError::Parse("Unsupported timestamp column type".into()));
        };

        // Convert using TradingTimestamp
        let timestamp = TradingTimestamp::from_nanos(timestamp_nanos).to_datetime_utc();

        Ok(SpotPrice {
            value: Decimal::try_from(close)
                .map_err(|e| RepositoryError::Parse(format!("Invalid decimal: {}", e)))?,
            timestamp,
        })
    }

    async fn get_bars(
        &self,
        symbol: &str,
        date: NaiveDate,
    ) -> Result<DataFrame, RepositoryError> {
        self.repository
            .get_bars_dataframe(symbol, Timeframe::MINUTE, date, date)
            .await
            .map_err(|e| RepositoryError::NotFound(format!(
                "Failed to load equity bars for {} on {}: {}",
                symbol, date, e
            )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires actual finq data
    async fn test_finq_equity_repository_get_bars() {
        let repo = FinqEquityRepository::new(PathBuf::from(
            std::env::var("FINQ_DATA_DIR").unwrap_or_else(|_| "~/finq_data".to_string())
        ));

        let date = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let result = repo.get_bars("AAPL", date).await;

        if result.is_ok() {
            let df = result.unwrap();
            assert!(df.height() > 0);
        }
    }

    #[tokio::test]
    #[ignore] // Requires actual finq data
    async fn test_finq_equity_repository_get_spot_price() {
        let repo = FinqEquityRepository::new(PathBuf::from(
            std::env::var("FINQ_DATA_DIR").unwrap_or_else(|_| "~/finq_data".to_string())
        ));

        let date = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let target_time = date.and_hms_opt(15, 30, 0).unwrap().and_utc();

        let result = repo.get_spot_price("AAPL", target_time).await;

        if result.is_ok() {
            let spot = result.unwrap();
            assert!(spot.value > Decimal::ZERO);
            assert!(spot.timestamp <= target_time);
        }
    }
}
