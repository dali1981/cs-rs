//! P&L attribution analysis arguments

use clap::Args;

/// P&L attribution arguments
#[derive(Debug, Clone, Args)]
pub struct AttributionArgs {
    /// Enable P&L attribution (requires hedging to be enabled)
    #[arg(long)]
    pub attribution: bool,

    /// Volatility source for attribution
    #[arg(long)]
    pub attribution_vol_source: Option<String>,

    /// Attribution snapshot times: open_close, close_only
    #[arg(long)]
    pub attribution_snapshots: Option<String>,
}
