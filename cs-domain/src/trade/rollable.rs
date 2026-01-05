//! Generic trade interface for rolling strategies
//!
//! The RollableTrade trait enables any trade type (straddle, calendar spread,
//! iron butterfly, etc.) to be used with the generic RollingExecutor.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

use crate::trade::TradeFactory;
use crate::hedging::HedgePosition;
use crate::PositionAttribution;

/// A trade that can be constructed, executed, and rolled
///
/// This trait provides a generic interface for any trade type that can be
/// used in a rolling strategy. The generic RollingExecutor uses this trait
/// to enable rolling ANY trade type without code duplication.
///
/// # Example
/// ```ignore
/// // Rolling straddles
/// let executor = RollingExecutor::<Straddle, _, _>::new(...);
///
/// // Rolling calendar spreads (same code!)
/// let executor = RollingExecutor::<CalendarSpread, _, _>::new(...);
/// ```
#[async_trait]
pub trait RollableTrade: Sized + Send + Sync {
    /// Result type returned by execution
    type Result: TradeResult;

    /// Construct trade at given datetime
    ///
    /// # Arguments
    /// * `factory` - Trade factory for option chain queries
    /// * `symbol` - Underlying symbol
    /// * `dt` - Entry datetime (for spot/IV lookup)
    /// * `min_expiration` - Earliest acceptable expiration date
    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError>;

    /// Get expiration date (for roll scheduling)
    ///
    /// For single-leg trades, this is the option expiration.
    /// For multi-leg trades, this is typically the short leg expiration.
    fn expiration(&self) -> NaiveDate;

    /// Get strike (for logging/display)
    ///
    /// For single-leg trades, this is the strike.
    /// For multi-leg trades, this is typically the shared strike or ATM strike.
    fn strike(&self) -> Decimal;

    /// Get symbol
    fn symbol(&self) -> &str;
}

/// Common interface for trade execution results
///
/// All trade result types (StraddleResult, CalendarSpreadResult, etc.)
/// must implement this trait to enable generic result aggregation and hedging.
pub trait TradeResult: Send + Sync {
    /// Symbol being traded
    fn symbol(&self) -> &str;

    /// Net P&L from the trade
    fn pnl(&self) -> Decimal;

    /// Entry cost (debit paid)
    fn entry_cost(&self) -> Decimal;

    /// Exit value (credit received)
    fn exit_value(&self) -> Decimal;

    /// Whether the trade executed successfully
    fn success(&self) -> bool;

    /// Entry timestamp
    fn entry_time(&self) -> DateTime<Utc>;

    /// Exit timestamp
    fn exit_time(&self) -> DateTime<Utc>;

    /// Spot price at entry
    fn spot_at_entry(&self) -> f64;

    /// Spot price at exit
    fn spot_at_exit(&self) -> f64;

    // --- Hedging support (trade-agnostic) ---

    /// Net delta of the position at entry
    fn net_delta(&self) -> Option<f64>;

    /// Net gamma of the position at entry
    fn net_gamma(&self) -> Option<f64>;

    // --- IV support (for volatility tracking) ---

    /// Implied volatility at entry
    fn iv_entry(&self) -> Option<f64> {
        None
    }

    /// Implied volatility at exit
    fn iv_exit(&self) -> Option<f64> {
        None
    }

    /// IV change (exit - entry)
    fn iv_change(&self) -> Option<f64> {
        None
    }

    /// Hedge P&L if hedging was applied
    fn hedge_pnl(&self) -> Option<Decimal>;

    /// Total P&L including hedge
    fn total_pnl_with_hedge(&self) -> Option<Decimal>;

    /// Get the hedge position if hedging was applied
    fn hedge_position(&self) -> Option<&HedgePosition> {
        None
    }

    /// Apply hedge results to this trade result
    ///
    /// This is called after hedging simulation completes to store the results.
    fn apply_hedge_results(
        &mut self,
        position: HedgePosition,
        hedge_pnl: Decimal,
        total_pnl: Decimal,
        attribution: Option<PositionAttribution>,
    );
}

/// Errors that can occur during trade construction
#[derive(Debug, thiserror::Error)]
pub enum TradeConstructionError {
    #[error("No options data available: {0}")]
    NoOptionsData(String),

    #[error("No valid expiration found: {0}")]
    NoExpiration(String),

    #[error("No ATM strike found: {0}")]
    NoStrike(String),

    #[error("Factory error: {0}")]
    FactoryError(String),

    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),
}
