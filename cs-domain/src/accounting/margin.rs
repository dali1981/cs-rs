//! Margin Calculator
//!
//! Implements CBOE strategy-based margin rules for options and Reg-T
//! margin rules for equities.
//!
//! # References
//! - [CBOE Strategy-based Margin](https://www.cboe.com/us/options/strategy_based_margin)
//! - [CBOE Margin Manual](https://cdn.cboe.com/resources/membership/Margin_Manual.pdf)

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use super::CapitalRequirement;

/// Margin calculation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarginCalculator {
    /// Stock margin requirement for long positions (default 50% for Reg-T)
    pub stock_margin_long: Decimal,

    /// Stock margin requirement for short positions (default 150% for Reg-T)
    pub stock_margin_short: Decimal,

    /// Use portfolio margin rules (typically lower requirements)
    pub use_portfolio_margin: bool,

    /// Minimum option margin (floor value)
    pub min_option_margin: Decimal,
}

impl Default for MarginCalculator {
    fn default() -> Self {
        Self {
            stock_margin_long: dec!(0.50),   // 50% for Reg-T long
            stock_margin_short: dec!(1.50),  // 150% for Reg-T short
            use_portfolio_margin: false,
            min_option_margin: dec!(50),     // $50 minimum per contract
        }
    }
}

impl MarginCalculator {
    /// Create a calculator with full cash (no margin)
    pub fn cash() -> Self {
        Self {
            stock_margin_long: dec!(1.0),    // 100% for cash
            stock_margin_short: dec!(1.0),   // Not allowed, but 100% if it were
            use_portfolio_margin: false,
            min_option_margin: dec!(50),
        }
    }

    /// Create a calculator with Reg-T margin
    pub fn reg_t() -> Self {
        Self::default()
    }

    /// Create a calculator with portfolio margin
    pub fn portfolio_margin() -> Self {
        Self {
            stock_margin_long: dec!(0.15),   // ~15% for PM
            stock_margin_short: dec!(0.30),  // ~30% for PM
            use_portfolio_margin: true,
            min_option_margin: dec!(25),
        }
    }

    /// Calculate margin for a long option position
    ///
    /// CBOE Rules:
    /// - Options with DTE <= 9 months: 100% of premium
    /// - Options with DTE > 9 months: 75% of premium
    pub fn long_option_margin(&self, premium: Decimal, dte: u32) -> Decimal {
        if dte <= 270 {
            // 9 months (270 days) or less: full premium
            premium
        } else {
            // More than 9 months: 75% of premium
            premium * dec!(0.75)
        }
    }

    /// Calculate margin for a naked short option (equity)
    ///
    /// CBOE Formula:
    /// - 100% of option proceeds + 20% of underlying - OTM amount
    /// - Minimum: 10% of underlying + premium
    pub fn naked_short_equity_margin(
        &self,
        premium: Decimal,
        underlying_price: Decimal,
        strike: Decimal,
        is_call: bool,
    ) -> Decimal {
        let otm_amount = if is_call {
            (strike - underlying_price).max(Decimal::ZERO)
        } else {
            (underlying_price - strike).max(Decimal::ZERO)
        };

        // Standard calculation: 100% + 20% - OTM
        let standard = premium + (underlying_price * dec!(0.20)) - otm_amount;

        // Minimum floor: 10% + premium
        let minimum = (underlying_price * dec!(0.10)) + premium;

        // Use the greater of standard or minimum
        standard.max(minimum).max(self.min_option_margin)
    }

    /// Calculate margin for a naked short option (broad-based index)
    ///
    /// CBOE Formula:
    /// - 100% of option proceeds + 15% of underlying - OTM amount
    /// - Minimum: 10% of underlying + premium
    pub fn naked_short_index_margin(
        &self,
        premium: Decimal,
        underlying_price: Decimal,
        strike: Decimal,
        is_call: bool,
    ) -> Decimal {
        let otm_amount = if is_call {
            (strike - underlying_price).max(Decimal::ZERO)
        } else {
            (underlying_price - strike).max(Decimal::ZERO)
        };

        // Standard calculation: 100% + 15% - OTM
        let standard = premium + (underlying_price * dec!(0.15)) - otm_amount;

        // Minimum floor: 10% + premium
        let minimum = (underlying_price * dec!(0.10)) + premium;

        standard.max(minimum).max(self.min_option_margin)
    }

    /// Calculate margin for a debit spread
    ///
    /// Requirement: Pay the net debit in full
    pub fn debit_spread_margin(&self, net_debit: Decimal) -> Decimal {
        net_debit.abs()
    }

    /// Calculate margin for a credit spread
    ///
    /// Requirement: Max loss - credit received
    pub fn credit_spread_margin(&self, credit_received: Decimal, max_loss: Decimal) -> Decimal {
        (max_loss - credit_received).max(Decimal::ZERO)
    }

    /// Calculate margin for a straddle (long)
    ///
    /// Requirement: Sum of both leg premiums
    pub fn long_straddle_margin(&self, call_premium: Decimal, put_premium: Decimal) -> Decimal {
        call_premium + put_premium
    }

