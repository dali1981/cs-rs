//! Command handler implementations
//!
//! Individual command handlers for the CLI using async trait pattern.
//! Each command has a dedicated handler module:
//! - backtest: Run backtest simulations
//! - [experimental] atm_iv: Generate IV time series
//! - [experimental] earnings: Analyze earnings event impacts
//! - [experimental] campaign: Run campaign-based backtests
//! - [experimental] price: Price a single spread
//! - [experimental] analyze: Analyze backtest results
//!
//! Non-canonical command handlers are gated behind `experimental-cli`.

#[cfg(feature = "experimental-cli")]
pub mod analyze;
#[cfg(feature = "experimental-cli")]
pub mod atm_iv;
pub mod backtest;
#[cfg(feature = "experimental-cli")]
pub mod campaign;
#[cfg(feature = "experimental-cli")]
pub mod earnings;
pub mod handler;
#[cfg(feature = "experimental-cli")]
pub mod price;

pub use handler::CommandHandler;
