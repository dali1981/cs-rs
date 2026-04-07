use async_trait::async_trait;
use chrono::{NaiveDate, DateTime, Utc};
use thiserror::Error;

use crate::entities::*;
use crate::value_objects::*;

#[derive(Error, Debug)]
pub enum RepositoryError {
    #[error("Data not found: {0}")]
    NotFound(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Polars error: {0}")]
    Polars(String),
}

/// Earnings data repository
#[async_trait]
pub trait EarningsRepository: Send + Sync {
    async fn load_earnings(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        symbols: Option<&[String]>,
    ) -> Result<Vec<EarningsEvent>, RepositoryError>;
}

/// Options data repository
///
/// All methods return domain types (`Vec<OptionBar>`). Repository implementations
/// are responsible for converting provider-specific storage formats (DataFrames,
/// provider DTOs, parquet files) to these canonical types internally.
#[async_trait]
pub trait OptionsDataRepository: Send + Sync {
    /// Get option bars for a specific date (daily aggregated snapshot)
    async fn get_option_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<Vec<OptionBar>, RepositoryError>;

    /// Get minute-level option bars for a specific date
    async fn get_option_minute_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<Vec<OptionBar>, RepositoryError>;

    /// Get option chain snapshot at a specific point in time (minute-aligned).
    /// Returns the most recent trade for each contract at or before target_time.
    async fn get_option_bars_at_time(
        &self,
        underlying: &str,
        target_time: DateTime<Utc>,
    ) -> Result<Vec<OptionBar>, RepositoryError>;

    /// Get option chain snapshot at or after a specific time (forward-looking).
    ///
    /// For exit pricing when no data exists at the exact time (illiquid stocks).
    /// First tries backward lookup, then looks forward up to max_forward_minutes.
    /// Returns `(bars, actual_snapshot_time)` where `actual_snapshot_time` is the
    /// timestamp of the data actually used.
    async fn get_option_bars_at_or_after_time(
        &self,
        underlying: &str,
        target_time: DateTime<Utc>,
        max_forward_minutes: u32,
    ) -> Result<(Vec<OptionBar>, DateTime<Utc>), RepositoryError>;

    async fn get_available_expirations(
        &self,
        underlying: &str,
        as_of_date: NaiveDate,
    ) -> Result<Vec<NaiveDate>, RepositoryError>;

    async fn get_available_strikes(
        &self,
        underlying: &str,
        expiration: NaiveDate,
        as_of_date: NaiveDate,
    ) -> Result<Vec<Strike>, RepositoryError>;
}

/// Equity data repository
#[async_trait]
pub trait EquityDataRepository: Send + Sync {
    async fn get_spot_price(
        &self,
        symbol: &str,
        target_time: DateTime<Utc>,
    ) -> Result<SpotPrice, RepositoryError>;

    /// Get minute-level equity bars for a specific date.
    ///
    /// Returns domain `EquityBar` types. Repository implementations convert
    /// their internal storage format before returning.
    async fn get_bars(
        &self,
        symbol: &str,
        date: NaiveDate,
    ) -> Result<Vec<EquityBar>, RepositoryError>;
}

/// Results persistence repository
#[async_trait]
pub trait ResultsRepository: Send + Sync {
    async fn save_results(
        &self,
        results: &[CalendarSpreadResult],
        run_id: &str,
    ) -> Result<(), RepositoryError>;

    async fn load_results(
        &self,
        run_id: &str,
    ) -> Result<Vec<CalendarSpreadResult>, RepositoryError>;
}
