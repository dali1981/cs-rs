// cs-backtest: Backtest execution engine
//
// Generic trade execution, rolling strategies, backtesting.

pub mod config;
pub mod execution;  // Generic execution framework
pub mod iv_surface_builder;
pub mod spread_pricer;
pub mod straddle_pricer;
pub mod iron_butterfly_pricer;
pub mod calendar_straddle_pricer;
pub mod rolling_executor;  // Generic rolling executor for any trade type
pub mod trade_factory_impl;
pub mod hedging_analytics;
pub mod trade_orchestrator;  // Orchestrates strike selection, execution, and hedging
pub mod timing_strategy;
pub mod backtest_use_case;
pub mod atm_iv_use_case;
pub mod minute_aligned_iv_use_case;
pub mod earnings_analysis_use_case;

// Config
pub use config::{BacktestConfig, SpreadType, SelectionType};

// Use cases
pub use backtest_use_case::{BacktestUseCase, BacktestResult, SessionProgress, TradeGenerationError};
pub use atm_iv_use_case::{GenerateIvTimeSeriesUseCase, IvTimeSeriesResult, IvTimeSeriesError};
pub use minute_aligned_iv_use_case::{MinuteAlignedIvUseCase, MinuteAlignedIvResult, MinuteAlignedIvError};
pub use earnings_analysis_use_case::{EarningsAnalysisUseCase, EarningsAnalysisResult, EarningsAnalysisError};

// Generic execution framework
pub use execution::{ExecutableTrade, TradePricer, ExecutionConfig, ExecutionContext, ExecutionError, execute_trade};
pub use trade_orchestrator::{TradeResult, TradeStructure, TradeOrchestrator};
pub use timing_strategy::TimingStrategy;
pub use rolling_executor::RollingExecutor;

// Pricers
pub use spread_pricer::{SpreadPricer, SpreadPricing, LegPricing, PricingError};
pub use straddle_pricer::{StraddlePricer, StraddlePricing};
pub use iron_butterfly_pricer::{IronButterflyPricer, IronButterflyPricing};
pub use calendar_straddle_pricer::{CalendarStraddlePricer, CalendarStraddlePricing};

// Utilities
pub use trade_factory_impl::DefaultTradeFactory;
pub use hedging_analytics::{HedgingComparison, HedgingStats};
pub use iv_surface_builder::{build_iv_surface, build_iv_surface_minute_aligned};

// Re-export pricing model types for convenience
pub use cs_analytics::{PricingModel, PricingIVProvider, InterpolationMode};
