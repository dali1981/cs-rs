//! Traits for generic trade execution

use chrono::{DateTime, Utc};
use polars::prelude::DataFrame;
use cs_analytics::IVSurface;
use cs_domain::TradeResult;
use crate::spread_pricer::PricingError;
use crate::trade_executor::ExecutionError;
use super::types::{ExecutionConfig, ExecutionContext};

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
    type Pricing;

    /// Final execution result type
    type Result: TradeResult;

    /// Get symbol (for data fetching)
    fn symbol(&self) -> &str;

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
    fn to_result(
        &self,
        entry_pricing: Self::Pricing,
        exit_pricing: Self::Pricing,
        ctx: &ExecutionContext,
    ) -> Self::Result;

    /// Construct failure result
    ///
    /// Called when execution fails at any point.
    fn to_failed_result(
        &self,
        ctx: &ExecutionContext,
        error: ExecutionError,
    ) -> Self::Result;
}
