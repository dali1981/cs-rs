// cs-domain: Core business logic and domain models
//
// Calendar spreads, trading strategies, repositories, domain services.

/// Standard options contract multiplier (100 shares per contract)
pub const CONTRACT_MULTIPLIER: i32 = 100;

pub mod accounting;
pub mod campaign;
pub mod config;
pub mod datetime;
pub mod entities;
pub mod expiration;
pub mod hedging;
pub mod infrastructure;
pub mod pnl;
pub mod ports;
pub mod position;
pub mod repositories;
pub mod roll;
pub mod rules;
pub mod strategy;
pub mod strike_selection;
pub mod testing;
pub mod timing;
pub mod trade;
pub mod trading_costs;
pub mod trading_period;
pub mod value_objects;

// Re-exports for convenience
pub use datetime::{eastern_to_utc, MarketTime, TradingDate, TradingTimestamp};
pub use entities::*;
pub use strike_selection::{
    OptionStrategy, StrategyError, StrikeMatchMode, TradeSelectionCriteria,
};
pub use value_objects::*;

pub use accounting::{
    margin_engine_for_config, BprInputs, BprSnapshot, BprSummary, BprTimeline, CapitalBreakdown,
    CapitalCalculationMethod, CapitalRequirement, HasAccounting, HedgeInput, MarginCalculator,
    MarginConfig, MarginMode, OptionLegInput, OptionRight, OptionsMarginConfig, ReturnBasis,
    StockMarginConfig, StockMarginMode, TradeAccounting, TradeStatistics,
};
pub use campaign::{
    EarningsTimingType, PeriodPolicy, SessionAction, SessionContext, SessionSchedule,
    TradingCampaign, TradingSession,
};
pub use config::{FilterCriteria, PositionSpec, PositionStructure, StrikeSelection};
pub use expiration::{ExpirationCycle, ExpirationPolicy};
pub use hedging::*;
pub use pnl::{PnlStatistics, ToPnlRecord, TradePnlRecord};
pub use ports::{TradeFactory, TradeFactoryError};
pub use position::{DailyAttribution, PositionAttribution, PositionGreeks, PositionSnapshot};
pub use repositories::*;
pub use roll::{RollEvent, RollPolicy};
pub use rules::{
    EventRule, FileRulesConfig, MarketRule, RuleError, RuleLevel, RulesConfig, TradeRule,
};
pub use strategy::{
    FailedTrade, TradeFilters, TradeStrategy, TradeStructure, TradeStructureConfig,
};
pub use timing::*;
pub use trade::{CompositeTrade, LegPosition, RollableTrade, TradeConstructionError, TradeResult};
pub use trading_costs::{
    ApplyCosts, CostPreset, HasTradingCost, LegContext, TradeSide, TradeType, TradingContext,
    TradingCost, TradingCostBreakdown, TradingCostCalculator, TradingCostConfig,
};
pub use trading_period::{
    TimingError, TradableEvent, TradingPeriod, TradingPeriodSpec, TradingRange,
};
