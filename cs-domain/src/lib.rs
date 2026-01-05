// cs-domain: Core business logic and domain models
//
// Calendar spreads, trading strategies, repositories, domain services.

pub mod datetime;
pub mod value_objects;
pub mod entities;
pub mod strike_selection;
pub mod repositories;
pub mod timing;
pub mod infrastructure;
pub mod hedging;

// Re-exports for convenience
pub use datetime::{TradingDate, TradingTimestamp, MarketTime};
pub use value_objects::*;
pub use entities::*;
pub use strike_selection::{
    SelectionStrategy, OptionStrategy, StrategyError, TradeSelectionCriteria, OptionChainData,
    ATMStrategy, DeltaStrategy, DeltaScanMode, StrikeMatchMode,
};

// Re-export deprecated trait for backwards compatibility
#[allow(deprecated)]
pub use strike_selection::TradingStrategy;
pub use repositories::*;
pub use timing::*;
pub use hedging::*;
