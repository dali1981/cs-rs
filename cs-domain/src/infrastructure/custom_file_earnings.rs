use async_trait::async_trait;
use chrono::NaiveDate;
use polars::prelude::*;
use std::path::PathBuf;

use crate::datetime::TradingDate;
use crate::entities::EarningsEvent;
use crate::repositories::{EarningsRepository, RepositoryError};
use crate::value_objects::EarningsTime;

/// Custom file-based EarningsRepository for user-provided earnings files.
///
/// Supports:
/// - Parquet files with schema: symbol, earnings_date, earnings_time, company_name (opt), market_cap (opt)
/// - JSON files with array of {symbol, date, time} objects
///
/// File format is auto-detected by extension (.parquet or .json)
pub struct CustomFileEarningsReader {
    file_path: PathBuf,
}

impl CustomFileEarningsReader {
    /// Create reader from file path (auto-detects format)
    pub fn from_file(file_path: PathBuf) -> Result<Self, RepositoryError> {
        if !file_path.exists() {
            return Err(RepositoryError::NotFound(format!(
                "Earnings file not found: {:?}",
                file_path
            )));
        }

        Ok(Self { file_path })
    }

    fn parse_earnings_time(s: &str) -> EarningsTime {
        match s.to_uppercase().as_str() {
            "BMO" | "BEFORE_MARKET" | "BEFOREMARKET" => EarningsTime::BeforeMarketOpen,
            "AMC" | "AFTER_MARKET" | "AFTERMARKET" => EarningsTime::AfterMarketClose,
            _ => EarningsTime::Unknown,
        }
    }

    fn load_from_parquet(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        symbols: Option<&[String]>,
    ) -> Result<Vec<EarningsEvent>, RepositoryError> {
        // Load parquet file
        let df = LazyFrame::scan_parquet(&self.file_path, Default::default())
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        // Filter by date range
        let start_polars = TradingDate::from_naive_date(start_date).to_polars_date();
        let end_polars = TradingDate::from_naive_date(end_date).to_polars_date();

        let filtered = df.filter(
            col("earnings_date")
                .gt_eq(lit(start_polars))
                .and(col("earnings_date").lt_eq(lit(end_polars))),
        );

        let df = filtered
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if df.is_empty() {
            return Ok(Vec::new());
        }

        // Extract columns
        let symbols_col = df
            .column("symbol")
            .map_err(|e| RepositoryError::Parse(format!("Missing symbol column: {}", e)))?
            .str()
            .map_err(|e| RepositoryError::Parse(format!("Invalid symbol type: {}", e)))?;

        let dates_col = df
            .column("earnings_date")
            .map_err(|e| RepositoryError::Parse(format!("Missing earnings_date column: {}", e)))?
            .date()
            .map_err(|e| RepositoryError::Parse(format!("Invalid earnings_date type: {}", e)))?;

        let times_col = df
            .column("earnings_time")
            .map_err(|e| RepositoryError::Parse(format!("Missing earnings_time column: {}", e)))?
            .str()
            .map_err(|e| RepositoryError::Parse(format!("Invalid earnings_time type: {}", e)))?;

        let company_col = df.column("company_name").ok().and_then(|c| c.str().ok());

        let market_cap_col = df.column("market_cap").ok().and_then(|c| c.u64().ok());

        // Convert rows to events
        let mut events = Vec::new();
        for i in 0..df.height() {
            let symbol = symbols_col
                .get(i)
                .ok_or_else(|| RepositoryError::Parse("Missing symbol value".into()))?
                .to_string();

            // Filter by symbols if provided
            if let Some(filter_symbols) = symbols {
                if !filter_symbols.contains(&symbol) {
                    continue;
                }
            }

            let earnings_date = dates_col
                .get(i)
                .map(|days| TradingDate::from_polars_date(days).to_naive_date())
                .ok_or_else(|| RepositoryError::Parse("Invalid earnings date".into()))?;

            let earnings_time_str = times_col
                .get(i)
                .ok_or_else(|| RepositoryError::Parse("Missing earnings_time value".into()))?;
            let earnings_time = Self::parse_earnings_time(earnings_time_str);

            let company_name = company_col.and_then(|c| c.get(i).map(|s| s.to_string()));
            let market_cap = market_cap_col.and_then(|c| c.get(i));

            let mut event = EarningsEvent::new(symbol, earnings_date, earnings_time);
            if let Some(name) = company_name {
                event = event.with_company_name(name);
            }
            if let Some(cap) = market_cap {
                event = event.with_market_cap(cap);
            }

            events.push(event);
        }

        // Sort by date
        events.sort_by_key(|e| e.earnings_date);

        Ok(events)
    }

