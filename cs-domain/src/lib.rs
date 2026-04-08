// cs-domain: Core business logic and domain models
//
// Calendar spreads, trading strategies, repositories, domain services.

/// Standard options contract multiplier (100 shares per contract)
pub const CONTRACT_MULTIPLIER: i32 = 100;

pub mod accounting;
pub mod trading_costs;
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
pub mod campaign;
pub mod config;
pub mod pnl;
pub mod rules;
pub mod testing;

// Re-exports for convenience
pub use datetime::{TradingDate, TradingTimestamp, MarketTime, eastern_to_utc};
pub use value_objects::*;
pub use entities::*;
pub use strike_selection::{
    SelectionStrategy, OptionStrategy, StrategyError, TradeSelectionCriteria, OptionChainData,
    ATMStrategy, DeltaStrategy, DeltaScanMode, StrikeMatchMode,
};

pub use repositories::*;
pub use ports::{TradeFactory, TradeFactoryError};
pub use timing::*;
pub use hedging::*;
pub use position::{PositionGreeks, PositionSnapshot, DailyAttribution, PositionAttribution};
pub use expiration::{ExpirationCycle, ExpirationPolicy};
pub use trading_period::{TradingPeriod, TradingPeriodSpec, TimingError, TradingRange, TradableEvent};
pub use roll::{RollPolicy, RollEvent};
pub use strategy::{TradeStrategy, TradeStructureConfig, TradeFilters, TradeStructure, FailedTrade};
pub use trade::{RollableTrade, TradeResult, TradeConstructionError, CompositeTrade, LegPosition};
pub use campaign::{
    TradingCampaign, TradingSession, SessionAction, SessionContext,
    EarningsTimingType, SessionSchedule, PeriodPolicy
};
pub use config::{FilterCriteria, PositionSpec, PositionStructure, StrikeSelection};
pub use accounting::{
    TradeAccounting, TradeStatistics, CapitalRequirement, CapitalBreakdown,
    CapitalCalculationMethod, MarginCalculator, HasAccounting, ReturnBasis,
    BprInputs, BprSnapshot, BprSummary, BprTimeline, OptionLegInput, OptionRight, HedgeInput,
    MarginConfig, MarginMode, StockMarginConfig, StockMarginMode, OptionsMarginConfig,
    margin_engine_for_config,
};
pub use trading_costs::{
    TradingCost, TradingCostBreakdown, TradeSide, TradingContext, LegContext,
    TradeType, TradingCostCalculator, TradingCostConfig, CostPreset,
    HasTradingCost, ApplyCosts,
};
pub use pnl::{TradePnlRecord, PnlStatistics, ToPnlRecord};
pub use rules::{
    RulesConfig, FileRulesConfig, EventRule, MarketRule, TradeRule, RuleLevel, RuleError,
};
