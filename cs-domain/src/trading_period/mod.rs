//! Trading period abstractions
//!
//! This module provides flexible timing specifications that can be
//! earnings-relative, fixed-date, or holding-period based.

mod period;
mod spec;
mod range;
mod tradable_event;

pub use period::TradingPeriod;
pub use spec::{TradingPeriodSpec, TimingError};
pub use range::TradingRange;
pub use tradable_event::TradableEvent;
