//! Delta computation strategies for hedging
//!
//! This module provides different implementations of the `DeltaProvider` trait,
//! allowing flexible delta computation methods for option hedging.
//!
//! # Available Providers
//!
//! - `GammaApproximationProvider`: Fast incremental updates using gamma approximation
//! - `EntryVolatilityProvider`: Recompute from Black-Scholes with fixed volatility (EntryHV or EntryIV)
//! - `CurrentHVProvider`: Recompute using current historical volatility
//! - `CurrentMarketIVProvider`: Build fresh IV surface at each rehedge
//! - `HistoricalAverageIVProvider`: Use averaged IV over lookback period

mod common;
mod current_hv;
mod current_market_iv;
mod entry_volatility;
mod gamma_approximation;
mod historical_average_iv;

pub use current_hv::CurrentHVProvider;
pub use current_market_iv::CurrentMarketIVProvider;
pub use entry_volatility::EntryVolatilityProvider;
pub use gamma_approximation::GammaApproximationProvider;
pub use historical_average_iv::HistoricalAverageIVProvider;
