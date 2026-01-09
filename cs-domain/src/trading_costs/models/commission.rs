//! Commission model
//!
//! Broker fees per contract or per trade.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::trading_costs::{
    TradingCostCalculator, TradingContext, TradingCost, TradingCostBreakdown, TradeSide,
};

/// Commission model
///
/// Broker fees per contract or per trade.
///
/// # Common Structures
///
/// - Per contract: $0.65 per contract
/// - Per contract with cap: $0.65 per contract, max $10 per leg
/// - Tiered: Lower rates for higher volume
#[derive(Debug, Clone)]
pub struct CommissionModel {
    /// Commission per contract
    per_contract: Decimal,

    /// Maximum commission per leg (cap)
    pub max_per_leg: Option<Decimal>,

    /// Minimum commission per order (floor)
    min_per_order: Decimal,

    /// Whether to charge on close (some brokers like Tastytrade charge $0 to close)
    charge_on_close: bool,
}

impl CommissionModel {
    /// Create with per-contract fee
    pub fn new(per_contract: Decimal) -> Self {
        Self {
            per_contract,
            max_per_leg: None,
            min_per_order: Decimal::ZERO,
            charge_on_close: true,
        }
    }

    /// Interactive Brokers-like pricing
    ///
    /// $0.65 per contract, capped at $10 per leg
    pub fn ibkr() -> Self {
        Self {
            per_contract: dec!(0.65),
            max_per_leg: Some(dec!(10.00)),
            min_per_order: dec!(1.00),
            charge_on_close: true,
        }
    }

    /// Tastytrade-like pricing
    ///
    /// $1 to open, $0 to close, capped at $10 per leg
    pub fn tastytrade() -> Self {
        Self {
            per_contract: dec!(1.00),
            max_per_leg: Some(dec!(10.00)),
            min_per_order: Decimal::ZERO,
            charge_on_close: false, // $0 to close!
        }
    }

    /// Zero commission (Robinhood, etc.)
    ///
    /// Note: These brokers may have wider spreads (payment for order flow)
    pub fn zero() -> Self {
        Self::new(Decimal::ZERO)
    }

    /// Schwab/TDAmeritrade-like pricing
    ///
    /// $0.65 per contract, no cap
    pub fn schwab() -> Self {
        Self {
            per_contract: dec!(0.65),
            max_per_leg: None,
            min_per_order: Decimal::ZERO,
            charge_on_close: true,
        }
    }

    /// Get per-contract fee
    pub fn per_contract(&self) -> Decimal {
        self.per_contract
    }

    /// Calculate commission for one side
    fn calculate_side(&self, context: &TradingContext, is_entry: bool) -> Decimal {
        // Check if we charge for this side
        if !is_entry && !self.charge_on_close {
            return Decimal::ZERO;
        }

        let contracts = Decimal::from(context.num_contracts);
        let legs = context.num_legs();

        // Per-leg commission (capped if applicable)
        let per_leg = (self.per_contract * contracts)
            .min(self.max_per_leg.unwrap_or(Decimal::MAX));

        // Total (with minimum)
        (per_leg * Decimal::from(legs as u32))
            .max(self.min_per_order)
    }
}

impl TradingCostCalculator for CommissionModel {
    fn entry_cost(&self, context: &TradingContext) -> TradingCost {
        let total = self.calculate_side(context, true);

        TradingCost {
            total,
            breakdown: TradingCostBreakdown {
                commission: total,
                ..Default::default()
            },
            side: TradeSide::Entry,
        }
    }

    fn exit_cost(&self, context: &TradingContext) -> TradingCost {
        let total = self.calculate_side(context, false);

        TradingCost {
            total,
            breakdown: TradingCostBreakdown {
                commission: total,
                ..Default::default()
            },
            side: TradeSide::Exit,
        }
    }

    fn name(&self) -> &str {
        "Commission"
    }

    fn description(&self) -> &str {
        "Broker commission fees"
    }
}

impl Default for CommissionModel {
    fn default() -> Self {
        Self::ibkr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading_costs::{LegContext, TradeType};
    use chrono::Utc;

    #[test]
    fn test_commission_ibkr() {
        let calc = CommissionModel::ibkr();

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
        // 1 contract * $0.65 per leg (capped at $10) * 2 legs = $1.30
        // Minimum $1.00, so $1.30
        assert_eq!(entry.total, dec!(1.30));
    }

    #[test]
    fn test_commission_ibkr_cap() {
        let calc = CommissionModel::ibkr();

        let mut ctx = TradingContext::new(
            vec![LegContext::long(dec!(2.50), None)],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );
        ctx.num_contracts = 20; // 20 contracts

        let entry = calc.entry_cost(&ctx);
        // 20 contracts * $0.65 = $13.00, capped at $10.00 per leg
        // 1 leg * $10.00 = $10.00
        assert_eq!(entry.total, dec!(10.00));
    }

    #[test]
    fn test_commission_tastytrade() {
        let calc = CommissionModel::tastytrade();

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
        let exit = calc.exit_cost(&ctx);

        // Entry: 1 contract * $1.00 per leg * 2 legs = $2.00
        assert_eq!(entry.total, dec!(2.00));

        // Exit: $0 to close!
        assert_eq!(exit.total, dec!(0.00));
    }

    #[test]
    fn test_commission_zero() {
        let calc = CommissionModel::zero();

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(2.50), None)],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        assert!(calc.entry_cost(&ctx).is_zero());
        assert!(calc.exit_cost(&ctx).is_zero());
    }

    #[test]
    fn test_commission_minimum() {
        let calc = CommissionModel::ibkr();

        let mut ctx = TradingContext::new(
            vec![LegContext::long(dec!(0.10), None)], // Cheap option
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );
        ctx.num_contracts = 1;

        let entry = calc.entry_cost(&ctx);
        // 1 contract * $0.65 = $0.65, but minimum is $1.00
        assert_eq!(entry.total, dec!(1.00));
    }
}
