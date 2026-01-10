//! Entry rules for trade filtering
//!
//! Rules are evaluated at different stages of the backtest pipeline:
//! - Event-level: filter by earnings event metadata (no market data needed)
//! - Market-level: filter by IV surface, spot, chain data
//! - Trade-level: filter by trade execution result
//!
//! All rules at each level are evaluated with AND logic (all must pass).

mod error;
mod config;
mod event;
mod market;
mod trade;

pub use error::RuleError;
pub use config::{RulesConfig, FileRulesConfig};
pub use event::EventRule;
pub use market::MarketRule;
pub use trade::TradeRule;

/// Rule evaluation context level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleLevel {
    /// Only needs EarningsEvent metadata
    Event,
    /// Needs PreparedData (IV surface, spot, chain)
    Market,
    /// Needs trade execution result
    Trade,
}
