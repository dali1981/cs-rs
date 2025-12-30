use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use cs_analytics::IVModel;
use cs_domain::{TimingConfig, TradeSelectionCriteria};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub data_dir: PathBuf,
    pub timing: TimingConfig,
    pub selection: TradeSelectionCriteria,
    pub strategy: StrategyType,
    pub symbols: Option<Vec<String>>,
    pub min_market_cap: Option<u64>,
    pub parallel: bool,
    /// IV interpolation model for pricing
    #[serde(default)]
    pub iv_model: IVModel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StrategyType {
    ATM,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("data"),
            timing: TimingConfig::default(),
            selection: TradeSelectionCriteria::default(),
            strategy: StrategyType::ATM,
            symbols: None,
            min_market_cap: None,
            parallel: true,
            iv_model: IVModel::default(),
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
