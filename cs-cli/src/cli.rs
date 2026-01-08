//! Unified CLI command structure with flattened arguments

use clap::{Parser, Subcommand};

use crate::args::{
    GlobalArgs, BacktestArgs, AtmIvArgs, EarningsAnalysisArgs, CampaignArgs,
    PriceArgs, AnalyzeArgs,
};

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

    /// Analyze results from a run
    #[command(about = "Analyze backtest results")]
    Analyze(AnalyzeArgs),

    /// Price a single spread (for debugging)
    #[command(about = "Price a single spread")]
    Price(PriceArgs),

    /// Generate ATM IV time series for earnings detection
    #[command(about = "Generate ATM IV time series")]
    AtmIv(AtmIvArgs),

    /// Analyze expected vs actual moves on earnings events
    #[command(about = "Analyze earnings event impacts")]
    EarningsAnalysis(EarningsAnalysisArgs),

    /// Run campaign-based backtest (declarative scheduling)
    #[command(about = "Run campaign-based backtest")]
    Campaign(CampaignArgs),
}