    /// Calculate margin for a straddle (short)
    ///
    /// Requirement: Greater of call or put naked margin + the other leg's premium
    pub fn short_straddle_margin(
        &self,
        call_premium: Decimal,
        put_premium: Decimal,
        underlying_price: Decimal,
        strike: Decimal,
    ) -> Decimal {
        let call_margin = self.naked_short_equity_margin(
            call_premium,
            underlying_price,
            strike,
            true,
        );
        let put_margin = self.naked_short_equity_margin(
            put_premium,
            underlying_price,
            strike,
            false,
        );

        // Greater of (call margin + put premium) or (put margin + call premium)
        (call_margin + put_premium).max(put_margin + call_premium)
    }

    /// Calculate margin for an iron butterfly (credit strategy)
    ///
    /// Requirement: Width of the wider wing - net credit
    /// Iron butterfly is a defined-risk strategy
    pub fn iron_butterfly_margin(
        &self,
        net_credit: Decimal,
        wing_width: Decimal,
    ) -> Decimal {
        // Max loss = wing width - credit
        (wing_width - net_credit).max(Decimal::ZERO)
    }

    /// Calculate margin for an iron condor (credit strategy)
    ///
    /// Requirement: Width of the wider wing - net credit
    pub fn iron_condor_margin(
        &self,
        net_credit: Decimal,
        put_wing_width: Decimal,
        call_wing_width: Decimal,
    ) -> Decimal {
        // Max loss = max(put wing, call wing) - credit
        let max_wing = put_wing_width.max(call_wing_width);
        (max_wing - net_credit).max(Decimal::ZERO)
    }

    /// Calculate capital for a stock hedge position
    ///
    /// Uses Reg-T margin rates by default
    pub fn stock_hedge_capital(&self, shares: i32, price: Decimal) -> Decimal {
        let notional = Decimal::from(shares.abs()) * price;
        if shares > 0 {
            // Long stock: use long margin rate
            notional * self.stock_margin_long
        } else {
            // Short stock: use short margin rate
            notional * self.stock_margin_short
        }
    }

    /// Calculate capital requirement for a complete hedged position
    ///
    /// Combines option and stock hedge requirements
    pub fn hedged_position_capital(
        &self,
        option_margin: Decimal,
        hedge_shares: i32,
        stock_price: Decimal,
    ) -> CapitalRequirement {
        let hedge_capital = self.stock_hedge_capital(hedge_shares, stock_price);
        CapitalRequirement::for_debit(option_margin).with_hedge(hedge_capital)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_long_option_margin() {
        let calc = MarginCalculator::default();

        // Short-term option (< 9 months): 100% premium
        assert_eq!(calc.long_option_margin(dec!(150), 30), dec!(150));
        assert_eq!(calc.long_option_margin(dec!(150), 270), dec!(150));

        // Long-term option (> 9 months): 75% premium
        assert_eq!(calc.long_option_margin(dec!(200), 365), dec!(150));
    }

    #[test]
    fn test_naked_short_margin() {
        let calc = MarginCalculator::default();

        // ATM call: $5 premium, $100 stock, $100 strike
        // Standard = $5 + (0.20 * $100) - $0 = $25
        // Minimum = (0.10 * $100) + $5 = $15
        // Result = max($25, $15) = $25
        let margin = calc.naked_short_equity_margin(
            dec!(5),
            dec!(100),
            dec!(100),
            true,
        );
        assert_eq!(margin, dec!(25));

        // OTM call: $2 premium, $100 stock, $110 strike (10 OTM)
        // Standard = $2 + $20 - $10 = $12
        // Minimum = $10 + $2 = $12
        let margin = calc.naked_short_equity_margin(
            dec!(2),
            dec!(100),
            dec!(110),
            true,
        );
        assert_eq!(margin, dec!(12));
    }

    #[test]
    fn test_spread_margin() {
        let calc = MarginCalculator::default();

        // Debit spread: pay $1.50
        assert_eq!(calc.debit_spread_margin(dec!(1.50)), dec!(1.50));

        // Credit spread: receive $1.50, max loss $5.00
        // Margin = $5.00 - $1.50 = $3.50
        assert_eq!(calc.credit_spread_margin(dec!(1.50), dec!(5.00)), dec!(3.50));
    }

    #[test]
    fn test_iron_butterfly_margin() {
        let calc = MarginCalculator::default();

        // Iron butterfly: $2.00 credit, $5.00 wing width
        // Margin = $5.00 - $2.00 = $3.00
        assert_eq!(calc.iron_butterfly_margin(dec!(2.00), dec!(5.00)), dec!(3.00));
    }

    #[test]
    fn test_stock_hedge_capital() {
        let calc = MarginCalculator::default();

        // Long 100 shares at $50 = $5000 notional
        // Reg-T margin = 50% = $2500
        assert_eq!(calc.stock_hedge_capital(100, dec!(50)), dec!(2500));

        // Short 100 shares at $50 = $5000 notional
        // Reg-T margin = 150% = $7500
        assert_eq!(calc.stock_hedge_capital(-100, dec!(50)), dec!(7500));
    }

    #[test]
    fn test_hedged_position() {
        let calc = MarginCalculator::default();

        // Option: $150 debit
        // Hedge: long 50 shares at $100 = $5000 * 50% = $2500
        let cap = calc.hedged_position_capital(dec!(150), 50, dec!(100));

        assert_eq!(cap.initial_requirement, dec!(2650));
        assert_eq!(cap.breakdown.option_premium, dec!(150));
        assert_eq!(cap.breakdown.hedge_capital, dec!(2500));
    }
}
