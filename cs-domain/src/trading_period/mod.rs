//! Trading period abstractions
//!
//! This module provides flexible timing specifications that can be
//! earnings-relative, fixed-date, or holding-period based.

mod period;
mod range;
mod spec;
mod tradable_event;

pub use period::TradingPeriod;
pub use range::TradingRange;
pub use spec::{TimingError, TradingPeriodSpec};
pub use tradable_event::TradableEvent;
