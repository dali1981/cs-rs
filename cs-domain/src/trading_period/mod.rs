//! Trading period abstractions
//!
//! This module provides flexible timing specifications that can be
//! earnings-relative, fixed-date, or holding-period based.

mod period;
mod spec;

pub use period::TradingPeriod;
pub use spec::{TradingPeriodSpec, TimingError};
