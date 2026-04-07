//! Trading cost calculator trait
//!
//! Core abstraction for calculating trading costs. All cost models implement this trait.
//! Costs are computed separately from pricing and subtracted from P&L.

use super::{TradingContext, TradingCost};

/// Calculates trading costs for a trade
///
/// This is the core abstraction - all cost models implement this trait.
/// Costs are computed separately from pricing and subtracted from P&L.
///
/// # Design Philosophy
///
/// The calculator operates on TradingContext, which contains all market data
/// needed for cost calculations without coupling to specific pricing implementations.
///
/// # Example
///
/// ```rust,ignore
/// use cs_domain::trading_costs::{TradingCostCalculator, TradingContext};
/// use cs_domain::trading_costs::models::HalfSpreadSlippage;
///
/// let calculator = HalfSpreadSlippage::normal();
/// let entry_cost = calculator.entry_cost(&context);
/// let exit_cost = calculator.exit_cost(&context);
/// let round_trip = entry_cost + exit_cost;
/// ```
pub trait TradingCostCalculator: Send + Sync {
    /// Calculate the cost for entering a position
    ///
    /// # Arguments
    /// * `context` - Market context (prices, IV, etc.)
    ///
    /// # Returns
    /// TradingCost with breakdown
    fn entry_cost(&self, context: &TradingContext) -> TradingCost;

    /// Calculate the cost for exiting a position
    ///
    /// # Arguments
    /// * `context` - Market context at exit time
    ///
    /// # Returns
    /// TradingCost with breakdown
    fn exit_cost(&self, context: &TradingContext) -> TradingCost;

    /// Calculate round-trip cost (entry + exit)
    ///
    /// Default implementation adds entry and exit costs.
    /// Override if round-trip costs have different structure.
    fn round_trip_cost(
        &self,
        entry_context: &TradingContext,
        exit_context: &TradingContext,
    ) -> TradingCost {
        let entry = self.entry_cost(entry_context);
        let exit = self.exit_cost(exit_context);
        entry + exit
    }

    /// Name of this cost model (for logging/display)
    fn name(&self) -> &str;

    /// Description of the cost model
    fn description(&self) -> &str {
        self.name()
    }
}

/// Marker trait for cost calculators that can be cloned
///
/// This allows creating boxed cost calculators that can be cloned
/// for use across multiple threads or contexts.
#[allow(dead_code)]
pub trait ClonableCostCalculator: TradingCostCalculator {
    /// Clone into a boxed trait object
    fn clone_box(&self) -> Box<dyn TradingCostCalculator>;
}

impl<T: TradingCostCalculator + Clone + 'static> ClonableCostCalculator for T {
    fn clone_box(&self) -> Box<dyn TradingCostCalculator> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading_costs::{TradeSide, TradingCostBreakdown, LegContext, TradeType};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    /// Simple test calculator for trait verification
    struct TestCalculator {
        cost_per_leg: rust_decimal::Decimal,
    }

    impl TradingCostCalculator for TestCalculator {
        fn entry_cost(&self, context: &TradingContext) -> TradingCost {
            let cost = self.cost_per_leg * rust_decimal::Decimal::from(context.num_legs() as u32);
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
            "Test"
        }
    }

    #[test]
    fn test_calculator_trait() {
        let calc = TestCalculator { cost_per_leg: dec!(5.00) };

        let ctx = TradingContext::new(
            vec![
                LegContext::long(dec!(2.50), None),
                LegContext::long(dec!(2.50), None),
            ],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Straddle,
        );

        let entry = calc.entry_cost(&ctx);
        assert_eq!(entry.total, dec!(10.00)); // 2 legs * $5
        assert_eq!(entry.side, TradeSide::Entry);

        let exit = calc.exit_cost(&ctx);
        assert_eq!(exit.total, dec!(10.00));
        assert_eq!(exit.side, TradeSide::Exit);

        let round_trip = calc.round_trip_cost(&ctx, &ctx);
        assert_eq!(round_trip.total, dec!(20.00));
    }
}
