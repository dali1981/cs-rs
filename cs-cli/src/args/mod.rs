//! CLI argument definitions using flattened groups
//!
//! This module provides a modular approach to CLI arguments by grouping
//! related arguments into separate structs and using #[command(flatten)]
//! to compose them into command-specific argument structures.

pub mod common;
pub mod timing;
pub mod selection;
pub mod strategy;
pub mod hedging;
pub mod attribution;
pub mod rules;
pub mod backtest;
pub mod atm_iv;
pub mod earnings;
pub mod campaign;
pub mod price;
pub mod analyze;

// CLI wrapper types with ValueEnum (convert to domain types)
pub mod spread_type;
pub mod selection_type;
pub mod option_type;

pub use common::GlobalArgs;
pub use timing::TimingArgs;
pub use selection::SelectionArgs;
pub use strategy::StrategyArgs;
pub use hedging::HedgingArgs;
pub use attribution::AttributionArgs;
pub use rules::RulesArgs;
pub use backtest::BacktestArgs;
pub use atm_iv::AtmIvArgs;
pub use earnings::EarningsAnalysisArgs;
pub use campaign::CampaignArgs;
pub use price::PriceArgs;
pub use analyze::AnalyzeArgs;

// Re-export wrapper types
pub use spread_type::SpreadTypeArg;
pub use selection_type::SelectionTypeArg;
pub use option_type::OptionTypeArg;
