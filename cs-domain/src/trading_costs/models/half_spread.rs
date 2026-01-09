//! Half-spread slippage model
//!
//! The most realistic model - assumes you cross the spread.
//! Buy at ask, sell at bid. Cost = half the bid-ask spread.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::trading_costs::{
    TradingCostCalculator, TradingContext, TradingCost, TradingCostBreakdown, TradeSide,
};
use super::CONTRACT_MULTIPLIER;

/// Half-spread model (most realistic)
///
/// Assumes you cross the spread: buy at ask, sell at bid.
/// Cost = half the bid-ask spread on each side.
///
/// Since historical data typically has mid prices only,
/// we estimate the spread based on configurable percentages.
///
/// # Formula
///
/// - Entry: pay mid + half_spread (buying at ask)
/// - Exit: receive mid - half_spread (selling at bid)
/// - Round-trip cost = full spread
///
/// # Example
///
/// - spread_pct = 4% (bid $2.88, ask $3.12, mid $3.00)
/// - Entry: pay $3.00 + $0.06 = $3.06
/// - Exit: receive $3.00 - $0.06 = $2.94
/// - Round-trip slippage: $0.12 per share = $12 per contract
#[derive(Debug, Clone)]
pub struct HalfSpreadSlippage {
    /// Assumed bid-ask spread as percentage of mid price
    spread_pct: f64,

    /// Minimum spread in dollars (floor)
    min_spread: Decimal,

    /// Contract multiplier
    multiplier: u32,
}

impl HalfSpreadSlippage {
    /// Create with custom spread percentage
    pub fn new(spread_pct: f64) -> Self {
        Self {
            spread_pct,
            min_spread: dec!(0.01), // $0.01 minimum spread
            multiplier: CONTRACT_MULTIPLIER,
        }
    }

    /// Preset: Tight spread (2%)
    pub fn tight() -> Self {
        Self::new(0.02)
    }

    /// Preset: Normal spread (4%)
    pub fn normal() -> Self {
        Self::new(0.04)
    }

    /// Preset: Wide spread (8%)
    pub fn wide() -> Self {
        Self::new(0.08)
    }

    /// Preset: Very wide (12%) - illiquid options
    pub fn illiquid() -> Self {
        Self::new(0.12)
    }

    /// Get spread percentage
    pub fn spread_pct(&self) -> f64 {
        self.spread_pct
    }

    /// Calculate half spread for a given price
    fn half_spread(&self, price: Decimal) -> Decimal {
        let spread_pct_decimal = Decimal::try_from(self.spread_pct).unwrap_or(Decimal::ZERO);
        let full_spread = price.abs() * spread_pct_decimal;
        let spread = full_spread.max(self.min_spread);
        spread / dec!(2)
    }
}

impl TradingCostCalculator for HalfSpreadSlippage {
    fn entry_cost(&self, context: &TradingContext) -> TradingCost {
        // On entry, we cross the spread (pay the half-spread per leg)
        let leg_cost: Decimal = context.legs.iter()
            .map(|leg| self.half_spread(leg.price))
            .sum();

        let total = leg_cost
            * Decimal::from(self.multiplier)
            * Decimal::from(context.num_contracts);

        TradingCost {
            total,
            breakdown: TradingCostBreakdown {
                slippage: total,
                ..Default::default()
            },
            side: TradeSide::Entry,
        }
    }

    fn exit_cost(&self, context: &TradingContext) -> TradingCost {
        // On exit, we also cross the spread
        let mut cost = self.entry_cost(context);
        cost.side = TradeSide::Exit;
        cost
    }

    fn name(&self) -> &str {
        "HalfSpread"
    }

    fn description(&self) -> &str {
        "Half the bid-ask spread (most realistic)"
    }
}

impl Default for HalfSpreadSlippage {
    fn default() -> Self {
        Self::normal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading_costs::{LegContext, TradeType};
    use chrono::Utc;

    #[test]
    fn test_half_spread_single_leg() {
        let calc = HalfSpreadSlippage::new(0.04); // 4% spread

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(3.00), None)],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let entry = calc.entry_cost(&ctx);
        // $3.00 * 4% = $0.12 spread, half = $0.06
        // $0.06 * 100 = $6.00
        assert_eq!(entry.total, dec!(6.00));
    }

    #[test]
    fn test_half_spread_straddle() {
        let calc = HalfSpreadSlippage::normal(); // 4% spread

        let ctx = TradingContext::new(
            vec![
                LegContext::long(dec!(2.50), None), // Call
                LegContext::long(dec!(2.50), None), // Put
            ],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Straddle,
        );

        let entry = calc.entry_cost(&ctx);
        // Per leg: $2.50 * 4% / 2 = $0.05 half-spread
        // 2 legs * $0.05 * 100 = $10.00
        assert_eq!(entry.total, dec!(10.00));
    }

    #[test]
    fn test_round_trip_equals_full_spread() {
        let calc = HalfSpreadSlippage::new(0.04);

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(3.00), None)],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let round_trip = calc.round_trip_cost(&ctx, &ctx);
        // Full spread: $3.00 * 4% = $0.12
        // $0.12 * 100 = $12.00
        assert_eq!(round_trip.total, dec!(12.00));
    }

    #[test]
    fn test_min_spread() {
        let calc = HalfSpreadSlippage::new(0.04);

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(0.05), None)], // Very cheap option
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let entry = calc.entry_cost(&ctx);
        // $0.05 * 4% = $0.002 < $0.01 minimum
        // Half of $0.01 = $0.005 * 100 = $0.50
        assert_eq!(entry.total, dec!(0.50));
    }
}
