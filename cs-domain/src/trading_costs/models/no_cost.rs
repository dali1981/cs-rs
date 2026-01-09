//! No-cost model (null object pattern)
//!
//! Use when you want to disable costs without changing code structure.

use crate::trading_costs::{TradingCostCalculator, TradingContext, TradingCost};

/// No trading costs (null object pattern)
///
/// Use when you want to disable costs without changing code.
/// Returns zero for all cost calculations.
///
/// # Example
///
/// ```rust,ignore
/// let calculator = NoCost;
/// let cost = calculator.entry_cost(&context);
/// assert!(cost.is_zero());
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCost;

impl TradingCostCalculator for NoCost {
    fn entry_cost(&self, _context: &TradingContext) -> TradingCost {
        TradingCost::zero()
    }

    fn exit_cost(&self, _context: &TradingContext) -> TradingCost {
        TradingCost::zero()
    }

    fn name(&self) -> &str {
        "NoCost"
    }

    fn description(&self) -> &str {
        "No trading costs (disabled)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading_costs::{LegContext, TradeType};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    #[test]
    fn test_no_cost() {
        let calc = NoCost;

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(5.00), Some(0.40))],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        assert!(calc.entry_cost(&ctx).is_zero());
        assert!(calc.exit_cost(&ctx).is_zero());
    }
}
