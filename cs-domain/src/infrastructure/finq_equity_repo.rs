use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use finq_flatfiles::{StockBarReader, StockBarRepository, FlatfileConfig};
use finq_core::Timeframe;
use rust_decimal::Decimal;
use std::path::PathBuf;

use crate::repositories::{EquityDataRepository, RepositoryError};
use crate::value_objects::{EquityBar, SpotPrice};
use super::option_bar_conversions::dataframe_to_equity_bars;

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
        let bars = self.get_bars(symbol, date).await?;

        if bars.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No bars found for {} on {}",
                symbol, date
            )));
        }

        let bar = bars.iter()
            .filter(|b| b.timestamp <= target_time)
            .max_by_key(|b| b.timestamp)
            .ok_or_else(|| RepositoryError::NotFound(format!(
                "No spot price for {} at {} (no bars before this time)",
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
    ) -> Result<Vec<EquityBar>, RepositoryError> {
        let df = self.repository
            .get_bars_dataframe(symbol, Timeframe::MINUTE, date, date)
            .await
            .map_err(|e| RepositoryError::NotFound(format!(
                "Failed to load equity bars for {} on {}: {}",
                symbol, date, e
            )))?;
        dataframe_to_equity_bars(&df)
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
            let bars = result.unwrap();
            assert!(!bars.is_empty());
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
