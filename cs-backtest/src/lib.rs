// cs-backtest: Backtest execution engine
//
// BacktestUseCase, TradeExecutor, parallel processing.

pub mod config;
pub mod iv_surface_builder;
pub mod spread_pricer;
pub mod trade_executor;
pub mod backtest_use_case;

pub use config::{BacktestConfig, StrategyType};
pub use backtest_use_case::{BacktestUseCase, BacktestResult, SessionProgress, TradeGenerationError};
pub use trade_executor::{TradeExecutor, ExecutionError};
pub use spread_pricer::{SpreadPricer, SpreadPricing, LegPricing, PricingError};
pub use iv_surface_builder::build_iv_surface;

// Re-export IV model types for convenience
pub use cs_analytics::{IVModel, IVInterpolator, InterpolationMode};
