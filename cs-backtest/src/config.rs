use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use cs_analytics::{PricingModel, InterpolationMode};
use cs_domain::{TimingConfig, TradeSelectionCriteria};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub data_dir: PathBuf,
    pub earnings_dir: PathBuf,
    pub timing: TimingConfig,
    pub selection: TradeSelectionCriteria,
    pub strategy: StrategyType,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StrategyType {
    #[default]
    ATM,
    /// Fixed delta strategy (uses target_delta)
    Delta,
    /// Scanning delta strategy (scans delta_range for best opportunity)
    DeltaScan,
}

impl StrategyType {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().replace('-', "_").as_str() {
            "delta" => StrategyType::Delta,
            "delta_scan" | "deltascan" => StrategyType::DeltaScan,
            _ => StrategyType::ATM,
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
            strategy: StrategyType::ATM,
            symbols: None,
            min_market_cap: None,
            parallel: true,
            pricing_model: PricingModel::default(),
            target_delta: default_target_delta(),
            delta_range: default_delta_range(),
            delta_scan_steps: default_delta_scan_steps(),
            vol_model: InterpolationMode::default(),
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
        assert!(matches!(config.strategy, StrategyType::ATM));
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
