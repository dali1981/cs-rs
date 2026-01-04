use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use cs_analytics::{PricingModel, InterpolationMode};
use cs_domain::{TimingConfig, TradeSelectionCriteria, StrikeMatchMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub data_dir: PathBuf,
    pub earnings_dir: PathBuf,
    pub timing: TimingConfig,
    pub selection: TradeSelectionCriteria,
    pub spread: SpreadType,
    pub selection_strategy: SelectionType,
    pub symbols: Option<Vec<String>>,
    pub min_market_cap: Option<u64>,
    pub parallel: bool,
    /// Pricing IV interpolation model
    #[serde(default)]
    pub pricing_model: PricingModel,
    /// Target delta for delta strategies (default: 0.50)
    #[serde(default = "default_target_delta")]
    pub target_delta: f64,
    /// Delta range for scanning strategies (min, max)
    #[serde(default = "default_delta_range")]
    pub delta_range: (f64, f64),
    /// Number of steps for delta scanning
    #[serde(default = "default_delta_scan_steps")]
    pub delta_scan_steps: usize,
    /// Volatility interpolation mode (linear or svi)
    #[serde(default)]
    pub vol_model: InterpolationMode,
    /// Strike matching mode for calendar/diagonal spreads
    #[serde(default)]
    pub strike_match_mode: StrikeMatchMode,
    /// Maximum allowed IV at entry (filters out trades with unreliable pricing)
    /// Set to None to disable filtering. Common values: 1.5 (150%), 2.0 (200%)
    #[serde(default)]
    pub max_entry_iv: Option<f64>,
    /// Wing width for iron butterfly strategy (in dollars)
    #[serde(default = "default_wing_width")]
    pub wing_width: f64,
    /// Straddle: Entry N trading days before earnings (default: 5)
    #[serde(default = "default_straddle_entry_days")]
    pub straddle_entry_days: usize,
    /// Straddle: Exit N trading days before earnings (default: 1)
    #[serde(default = "default_straddle_exit_days")]
    pub straddle_exit_days: usize,
    /// Minimum daily option notional: sum(all option volumes for day) × 100 × stock_price
    /// Measures total dollar liquidity in options traded that day
    /// None = no filter, Some(100000.0) = $100k minimum daily option activity
    #[serde(default)]
    pub min_notional: Option<f64>,
    /// Straddle: Minimum days from entry to expiration (default: 7)
    #[serde(default = "default_min_straddle_dte")]
    pub min_straddle_dte: i32,
    /// Straddle: Minimum entry price (total debit paid for call + put)
    #[serde(default)]
    pub min_entry_price: Option<f64>,
    /// Straddle: Maximum entry price (caps max loss exposure)
    #[serde(default)]
    pub max_entry_price: Option<f64>,
}

fn default_wing_width() -> f64 {
    10.0
}

fn default_straddle_entry_days() -> usize {
    5
}

fn default_straddle_exit_days() -> usize {
    1
}

fn default_target_delta() -> f64 {
    0.50
}

fn default_delta_range() -> (f64, f64) {
    (0.25, 0.75)
}

fn default_delta_scan_steps() -> usize {
    5
}

fn default_min_straddle_dte() -> i32 {
    7 // At least 7 days from entry to expiration
}

/// Trade structure - WHAT to trade
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpreadType {
    #[default]
    Calendar,
    IronButterfly,
    Straddle,
}

impl SpreadType {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().replace('-', "_").as_str() {
            "iron_butterfly" | "ironbutterfly" | "butterfly" => SpreadType::IronButterfly,
            "straddle" | "long_straddle" => SpreadType::Straddle,
            _ => SpreadType::Calendar,
        }
    }
}

/// Selection method - HOW to select strikes/expirations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionType {
    #[default]
    ATM,
    /// Fixed delta strategy (uses target_delta)
    Delta,
    /// Scanning delta strategy (scans delta_range for best opportunity)
    DeltaScan,
}

impl SelectionType {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().replace('-', "_").as_str() {
            "delta" => SelectionType::Delta,
            "delta_scan" | "deltascan" => SelectionType::DeltaScan,
            _ => SelectionType::ATM,
        }
    }
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("data"),
            earnings_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("trading_project/nasdaq_earnings/data"),
            timing: TimingConfig::default(),
            selection: TradeSelectionCriteria::default(),
            spread: SpreadType::Calendar,
            selection_strategy: SelectionType::ATM,
            symbols: None,
            min_market_cap: None,
            parallel: true,
            pricing_model: PricingModel::default(),
            target_delta: default_target_delta(),
            delta_range: default_delta_range(),
            delta_scan_steps: default_delta_scan_steps(),
            vol_model: InterpolationMode::default(),
            strike_match_mode: StrikeMatchMode::default(),
            max_entry_iv: None, // No filtering by default
            wing_width: default_wing_width(),
            straddle_entry_days: default_straddle_entry_days(),
            straddle_exit_days: default_straddle_exit_days(),
            min_notional: None, // No filtering by default
            min_straddle_dte: default_min_straddle_dte(),
            min_entry_price: None, // No filtering by default
            max_entry_price: None, // No filtering by default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backtest_config_default() {
        let config = BacktestConfig::default();
        assert_eq!(config.data_dir, PathBuf::from("data"));
        assert!(config.parallel);
        assert!(matches!(config.spread, SpreadType::Calendar));
        assert!(matches!(config.selection_strategy, SelectionType::ATM));
    }

    #[test]
    fn test_backtest_config_serialization() {
        let config = BacktestConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: BacktestConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.data_dir, deserialized.data_dir);
        assert_eq!(config.parallel, deserialized.parallel);
    }
}
