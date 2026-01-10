//! Backtest command arguments

use clap::Args;
use std::path::PathBuf;
use super::{
    TimingArgs, SelectionArgs, StrategyArgs, HedgingArgs, AttributionArgs, RulesArgs, MetricsArgs,
};

/// Arguments for the backtest command
#[derive(Debug, Clone, Args)]
pub struct BacktestArgs {
    /// Configuration file(s) - can specify multiple, each merges on top of previous
    #[arg(long, short = 'c')]
    pub conf: Vec<PathBuf>,

    /// Earnings data directory (for earnings-rs adapter)
    #[arg(long, env = "EARNINGS_DATA_DIR", conflicts_with = "earnings_file")]
    pub earnings_dir: Option<PathBuf>,

    /// Custom earnings file (parquet or JSON) - alternative to --earnings-dir
    #[arg(long, conflicts_with = "earnings_dir")]
    pub earnings_file: Option<PathBuf>,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    pub start: String,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    pub end: String,

    /// Filter to specific symbols
    #[arg(long)]
    pub symbols: Option<Vec<String>>,

    /// Output file path
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Pricing IV interpolation model (sticky-strike, sticky-moneyness, sticky-delta)
    #[arg(long)]
    pub pricing_model: Option<String>,

    /// Volatility interpolation mode (linear, svi)
    #[arg(long)]
    pub vol_model: Option<String>,

    /// Strike matching mode (same-strike, same-delta)
    #[arg(long)]
    pub strike_match_mode: Option<String>,

    /// Disable parallel processing
    #[arg(long)]
    pub no_parallel: bool,

    /// Flattened argument groups
    #[command(flatten)]
    pub timing: TimingArgs,

    #[command(flatten)]
    pub selection: SelectionArgs,

    #[command(flatten)]
    pub strategy: StrategyArgs,

    #[command(flatten)]
    pub hedging: HedgingArgs,

    #[command(flatten)]
    pub attribution: AttributionArgs,

    #[command(flatten)]
    pub rules: RulesArgs,

    #[command(flatten)]
    pub metrics: MetricsArgs,
}
