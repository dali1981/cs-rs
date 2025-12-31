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
#[async_trait]
pub trait OptionsDataRepository: Send + Sync {
    async fn get_option_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<polars::frame::DataFrame, RepositoryError>;

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

    async fn get_bars(
        &self,
        symbol: &str,
        date: NaiveDate,
    ) -> Result<polars::frame::DataFrame, RepositoryError>;
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
