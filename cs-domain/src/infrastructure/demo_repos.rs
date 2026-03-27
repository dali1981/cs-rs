//! Demo repository implementations that load from fixtures for showcase purposes.
//!
//! These repositories are always compiled but are the primary data source
//! when the `demo` feature is enabled. They load data from the fixtures/
//! directory which contains a small slice of NVDA options/equity data
//! around the November 2024 earnings.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use polars::prelude::*;
use rust_decimal::Decimal;
use std::path::PathBuf;

use crate::datetime::{TradingDate, TradingTimestamp};
use crate::entities::EarningsEvent;
use crate::repositories::{EarningsRepository, EquityDataRepository, OptionsDataRepository, RepositoryError};
use crate::value_objects::{EarningsTime, SpotPrice, Strike};

/// Get the fixtures directory path
fn fixtures_dir() -> PathBuf {
    std::env::var("DEMO_FIXTURES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Default to ./fixtures relative to current working directory
            PathBuf::from("fixtures")
        })
}

/// Demo options repository that loads from fixtures/nvda_options.parquet
pub struct DemoOptionsRepository {
    options_df: DataFrame,
}

impl DemoOptionsRepository {
    pub fn new() -> Result<Self, RepositoryError> {
        let path = fixtures_dir().join("nvda_options.parquet");
        tracing::debug!("Loading demo options from {:?}", path);
        let options_df = LazyFrame::scan_parquet(&path, Default::default())
            .map_err(|e| RepositoryError::NotFound(format!("Failed to load demo options: {}. Make sure fixtures/nvda_options.parquet exists.", e)))?
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        tracing::info!("Demo options loaded: {} rows", options_df.height());
        Ok(Self { options_df })
    }
}

impl Default for DemoOptionsRepository {
    fn default() -> Self {
        Self::new().expect("Failed to load demo options data. Ensure fixtures/nvda_options.parquet exists.")
    }
}

