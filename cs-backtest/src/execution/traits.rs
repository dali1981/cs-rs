//! Traits for generic trade execution

use chrono::{DateTime, Utc};
use polars::prelude::DataFrame;
use cs_analytics::IVSurface;
use cs_domain::{EarningsEvent, TradeResult, TradeType, ApplyCosts};
use crate::spread_pricer::PricingError;
use super::types::ExecutionError;
use super::types::{ExecutionConfig, SimulationOutput};
use super::cost_helpers::ToTradingContext;

/// Generic pricing interface for trade pricers
///
/// All trade-specific pricers (StraddlePricer, SpreadPricer, etc.) implement this trait
/// to enable generic execution.
pub trait TradePricer: Send + Sync {
    /// The trade type this pricer handles
    type Trade;

    /// The pricing result type (contains leg prices, IVs, greeks)
    type Pricing;

    /// Price a trade using pre-built IV surface
    ///
    /// # Arguments
    /// * `trade` - The trade to price
    /// * `chain_df` - Option chain data at pricing time
    /// * `spot` - Underlying spot price
    /// * `timestamp` - Pricing timestamp
    /// * `iv_surface` - Pre-built IV surface for interpolation
    fn price_with_surface(
        &self,
        trade: &Self::Trade,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<Self::Pricing, PricingError>;
}

/// Generic execution interface for trade types
///
/// Trades that implement this trait can be executed using the generic execute_trade() function.
/// This trait ties together trade → pricer → pricing → result with validation.
pub trait ExecutableTrade: Sized + Send + Sync {
    /// The pricer type for this trade
    type Pricer: TradePricer<Trade = Self, Pricing = Self::Pricing>;

    /// Pricing output from the pricer (must match Pricer::Pricing)
    /// Clone is needed for cost calculation (we clone pricing to keep original for costs)
    type Pricing: Clone + ToTradingContext;

    /// Final execution result type
    /// ApplyCosts is needed for post-processing cost application
    type Result: TradeResult + ApplyCosts;

    /// Get symbol (for data fetching)
    fn symbol(&self) -> &str;

    /// Get the trade type (for cost calculations)
    fn trade_type() -> TradeType;

    /// Validate entry pricing against config
    ///
    /// Returns Ok(()) if valid, Err with reason if invalid.
    /// Called after entry pricing, before exit pricing.
    fn validate_entry(
        pricing: &Self::Pricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError>;

    /// Construct success result from entry/exit pricing
    ///
    /// Called when both entry and exit pricing succeed.
    /// The `output` contains simulation data (spots, times), while `event` provides
    /// the business context (earnings date/time) - keeping them separate.
    ///
    /// `event` is optional to support non-earnings scenarios like rolling trades.
    ///
    /// NOTE: Results contain GROSS P&L. Trading costs are applied separately via
    /// the `ApplyCosts` trait at the executor level (post-processing pattern).
    fn to_result(
        &self,
        entry_pricing: Self::Pricing,
        exit_pricing: Self::Pricing,
        output: &SimulationOutput,
        event: Option<&EarningsEvent>,
    ) -> Self::Result;

    /// Construct failure result
    ///
    /// Called when execution fails at any point.
    ///
    /// `event` is optional to support non-earnings scenarios like rolling trades.
    fn to_failed_result(
        &self,
        output: &SimulationOutput,
        event: Option<&EarningsEvent>,
        error: ExecutionError,
    ) -> Self::Result;
}
