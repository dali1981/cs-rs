//! CLI argument definitions using flattened groups
//!
//! This module provides a modular approach to CLI arguments by grouping
//! related arguments into separate structs and using #[command(flatten)]
//! to compose them into command-specific argument structures.

#[cfg(feature = "experimental-cli")]
pub mod analyze;
#[cfg(feature = "experimental-cli")]
pub mod atm_iv;
pub mod attribution;
pub mod backtest;
#[cfg(feature = "experimental-cli")]
pub mod campaign;
pub mod common;
#[cfg(feature = "experimental-cli")]
pub mod earnings;
pub mod hedging;
pub mod metrics;
#[cfg(feature = "experimental-cli")]
pub mod price;
pub mod rules;
pub mod selection;
pub mod strategy;
pub mod timing;

// CLI wrapper types with ValueEnum (convert to domain types)
pub mod option_type;
pub mod selection_type;
pub mod spread_type;

#[cfg(feature = "experimental-cli")]
pub use analyze::AnalyzeArgs;
#[cfg(feature = "experimental-cli")]
pub use atm_iv::AtmIvArgs;
pub use attribution::AttributionArgs;
pub use backtest::BacktestArgs;
#[cfg(feature = "experimental-cli")]
pub use campaign::CampaignArgs;
pub use common::GlobalArgs;
#[cfg(feature = "experimental-cli")]
pub use earnings::EarningsAnalysisArgs;
pub use hedging::HedgingArgs;
pub use metrics::MetricsArgs;
#[cfg(feature = "experimental-cli")]
pub use price::PriceArgs;
pub use rules::RulesArgs;
pub use selection::SelectionArgs;
pub use strategy::StrategyArgs;
pub use timing::TimingArgs;

// Re-export wrapper types
pub use option_type::OptionTypeArg;
pub use selection_type::SelectionTypeArg;
pub use spread_type::SpreadTypeArg;
