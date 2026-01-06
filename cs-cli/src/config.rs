//! Layered configuration loading for cs-cli
//!
//! Configuration priority (highest to lowest):
//! 1. CLI arguments
//! 2. Strategy config file (--conf)
//! 3. System config (~/.config/cs/system.toml)
//! 4. Code defaults

use figment::{Figment, providers::{Format, Toml, Serialized}};
use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use anyhow::Result;

use cs_backtest::{SpreadType, SelectionType};
use cs_analytics::{PricingModel, InterpolationMode};
use cs_domain::{StrikeMatchMode, AttributionConfig};
use crate::cli_args::CliOverrides;

/// Full layered configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub paths: PathsConfig,
    pub timing: TimingConfig,
    pub selection: SelectionConfig,
    pub strategy: StrategyConfig,
    pub pricing: PricingConfig,
    pub hedging: HedgingConfig,
    pub attribution: AttributionConfig,
    #[serde(default)]
    pub strike_match_mode: StrikeMatchMode,
    pub symbols: Option<Vec<String>>,
    pub min_market_cap: Option<u64>,
    pub parallel: bool,
    pub max_entry_iv: Option<f64>,
    pub min_notional: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PathsConfig {
    pub data_dir: PathBuf,
    pub earnings_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TimingConfig {
    pub entry_hour: u32,
    pub entry_minute: u32,
    pub exit_hour: u32,
    pub exit_minute: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectionConfig {
    pub min_short_dte: i32,
    pub max_short_dte: i32,
    pub min_long_dte: i32,
    pub max_long_dte: i32,
    pub min_iv_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StrategyConfig {
    #[serde(default)]
    pub spread_type: SpreadType,
    #[serde(default)]
    pub selection_type: SelectionType,
    pub target_delta: f64,
    pub delta_range: (f64, f64),
    pub delta_scan_steps: usize,
    pub wing_width: f64,
    pub straddle_entry_days: usize,
    pub straddle_exit_days: usize,
    pub min_straddle_dte: i32,
    pub min_entry_price: Option<f64>,
    pub max_entry_price: Option<f64>,
    pub post_earnings_holding_days: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PricingConfig {
    #[serde(default)]
    pub model: PricingModel,
    #[serde(default)]
    pub vol_model: InterpolationMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HedgingConfig {
    pub enabled: bool,
    pub strategy: String,  // "time", "delta", "gamma"
    pub interval_hours: u64,
    pub delta_threshold: f64,
    pub max_rehedges: Option<usize>,
    pub cost_per_share: f64,
    pub delta_mode: String,  // "gamma", "entry-hv", "entry-iv", "current-hv", "current-iv", "historical-iv"
    pub hv_window: u32,  // HV lookback window in days
    pub track_realized_vol: bool,
}

// Default implementations
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            paths: PathsConfig::default(),
            timing: TimingConfig::default(),
            selection: SelectionConfig::default(),
            strategy: StrategyConfig::default(),
            pricing: PricingConfig::default(),
            hedging: HedgingConfig::default(),
            attribution: AttributionConfig::default(),
            strike_match_mode: StrikeMatchMode::default(),
            symbols: None,
            min_market_cap: None,
            parallel: true,
            max_entry_iv: None,
            min_notional: None,
        }
    }
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("data"),
            earnings_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("trading_project/nasdaq_earnings/data"),
        }
    }
}

impl Default for TimingConfig {
    fn default() -> Self {
        use cs_domain::MarketTime;
        Self {
            entry_hour: MarketTime::DEFAULT_ENTRY.hour,
            entry_minute: MarketTime::DEFAULT_ENTRY.minute,
            exit_hour: MarketTime::DEFAULT_HEDGE_CHECK.hour,
            exit_minute: MarketTime::DEFAULT_HEDGE_CHECK.minute,
        }
    }
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            min_short_dte: 3,
            max_short_dte: 45,
            min_long_dte: 14,
            max_long_dte: 90,
            min_iv_ratio: None,
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            spread_type: SpreadType::default(),
            selection_type: SelectionType::default(),
            target_delta: 0.50,
            delta_range: (0.25, 0.75),
            delta_scan_steps: 5,
            wing_width: 10.0,
            straddle_entry_days: 5,
            straddle_exit_days: 1,
            min_straddle_dte: 7,
            min_entry_price: None,
            max_entry_price: None,
            post_earnings_holding_days: 5,
        }
    }
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            model: PricingModel::default(),
            vol_model: InterpolationMode::default(),
        }
    }
}

impl Default for HedgingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: "delta".to_string(),
            interval_hours: 24,
            delta_threshold: 0.10,
            max_rehedges: None,
            cost_per_share: 0.01,
            delta_mode: "gamma".to_string(),
            hv_window: 20,
            track_realized_vol: false,
        }
    }
}

/// Load configuration with full layering
pub fn load_config(
    conf_files: &[PathBuf],
    cli_overrides: CliOverrides,
) -> Result<AppConfig> {
    let system_config = dirs::config_dir()
        .map(|p| p.join("cs/system.toml"))
        .unwrap_or_else(|| PathBuf::from("~/.config/cs/system.toml"));

    let mut figment = Figment::new()
        // 1. Code defaults (lowest priority)
        .merge(Serialized::defaults(AppConfig::default()));

    // 2. System config (if exists)
    if system_config.exists() {
        figment = figment.merge(Toml::file(&system_config));
    }

    // 3. Strategy config files (in order, each merges on top)
    for conf_path in conf_files {
        figment = figment.merge(Toml::file(conf_path));
    }

    // 4. CLI overrides (highest priority)
    figment = figment.merge(Serialized::defaults(cli_overrides));

    // Extract and post-process
    let mut config: AppConfig = figment.extract()?;

    // Expand tilde in paths
    config.paths.data_dir = expand_tilde(&config.paths.data_dir);
    config.paths.earnings_dir = expand_tilde(&config.paths.earnings_dir);

    Ok(config)
}

