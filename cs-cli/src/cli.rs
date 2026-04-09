//! Unified CLI command structure with flattened arguments

use clap::{Parser, Subcommand};

#[cfg(feature = "experimental-cli")]
use crate::args::{AnalyzeArgs, AtmIvArgs, CampaignArgs, EarningsAnalysisArgs, PriceArgs};
use crate::args::{BacktestArgs, GlobalArgs};

/// Calendar Spread Backtest CLI
#[derive(Parser)]
#[command(name = "cs")]
#[command(about = "Calendar Spread Backtest CLI")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[command(flatten)]
    pub global: GlobalArgs,
}

/// CLI subcommands
#[derive(Subcommand)]
pub enum Commands {
    /// Run backtest
    #[command(about = "Run backtest simulation")]
    Backtest(BacktestArgs),

    /// [experimental] Analyze results from a run
    #[cfg(feature = "experimental-cli")]
    #[command(about = "[experimental] Analyze backtest results")]
    Analyze(AnalyzeArgs),

    /// [experimental] Price a single spread (for debugging)
    #[cfg(feature = "experimental-cli")]
    #[command(about = "[experimental] Price a single spread")]
    Price(PriceArgs),

    /// [experimental] Generate ATM IV time series for earnings detection
    #[cfg(feature = "experimental-cli")]
    #[command(about = "[experimental] Generate ATM IV time series")]
    AtmIv(AtmIvArgs),

    /// [experimental] Analyze expected vs actual moves on earnings events
    #[cfg(feature = "experimental-cli")]
    #[command(about = "[experimental] Analyze earnings event impacts")]
    EarningsAnalysis(EarningsAnalysisArgs),

    /// [experimental] Run campaign-based backtest (declarative scheduling)
    #[cfg(feature = "experimental-cli")]
    #[command(about = "[experimental] Run campaign-based backtest")]
    Campaign(CampaignArgs),
}
