//! Delta hedging configuration arguments

use clap::Args;

/// Delta hedging arguments
#[derive(Debug, Clone, Args)]
pub struct HedgingArgs {
    /// Enable delta hedging
    #[arg(long)]
    pub hedge: bool,

    /// Hedging strategy: time, delta, gamma
    #[arg(long)]
    pub hedge_strategy: Option<String>,

    /// For time-based: rehedge interval in hours
    #[arg(long)]
    pub hedge_interval_hours: Option<u64>,

    /// For delta-based: threshold to trigger rehedge
    #[arg(long)]
    pub delta_threshold: Option<f64>,

    /// Maximum number of rehedges per trade
    #[arg(long)]
    pub max_rehedges: Option<usize>,

    /// Transaction cost per share
    #[arg(long)]
    pub hedge_cost_per_share: Option<f64>,

    /// Delta computation mode: gamma, entry-hv, entry-iv, current-hv, current-iv, historical-iv
    #[arg(long)]
    pub hedge_delta_mode: Option<String>,

    /// HV window for HV-based delta modes
    #[arg(long)]
    pub hv_window: Option<u32>,

    /// Enable realized volatility tracking during hedging
    #[arg(long)]
    pub track_realized_vol: bool,
}
