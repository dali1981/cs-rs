// cs-backtest: Backtest execution engine
//
// Generic trade execution, rolling strategies, backtesting.

pub mod atm_iv_use_case;
pub mod attribution;
pub mod backtest_use_case;
mod backtest_use_case_helpers;
pub mod bpr;
pub mod calendar_straddle_pricer;
pub mod campaign_config;
pub mod campaign_use_case;
pub mod commands;
pub mod composite_pricer; // Generic pricer for CompositeTrade types
pub mod config;
pub mod delta_providers; // Delta computation strategies for hedging
pub mod earnings_analysis_use_case;
pub mod execution; // Generic execution framework
pub mod greeks_helpers; // Greeks computation helpers
pub mod hedging_analytics;
pub mod hedging_executor; // Hedging support for BacktestUseCase strategies
pub mod hedging_simulator; // Trade simulation with integrated hedging
pub mod iron_butterfly_pricer;
pub mod iv_surface_builder;
pub mod iv_validation; // IV validation and bounds checking
pub mod minute_aligned_iv_use_case;
pub mod multi_leg_pricer;
pub mod option_bar_adapter; // OptionBar → DataFrame adapter for pricer internals
pub mod rules; // Rule evaluation for entry filtering
pub mod run_contract;
pub mod session_executor; // Session-based executor for campaign execution
pub mod spread_pricer;
pub mod straddle_pricer;
pub mod strike_selection;
pub mod timing_strategy;
pub mod trade_executor; // Unified executor with rolling + hedging support
pub mod trade_executor_factory; // Factory for creating trade executors
pub mod trade_factory_impl;
pub mod trade_strategy; // IVSurface-based strike selection strategies

// Application commands (clean boundary between CLI/config and use cases)
pub use commands::{
    BacktestPeriod, ExecutionSpec, FilterSet, RiskConfig, RunBacktestCommand, StrategySpec,
};

// Config
pub use campaign_config::CampaignConfig;
pub use config::{
    BacktestConfig, DataSourceConfig, EarningsProvider, EarningsSourceConfig, SelectionType,
    SpreadType,
};

// Use cases
pub use atm_iv_use_case::{GenerateIvTimeSeriesUseCase, IvTimeSeriesError, IvTimeSeriesResult};
pub use backtest_use_case::{
    BacktestError, BacktestResult, BacktestUseCase, TradeGenerationError, TradeResultMethods,
    UnifiedBacktestResult,
};
pub use campaign_use_case::{CampaignError, CampaignResult, CampaignUseCase};
pub use earnings_analysis_use_case::{
    EarningsAnalysisError, EarningsAnalysisResult, EarningsAnalysisUseCase,
};
pub use minute_aligned_iv_use_case::{
    MinuteAlignedIvError, MinuteAlignedIvResult, MinuteAlignedIvUseCase,
};
pub use run_contract::{RunInput, RunOutput, RunSummary, StrategyFamily};

// Execution framework
pub use execution::{
    ExecutableTrade, ExecutionConfig, ExecutionError, SimulationOutput, TradePricer,
};
pub use timing_strategy::TimingStrategy;
pub use trade_executor::TradeExecutor;
pub use trade_executor_factory::TradeExecutorFactory;
pub use trade_strategy::{
    CalendarSpreadStrategy, CalendarStraddleStrategy, IronButterflyStrategy, LongStraddleStrategy,
    PostEarningsStraddleStrategy, StrategyDispatch, TradeExecutionOutcome, TradeStrategy,
};

// Session-based execution
pub use session_executor::{BatchResult, SessionExecutor, SessionPnL, SessionResult};

// Pricers
pub use composite_pricer::{
    CalendarSpreadPricer, CalendarStraddleCompositePricer, CompositePricer, CompositePricing,
    IronButterflyCompositePricer,
};
pub use multi_leg_pricer::{
    ButterflyPricer, ButterflyPricing, CondorPricer, CondorPricing, IronCondorPricer,
    IronCondorPricing, StranglePricer, StranglePricing,
};
pub use spread_pricer::{LegPricing, PricingError, SpreadPricer, SpreadPricing};

// Legacy pricers (deprecated - use CompositePricer instead)
pub use calendar_straddle_pricer::{CalendarStraddlePricer, CalendarStraddlePricing};
pub use iron_butterfly_pricer::{IronButterflyPricer, IronButterflyPricing};
pub use straddle_pricer::{StraddlePricer, StraddlePricing};

// Utilities
pub use greeks_helpers::{
    average_iv, compute_calendar_straddle_net_greeks, compute_iron_butterfly_net_greeks,
    compute_iv_change, compute_spread_net_greeks, compute_straddle_greeks,
};
pub use hedging_analytics::{HedgingComparison, HedgingStats};
pub use hedging_simulator::{
    simulate_with_hedging, simulate_with_hedging_prepriced, EntryPricingContext, HasDelta,
    HasGamma, HasIV, HedgedSimulationOutput,
};
pub use iv_surface_builder::{build_iv_surface, build_iv_surface_minute_aligned};
pub use iv_validation::{
    validate_entry_iv, validate_iv_for_surface, IVValidationError, IVValidator,
};
pub use trade_factory_impl::DefaultTradeFactory;

// Rules
pub use rules::RuleEvaluator;

// Re-export pricing model types for convenience
pub use cs_analytics::{InterpolationMode, PricingIVProvider, PricingModel};
