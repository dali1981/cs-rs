//! Fixed per-leg slippage model
//!
//! Simple model: each leg costs a fixed amount to trade.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::trading_costs::{
    TradingCostCalculator, TradingContext, TradingCost, TradingCostBreakdown, TradeSide,
};
use super::CONTRACT_MULTIPLIER;

/// Fixed cost per leg
///
/// Simple model: each leg costs a fixed amount to trade.
/// Good baseline for liquid options.
///
/// # Example
///
/// - cost_per_leg = $0.02 per share
/// - Straddle (2 legs): $0.02 * 2 * 100 = $4.00 per contract
/// - Iron Butterfly (4 legs): $0.02 * 4 * 100 = $8.00 per contract
#[derive(Debug, Clone)]
pub struct FixedPerLegSlippage {
    /// Cost per leg per share (e.g., $0.02)
    cost_per_leg: Decimal,

    /// Contract multiplier (typically 100)
    multiplier: u32,
}

impl FixedPerLegSlippage {
    /// Create with custom cost per leg (per share)
    pub fn new(cost_per_leg: Decimal) -> Self {
        Self {
            cost_per_leg,
            multiplier: CONTRACT_MULTIPLIER,
        }
    }

    /// Common preset: $0.01 per leg (tight markets)
    pub fn tight() -> Self {
        Self::new(dec!(0.01))
    }

    /// Common preset: $0.02 per leg (normal markets)
    pub fn normal() -> Self {
        Self::new(dec!(0.02))
    }

    /// Common preset: $0.05 per leg (wide markets / illiquid)
    pub fn wide() -> Self {
        Self::new(dec!(0.05))
    }

    /// Get the cost per leg
    pub fn cost_per_leg(&self) -> Decimal {
        self.cost_per_leg
    }
}

impl TradingCostCalculator for FixedPerLegSlippage {
    fn entry_cost(&self, context: &TradingContext) -> TradingCost {
        let cost = self.cost_per_leg
            * Decimal::from(context.num_legs() as u32)
            * Decimal::from(self.multiplier)
            * Decimal::from(context.num_contracts);

        TradingCost {
            total: cost,
            breakdown: TradingCostBreakdown {
                slippage: cost,
                ..Default::default()
            },
            side: TradeSide::Entry,
        }
    }

    fn exit_cost(&self, context: &TradingContext) -> TradingCost {
        let mut cost = self.entry_cost(context);
        cost.side = TradeSide::Exit;
        cost
    }

    fn name(&self) -> &str {
        "FixedPerLeg"
    }

    fn description(&self) -> &str {
        "Fixed dollar amount per leg"
    }
}

impl Default for FixedPerLegSlippage {
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
    fn test_fixed_per_leg_straddle() {
        let calc = FixedPerLegSlippage::normal(); // $0.02 per leg

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
        // 2 legs * $0.02 * 100 = $4.00
        assert_eq!(entry.total, dec!(4.00));
    }

    #[test]
    fn test_fixed_per_leg_iron_butterfly() {
        let calc = FixedPerLegSlippage::normal(); // $0.02 per leg

        let ctx = TradingContext::new(
            vec![
                LegContext::long(dec!(1.00), None),  // Long put wing
                LegContext::short(dec!(2.50), None), // Short put
                LegContext::short(dec!(2.50), None), // Short call
                LegContext::long(dec!(1.00), None),  // Long call wing
            ],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::IronButterfly,
        );

        let entry = calc.entry_cost(&ctx);
        // 4 legs * $0.02 * 100 = $8.00
        assert_eq!(entry.total, dec!(8.00));
    }

    #[test]
    fn test_round_trip() {
        let calc = FixedPerLegSlippage::normal();

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(2.50), None)],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let round_trip = calc.round_trip_cost(&ctx, &ctx);
        // 1 leg * $0.02 * 100 * 2 (entry + exit) = $4.00
        assert_eq!(round_trip.total, dec!(4.00));
    }
}
