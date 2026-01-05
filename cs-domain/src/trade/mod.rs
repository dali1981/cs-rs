//! Trade types and factory for creating trades

pub mod rollable;
pub mod composite;

pub use rollable::{RollableTrade, TradeResult, TradeConstructionError};
pub use composite::{CompositeTrade, LegPosition, CompositeIV, CompositeIVChange};

// Re-export TradeFactory from root (it's currently defined elsewhere)
pub use crate::TradeFactory;
