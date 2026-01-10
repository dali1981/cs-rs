//! Strategy-specific arguments

use clap::Args;
use super::{SpreadTypeArg, SelectionTypeArg, OptionTypeArg};

/// Strategy configuration arguments
#[derive(Debug, Clone, Args)]
pub struct StrategyArgs {
    /// Trade structure
    #[arg(long)]
    pub spread: Option<SpreadTypeArg>,

    /// Strike selection method
    #[arg(long)]
    pub selection: Option<SelectionTypeArg>,

    /// Option type (call/put) - required for calendar spreads only
    #[arg(long)]
    pub option_type: Option<OptionTypeArg>,

    /// Delta range for delta-scan strategy (format: "0.25,0.75")
    #[arg(long)]
    pub delta_range: Option<String>,

    /// Number of delta steps for delta-scan strategy
    #[arg(long)]
    pub delta_scan_steps: Option<usize>,

    /// Wing width for iron butterfly strategy
    #[arg(long)]
    pub wing_width: Option<f64>,

    /// Straddle: Entry N trading days before earnings
    #[arg(long)]
    pub straddle_entry_days: Option<usize>,

    /// Straddle: Exit N trading days before earnings
    #[arg(long)]
    pub straddle_exit_days: Option<usize>,

    /// Straddle: Minimum days from entry to expiration
    #[arg(long)]
    pub min_straddle_dte: Option<i32>,

    /// Straddle: Minimum entry price
    #[arg(long)]
    pub min_entry_price: Option<f64>,

    /// Straddle: Maximum entry price
    #[arg(long)]
    pub max_entry_price: Option<f64>,

    /// Post-earnings straddle: holding period in trading days
    #[arg(long)]
    pub post_earnings_holding_days: Option<usize>,

    /// Rolling strategy (weekly, monthly, or days:N)
    #[arg(long)]
    pub roll_strategy: Option<String>,

    /// Day of week for weekly rolls (monday, tuesday, ..., friday)
    #[arg(long)]
    pub roll_day: Option<String>,
}
