//! P&L attribution analysis arguments

use clap::Args;

/// P&L attribution arguments
#[derive(Debug, Clone, Args)]
pub struct AttributionArgs {
    /// Enable P&L attribution (requires hedging to be enabled)
    #[arg(long)]
    pub attribution: bool,

    /// Volatility source for attribution
    #[arg(long, default_value = "current_market_iv")]
    pub attribution_vol_source: String,

    /// Attribution snapshot times: open_close, close_only (default: open_close)
    #[arg(long, default_value = "open_close")]
    pub attribution_snapshots: String,
}
