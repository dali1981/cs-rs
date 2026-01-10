//! Campaign-based backtest command arguments

use clap::Args;
use std::path::PathBuf;
use super::HedgingArgs;

/// Arguments for the campaign command
#[derive(Debug, Clone, Args)]
pub struct CampaignArgs {
    /// Symbols to trade
    #[arg(long, num_args = 1.., required = true)]
    pub symbols: Vec<String>,

    /// Strategy (calendar-spread, straddle, iron-butterfly)
    #[arg(long)]
    pub strategy: String,

    /// Period policy (earnings-only, inter-earnings, pre-earnings, post-earnings, continuous)
    #[arg(long)]
    pub period_policy: Option<String>,

    /// Roll policy (none, weekly, monthly)
    #[arg(long)]
    pub roll_policy: Option<String>,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    pub start: String,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    pub end: String,

    /// Custom earnings file (parquet or JSON)
    #[arg(long)]
    pub earnings_file: Option<PathBuf>,

    /// Entry time in HH:MM format
    #[arg(long)]
    pub entry_time: Option<String>,

    /// Exit time in HH:MM format
    #[arg(long)]
    pub exit_time: Option<String>,

    /// Days before earnings to enter (for pre-earnings policy)
    #[arg(long)]
    pub entry_days_before: Option<u16>,

    /// Days after earnings to exit (for cross-earnings policy)
    #[arg(long)]
    pub exit_days_after: Option<u16>,

    /// Days after earnings to start inter-period trading
    #[arg(long)]
    pub inter_entry_days_after: Option<u16>,

    /// Days before next earnings to stop inter-period trading
    #[arg(long)]
    pub inter_exit_days_before: Option<u16>,

    /// Roll day for weekly policy (Mon, Tue, Wed, Thu, Fri)
    #[arg(long)]
    pub roll_day: Option<String>,

    /// Volatility strategy type: iron-butterfly, strangle, butterfly, condor, iron-condor
    #[arg(long)]
    pub vol_strategy: Option<String>,

    /// Wing selection mode for multi-leg strategies (delta:0.25 or moneyness:0.10)
    #[arg(long)]
    pub wing_mode: Option<String>,

    /// Trade direction (short or long) - applies to all strategies
    #[arg(long)]
    pub direction: Option<String>,

    /// Output file path (CSV format)
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Output directory for detailed JSON files (one per symbol)
    #[arg(long)]
    pub output_dir: Option<PathBuf>,

    /// Enable P&L attribution
    #[arg(long)]
    pub attribution: bool,

    /// Flattened argument groups
    #[command(flatten)]
    pub hedging: HedgingArgs,
}