#[async_trait]
impl OptionsDataRepository for DemoOptionsRepository {
    async fn get_option_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<DataFrame, RepositoryError> {
        tracing::debug!("DemoOptionsRepository::get_option_bars({}, {})", underlying, date);

        // Filter to requested underlying and date
        let filtered = self.options_df
            .clone()
            .lazy()
            .filter(
                col("underlying").eq(lit(underlying))
                    .and(col("timestamp").dt().date().eq(lit(date)))
            )
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        tracing::debug!("  -> found {} rows", filtered.height());

        if filtered.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No demo option bars for {} on {} (demo data only contains NVDA for Nov 2024)",
                underlying, date
            )));
        }

        Ok(filtered)
    }

    async fn get_option_minute_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<DataFrame, RepositoryError> {
        // Demo data is already minute-level snapshots
        self.get_option_bars(underlying, date).await
    }

    async fn get_option_bars_at_time(
        &self,
        underlying: &str,
        target_time: DateTime<Utc>,
    ) -> Result<DataFrame, RepositoryError> {
        let date = target_time.date_naive();
        let df = self.get_option_bars(underlying, date).await?;

        // Convert target time to milliseconds (Polars Datetime[ms])
        let target_millis = target_time.timestamp_millis();

        // Filter to trades at or before target time, then take latest per contract
        let filtered = df
            .lazy()
            .filter(col("timestamp").lt_eq(lit(target_millis).cast(DataType::Datetime(TimeUnit::Milliseconds, Some("UTC".into())))))
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

        if filtered.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No demo option bars at or before {} for {} on {}",
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
        // First try at target time
        if let Ok(df) = self.get_option_bars_at_time(underlying, target_time).await {
            return Ok((df, target_time));
        }

        // Try forward lookup
        let date = target_time.date_naive();
        let df = self.get_option_bars(underlying, date).await?;

        let target_millis = target_time.timestamp_millis();
        let max_forward_millis = target_millis + (max_forward_minutes as i64 * 60 * 1000);

        let forward = df
            .lazy()
            .filter(
                col("timestamp").gt(lit(target_millis).cast(DataType::Datetime(TimeUnit::Milliseconds, Some("UTC".into()))))
                    .and(col("timestamp").lt_eq(lit(max_forward_millis).cast(DataType::Datetime(TimeUnit::Milliseconds, Some("UTC".into())))))
            )
            .sort(
                ["strike", "expiration", "option_type", "timestamp"],
                SortMultipleOptions::default()
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
                "No demo option bars within {} minutes of {} for {} on {}",
                max_forward_minutes, target_time, underlying, date
            )));
        }

        // Get actual snapshot time from data
        let timestamps = forward
            .column("timestamp")
            .map_err(|e| RepositoryError::Polars(e.to_string()))?
            .datetime()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        let max_ts = timestamps
            .max()
            .ok_or_else(|| RepositoryError::NotFound("No timestamp in forward data".to_string()))?;

        let actual_snapshot_time = TradingTimestamp::from_nanos(max_ts * 1_000_000).to_datetime_utc();

        Ok((forward, actual_snapshot_time))
    }

    async fn get_available_expirations(
        &self,
        underlying: &str,
        as_of_date: NaiveDate,
    ) -> Result<Vec<NaiveDate>, RepositoryError> {
        let bars = self.get_option_bars(underlying, as_of_date).await?;

        let expirations = bars
            .column("expiration")
            .map_err(|e| RepositoryError::Parse(format!("Missing expiration column: {}", e)))?
            .date()
            .map_err(|e| RepositoryError::Parse(format!("Invalid date type: {}", e)))?
            .unique()
            .map_err(|e| RepositoryError::Parse(format!("Failed to get unique dates: {}", e)))?;

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

/// Demo equity repository that loads from fixtures/nvda_equity.parquet
pub struct DemoEquityRepository {
    equity_df: DataFrame,
}

impl DemoEquityRepository {
    pub fn new() -> Result<Self, RepositoryError> {
        let path = fixtures_dir().join("nvda_equity.parquet");
        let equity_df = LazyFrame::scan_parquet(&path, Default::default())
            .map_err(|e| RepositoryError::NotFound(format!("Failed to load demo equity: {}. Make sure fixtures/nvda_equity.parquet exists.", e)))?
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        Ok(Self { equity_df })
    }
}

impl Default for DemoEquityRepository {
    fn default() -> Self {
        Self::new().expect("Failed to load demo equity data. Ensure fixtures/nvda_equity.parquet exists.")
    }
}

#[async_trait]
impl EquityDataRepository for DemoEquityRepository {
    async fn get_spot_price(
        &self,
        symbol: &str,
        target_time: DateTime<Utc>,
    ) -> Result<SpotPrice, RepositoryError> {
        let date = target_time.date_naive();
        let df = self.get_bars(symbol, date).await?;

        if df.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No demo bars found for {} on {}",
                symbol, date
            )));
        }

        // Convert target time to milliseconds
        let target_millis = target_time.timestamp_millis();

        // Filter to bars at or before target time
        let filtered = df
            .lazy()
            .filter(col("timestamp").lt_eq(lit(target_millis).cast(DataType::Datetime(TimeUnit::Milliseconds, Some("UTC".into())))))
            .sort(
                ["timestamp"],
                SortMultipleOptions::default().with_order_descending(true),
            )
            .limit(1)
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if filtered.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No demo spot price for {} at {} (no bars before this time)",
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

        let timestamp_millis = timestamp_col
            .datetime()
            .map_err(|e| RepositoryError::Parse(format!("Invalid timestamp type: {}", e)))?
            .get(0)
            .ok_or_else(|| RepositoryError::NotFound("Empty timestamp column".into()))?;

        let timestamp = TradingTimestamp::from_nanos(timestamp_millis * 1_000_000).to_datetime_utc();

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
        let filtered = self.equity_df
            .clone()
            .lazy()
            .filter(
                col("ticker").eq(lit(symbol))
                    .and(col("timestamp").dt().date().eq(lit(date)))
            )
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if filtered.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No demo equity bars for {} on {} (demo data only contains NVDA for Nov 2024)",
                symbol, date
            )));
        }

        Ok(filtered)
    }
}

