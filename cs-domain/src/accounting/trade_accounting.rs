//! Trade accounting record
//!
//! Complete financial record for a trade including cash flows,
//! capital requirements, and return calculations.

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use super::CapitalRequirement;

/// Direction of cash flow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CashFlowDirection {
    /// Cash paid out (debit)
    Outflow,
    /// Cash received (credit)
    Inflow,
}

/// A single cash flow event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlow {
    /// Amount (always positive, direction indicates sign)
    pub amount: Decimal,
    /// Direction of the flow
    pub direction: CashFlowDirection,
    /// Description of the cash flow
    pub description: String,
}

impl CashFlow {
    /// Create an outflow (money paid)
    pub fn outflow(amount: Decimal, description: impl Into<String>) -> Self {
        Self {
            amount: amount.abs(),
            direction: CashFlowDirection::Outflow,
            description: description.into(),
        }
    }

    /// Create an inflow (money received)
    pub fn inflow(amount: Decimal, description: impl Into<String>) -> Self {
        Self {
            amount: amount.abs(),
            direction: CashFlowDirection::Inflow,
            description: description.into(),
        }
    }

    /// Get the signed value (negative for outflow, positive for inflow)
    pub fn signed_value(&self) -> Decimal {
        match self.direction {
            CashFlowDirection::Outflow => -self.amount,
            CashFlowDirection::Inflow => self.amount,
        }
    }
}

/// Complete trade accounting record
///
/// This tracks all financial aspects of a trade:
/// - Capital required to enter
/// - Cash flows at entry and exit
/// - Transaction costs
/// - Realized P&L and return on capital
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TradeAccounting {
    /// Capital required to enter the position
    pub capital_required: CapitalRequirement,

    /// Entry cash flow (negative = paid debit, positive = received credit)
    pub entry_cash_flow: Decimal,

    /// Exit cash flow (negative = paid to close, positive = received)
    pub exit_cash_flow: Decimal,

    /// Transaction costs (commissions, fees - always negative or zero)
    pub transaction_costs: Decimal,

    /// Hedge P&L (if applicable)
    pub hedge_pnl: Option<Decimal>,

    /// Realized P&L (locked in at close)
    pub realized_pnl: Decimal,

    /// Return on capital deployed (as decimal, e.g., 0.10 = 10%)
    pub return_on_capital: f64,
}

impl TradeAccounting {
    /// Create accounting for a debit trade (long options)
    ///
    /// # Arguments
    /// * `entry_debit` - Premium paid to enter (positive number)
    /// * `exit_credit` - Value received at exit (positive number)
    /// * `multiplier` - Contract multiplier (typically 100)
    pub fn for_debit_trade(
        entry_debit: Decimal,
        exit_credit: Decimal,
        multiplier: u32,
    ) -> Self {
        let mult = Decimal::from(multiplier);
        let entry_cash_flow = -entry_debit * mult; // Paid out
        let exit_cash_flow = exit_credit * mult;   // Received
        let realized_pnl = exit_cash_flow + entry_cash_flow;

        let capital = entry_debit * mult;
        let return_on_capital = if capital.is_zero() {
            0.0
        } else {
            (realized_pnl / capital).to_f64().unwrap_or(0.0)
        };

        Self {
            capital_required: CapitalRequirement::for_debit(capital),
            entry_cash_flow,
            exit_cash_flow,
            transaction_costs: Decimal::ZERO,
            hedge_pnl: None,
            realized_pnl,
            return_on_capital,
        }
    }

    /// Create accounting for a credit trade (short options/spreads)
    ///
    /// # Arguments
    /// * `entry_credit` - Premium received at entry (positive number)
    /// * `exit_debit` - Cost to close (positive number)
    /// * `max_loss` - Maximum possible loss (for margin calculation)
    /// * `multiplier` - Contract multiplier (typically 100)
    pub fn for_credit_trade(
        entry_credit: Decimal,
        exit_debit: Decimal,
        max_loss: Decimal,
        multiplier: u32,
    ) -> Self {
        let mult = Decimal::from(multiplier);
        let entry_cash_flow = entry_credit * mult;  // Received
        let exit_cash_flow = -exit_debit * mult;    // Paid to close
        let realized_pnl = entry_cash_flow + exit_cash_flow;

        let capital = CapitalRequirement::for_credit(entry_credit * mult, max_loss * mult);
        let cap_required = capital.initial_requirement;
        let return_on_capital = if cap_required.is_zero() {
            0.0
        } else {
            (realized_pnl / cap_required).to_f64().unwrap_or(0.0)
        };

        Self {
            capital_required: capital,
            entry_cash_flow,
            exit_cash_flow,
            transaction_costs: Decimal::ZERO,
            hedge_pnl: None,
            realized_pnl,
            return_on_capital,
        }
    }

