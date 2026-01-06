//! Generic trade execution framework
//!
//! This module provides a generic execution framework that works with any trade type.
//! It eliminates the need for type-specific executors by using trait-based dispatch.
//!
//! # Example
//! ```ignore
//! use cs_backtest::execution::{execute_trade, ExecutionConfig};
//!
//! // Execute a straddle
//! let result = execute_trade(
//!     &straddle,
//!     &straddle_pricer,
//!     options_repo,
//!     equity_repo,
//!     &ExecutionConfig::for_straddle(Some(2.0)),
//!     &earnings_event,
//!     entry_time,
//!     exit_time,
//! ).await;
//!
//! // Execute a calendar spread (same function!)
//! let result = execute_trade(
//!     &calendar_spread,
//!     &spread_pricer,
//!     options_repo,
//!     equity_repo,
//!     &ExecutionConfig::for_calendar_spread(Some(2.0)),
//!     &earnings_event,
//!     entry_time,
//!     exit_time,
//! ).await;
//! ```

mod traits;
mod types;
mod generic_executor;

// Trade implementations (ExecutableTrade impls - these don't export the types themselves)
pub(crate) mod straddle_impl;
pub(crate) mod calendar_spread_impl;
pub(crate) mod calendar_straddle_impl;
pub(crate) mod iron_butterfly_impl;

pub use traits::{TradePricer, ExecutableTrade};
pub use types::{ExecutionConfig, ExecutionContext, ExecutionError};
pub use generic_executor::execute_trade;
