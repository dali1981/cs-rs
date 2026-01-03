// cs-backtest: Backtest execution engine
//
// BacktestUseCase, TradeExecutor, parallel processing.

pub mod config;
pub mod iv_surface_builder;
pub mod spread_pricer;
pub mod trade_executor;
pub mod iron_butterfly_pricer;
pub mod iron_butterfly_executor;
pub mod straddle_pricer;
pub mod straddle_executor;
pub mod backtest_use_case;
pub mod atm_iv_use_case;
pub mod minute_aligned_iv_use_case;
pub mod earnings_analysis_use_case;

pub use config::{BacktestConfig, SpreadType, SelectionType};
pub use backtest_use_case::{BacktestUseCase, BacktestResult, SessionProgress, TradeGenerationError, TradeResult};
pub use trade_executor::{TradeExecutor, ExecutionError};
pub use spread_pricer::{SpreadPricer, SpreadPricing, LegPricing, PricingError};
pub use iron_butterfly_pricer::{IronButterflyPricer, IronButterflyPricing};
pub use iron_butterfly_executor::IronButterflyExecutor;
pub use straddle_pricer::{StraddlePricer, StraddlePricing};
pub use straddle_executor::StraddleExecutor;
pub use iv_surface_builder::{build_iv_surface, build_iv_surface_minute_aligned};
pub use atm_iv_use_case::{GenerateIvTimeSeriesUseCase, IvTimeSeriesResult, IvTimeSeriesError};
pub use minute_aligned_iv_use_case::{MinuteAlignedIvUseCase, MinuteAlignedIvResult, MinuteAlignedIvError};
pub use earnings_analysis_use_case::{EarningsAnalysisUseCase, EarningsAnalysisResult, EarningsAnalysisError};

// Re-export pricing model types for convenience
pub use cs_analytics::{PricingModel, PricingIVProvider, InterpolationMode};
