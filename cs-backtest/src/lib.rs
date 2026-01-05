// cs-backtest: Backtest execution engine
//
// BacktestUseCase, TradeExecutor, parallel processing.

pub mod config;
pub mod execution;  // Generic execution framework
pub mod iv_surface_builder;
pub mod spread_pricer;
pub mod straddle_pricer;
pub mod rolling_executor;  // Generic rolling executor for any trade type
pub mod trade_factory_impl;  // Trade factory implementation
pub mod hedging_executor;  // Delta hedging wrapper (TODO: migrate to generic execution)
pub mod hedging_analytics; // Hedging performance analytics
pub mod calendar_straddle_pricer;

// Legacy executors - kept temporarily for HedgingExecutor
// TODO: Refactor HedgingExecutor to use generic execution, then delete these
pub mod trade_executor;
pub mod iron_butterfly_pricer;
pub mod iron_butterfly_executor;
pub mod straddle_executor;
pub mod calendar_straddle_executor;
pub mod trade_orchestrator;  // Orchestrates strike selection, execution, and hedging
pub mod timing_strategy;   // Timing strategy enum for different trade types
pub mod backtest_use_case;
pub mod atm_iv_use_case;
pub mod minute_aligned_iv_use_case;
pub mod earnings_analysis_use_case;

pub use config::{BacktestConfig, SpreadType, SelectionType};
pub use backtest_use_case::{BacktestUseCase, BacktestResult, SessionProgress, TradeGenerationError};
pub use trade_orchestrator::{TradeResult, TradeStructure, TradeOrchestrator};
pub use timing_strategy::TimingStrategy;
pub use trade_executor::{TradeExecutor, ExecutionError};
pub use spread_pricer::{SpreadPricer, SpreadPricing, LegPricing, PricingError};
pub use execution::{ExecutableTrade, TradePricer, ExecutionConfig, ExecutionContext, execute_trade};
pub use iron_butterfly_pricer::{IronButterflyPricer, IronButterflyPricing};
pub use iron_butterfly_executor::IronButterflyExecutor;
pub use straddle_pricer::{StraddlePricer, StraddlePricing};
pub use straddle_executor::StraddleExecutor;
pub use rolling_executor::RollingExecutor;  // Generic rolling executor
pub use trade_factory_impl::DefaultTradeFactory;
pub use hedging_executor::HedgingExecutor;
pub use hedging_analytics::{HedgingComparison, HedgingStats};
pub use calendar_straddle_pricer::{CalendarStraddlePricer, CalendarStraddlePricing};
pub use calendar_straddle_executor::CalendarStraddleExecutor;
pub use iv_surface_builder::{build_iv_surface, build_iv_surface_minute_aligned};
pub use atm_iv_use_case::{GenerateIvTimeSeriesUseCase, IvTimeSeriesResult, IvTimeSeriesError};
pub use minute_aligned_iv_use_case::{MinuteAlignedIvUseCase, MinuteAlignedIvResult, MinuteAlignedIvError};
pub use earnings_analysis_use_case::{EarningsAnalysisUseCase, EarningsAnalysisResult, EarningsAnalysisError};

// Re-export pricing model types for convenience
pub use cs_analytics::{PricingModel, PricingIVProvider, InterpolationMode};
