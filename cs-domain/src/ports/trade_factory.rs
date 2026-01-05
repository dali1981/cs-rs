use crate::entities::Straddle;
use chrono::{DateTime, NaiveDate, Utc};
use thiserror::Error;

/// Port for creating trades from market data
///
/// This abstraction separates trade construction (selection + creation)
/// from trade execution. Implementations query market data repositories,
/// build IV surfaces, and use strike selectors to construct valid trades.
#[async_trait::async_trait]
pub trait TradeFactory: Send + Sync {
    /// Create an ATM straddle at the given date with minimum expiration
    ///
    /// # Arguments
    /// * `symbol` - Ticker symbol
    /// * `as_of` - Date/time to query market data
    /// * `min_expiration` - Minimum required expiration date (options must expire AFTER this)
    ///
    /// # Returns
    /// A Straddle with:
    /// - ATM strike (closest to spot price)
    /// - First available expiration after min_expiration
    /// - Both call and put legs at same strike/expiration
    ///
    /// # Errors
    /// Returns error if:
    /// - No market data available
    /// - No valid expirations found
    /// - No strikes available
    /// - IV surface construction fails
    async fn create_atm_straddle(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Straddle, TradeFactoryError>;

    /// Query available expiration dates for a symbol at a given time
    ///
    /// # Arguments
    /// * `symbol` - Ticker symbol
    /// * `as_of` - Date/time to query market data
    ///
    /// # Returns
    /// Sorted list of expiration dates available in the market data
    ///
    /// # Errors
    /// Returns error if no market data available or IV surface fails to build
    async fn available_expirations(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<NaiveDate>, TradeFactoryError>;
}

/// Errors that can occur during trade creation
#[derive(Debug, Error)]
pub enum TradeFactoryError {
    #[error("No expirations available")]
    NoExpirations,

    #[error("No strikes available")]
    NoStrikes,

    #[error("Data error: {0}")]
    DataError(String),

    #[error("Selection error: {0}")]
    SelectionError(String),
}
