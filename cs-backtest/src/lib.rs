// cs-backtest: Backtest execution engine
//
// BacktestUseCase, TradeExecutor, parallel processing.

pub mod config;
pub mod iv_surface_builder;
pub mod spread_pricer;
pub mod trade_executor;
pub mod iron_butterfly_pricer;
pub mod iron_butterfly_executor;
pub mod backtest_use_case;

pub use config::{BacktestConfig, SpreadType, SelectionType};
pub use backtest_use_case::{BacktestUseCase, BacktestResult, SessionProgress, TradeGenerationError, TradeResult};
pub use trade_executor::{TradeExecutor, ExecutionError};
pub use spread_pricer::{SpreadPricer, SpreadPricing, LegPricing, PricingError};
pub use iron_butterfly_pricer::{IronButterflyPricer, IronButterflyPricing};
pub use iron_butterfly_executor::IronButterflyExecutor;
pub use iv_surface_builder::build_iv_surface;

// Re-export pricing model types for convenience
pub use cs_analytics::{PricingModel, PricingIVProvider, InterpolationMode};