    /// Create accounting from existing P&L data
    ///
    /// This is the most common case when retrofitting existing trade results.
    pub fn from_pnl(
        entry_cost: Decimal,
        exit_value: Decimal,
        pnl: Decimal,
    ) -> Self {
        let return_on_capital = if entry_cost.is_zero() || entry_cost < Decimal::ZERO {
            // Credit trade or zero cost - use absolute value for return calc
            if entry_cost.abs() > Decimal::ZERO {
                (pnl / entry_cost.abs()).to_f64().unwrap_or(0.0)
            } else {
                0.0
            }
        } else {
            (pnl / entry_cost).to_f64().unwrap_or(0.0)
        };

        Self {
            capital_required: CapitalRequirement::for_debit(entry_cost.abs()),
            entry_cash_flow: -entry_cost, // Entry cost is what we paid
            exit_cash_flow: exit_value,
            transaction_costs: Decimal::ZERO,
            hedge_pnl: None,
            realized_pnl: pnl,
            return_on_capital,
        }
    }

    /// Add hedge P&L to the accounting
    pub fn with_hedge_pnl(mut self, hedge_pnl: Decimal) -> Self {
        self.hedge_pnl = Some(hedge_pnl);
        self.realized_pnl += hedge_pnl;
        // Recalculate return on capital
        if !self.capital_required.initial_requirement.is_zero() {
            self.return_on_capital = (self.realized_pnl / self.capital_required.initial_requirement)
                .to_f64()
                .unwrap_or(0.0);
        }
        self
    }

    /// Add hedge capital to the requirement
    pub fn with_hedge_capital(mut self, hedge_capital: Decimal) -> Self {
        self.capital_required = self.capital_required.with_hedge(hedge_capital);
        // Recalculate return on capital with new capital base
        if !self.capital_required.initial_requirement.is_zero() {
            self.return_on_capital = (self.realized_pnl / self.capital_required.initial_requirement)
                .to_f64()
                .unwrap_or(0.0);
        }
        self
    }

    /// Add transaction costs
    pub fn with_transaction_costs(mut self, costs: Decimal) -> Self {
        self.transaction_costs = -costs.abs(); // Always negative
        self.realized_pnl += self.transaction_costs;
        // Recalculate return
        if !self.capital_required.initial_requirement.is_zero() {
            self.return_on_capital = (self.realized_pnl / self.capital_required.initial_requirement)
                .to_f64()
                .unwrap_or(0.0);
        }
        self
    }

    /// Get the capital deployed
    pub fn capital_deployed(&self) -> Decimal {
        self.capital_required.initial_requirement
    }

    /// Get total P&L including hedge
    pub fn total_pnl(&self) -> Decimal {
        self.realized_pnl
    }

    /// Check if trade was profitable
    pub fn is_winner(&self) -> bool {
        self.realized_pnl > Decimal::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_debit_trade_accounting() {
        // Buy straddle at $1.50, sell at $2.00
        let acct = TradeAccounting::for_debit_trade(
            dec!(1.50),  // entry debit
            dec!(2.00),  // exit credit
            100,         // multiplier
        );

        assert_eq!(acct.entry_cash_flow, dec!(-150)); // Paid $150
        assert_eq!(acct.exit_cash_flow, dec!(200));   // Received $200
        assert_eq!(acct.realized_pnl, dec!(50));      // Profit $50
        assert_eq!(acct.capital_required.initial_requirement, dec!(150));

        // Return = $50 / $150 = 33.33%
        assert!((acct.return_on_capital - 0.3333).abs() < 0.01);
    }

    #[test]
    fn test_credit_trade_accounting() {
        // Sell credit spread: receive $1.50, close at $0.50, max loss $5.00
        let acct = TradeAccounting::for_credit_trade(
            dec!(1.50),  // entry credit
            dec!(0.50),  // exit debit (cost to close)
            dec!(5.00),  // max loss
            100,         // multiplier
        );

        assert_eq!(acct.entry_cash_flow, dec!(150));  // Received $150
        assert_eq!(acct.exit_cash_flow, dec!(-50));   // Paid $50 to close
        assert_eq!(acct.realized_pnl, dec!(100));     // Profit $100

        // Margin = ($500 - $150) = $350
        assert_eq!(acct.capital_required.initial_requirement, dec!(350));

        // Return = $100 / $350 = 28.57%
        assert!((acct.return_on_capital - 0.2857).abs() < 0.01);
    }

    #[test]
    fn test_from_pnl() {
        let acct = TradeAccounting::from_pnl(
            dec!(150),   // entry cost
            dec!(200),   // exit value
            dec!(50),    // pnl
        );

        assert_eq!(acct.realized_pnl, dec!(50));
        assert_eq!(acct.capital_required.initial_requirement, dec!(150));
        assert!((acct.return_on_capital - 0.3333).abs() < 0.01);
    }

    #[test]
    fn test_with_hedge() {
        let acct = TradeAccounting::for_debit_trade(dec!(1.50), dec!(2.00), 100)
            .with_hedge_pnl(dec!(-20))
            .with_hedge_capital(dec!(500));

        // Original P&L $50, hedge P&L -$20
        assert_eq!(acct.realized_pnl, dec!(30));

        // Original capital $150, hedge capital $500
        assert_eq!(acct.capital_required.initial_requirement, dec!(650));

        // Return = $30 / $650 = 4.62%
        assert!((acct.return_on_capital - 0.0462).abs() < 0.01);
    }
}