/// Demo earnings repository that loads from fixtures/earnings.csv
pub struct DemoEarningsRepository {
    earnings: Vec<EarningsEvent>,
}

impl DemoEarningsRepository {
    pub fn new() -> Result<Self, RepositoryError> {
        let path = fixtures_dir().join("earnings.csv");

        // Use LazyCsvReader for Polars 0.41+ API
        let df = LazyCsvReader::new(&path)
            .with_has_header(true)
            .finish()
            .map_err(|e| RepositoryError::NotFound(format!("Failed to load demo earnings: {}. Make sure fixtures/earnings.csv exists.", e)))?
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        let mut earnings = Vec::new();

        let symbols = df.column("symbol")
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .str()
            .map_err(|e| RepositoryError::Parse(e.to_string()))?;

        let dates = df.column("earnings_date")
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .str()
            .map_err(|e| RepositoryError::Parse(e.to_string()))?;

        let times = df.column("earnings_time")
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .str()
            .map_err(|e| RepositoryError::Parse(e.to_string()))?;

        let companies: Option<&StringChunked> = df.column("company_name").ok()
            .and_then(|c| c.str().ok());

        let market_caps: Option<&Int64Chunked> = df.column("market_cap").ok()
            .and_then(|c| c.i64().ok());

        for i in 0..df.height() {
            let symbol = symbols.get(i)
                .ok_or_else(|| RepositoryError::Parse("Missing symbol".into()))?
                .to_string();

            let date_str = dates.get(i)
                .ok_or_else(|| RepositoryError::Parse("Missing date".into()))?;
            let earnings_date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .map_err(|e| RepositoryError::Parse(format!("Invalid date: {}", e)))?;

            let time_str = times.get(i)
                .ok_or_else(|| RepositoryError::Parse("Missing time".into()))?;
            let earnings_time = EarningsTime::from_str(time_str);

            let mut event = EarningsEvent::new(symbol, earnings_date, earnings_time);

            if let Some(companies) = companies {
                if let Some(name) = companies.get(i) {
                    event = event.with_company_name(name.to_string());
                }
            }

            if let Some(caps) = market_caps {
                if let Some(cap) = caps.get(i) {
                    event = event.with_market_cap(cap as u64);
                }
            }

            earnings.push(event);
        }

        Ok(Self { earnings })
    }
}

impl Default for DemoEarningsRepository {
    fn default() -> Self {
        Self::new().expect("Failed to load demo earnings data. Ensure fixtures/earnings.csv exists.")
    }
}

#[async_trait]
impl EarningsRepository for DemoEarningsRepository {
    async fn load_earnings(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        symbols: Option<&[String]>,
    ) -> Result<Vec<EarningsEvent>, RepositoryError> {
        let filtered: Vec<EarningsEvent> = self.earnings
            .iter()
            .filter(|e| {
                e.earnings_date >= start_date && e.earnings_date <= end_date
            })
            .filter(|e| {
                symbols.map_or(true, |s| s.contains(&e.symbol))
            })
            .cloned()
            .collect();

        Ok(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires fixtures directory
    async fn test_demo_options_repository() {
        let repo = DemoOptionsRepository::new().unwrap();
        let date = NaiveDate::from_ymd_opt(2024, 11, 14).unwrap();
        let result = repo.get_option_bars("NVDA", date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore] // Requires fixtures directory
    async fn test_demo_equity_repository() {
        let repo = DemoEquityRepository::new().unwrap();
        let date = NaiveDate::from_ymd_opt(2024, 11, 14).unwrap();
        let result = repo.get_bars("NVDA", date).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore] // Requires fixtures directory
    async fn test_demo_earnings_repository() {
        let repo = DemoEarningsRepository::new().unwrap();
        let start = NaiveDate::from_ymd_opt(2024, 11, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 11, 30).unwrap();
        let result = repo.load_earnings(start, end, None).await;
        assert!(result.is_ok());
        let events = result.unwrap();
        assert!(!events.is_empty());
    }
}
