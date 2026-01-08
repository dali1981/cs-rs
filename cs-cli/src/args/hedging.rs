//! Delta hedging configuration arguments

use clap::Args;

/// Delta hedging arguments
#[derive(Debug, Clone, Args)]
pub struct HedgingArgs {
    /// Enable delta hedging
    #[arg(long)]
    pub hedge: bool,

    /// Hedging strategy: time, delta, gamma (default: delta)
    #[arg(long, default_value = "delta")]
    pub hedge_strategy: String,

    /// For time-based: rehedge interval in hours (default: 24)
    #[arg(long, default_value = "24")]
    pub hedge_interval_hours: u64,

    /// For delta-based: threshold to trigger rehedge (default: 0.10)
    #[arg(long, default_value = "0.10")]
    pub delta_threshold: f64,

    /// Maximum number of rehedges per trade
    #[arg(long)]
    pub max_rehedges: Option<usize>,

    /// Transaction cost per share (default: 0.01)
    #[arg(long, default_value = "0.01")]
    pub hedge_cost_per_share: f64,

    /// Delta computation mode: gamma, entry-hv, entry-iv, current-hv, current-iv, historical-iv (default: gamma)
    #[arg(long, default_value = "gamma")]
    pub hedge_delta_mode: String,

    /// HV window for HV-based delta modes (default: 20 days)
    #[arg(long, default_value = "20")]
    pub hv_window: u32,

    /// Enable realized volatility tracking during hedging
    #[arg(long)]
    pub track_realized_vol: bool,
}
