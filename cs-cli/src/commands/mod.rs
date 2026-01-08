//! Command handler implementations
//!
//! Individual command handlers for the CLI using async trait pattern.
//! Each command has a dedicated handler module:
//! - backtest: Run backtest simulations
//! - atm_iv: Generate IV time series
//! - earnings: Analyze earnings event impacts
//! - campaign: Run campaign-based backtests
//! - price: Price a single spread
//! - analyze: Analyze backtest results

pub mod handler;
pub mod backtest;
pub mod atm_iv;
pub mod earnings;
pub mod campaign;
pub mod price;
pub mod analyze;

pub use handler::CommandHandler;
