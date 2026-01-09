//! P&L Accounting Module
//!
//! Provides proper accounting for options trades including:
//! - Capital requirements (margin, buying power reduction)
//! - Cash flow tracking (entry/exit)
//! - Return calculations (capital-weighted, time-weighted)
//! - Trade statistics
//!
//! # Why This Module Exists
//!
//! Simple percentage return averages can diverge significantly from dollar P&L
//! when position sizes vary. For example:
//!
//! | Trade | Debit | P&L ($) | Return (%) |
//! |-------|-------|---------|------------|
//! | A     | $75   | +$7.50  | +10%       |
//! | B     | $170  | -$85    | -50%       |
//! | C     | $50   | +$25    | +50%       |
//!
//! - Simple Mean Return: (10 - 50 + 50) / 3 = +3.33%
//! - Total P&L: 7.50 - 85 + 25 = -$52.50
//!
//! The solution is **capital-weighted returns**:
//! ```text
//! weighted_return = sum(capital_i * return_i) / sum(capital_i)
//!                 = (75*0.10 + 170*(-0.50) + 50*0.50) / (75 + 170 + 50)
//!                 = (7.5 - 85 + 25) / 295
//!                 = -52.5 / 295
//!                 = -17.8%
//! ```
//!
//! Now the return matches the economic reality.

mod capital;
mod has_accounting;
mod margin;
mod statistics;
mod trade_accounting;

pub use capital::{
    CapitalBreakdown, CapitalCalculationMethod, CapitalRequirement,
};
pub use has_accounting::HasAccounting;
pub use margin::MarginCalculator;
pub use statistics::TradeStatistics;
pub use trade_accounting::{CashFlow, TradeAccounting};
