use async_trait::async_trait;
use chrono::{NaiveDate, Datelike};
use std::path::PathBuf;
use polars::prelude::*;

use crate::datetime::TradingDate;
use crate::entities::EarningsEvent;
use crate::repositories::{EarningsRepository, RepositoryError};
use crate::value_objects::EarningsTime;

/// Stub implementation of EarningsRepository
///
/// In production, this would load from:
/// - nasdaq-earnings parquet files
/// - CSV files
/// - External API
///
/// For now, returns empty list to allow compilation and testing of other components.
pub struct StubEarningsRepository {
    #[allow(dead_code)]
    data_dir: Option<PathBuf>,
}

impl StubEarningsRepository {
    pub fn new(data_dir: Option<PathBuf>) -> Self {
        Self { data_dir }
    }
}

#[async_trait]
impl EarningsRepository for StubEarningsRepository {
    async fn load_earnings(
        &self,
        _start_date: NaiveDate,
        _end_date: NaiveDate,
        _symbols: Option<&[String]>,
    ) -> Result<Vec<EarningsEvent>, RepositoryError> {
        // TODO: Implement actual earnings data loading
        // For now, return empty list
        Ok(Vec::new())
    }
}

/// Parquet-based EarningsRepository implementation
///
/// Loads earnings from parquet files with schema:
/// - symbol: String
/// - earnings_date: Date
/// - earnings_time: String (BMO/AMC)
/// - company_name: String (optional)
/// - market_cap: UInt64 (optional)
pub struct ParquetEarningsRepository {
    data_dir: PathBuf,
}

impl ParquetEarningsRepository {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    fn get_parquet_path(&self, year: i32) -> PathBuf {
        self.data_dir.join(format!("earnings_{}.parquet", year))
    }
}

#[async_trait]
impl EarningsRepository for ParquetEarningsRepository {
    async fn load_earnings(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        symbols: Option<&[String]>,
    ) -> Result<Vec<EarningsEvent>, RepositoryError> {
        // Determine which parquet files to read based on date range
        let start_year = start_date.year();
        let end_year = end_date.year();

        let mut all_events = Vec::new();

        for year in start_year..=end_year {
            let path = self.get_parquet_path(year);

            if !path.exists() {
                continue; // Skip missing years
            }

            // Convert NaiveDate to Polars date (days since Unix epoch)
            let start_polars = TradingDate::from_naive_date(start_date).to_polars_date();
            let end_polars = TradingDate::from_naive_date(end_date).to_polars_date();

            let df = LazyFrame::scan_parquet(&path, Default::default())
                .map_err(|e| RepositoryError::Polars(e.to_string()))?
                .filter(
                    col("earnings_date")
                        .gt_eq(lit(start_polars))
                        .and(col("earnings_date").lt_eq(lit(end_polars)))
                )
                .collect()
                .map_err(|e| RepositoryError::Polars(e.to_string()))?;

            if df.is_empty() {
                continue;
            }

            // Convert DataFrame to EarningsEvents
            let symbols_col = df.column("symbol")
                .map_err(|e| RepositoryError::Parse(format!("Missing symbol column: {}", e)))?
                .str()
                .map_err(|e| RepositoryError::Parse(format!("Invalid symbol type: {}", e)))?;

            let dates_col = df.column("earnings_date")
                .map_err(|e| RepositoryError::Parse(format!("Missing earnings_date column: {}", e)))?
                .date()
                .map_err(|e| RepositoryError::Parse(format!("Invalid earnings_date type: {}", e)))?;

            let times_col = df.column("earnings_time")
                .map_err(|e| RepositoryError::Parse(format!("Missing earnings_time column: {}", e)))?
                .str()
                .map_err(|e| RepositoryError::Parse(format!("Invalid earnings_time type: {}", e)))?;

            let company_col = df.column("company_name").ok()
                .and_then(|c| c.str().ok());

            let market_cap_col = df.column("market_cap").ok()
                .and_then(|c| c.u64().ok());

            for i in 0..df.height() {
                let symbol = symbols_col.get(i)
                    .ok_or_else(|| RepositoryError::Parse("Missing symbol value".into()))?
                    .to_string();

                // Filter by symbols if provided
                if let Some(filter_symbols) = symbols {
                    if !filter_symbols.contains(&symbol) {
                        continue;
                    }
                }

                let earnings_date = dates_col.get(i)
                    .map(|days| TradingDate::from_polars_date(days).to_naive_date())
                    .ok_or_else(|| RepositoryError::Parse("Invalid earnings date".into()))?;

                let earnings_time_str = times_col.get(i)
                    .ok_or_else(|| RepositoryError::Parse("Missing earnings_time value".into()))?;
                let earnings_time = EarningsTime::from_str(earnings_time_str);

                let company_name = company_col.and_then(|c| c.get(i).map(|s| s.to_string()));
                let market_cap = market_cap_col.and_then(|c| c.get(i));

                let mut event = EarningsEvent::new(symbol, earnings_date, earnings_time);
                if let Some(name) = company_name {
                    event = event.with_company_name(name);
                }
                if let Some(cap) = market_cap {
                    event = event.with_market_cap(cap);
                }

                all_events.push(event);
            }
        }

        Ok(all_events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stub_earnings_repository() {
        let repo = StubEarningsRepository::new(None);

        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 1, 31).unwrap();

        let result = repo.load_earnings(start, end, None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0); // Stub returns empty
    }

    #[tokio::test]
    #[ignore] // Requires actual earnings data
    async fn test_parquet_earnings_repository() {
        let data_dir = PathBuf::from(
            std::env::var("EARNINGS_DATA_DIR").unwrap_or_else(|_| "~/earnings_data".to_string())
        );
        let repo = ParquetEarningsRepository::new(data_dir);

        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 1, 31).unwrap();

        let result = repo.load_earnings(start, end, None).await;

        if result.is_ok() {
            let events = result.unwrap();
            // Events should be within date range
            for event in &events {
                assert!(event.earnings_date >= start);
                assert!(event.earnings_date <= end);
            }
        }
    }
}
