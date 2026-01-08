// cs-backtest: Backtest execution engine
//
// Generic trade execution, rolling strategies, backtesting.

pub mod config;
pub mod delta_providers;  // Delta computation strategies for hedging
pub mod execution;  // Generic execution framework
pub mod greeks_helpers;  // Greeks computation helpers
pub mod iv_validation;  // IV validation and bounds checking
pub mod iv_surface_builder;
pub mod spread_pricer;
pub mod composite_pricer;  // Generic pricer for CompositeTrade types
pub mod straddle_pricer;
pub mod iron_butterfly_pricer;
pub mod calendar_straddle_pricer;
pub mod trade_executor;  // Unified executor with rolling + hedging support
pub mod trade_executor_factory;  // Factory for creating trade executors
pub mod session_executor;  // Session-based executor for campaign execution
pub mod trade_factory_impl;
pub mod hedging_analytics;
pub mod timing_strategy;
pub mod backtest_use_case;
mod backtest_use_case_helpers;
pub mod atm_iv_use_case;
pub mod minute_aligned_iv_use_case;
pub mod earnings_analysis_use_case;
pub mod attribution;

// Config
pub use config::{BacktestConfig, SpreadType, SelectionType};

// Use cases
pub use backtest_use_case::{BacktestUseCase, BacktestResult, SessionProgress, TradeGenerationError, TradeResultMethods};
pub use atm_iv_use_case::{GenerateIvTimeSeriesUseCase, IvTimeSeriesResult, IvTimeSeriesError};
pub use minute_aligned_iv_use_case::{MinuteAlignedIvUseCase, MinuteAlignedIvResult, MinuteAlignedIvError};
pub use earnings_analysis_use_case::{EarningsAnalysisUseCase, EarningsAnalysisResult, EarningsAnalysisError};

// Generic execution framework
pub use execution::{ExecutableTrade, TradePricer, ExecutionConfig, ExecutionContext, ExecutionError, execute_trade};
pub use timing_strategy::TimingStrategy;
pub use trade_executor::TradeExecutor;
pub use trade_executor_factory::TradeExecutorFactory;

// Session-based execution
pub use session_executor::{SessionExecutor, SessionResult, SessionPnL, BatchResult};

// Pricers
pub use spread_pricer::{SpreadPricer, SpreadPricing, LegPricing, PricingError};
pub use composite_pricer::{CompositePricer, CompositePricing};
pub use straddle_pricer::{StraddlePricer, StraddlePricing};
pub use iron_butterfly_pricer::{IronButterflyPricer, IronButterflyPricing};
pub use calendar_straddle_pricer::{CalendarStraddlePricer, CalendarStraddlePricing};

// Utilities
pub use trade_factory_impl::DefaultTradeFactory;
pub use hedging_analytics::{HedgingComparison, HedgingStats};
pub use greeks_helpers::{
    compute_straddle_greeks, compute_spread_net_greeks, compute_iron_butterfly_net_greeks,
    compute_calendar_straddle_net_greeks, compute_iv_change, average_iv,
};
pub use iv_validation::{IVValidator, IVValidationError, validate_iv_for_surface, validate_entry_iv};
pub use iv_surface_builder::{build_iv_surface, build_iv_surface_minute_aligned};

// Re-export pricing model types for convenience
pub use cs_analytics::{PricingModel, PricingIVProvider, InterpolationMode};
