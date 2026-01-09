//! IV-based slippage model
//!
//! Higher IV = wider bid-ask spreads = more slippage.
//! Particularly relevant for earnings trades where IV is elevated.

use rust_decimal::Decimal;

use crate::trading_costs::{
    TradingCostCalculator, TradingContext, TradingCost, TradingCostBreakdown, TradeSide,
};
use super::CONTRACT_MULTIPLIER;

/// IV-based slippage model
///
/// Higher IV = wider bid-ask spreads = more slippage.
/// Particularly relevant for earnings trades where IV is elevated.
///
/// # Formula
///
/// ```text
/// spread_pct = base_spread + (iv_multiplier * IV)
/// cost = premium * spread_pct / 2  (half-spread)
/// ```
///
/// # Example
///
/// - base_spread = 2% (0.02)
/// - iv_multiplier = 0.05
/// - IV = 80% (0.80)
/// - spread_pct = 0.02 + (0.05 * 0.80) = 0.06 (6%)
/// - premium = $3.00
/// - half_spread_cost = $3.00 * 6% / 2 = $0.09 per share
#[derive(Debug, Clone)]
pub struct IVBasedSlippage {
    /// Base spread percentage (applied even at 0 IV)
    base_spread_pct: f64,

    /// How much spread widens per unit of IV
    iv_multiplier: f64,

    /// Maximum spread percentage (cap)
    pub max_spread_pct: f64,

    /// Contract multiplier
    multiplier: u32,
}

impl IVBasedSlippage {
    /// Create with custom parameters
    pub fn new(base_spread_pct: f64, iv_multiplier: f64) -> Self {
        Self {
            base_spread_pct,
            iv_multiplier,
            max_spread_pct: 0.20, // 20% max spread
            multiplier: CONTRACT_MULTIPLIER,
        }
    }

    /// Preset: Conservative (tighter spreads)
    ///
    /// Base 1% + 2% per IV unit
    /// At 50% IV: 1% + 2%*0.5 = 2% spread
    pub fn conservative() -> Self {
        Self::new(0.01, 0.02)
    }

    /// Preset: Moderate
    ///
    /// Base 2% + 5% per IV unit
    /// At 50% IV: 2% + 5%*0.5 = 4.5% spread
    pub fn moderate() -> Self {
        Self::new(0.02, 0.05)
    }

    /// Preset: Aggressive (wider spreads, illiquid)
    ///
    /// Base 3% + 10% per IV unit
    /// At 50% IV: 3% + 10%*0.5 = 8% spread
    pub fn aggressive() -> Self {
        Self::new(0.03, 0.10)
    }

    /// Get base spread percentage
    pub fn base_spread_pct(&self) -> f64 {
        self.base_spread_pct
    }

    /// Get IV multiplier
    pub fn iv_multiplier(&self) -> f64 {
        self.iv_multiplier
    }

    /// Calculate spread percentage for a given IV
    fn spread_percentage(&self, iv: f64) -> f64 {
        let spread = self.base_spread_pct + (self.iv_multiplier * iv);
        spread.min(self.max_spread_pct)
    }

    /// Calculate half-spread cost for a leg
    fn half_spread_cost(&self, price: Decimal, iv: f64) -> Decimal {
        let spread_pct = self.spread_percentage(iv);
        let half_spread = spread_pct / 2.0;
        let half_spread_decimal = Decimal::try_from(half_spread).unwrap_or(Decimal::ZERO);
        price.abs() * half_spread_decimal
    }
}

impl TradingCostCalculator for IVBasedSlippage {
    fn entry_cost(&self, context: &TradingContext) -> TradingCost {
        let default_iv = context.avg_iv().unwrap_or(0.30); // Default 30% IV

        // Calculate per-leg costs using leg-specific IV if available
        let leg_cost: Decimal = context.legs.iter()
            .map(|leg| {
                let leg_iv = leg.iv.unwrap_or(default_iv);
                self.half_spread_cost(leg.price, leg_iv)
            })
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
        "IVBased"
    }

    fn description(&self) -> &str {
        "Spread widens with implied volatility"
    }
}

impl Default for IVBasedSlippage {
    fn default() -> Self {
        Self::moderate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading_costs::{LegContext, TradeType};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    #[test]
    fn test_iv_based_low_iv() {
        let calc = IVBasedSlippage::new(0.02, 0.05); // 2% base + 5% per IV

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(3.00), Some(0.20))], // 20% IV
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let entry = calc.entry_cost(&ctx);
        // spread_pct = 2% + 5% * 0.20 = 3%
        // half_spread = 1.5%
        // $3.00 * 1.5% * 100 = $4.50
        assert_eq!(entry.total, dec!(4.50));
    }

    #[test]
    fn test_iv_based_high_iv() {
        let calc = IVBasedSlippage::new(0.02, 0.05); // 2% base + 5% per IV

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(5.00), Some(0.80))], // 80% IV (earnings!)
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let entry = calc.entry_cost(&ctx);
        // spread_pct = 2% + 5% * 0.80 = 6%
        // half_spread = 3%
        // $5.00 * 3% * 100 = $15.00
        assert_eq!(entry.total, dec!(15.00));
    }

    #[test]
    fn test_iv_based_capped() {
        let mut calc = IVBasedSlippage::new(0.02, 0.10); // Would result in very high spread
        calc.max_spread_pct = 0.10; // Cap at 10%

        let ctx = TradingContext::new(
            vec![LegContext::long(dec!(4.00), Some(1.50))], // 150% IV
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Single,
        );

        let entry = calc.entry_cost(&ctx);
        // Uncapped spread = 2% + 10% * 1.50 = 17%
        // Capped spread = 10%
        // half_spread = 5%
        // $4.00 * 5% * 100 = $20.00
        assert_eq!(entry.total, dec!(20.00));
    }

    #[test]
    fn test_iv_based_per_leg_iv() {
        let calc = IVBasedSlippage::moderate();

        // Calendar spread with different IVs
        let ctx = TradingContext::new(
            vec![
                LegContext::long(dec!(3.00), Some(0.40)),  // Far leg, lower IV
                LegContext::short(dec!(2.00), Some(0.80)), // Near leg, higher IV
            ],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::CalendarSpread,
        );

        // Each leg uses its own IV
        let entry = calc.entry_cost(&ctx);
        // Leg 1: 2% + 5%*0.40 = 4% -> half = 2% -> $3.00 * 2% = $0.06
        // Leg 2: 2% + 5%*0.80 = 6% -> half = 3% -> $2.00 * 3% = $0.06
        // Total per share = $0.12
        // * 100 = $12.00
        assert_eq!(entry.total, dec!(12.00));
    }
}
