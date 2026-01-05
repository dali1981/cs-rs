//! Trade types and factory for creating trades

pub mod rollable;

pub use rollable::{RollableTrade, TradeResult, TradeConstructionError};

// Re-export TradeFactory from root (it's currently defined elsewhere)
pub use crate::TradeFactory;