fn expand_tilde(path: &PathBuf) -> PathBuf {
    if path.starts_with("~") {
        if let Some(home) = dirs::home_dir() {
            let path_str = path.to_string_lossy();
            let without_tilde = path_str.strip_prefix("~").unwrap_or(&path_str);
            let without_tilde = without_tilde.strip_prefix("/").unwrap_or(without_tilde);
            return home.join(without_tilde);
        }
    }
    path.clone()
}

impl AppConfig {
    /// Convert to BacktestConfig for use by backtest use case
    pub fn to_backtest_config(&self) -> cs_backtest::BacktestConfig {
        cs_backtest::BacktestConfig {
            data_dir: self.paths.data_dir.clone(),
            earnings_dir: self.paths.earnings_dir.clone(),
            timing: cs_domain::TimingConfig {
                entry_hour: self.timing.entry_hour,
                entry_minute: self.timing.entry_minute,
                exit_hour: self.timing.exit_hour,
                exit_minute: self.timing.exit_minute,
            },
            selection: cs_domain::TradeSelectionCriteria {
                min_short_dte: self.selection.min_short_dte,
                max_short_dte: self.selection.max_short_dte,
                min_long_dte: self.selection.min_long_dte,
                max_long_dte: self.selection.max_long_dte,
                target_delta: Some(self.strategy.target_delta),
                min_iv_ratio: self.selection.min_iv_ratio,
                max_bid_ask_spread_pct: None,
            },
            spread: self.strategy.spread_type,
            selection_strategy: self.strategy.selection_type,
            symbols: self.symbols.clone(),
            min_market_cap: self.min_market_cap,
            parallel: self.parallel,
            pricing_model: self.pricing.model,
            target_delta: self.strategy.target_delta,
            delta_range: self.strategy.delta_range,
            delta_scan_steps: self.strategy.delta_scan_steps,
            vol_model: self.pricing.vol_model,
            strike_match_mode: self.strike_match_mode,
            max_entry_iv: self.max_entry_iv,
            wing_width: self.strategy.wing_width,
            straddle_entry_days: self.strategy.straddle_entry_days,
            straddle_exit_days: self.strategy.straddle_exit_days,
            min_notional: self.min_notional,
            min_straddle_dte: self.strategy.min_straddle_dte,
            min_entry_price: self.strategy.min_entry_price,
            max_entry_price: self.strategy.max_entry_price,
            post_earnings_holding_days: self.strategy.post_earnings_holding_days,
            hedge_config: self.hedging_to_domain_config(),
            attribution_config: if self.attribution.enabled {
                Some(self.attribution.clone())
            } else {
                None
            },
        }
    }

    /// Convert hedging config to domain HedgeConfig
    fn hedging_to_domain_config(&self) -> cs_domain::HedgeConfig {
        use cs_domain::{HedgeConfig, HedgeStrategy};
        use chrono::Duration;
        use rust_decimal::Decimal;

        if !self.hedging.enabled {
            return HedgeConfig::default();
        }

        let strategy = match self.hedging.strategy.to_lowercase().as_str() {
            "time" => HedgeStrategy::TimeBased {
                interval: Duration::hours(self.hedging.interval_hours as i64),
            },
            "delta" => HedgeStrategy::DeltaThreshold {
                threshold: self.hedging.delta_threshold,
            },
            "gamma" => HedgeStrategy::GammaDollar {
                threshold: self.hedging.delta_threshold * 100.0,
            },
            _ => HedgeStrategy::None,
        };

        // Parse delta computation mode
        let delta_computation = match self.hedging.delta_mode.to_lowercase().as_str() {
            "gamma" | "gamma-approximation" => cs_domain::DeltaComputation::GammaApproximation,
            "entry-hv" => cs_domain::DeltaComputation::EntryHV {
                window: self.hedging.hv_window,
            },
            "entry-iv" => cs_domain::DeltaComputation::EntryIV { _marker: () },
            "current-hv" => cs_domain::DeltaComputation::CurrentHV {
                window: self.hedging.hv_window,
            },
            "current-iv" | "current-market-iv" => cs_domain::DeltaComputation::CurrentMarketIV { _marker: () },
            "historical-iv" | "historical-average-iv" => cs_domain::DeltaComputation::HistoricalAverageIV {
                lookback_days: self.hedging.hv_window,
                _marker: (),
            },
            _ => {
                tracing::warn!("Unknown delta mode '{}', using GammaApproximation", self.hedging.delta_mode);
                cs_domain::DeltaComputation::GammaApproximation
            }
        };

        HedgeConfig {
            strategy,
            max_rehedges: self.hedging.max_rehedges,
            min_hedge_size: 1,
            transaction_cost_per_share: Decimal::try_from(self.hedging.cost_per_share).unwrap_or(Decimal::ZERO),
            contract_multiplier: 100,
            delta_computation,
            // Auto-enable RV tracking when attribution is enabled (attribution needs vol data)
            track_realized_vol: self.hedging.track_realized_vol || self.attribution.enabled,
        }
    }
}
