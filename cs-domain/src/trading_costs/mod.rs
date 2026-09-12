//! Trading Costs Module
//!
//! This module provides a clean separation between pricing and trading costs.
//!
//! # Design Philosophy
//!
//! - **Pricing is pure:** Pricers return theoretical mid-price values
//! - **Costs are separate:** TradingCostCalculator computes costs independently
//! - **Results combine both:** Trade results subtract costs from P&L
//!
//! # Module Structure
//!
//! - `cost`: TradingCost value object with breakdown
//! - `context`: TradingContext for passing market data to calculators
//! - `calculator`: TradingCostCalculator trait (all cost models implement this)
//! - `config`: Configuration for cost models (serde-compatible)
//! - `models`: Concrete cost model implementations

mod calculator;
mod config;
mod context;
mod cost;
mod has_cost;
pub mod models;

pub use calculator::TradingCostCalculator;
pub use config::{CostPreset, TradingCostConfig};
pub use context::{LegContext, TradeType, TradingContext};
pub use cost::{TradeSide, TradingCost, TradingCostBreakdown};
pub use has_cost::{ApplyCosts, HasTradingCost};
