//! Helper functions for applying trading costs to results (post-processing pattern)
//!
//! This module provides the single point of cost application, eliminating duplication
//! across individual strategy implementations.

use chrono::{DateTime, Utc};
use cs_domain::{ApplyCosts, TradeType, TradingContext, TradingCostCalculator};
use super::types::ExecutionConfig;

/// Trait for pricing types that can generate a TradingContext
pub trait ToTradingContext {
    fn to_trading_context(
        &self,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> TradingContext;
}

/// Apply trading costs to a result using entry/exit pricing
///
/// This is the single point of cost calculation, called from the executor level.
/// The result's P&L is assumed to be gross (before costs).
pub fn apply_costs_to_result<R, P>(
    result: &mut R,
    entry_pricing: &P,
    exit_pricing: &P,
    symbol: &str,
    entry_spot: f64,
    exit_spot: f64,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    trade_type: TradeType,
    config: &ExecutionConfig,
) where
    R: ApplyCosts,
    P: ToTradingContext,
{
    // Build trading contexts
    let entry_ctx = entry_pricing.to_trading_context(symbol, entry_spot, entry_time, trade_type);
    let exit_ctx = exit_pricing.to_trading_context(symbol, exit_spot, exit_time, trade_type);

    // Get cost calculator and calculate costs
    let calculator = config.trading_costs.build();
    let entry_cost = calculator.entry_cost(&entry_ctx);
    let exit_cost = calculator.exit_cost(&exit_ctx);
    let total_cost = entry_cost + exit_cost;

    // Apply costs to result (modifies P&L and sets cost_summary)
    result.apply_costs(total_cost);
}

// ============================================================================
// ToTradingContext implementations for all pricing types
// ============================================================================

// CompositePricing already has to_trading_context() method
impl ToTradingContext for crate::composite_pricer::CompositePricing {
    fn to_trading_context(
        &self,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> TradingContext {
        self.to_trading_context(symbol, spot, time, trade_type)
    }
}

impl ToTradingContext for crate::multi_leg_pricer::StranglePricing {
    fn to_trading_context(
        &self,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> TradingContext {
        self.to_trading_context(symbol, spot, time, trade_type)
    }
}

impl ToTradingContext for crate::multi_leg_pricer::ButterflyPricing {
    fn to_trading_context(
        &self,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> TradingContext {
        self.to_trading_context(symbol, spot, time, trade_type)
    }
}

impl ToTradingContext for crate::multi_leg_pricer::CondorPricing {
    fn to_trading_context(
        &self,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> TradingContext {
        self.to_trading_context(symbol, spot, time, trade_type)
    }
}

impl ToTradingContext for crate::multi_leg_pricer::IronCondorPricing {
    fn to_trading_context(
        &self,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> TradingContext {
        self.to_trading_context(symbol, spot, time, trade_type)
    }
}
