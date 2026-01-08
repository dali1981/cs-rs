//! Strike selection and filtering arguments

use clap::Args;

/// Strike selection and filtering arguments
#[derive(Debug, Clone, Args)]
pub struct SelectionArgs {
    /// Minimum short DTE
    #[arg(long)]
    pub min_short_dte: Option<i32>,

    /// Maximum short DTE
    #[arg(long)]
    pub max_short_dte: Option<i32>,

    /// Minimum long DTE
    #[arg(long)]
    pub min_long_dte: Option<i32>,

    /// Maximum long DTE
    #[arg(long)]
    pub max_long_dte: Option<i32>,

    /// Target delta
    #[arg(long)]
    pub target_delta: Option<f64>,

    /// Minimum IV ratio (long/short)
    #[arg(long)]
    pub min_iv_ratio: Option<f64>,

    /// Minimum market cap filter
    #[arg(long)]
    pub min_market_cap: Option<u64>,

    /// Minimum daily option notional
    #[arg(long)]
    pub min_notional: Option<f64>,

    /// Maximum allowed IV at entry
    #[arg(long)]
    pub max_entry_iv: Option<f64>,
}
