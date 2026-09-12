//! Trade types and factory for creating trades

pub mod composite;
pub mod rollable;

pub use composite::{CompositeIV, CompositeIVChange, CompositeTrade, LegPosition};
pub use rollable::{RollableTrade, TradeConstructionError, TradeResult};

// Re-export TradeFactory from root (it's currently defined elsewhere)
pub use crate::TradeFactory;
