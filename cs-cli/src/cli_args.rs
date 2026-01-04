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
    pub straddle: Option<CliStraddle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_notional: Option<f64>,
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
pub struct CliStraddle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub straddle_entry_days: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub straddle_exit_days: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_straddle_dte: Option<i32>,
}
