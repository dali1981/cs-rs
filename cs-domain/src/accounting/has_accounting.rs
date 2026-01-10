//! HasAccounting trait for extracting accounting data from trade results
//!
//! This provides a non-invasive way to extract accounting information from
//! existing trade result types without modifying their structure.

use rust_decimal::Decimal;

use super::{CapitalRequirement, TradeAccounting};

/// Trait for types that can provide accounting data
///
/// Implement this for trade result types to enable capital-weighted returns
/// and other proper accounting metrics.
pub trait HasAccounting {
    /// Capital requirement (non-negative) derived from a capital basis.
    fn capital_required(&self) -> CapitalRequirement;

    /// Entry cash flow (negative = paid debit, positive = received credit).
    fn entry_cash_flow(&self) -> Decimal;

    /// Exit cash flow (negative = paid to close, positive = received).
    fn exit_cash_flow(&self) -> Decimal;

    /// Get the realized P&L
    fn realized_pnl(&self) -> Decimal;

    /// Get the hedge P&L (if any)
    fn hedge_pnl(&self) -> Option<Decimal> {
        None
    }

    /// Get the hedge capital required (if any)
    fn hedge_capital(&self) -> Option<Decimal> {
        None
    }

    /// Get return on capital (computed from realized P&L and capital)
    fn return_on_capital(&self) -> f64 {
        let capital = self.capital_required().initial_requirement;
        if capital.is_zero() {
            return 0.0;
        }
        let pnl = self.realized_pnl();
        (pnl / capital).try_into().unwrap_or(0.0)
    }

    /// Convert to full TradeAccounting record
    fn to_accounting(&self) -> TradeAccounting {
        let capital_required = self.capital_required();
        let pnl = self.realized_pnl();

        let mut accounting = TradeAccounting::from_cashflows(
            capital_required,
            self.entry_cash_flow(),
            self.exit_cash_flow(),
            pnl,
        );

        accounting.hedge_pnl = self.hedge_pnl();

        if let Some(hc) = self.hedge_capital() {
            accounting = accounting.with_hedge_capital(hc);
        }

        accounting
    }
}

/// Helper to compute capital requirement from entry debit
pub fn capital_from_debit(entry_debit: Decimal, multiplier: u32) -> CapitalRequirement {
    let capital = entry_debit * Decimal::from(multiplier);
    CapitalRequirement::for_debit(capital)
}
