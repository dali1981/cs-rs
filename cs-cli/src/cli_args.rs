//! CLI argument overrides for figment merging
//!
//! All fields are Option<T> with skip_serializing_if to ensure
//! only explicitly-provided CLI args override config file values.

use serde::Serialize;
use std::path::PathBuf;

/// CLI overrides - only serializes fields that were actually provided
#[derive(Debug, Clone, Serialize, Default)]
#[serde(default)]
pub struct CliOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<CliPaths>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<CliTiming>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<CliSelection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<CliStrategy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing: Option<CliPricing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hedging: Option<CliHedging>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<CliAttribution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<CliMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike_match_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_market_cap: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_entry_iv: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_notional: Option<f64>,
    /// Rules are handled separately (not via figment) due to complex structure
    #[serde(skip)]
    pub rules: Option<CliRules>,
}

/// CLI rules configuration (applied post-figment)
#[derive(Debug, Clone, Default)]
pub struct CliRules {
    /// Enable IV slope rule
    pub iv_slope_enabled: bool,
    /// IV slope short-term DTE
    pub iv_slope_short_dte: Option<u16>,
    /// IV slope long-term DTE
    pub iv_slope_long_dte: Option<u16>,
    /// IV slope threshold in percentage points
    pub iv_slope_threshold: Option<f64>,
    /// Enable IV vs HV rule
    pub iv_vs_hv_enabled: bool,
    /// HV window for IV vs HV rule
    pub iv_hv_window: Option<u16>,
    /// Minimum IV/HV ratio
    pub iv_hv_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(default)]
pub struct CliPaths {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earnings_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(default)]
pub struct CliTiming {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_hour: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_minute: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_hour: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_minute: Option<u32>,
    // Generic timing specification fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_days_before: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_days_before: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_offset: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holding_days: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_days_after: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(default)]
pub struct CliSelection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_short_dte: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_short_dte: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_long_dte: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_long_dte: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_iv_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(default)]
pub struct CliStrategy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spread_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_range: Option<(f64, f64)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_scan_steps: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wing_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub straddle_entry_days: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub straddle_exit_days: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_straddle_dte: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_entry_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_entry_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_earnings_holding_days: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(default)]
pub struct CliPricing {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vol_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(default)]
pub struct CliHedging {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_hours: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rehedges: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_per_share: Option<f64>,
    /// Delta computation mode: gamma, entry-hv, entry-iv, current-hv, current-iv, historical-iv
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_mode: Option<String>,
    /// HV window for HV-based delta modes (default: 20 days)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hv_window: Option<u32>,
    /// Enable realized volatility tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_realized_vol: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(default)]
pub struct CliAttribution {
    /// Enable P&L attribution (requires hedging to be enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Volatility source for Greeks recomputation: current_market_iv, current_hv, entry_iv, entry_hv, historical_average_iv
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vol_source: Option<String>,
    /// Snapshot times: open_close or close_only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_times: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(default)]
pub struct CliMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_basis: Option<String>,
}