    fn load_from_json(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        symbols: Option<&[String]>,
    ) -> Result<Vec<EarningsEvent>, RepositoryError> {
        #[derive(serde::Deserialize)]
        struct JsonEarningsEvent {
            symbol: String,
            date: String,
            time: String,
            #[serde(default)]
            company_name: Option<String>,
            #[serde(default)]
            market_cap: Option<u64>,
        }

        let content = std::fs::read_to_string(&self.file_path)
            .map_err(|e| RepositoryError::Io(e))?;

        let json_events: Vec<JsonEarningsEvent> = serde_json::from_str(&content)
            .map_err(|e| RepositoryError::Parse(format!("Invalid JSON: {}", e)))?;

        let mut events = Vec::new();
        for json_event in json_events {
            // Parse date
            let earnings_date = NaiveDate::parse_from_str(&json_event.date, "%Y-%m-%d")
                .map_err(|e| RepositoryError::Parse(format!("Invalid date '{}': {}", json_event.date, e)))?;

            // Filter by date range
            if earnings_date < start_date || earnings_date > end_date {
                continue;
            }

            // Filter by symbols
            if let Some(syms) = symbols {
                if !syms.contains(&json_event.symbol) {
                    continue;
                }
            }

            let earnings_time = Self::parse_earnings_time(&json_event.time);

            let mut event = EarningsEvent::new(json_event.symbol, earnings_date, earnings_time);
            if let Some(name) = json_event.company_name {
                event = event.with_company_name(name);
            }
            if let Some(cap) = json_event.market_cap {
                event = event.with_market_cap(cap);
            }

            events.push(event);
        }

        // Sort by date
        events.sort_by_key(|e| e.earnings_date);

        Ok(events)
    }
}

#[async_trait]
impl EarningsRepository for CustomFileEarningsReader {
    async fn load_earnings(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        symbols: Option<&[String]>,
    ) -> Result<Vec<EarningsEvent>, RepositoryError> {
        // Auto-detect format by extension
        let extension = self
            .file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| {
                RepositoryError::Parse("File has no extension (expected .parquet or .json)".into())
            })?;

        match extension {
            "parquet" => self.load_from_parquet(start_date, end_date, symbols),
            "json" => self.load_from_json(start_date, end_date, symbols),
            ext => Err(RepositoryError::Parse(format!(
                "Unsupported file extension '{}'. Use .parquet or .json",
                ext
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_load_parquet() {
        // Create test DataFrame
        let symbols = Series::new("symbol".into(), &["AAPL", "MSFT"]);
        let dates = Series::new(
            "earnings_date".into(),
            &[
                TradingDate::from_naive_date(NaiveDate::from_ymd_opt(2025, 1, 15).unwrap())
                    .to_polars_date(),
                TradingDate::from_naive_date(NaiveDate::from_ymd_opt(2025, 1, 20).unwrap())
                    .to_polars_date(),
            ],
        );
        let times = Series::new("earnings_time".into(), &["BMO", "AMC"]);

        let mut df = DataFrame::new(vec![symbols, dates, times]).unwrap();
        df = df
            .with_column(
                df.column("earnings_date")
                    .unwrap()
                    .clone()
                    .cast(&DataType::Date)
                    .unwrap(),
            )
            .unwrap();

        // Write to temp parquet
        let temp_file = NamedTempFile::new().unwrap();
        let file = std::fs::File::create(temp_file.path()).unwrap();
        ParquetWriter::new(file).finish(&mut df).unwrap();

        // Test loading
        let reader = CustomFileEarningsReader::from_file(temp_file.path().to_path_buf()).unwrap();
        let events = reader
            .load_earnings(
                NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].symbol, "AAPL");
        assert_eq!(events[0].earnings_time, EarningsTime::BeforeMarketOpen);
        assert_eq!(events[1].symbol, "MSFT");
        assert_eq!(events[1].earnings_time, EarningsTime::AfterMarketClose);
    }

    #[tokio::test]
    async fn test_load_json() {
        let json_content = r#"[
            {"symbol": "AAPL", "date": "2025-01-15", "time": "BMO"},
            {"symbol": "MSFT", "date": "2025-01-20", "time": "AMC"}
        ]"#;

        let temp_file = NamedTempFile::with_suffix(".json").unwrap();
        std::fs::write(temp_file.path(), json_content).unwrap();

        let reader = CustomFileEarningsReader::from_file(temp_file.path().to_path_buf()).unwrap();
        let events = reader
            .load_earnings(
                NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].symbol, "AAPL");
        assert_eq!(events[1].symbol, "MSFT");
    }

    #[tokio::test]
    async fn test_symbol_filter() {
        let json_content = r#"[
            {"symbol": "AAPL", "date": "2025-01-15", "time": "BMO"},
            {"symbol": "MSFT", "date": "2025-01-20", "time": "AMC"},
            {"symbol": "GOOGL", "date": "2025-01-25", "time": "BMO"}
        ]"#;

        let temp_file = NamedTempFile::with_suffix(".json").unwrap();
        std::fs::write(temp_file.path(), json_content).unwrap();

        let reader = CustomFileEarningsReader::from_file(temp_file.path().to_path_buf()).unwrap();
        let events = reader
            .load_earnings(
                NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
                Some(&["AAPL".to_string(), "GOOGL".to_string()]),
            )
            .await
            .unwrap();

        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.symbol == "AAPL" || e.symbol == "GOOGL"));
    }
}
