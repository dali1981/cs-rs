//! Composite cost calculator
//!
//! Combines multiple cost calculators (slippage + commission + market impact).

use crate::trading_costs::{
    TradingCostCalculator, TradingContext, TradingCost,
};
use super::{HalfSpreadSlippage, CommissionModel};

/// Combines multiple cost calculators
///
/// Allows stacking slippage + commission + market impact.
///
/// # Example
///
/// ```rust,ignore
/// use cs_domain::trading_costs::models::{
///     CompositeCostCalculator, HalfSpreadSlippage, CommissionModel
/// };
///
/// let calculator = CompositeCostCalculator::new()
///     .with(HalfSpreadSlippage::normal())
///     .with(CommissionModel::ibkr());
/// ```
pub struct CompositeCostCalculator {
    calculators: Vec<Box<dyn TradingCostCalculator>>,
}

impl CompositeCostCalculator {
    /// Create an empty composite calculator
    pub fn new() -> Self {
        Self { calculators: Vec::new() }
    }

    /// Add a calculator
    pub fn with<C: TradingCostCalculator + 'static>(mut self, calc: C) -> Self {
        self.calculators.push(Box::new(calc));
        self
    }

    /// Add a boxed calculator
    pub fn with_boxed(mut self, calc: Box<dyn TradingCostCalculator>) -> Self {
        self.calculators.push(calc);
        self
    }

    /// Common preset: Slippage + Commission (realistic trading)
    ///
    /// Uses 4% half-spread slippage + IBKR commissions
    pub fn realistic() -> Self {
        Self::new()
            .with(HalfSpreadSlippage::normal())
            .with(CommissionModel::ibkr())
    }

    /// Slippage only (no commission)
    pub fn slippage_only() -> Self {
        Self::new()
            .with(HalfSpreadSlippage::normal())
    }

    /// IBKR-like costs (slippage + commission)
    pub fn ibkr() -> Self {
        Self::new()
            .with(HalfSpreadSlippage::normal())
            .with(CommissionModel::ibkr())
    }

    /// Tastytrade-like costs
    pub fn tastytrade() -> Self {
        Self::new()
            .with(HalfSpreadSlippage::normal())
            .with(CommissionModel::tastytrade())
    }

    /// Tight market conditions
    pub fn tight() -> Self {
        Self::new()
            .with(HalfSpreadSlippage::tight())
            .with(CommissionModel::ibkr())
    }

    /// Wide market conditions (illiquid)
    pub fn wide() -> Self {
        Self::new()
            .with(HalfSpreadSlippage::wide())
            .with(CommissionModel::ibkr())
    }

    /// Number of calculators in the composite
    pub fn len(&self) -> usize {
        self.calculators.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.calculators.is_empty()
    }
}

impl TradingCostCalculator for CompositeCostCalculator {
    fn entry_cost(&self, context: &TradingContext) -> TradingCost {
        self.calculators.iter()
            .map(|c| c.entry_cost(context))
            .fold(TradingCost::default(), |acc, cost| acc + cost)
    }

    fn exit_cost(&self, context: &TradingContext) -> TradingCost {
        self.calculators.iter()
            .map(|c| c.exit_cost(context))
            .fold(TradingCost::default(), |acc, cost| acc + cost)
    }

    fn name(&self) -> &str {
        "Composite"
    }

    fn description(&self) -> &str {
        "Combined cost models (slippage + commission)"
    }
}

impl Default for CompositeCostCalculator {
    fn default() -> Self {
        Self::realistic()
    }
}

// Debug implementation
impl std::fmt::Debug for CompositeCostCalculator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeCostCalculator")
            .field("num_calculators", &self.calculators.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading_costs::{LegContext, TradeType, TradeSide};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    #[test]
    fn test_composite_realistic() {
        let calc = CompositeCostCalculator::realistic();

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

        // HalfSpread (4%): 2 legs * $2.50 * 4% / 2 * 100 = $10.00
        // Commission (IBKR): 2 legs * 1 contract * $0.65 = $1.30
        // Total: $11.30
        assert_eq!(entry.total, dec!(11.30));
        assert_eq!(entry.breakdown.slippage, dec!(10.00));
        assert_eq!(entry.breakdown.commission, dec!(1.30));
    }

    #[test]
    fn test_composite_round_trip() {
        let calc = CompositeCostCalculator::realistic();

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(3.00), None)],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let round_trip = calc.round_trip_cost(&ctx, &ctx);

        // HalfSpread (4%): $3.00 * 4% / 2 * 100 * 2 = $12.00
        // Commission (IBKR): min($1.00) * 2 = $2.00
        // Total: $14.00
        assert_eq!(round_trip.total, dec!(14.00));
        assert_eq!(round_trip.breakdown.slippage, dec!(12.00));
        assert_eq!(round_trip.breakdown.commission, dec!(2.00));
        assert_eq!(round_trip.side, TradeSide::RoundTrip);
    }

    #[test]
    fn test_composite_tastytrade() {
        let calc = CompositeCostCalculator::tastytrade();

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(3.00), None)],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let entry = calc.entry_cost(&ctx);
        let exit = calc.exit_cost(&ctx);

        // Entry slippage: $3.00 * 4% / 2 * 100 = $6.00
        // Entry commission: 1 contract * $1.00 = $1.00
        assert_eq!(entry.total, dec!(7.00));

        // Exit slippage: $6.00
        // Exit commission: $0 (Tastytrade is $0 to close)
        assert_eq!(exit.total, dec!(6.00));
        assert_eq!(exit.breakdown.commission, dec!(0.00));
    }

    #[test]
    fn test_empty_composite() {
        let calc = CompositeCostCalculator::new(); // Empty!

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(3.00), None)],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        assert!(calc.entry_cost(&ctx).is_zero());
    }
}
