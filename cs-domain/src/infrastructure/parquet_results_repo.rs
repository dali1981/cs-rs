use async_trait::async_trait;
use std::path::PathBuf;
use std::fs;

use crate::entities::CalendarSpreadResult;
use crate::repositories::{ResultsRepository, RepositoryError};

/// Parquet-based results persistence
///
/// Saves results to parquet files for efficient storage and querying
pub struct ParquetResultsRepository {
    base_dir: PathBuf,
}

impl ParquetResultsRepository {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn get_json_path(&self, run_id: &str) -> PathBuf {
        self.base_dir.join(format!("{}.json", run_id))
    }
}

#[async_trait]
impl ResultsRepository for ParquetResultsRepository {
    async fn save_results(
        &self,
        results: &[CalendarSpreadResult],
        run_id: &str,
    ) -> Result<(), RepositoryError> {
        if results.is_empty() {
            return Ok(());
        }

        // Ensure directory exists
        fs::create_dir_all(&self.base_dir)?;

        // For now, serialize to JSON (simpler than building Arrow schema)
        // TODO: Convert to Parquet for better performance
        let json_path = self.get_json_path(run_id);
        let json = serde_json::to_string_pretty(results)
            .map_err(|e| RepositoryError::Parse(format!("Failed to serialize results: {}", e)))?;

        fs::write(json_path, json)?;

        Ok(())
    }

    async fn load_results(
        &self,
        run_id: &str,
    ) -> Result<Vec<CalendarSpreadResult>, RepositoryError> {
        let json_path = self.get_json_path(run_id);

        if !json_path.exists() {
            return Err(RepositoryError::NotFound(format!(
                "Results not found for run_id: {}",
                run_id
            )));
        }

        let json = fs::read_to_string(json_path)?;
        let results: Vec<CalendarSpreadResult> = serde_json::from_str(&json)
            .map_err(|e| RepositoryError::Parse(format!("Failed to deserialize results: {}", e)))?;

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};
    use rust_decimal::Decimal;
    use finq_core::OptionType;
    use crate::value_objects::{EarningsTime, Strike};

    fn create_test_result() -> CalendarSpreadResult {
        CalendarSpreadResult {
            symbol: "TEST".to_string(),
            earnings_date: NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            earnings_time: EarningsTime::AfterMarketClose,
            strike: Strike::new(Decimal::new(100, 0)).unwrap(),
            long_strike: None,
            option_type: OptionType::Call,
            short_expiry: NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
            long_expiry: NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
            entry_time: Utc::now(),
            short_entry_price: Decimal::new(5, 0),
            long_entry_price: Decimal::new(6, 0),
            entry_cost: Decimal::new(1, 0),
            exit_time: Utc::now(),
            short_exit_price: Decimal::new(2, 0),
            long_exit_price: Decimal::new(4, 0),
            exit_value: Decimal::new(2, 0),
            pnl: Decimal::new(1, 0),
            pnl_per_contract: Decimal::new(1, 0),
            pnl_pct: Decimal::new(100, 0),
            short_delta: Some(0.5),
            short_gamma: Some(0.1),
            short_theta: Some(-0.05),
            short_vega: Some(0.2),
            long_delta: Some(0.4),
            long_gamma: Some(0.08),
            long_theta: Some(-0.03),
            long_vega: Some(0.15),
            iv_short_entry: Some(0.30),
            iv_long_entry: Some(0.25),
            iv_short_exit: Some(0.28),
            iv_long_exit: Some(0.26),
            iv_ratio_entry: Some(1.2),
            delta_pnl: Some(Decimal::new(5, 1)),
            gamma_pnl: Some(Decimal::new(2, 1)),
            theta_pnl: Some(Decimal::new(-1, 1)),
            vega_pnl: Some(Decimal::new(3, 1)),
            unexplained_pnl: Some(Decimal::new(1, 1)),
            spot_at_entry: 100.0,
            spot_at_exit: 102.0,
            success: true,
            failure_reason: None,
        }
    }

    #[tokio::test]
    async fn test_parquet_results_repository_save_load() {
        let temp_dir = std::env::temp_dir().join("cs_test_results");
        fs::create_dir_all(&temp_dir).unwrap();

        let repo = ParquetResultsRepository::new(temp_dir.clone());

        let results = vec![create_test_result()];
        let run_id = "test_run_123";

        // Save
        let save_result = repo.save_results(&results, run_id).await;
        assert!(save_result.is_ok());

        // Load
        let load_result = repo.load_results(run_id).await;
        assert!(load_result.is_ok());

        let loaded = load_result.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].symbol, "TEST");
        assert_eq!(loaded[0].pnl, Decimal::new(1, 0));

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_parquet_results_repository_load_not_found() {
        let temp_dir = std::env::temp_dir().join("cs_test_results_2");
        let repo = ParquetResultsRepository::new(temp_dir);

        let result = repo.load_results("nonexistent_run").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepositoryError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_parquet_results_repository_empty_results() {
        let temp_dir = std::env::temp_dir().join("cs_test_results_3");
        fs::create_dir_all(&temp_dir).unwrap();

        let repo = ParquetResultsRepository::new(temp_dir.clone());

        let results: Vec<CalendarSpreadResult> = vec![];
        let save_result = repo.save_results(&results, "empty_run").await;
        assert!(save_result.is_ok());

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }
}
