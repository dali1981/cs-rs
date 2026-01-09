//! Percentage of premium slippage model
//!
//! Cost scales with the size of the trade premium.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::trading_costs::{
    TradingCostCalculator, TradingContext, TradingCost, TradingCostBreakdown, TradeSide,
};
use super::CONTRACT_MULTIPLIER;

/// Slippage as percentage of premium
///
/// Cost scales with the size of the trade.
/// More realistic for varying premium sizes.
///
/// # Example
///
/// - slippage_bps = 50 (0.50%)
/// - Entry premium = $2.00 per share
/// - Slippage = $2.00 * 0.50% * 100 = $1.00 per contract
#[derive(Debug, Clone)]
pub struct PercentageOfPremiumSlippage {
    /// Slippage in basis points (1 bp = 0.01%)
    slippage_bps: u32,

    /// Minimum cost per leg (floor)
    pub min_cost_per_leg: Decimal,

    /// Maximum cost per leg (cap)
    pub max_cost_per_leg: Option<Decimal>,

    /// Contract multiplier
    multiplier: u32,
}

impl PercentageOfPremiumSlippage {
    /// Create with slippage in basis points
    pub fn new(slippage_bps: u32) -> Self {
        Self {
            slippage_bps,
            min_cost_per_leg: dec!(0.01), // $0.01 minimum
            max_cost_per_leg: None,
            multiplier: CONTRACT_MULTIPLIER,
        }
    }

    /// Create with bounds
    pub fn with_bounds(slippage_bps: u32, min: Decimal, max: Decimal) -> Self {
        Self {
            slippage_bps,
            min_cost_per_leg: min,
            max_cost_per_leg: Some(max),
            multiplier: CONTRACT_MULTIPLIER,
        }
    }

    /// Preset: 25 bps (tight)
    pub fn tight() -> Self {
        Self::new(25)
    }

    /// Preset: 50 bps (normal)
    pub fn normal() -> Self {
        Self::new(50)
    }

    /// Preset: 100 bps (wide)
    pub fn wide() -> Self {
        Self::new(100)
    }

    /// Get slippage in basis points
    pub fn slippage_bps(&self) -> u32 {
        self.slippage_bps
    }

    /// Calculate cost for a single leg
    fn calculate_leg_cost(&self, leg_price: Decimal) -> Decimal {
        let pct = Decimal::from(self.slippage_bps) / dec!(10000);
        let cost = (leg_price.abs() * pct).max(self.min_cost_per_leg);

        match self.max_cost_per_leg {
            Some(max) => cost.min(max),
            None => cost,
        }
    }
}

impl TradingCostCalculator for PercentageOfPremiumSlippage {
    fn entry_cost(&self, context: &TradingContext) -> TradingCost {
        // Sum cost across all legs
        let leg_cost: Decimal = context.legs.iter()
            .map(|leg| self.calculate_leg_cost(leg.price))
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
        let mut cost = self.entry_cost(context);
        cost.side = TradeSide::Exit;
        cost
    }

    fn name(&self) -> &str {
        "PercentageOfPremium"
    }

    fn description(&self) -> &str {
        "Slippage as percentage of premium"
    }
}

impl Default for PercentageOfPremiumSlippage {
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
    fn test_percentage_slippage() {
        let calc = PercentageOfPremiumSlippage::new(100); // 1%

        let ctx = TradingContext::new(
            vec![
                LegContext::long(dec!(2.00), None), // $2.00 call
                LegContext::long(dec!(2.00), None), // $2.00 put
            ],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Straddle,
        );

        let entry = calc.entry_cost(&ctx);
        // 2 legs * $2.00 * 1% * 100 = $4.00
        assert_eq!(entry.total, dec!(4.00));
    }

    #[test]
    fn test_percentage_with_min() {
        let calc = PercentageOfPremiumSlippage::new(100); // 1%

        let ctx = TradingContext::new(
            vec![
                LegContext::long(dec!(0.05), None), // Very cheap option
            ],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let entry = calc.entry_cost(&ctx);
        // $0.05 * 1% = $0.0005, but min is $0.01
        // $0.01 * 100 = $1.00
        assert_eq!(entry.total, dec!(1.00));
    }

    #[test]
    fn test_percentage_with_max() {
        let calc = PercentageOfPremiumSlippage::with_bounds(100, dec!(0.01), dec!(0.10));

        let ctx = TradingContext::new(
            vec![
                LegContext::long(dec!(20.00), None), // Expensive option
            ],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let entry = calc.entry_cost(&ctx);
        // $20.00 * 1% = $0.20, but max is $0.10
        // $0.10 * 100 = $10.00
        assert_eq!(entry.total, dec!(10.00));
    }
}
