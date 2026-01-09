//! Trade PnL record with proper capital and duration tracking
//!
//! Implements sections 1-4 of the PnL computation spec.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A complete PnL record for a single trade.
///
/// This captures all components needed for normalized return computation:
/// - Option leg: premium paid, realized PnL
/// - Hedge leg: cumulative hedge PnL, cumulative hedge costs
/// - Capital: peak capital deployed (max over trade lifetime)
/// - Duration: trade duration in calendar days
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradePnlRecord {
    /// Option premium paid (C_opt) or max loss for spreads
    pub option_premium: Decimal,

    /// Realized PnL from the option leg
    pub option_pnl: Decimal,

    /// Cumulative PnL from hedge trades (Σ Hedge_PnL_t)
    pub hedge_pnl: Decimal,

    /// Cumulative transaction costs from hedge rebalances (Σ HedgeCost_t)
    pub hedge_costs: Decimal,

    /// Peak capital deployed during the trade: max_t(C_opt + Hedge_Capital_t)
    /// This is the denominator for normalized return calculation.
    pub peak_capital: Decimal,

    /// Trade duration in calendar days (T_i)
    pub duration_days: i64,
}

impl TradePnlRecord {
    /// Create a new trade PnL record.
    pub fn new(
        option_premium: Decimal,
        option_pnl: Decimal,
        hedge_pnl: Decimal,
        hedge_costs: Decimal,
        peak_capital: Decimal,
        duration_days: i64,
    ) -> Self {
        Self {
            option_premium,
            option_pnl,
            hedge_pnl,
            hedge_costs,
            peak_capital,
            duration_days,
        }
    }

    /// Create a record for an unhedged trade (no hedge leg).
    pub fn unhedged(option_premium: Decimal, option_pnl: Decimal, duration_days: i64) -> Self {
        Self {
            option_premium,
            option_pnl,
            hedge_pnl: Decimal::ZERO,
            hedge_costs: Decimal::ZERO,
            peak_capital: option_premium,
            duration_days,
        }
    }

    /// Total PnL including all components (spec section 2).
    ///
    /// `Total_PnL = Option_PnL + Σ Hedge_PnL_t - Σ HedgeCost_t`
    pub fn total_pnl(&self) -> Decimal {
        self.option_pnl + self.hedge_pnl - self.hedge_costs
    }

    /// Normalized return per trade (spec section 4).
    ///
    /// `r_i = Total_PnL_i / Capital_i`
    ///
    /// Returns 0.0 if peak_capital is zero.
    pub fn normalized_return(&self) -> f64 {
        if self.peak_capital.is_zero() {
            return 0.0;
        }
        let r: f64 = (self.total_pnl() / self.peak_capital)
            .try_into()
            .unwrap_or(0.0);
        r
    }

    /// Daily-equivalent return (spec section 5).
    ///
    /// `r_i_daily = (1 + r_i)^(1/T_i) - 1`
    ///
    /// This normalizes returns to a daily basis for cross-trade comparability.
    /// Returns the raw return if duration is <= 1 day.
    pub fn daily_return(&self) -> f64 {
        let r = self.normalized_return();
        let days = self.duration_days;

        if days <= 1 {
            return r;
        }

        // (1 + r)^(1/T) - 1
        // Handle negative returns: if r < -1, the trade lost more than 100%
        let base = 1.0 + r;
        if base <= 0.0 {
            // Total loss or worse - return the raw return
            // This is a degenerate case (lost more than capital)
            return r;
        }

        base.powf(1.0 / days as f64) - 1.0
    }

    /// Hedge cost ratio diagnostic (spec section 8).
    ///
    /// `HedgeCostRatio = Σ HedgeCost_t / C_opt`
    ///
    /// High values (>30-40%) indicate hedge friction destroying edge.
    /// Returns 0.0 if option_premium is zero.
    pub fn hedge_cost_ratio(&self) -> f64 {
        if self.option_premium.is_zero() {
            return 0.0;
        }
        let ratio: f64 = (self.hedge_costs / self.option_premium)
            .try_into()
            .unwrap_or(0.0);
        ratio
    }

    /// Check if hedge costs are excessive (>30% of option premium).
    pub fn has_excessive_hedge_costs(&self) -> bool {
        self.hedge_cost_ratio() > 0.30
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_total_pnl() {
        let record = TradePnlRecord::new(
            dec!(100),  // option premium
            dec!(20),   // option pnl (+$20)
            dec!(5),    // hedge pnl (+$5)
            dec!(3),    // hedge costs (-$3)
            dec!(150),  // peak capital
            10,         // 10 days
        );

        // Total = 20 + 5 - 3 = 22
        assert_eq!(record.total_pnl(), dec!(22));
    }

    #[test]
    fn test_normalized_return() {
        let record = TradePnlRecord::new(
            dec!(100),
            dec!(15),   // +$15 option pnl
            dec!(0),    // no hedge pnl
            dec!(0),    // no hedge costs
            dec!(100),  // peak capital = premium (unhedged)
            5,
        );

        // r = 15 / 100 = 0.15 (15%)
        assert!((record.normalized_return() - 0.15).abs() < 0.001);
    }

    #[test]
    fn test_daily_return() {
        let record = TradePnlRecord::new(
            dec!(100),
            dec!(10),   // +10% total return
            dec!(0),
            dec!(0),
            dec!(100),
            10,         // 10 days
        );

        // r_daily = (1.10)^(1/10) - 1 ≈ 0.00957 (0.957% per day)
        let daily = record.daily_return();
        assert!((daily - 0.00957).abs() < 0.001);

        // Verify: compounding back should give ~10%
        let compounded = (1.0 + daily).powi(10) - 1.0;
        assert!((compounded - 0.10).abs() < 0.001);
    }

    #[test]
    fn test_daily_return_single_day() {
        let record = TradePnlRecord::unhedged(dec!(100), dec!(5), 1);

        // For 1-day trade, daily return = raw return
        assert!((record.daily_return() - 0.05).abs() < 0.001);
    }

    #[test]
    fn test_hedge_cost_ratio() {
        let record = TradePnlRecord::new(
            dec!(100),  // option premium
            dec!(10),
            dec!(5),
            dec!(35),   // hedge costs = 35% of premium (excessive!)
            dec!(150),
            10,
        );

        assert!((record.hedge_cost_ratio() - 0.35).abs() < 0.001);
        assert!(record.has_excessive_hedge_costs());
    }

    #[test]
    fn test_unhedged_trade() {
        let record = TradePnlRecord::unhedged(dec!(200), dec!(40), 7);

        assert_eq!(record.hedge_pnl, Decimal::ZERO);
        assert_eq!(record.hedge_costs, Decimal::ZERO);
        assert_eq!(record.peak_capital, dec!(200));
        assert_eq!(record.total_pnl(), dec!(40));
        assert!((record.normalized_return() - 0.20).abs() < 0.001);
    }

    #[test]
    fn test_negative_return() {
        let record = TradePnlRecord::new(
            dec!(100),
            dec!(-30),  // -30% loss
            dec!(0),
            dec!(0),
            dec!(100),
            5,
        );

        // Normalized return = -0.30
        assert!((record.normalized_return() - (-0.30)).abs() < 0.001);

        // Daily return = (0.70)^(1/5) - 1 ≈ -0.069 (-6.9% per day)
        let daily = record.daily_return();
        assert!(daily < 0.0);

        // Verify compounding
        let compounded = (1.0 + daily).powi(5) - 1.0;
        assert!((compounded - (-0.30)).abs() < 0.001);
    }
}
