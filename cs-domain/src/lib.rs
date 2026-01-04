// cs-domain: Core business logic and domain models
//
// Calendar spreads, trading strategies, repositories, domain services.

pub mod datetime;
pub mod value_objects;
pub mod entities;
pub mod strategies;
pub mod repositories;
pub mod timing;
pub mod infrastructure;

// Re-exports for convenience
pub use datetime::{TradingDate, TradingTimestamp, MarketTime};
pub use value_objects::*;
pub use entities::*;
pub use strategies::{
    SelectionStrategy, OptionStrategy, StrategyError, TradeSelectionCriteria, OptionChainData,
    ATMStrategy, DeltaStrategy, DeltaScanMode, StrikeMatchMode,
};

// Re-export deprecated trait for backwards compatibility
#[allow(deprecated)]
pub use strategies::TradingStrategy;
pub use repositories::*;
pub use timing::*;
