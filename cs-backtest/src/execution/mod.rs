//! Trade execution types and traits
//!
//! This module provides:
//! - `ExecutableTrade` trait for trade types that can be simulated
//! - `TradePricer` trait for pricing trades
//! - `ExecutionConfig` for simulation parameters
//! - `SimulationOutput` for raw simulation results
//!
//! The actual simulation is done by `TradeSimulator` in `backtest_use_case_helpers`.

pub(crate) mod cost_helpers;
pub(crate) mod helpers;
mod traits;
mod types;

// Trade implementations (ExecutableTrade impls)
pub(crate) mod butterfly_impl;
pub(crate) mod calendar_spread_impl;
pub(crate) mod calendar_straddle_impl;
pub(crate) mod condor_impl;
pub(crate) mod iron_butterfly_impl;
pub(crate) mod iron_condor_impl;
pub(crate) mod straddle_impl;
pub(crate) mod strangle_impl;

pub use helpers::run_batch;
pub use traits::{ExecutableTrade, TradePricer};
pub use types::{ExecutionConfig, ExecutionError, SimulationOutput};
