//! PnL computation and normalization module
//!
//! Implements the hedged options return & capital normalization specification.
//! This module provides:
//! - Trade PnL records with proper capital tracking
//! - Daily-normalized returns for cross-trade comparability
//! - Strategy-level statistics (Sharpe ratio, hedge cost ratio)
//!
//! See `specs/pnl_computation.md` for the full specification.

mod record;
mod statistics;
mod convert;

pub use record::TradePnlRecord;
pub use statistics::PnlStatistics;
pub use convert::ToPnlRecord;
