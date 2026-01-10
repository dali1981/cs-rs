//! CLI arguments for entry rules
//!
//! These use Option<T> to ensure only explicitly-provided values override config.

use clap::Args;

/// Entry rules configuration
#[derive(Debug, Clone, Args, Default)]
pub struct RulesArgs {
    // ===== IV Slope Rule =====
    /// Enable IV slope entry rule (iv_short > iv_long + threshold)
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub entry_iv_slope: bool,

    /// IV slope: short-term DTE window
    #[arg(long)]
    pub iv_slope_short_dte: Option<u16>,

    /// IV slope: long-term DTE window
    #[arg(long)]
    pub iv_slope_long_dte: Option<u16>,

    /// IV slope: threshold in percentage points (e.g., 0.05 = 5pp)
    #[arg(long)]
    pub iv_slope_threshold: Option<f64>,

    // ===== IV vs HV Rule =====
    /// Enable IV vs HV comparison rule
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub entry_iv_vs_hv: bool,

    /// IV vs HV: historical volatility window in days
    #[arg(long)]
    pub iv_hv_window: Option<u16>,

    /// IV vs HV: minimum ratio (iv >= hv * ratio)
    #[arg(long)]
    pub iv_hv_ratio: Option<f64>,
}

impl RulesArgs {
    /// Check if any rule flags were provided
    pub fn has_rules(&self) -> bool {
        self.entry_iv_slope || self.entry_iv_vs_hv
    }
}
