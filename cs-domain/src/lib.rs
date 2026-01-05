// cs-domain: Core business logic and domain models
//
// Calendar spreads, trading strategies, repositories, domain services.

/// Standard options contract multiplier (100 shares per contract)
pub const CONTRACT_MULTIPLIER: i32 = 100;

pub mod datetime;
pub mod value_objects;
pub mod entities;
pub mod strike_selection;
pub mod repositories;
pub mod ports;
pub mod timing;
pub mod infrastructure;
pub mod hedging;
pub mod position;
pub mod expiration;
pub mod trading_period;
pub mod roll;
pub mod strategy;
pub mod trade;

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
pub use ports::{TradeFactory, TradeFactoryError};
pub use timing::*;
pub use hedging::*;
pub use position::{PositionGreeks, PositionSnapshot, DailyAttribution, PositionAttribution};
pub use expiration::{ExpirationCycle, ExpirationPolicy};
pub use trading_period::{TradingPeriod, TradingPeriodSpec, TimingError};
pub use roll::{RollPolicy, RollEvent};
pub use strategy::{TradeStrategy, TradeStructureConfig, TradeFilters, TradeStructure, FailedTrade};
pub use trade::{RollableTrade, TradeResult, TradeConstructionError};
