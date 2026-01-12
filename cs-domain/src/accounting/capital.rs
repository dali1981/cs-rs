//! Capital requirement types for options trading
//!
//! Tracks the capital/margin required to enter and maintain a position.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Method used to calculate capital requirements
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapitalCalculationMethod {
    /// Full debit for long options (100% of premium)
    LongOptionDebit,

    /// Long-term option margin (75% of premium for > 9 months DTE)
    LongTermOptionMargin,

    /// Strategy-based margin for spreads (CBOE rules)
    StrategyBasedMargin,

    /// Reg-T margin for equities (50% long, 150% short)
    RegTMargin,

    /// Portfolio margin (risk-based, typically lower)
    PortfolioMargin,

    /// Credit received (for credit spreads, net credit acts as partial offset)
    CreditReceived,
}

impl Default for CapitalCalculationMethod {
    fn default() -> Self {
        Self::LongOptionDebit
    }
}

/// Breakdown of capital requirements by component
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapitalBreakdown {
    /// Option premium (positive = debit paid, negative = credit received)
    pub option_premium: Decimal,

    /// Stock hedge capital required (for delta hedging)
    pub hedge_capital: Decimal,

    /// Additional margin for short positions
    pub short_margin: Decimal,

    /// Total buying power reduction
    pub total_bpr: Decimal,
}

impl CapitalBreakdown {
    /// Create a simple debit trade breakdown
    pub fn debit(premium: Decimal) -> Self {
        Self {
            option_premium: premium,
            hedge_capital: Decimal::ZERO,
            short_margin: Decimal::ZERO,
            total_bpr: premium,
        }
    }

    /// Create a credit trade breakdown with margin
    pub fn credit(credit_received: Decimal, margin_required: Decimal) -> Self {
        Self {
            option_premium: -credit_received, // Negative = credit
            hedge_capital: Decimal::ZERO,
            short_margin: margin_required,
            total_bpr: margin_required,
        }
    }

    /// Create a hedged position breakdown
    pub fn hedged(option_premium: Decimal, hedge_capital: Decimal) -> Self {
        Self {
            option_premium,
            hedge_capital,
            short_margin: Decimal::ZERO,
            total_bpr: option_premium + hedge_capital,
        }
    }

    /// Add hedge capital to existing breakdown
    pub fn with_hedge(mut self, hedge_capital: Decimal) -> Self {
        self.hedge_capital = hedge_capital;
        self.total_bpr = self.option_premium.max(Decimal::ZERO)
            + self.hedge_capital
            + self.short_margin;
        self
    }
}

/// Represents capital required for a trade
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapitalRequirement {
    /// Initial margin/debit required to enter the position
    pub initial_requirement: Decimal,

    /// Maintenance margin (may differ from initial for some strategies)
    pub maintenance_requirement: Decimal,

    /// Method used to calculate the requirement
    pub calculation_method: CapitalCalculationMethod,

    /// Breakdown by component
    pub breakdown: CapitalBreakdown,
}

impl CapitalRequirement {
    /// Create a requirement for a debit trade (long options)
    pub fn for_debit(debit: Decimal) -> Self {
        Self {
            initial_requirement: debit,
            maintenance_requirement: debit,
            calculation_method: CapitalCalculationMethod::LongOptionDebit,
            breakdown: CapitalBreakdown::debit(debit),
        }
    }

    /// Create a requirement for a credit trade (short options/spreads)
    pub fn for_credit(credit_received: Decimal, max_loss: Decimal) -> Self {
        // Credit spread margin = max_loss - credit_received
        let margin = (max_loss - credit_received).max(Decimal::ZERO);
        Self {
            initial_requirement: margin,
            maintenance_requirement: margin,
            calculation_method: CapitalCalculationMethod::StrategyBasedMargin,
            breakdown: CapitalBreakdown::credit(credit_received, margin),
        }
    }

    /// Create a requirement for a defined-risk spread
    pub fn for_spread(is_debit: bool, net_premium: Decimal, max_loss: Decimal) -> Self {
        if is_debit {
            Self::for_debit(net_premium.abs())
        } else {
            Self::for_credit(net_premium.abs(), max_loss)
        }
    }

    /// Create a requirement using premium magnitude as the capital basis.
    ///
    /// This is a placeholder for strategies without a proper margin model.
    pub fn for_premium_basis(net_premium: Decimal) -> Self {
        let premium = net_premium.abs();
        let is_credit = net_premium < Decimal::ZERO;
        let (calculation_method, breakdown) = if is_credit {
            // Use premium magnitude as a conservative proxy for capital.
            (CapitalCalculationMethod::CreditReceived, CapitalBreakdown::credit(premium, premium))
        } else {
            (CapitalCalculationMethod::LongOptionDebit, CapitalBreakdown::debit(premium))
        };

        Self {
            initial_requirement: premium,
            maintenance_requirement: premium,
            calculation_method,
            breakdown,
        }
    }

    /// Add hedge capital requirement
    pub fn with_hedge(mut self, hedge_capital: Decimal) -> Self {
        self.initial_requirement += hedge_capital;
        self.maintenance_requirement += hedge_capital;
        self.breakdown = self.breakdown.with_hedge(hedge_capital);
        self
    }

    /// The capital at risk (could be lost)
    pub fn capital_at_risk(&self) -> Decimal {
        self.initial_requirement
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_debit_requirement() {
        let req = CapitalRequirement::for_debit(dec!(150));
        assert_eq!(req.initial_requirement, dec!(150));
        assert_eq!(req.breakdown.option_premium, dec!(150));
        assert_eq!(req.breakdown.total_bpr, dec!(150));
    }

    #[test]
    fn test_credit_requirement() {
        // Credit spread: received $1.50, max loss $5.00
        // Margin = $5.00 - $1.50 = $3.50
        let req = CapitalRequirement::for_credit(dec!(1.50), dec!(5.00));
        assert_eq!(req.initial_requirement, dec!(3.50));
        assert_eq!(req.breakdown.option_premium, dec!(-1.50));
        assert_eq!(req.breakdown.short_margin, dec!(3.50));
    }

    #[test]
    fn test_hedged_requirement() {
        let req = CapitalRequirement::for_debit(dec!(100))
            .with_hedge(dec!(500)); // $500 for stock hedge
        assert_eq!(req.initial_requirement, dec!(600));
        assert_eq!(req.breakdown.option_premium, dec!(100));
        assert_eq!(req.breakdown.hedge_capital, dec!(500));
    }
}
