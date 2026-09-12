//! Trade strategy configuration
//!
//! Combines trade structure, timing, expiration policy, and rolling
//! into a single unified configuration.

mod config;
mod presets;
mod types;

pub use config::{TradeFilters, TradeStrategy, TradeStructureConfig};
pub use presets::*;
pub use types::{FailedTrade, TradeStructure};
