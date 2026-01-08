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
    #[arg(long, default_value = "pre-earnings")]
    pub period_policy: String,

    /// Roll policy (none, weekly, monthly)
    #[arg(long, default_value = "none")]
    pub roll_policy: String,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    pub start: String,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    pub end: String,

    /// Custom earnings file (parquet or JSON)
    #[arg(long)]
    pub earnings_file: Option<PathBuf>,

    /// Entry time in HH:MM format (default: 09:35)
    #[arg(long, default_value = "09:35")]
    pub entry_time: String,

    /// Exit time in HH:MM format (default: 15:55)
    #[arg(long, default_value = "15:55")]
    pub exit_time: String,

    /// Days before earnings to enter (for pre-earnings policy)
    #[arg(long)]
    pub entry_days_before: Option<u16>,

    /// Days after earnings to exit (for cross-earnings policy)
    #[arg(long)]
    pub exit_days_after: Option<u16>,

    /// Days after earnings to start inter-period trading
    #[arg(long, default_value = "2")]
    pub inter_entry_days_after: u16,

    /// Days before next earnings to stop inter-period trading
    #[arg(long, default_value = "3")]
    pub inter_exit_days_before: u16,

    /// Roll day for weekly policy (Mon, Tue, Wed, Thu, Fri)
    #[arg(long, default_value = "Fri")]
    pub roll_day: String,

    /// Volatility strategy type: iron-butterfly, strangle, butterfly, condor, iron-condor
    #[arg(long, default_value = "iron-butterfly")]
    pub vol_strategy: String,

    /// Wing selection mode for multi-leg strategies (delta:0.25 or moneyness:0.10)
    #[arg(long)]
    pub wing_mode: Option<String>,

    /// Trade direction (short or long) - applies to all strategies
    #[arg(long, default_value = "short")]
    pub direction: String,

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
