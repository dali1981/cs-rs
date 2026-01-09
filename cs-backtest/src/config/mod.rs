use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use chrono::NaiveDate;
use cs_analytics::{PricingModel, InterpolationMode};
use cs_domain::{
    TimingConfig, TradeSelectionCriteria, StrikeMatchMode, HedgeConfig, AttributionConfig,
    TradingRange, TradingPeriodSpec, FilterCriteria, TradingCostConfig,
};

// Infrastructure config types (separated into submodules)
mod data_source;
mod execution;

pub use data_source::DataSourceConfig;
pub use execution::ExecutionConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub data_dir: PathBuf,
    pub earnings_dir: PathBuf,
    /// Optional earnings file (takes precedence over earnings_dir)
    #[serde(default)]
    pub earnings_file: Option<PathBuf>,
    /// Backtest start date
    pub start_date: NaiveDate,
    /// Backtest end date
    pub end_date: NaiveDate,
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
    /// Post-earnings straddle: holding period in trading days (default: 5)
    #[serde(default = "default_post_earnings_holding_days")]
    pub post_earnings_holding_days: usize,
    /// Delta hedging configuration
    #[serde(default)]
    pub hedge_config: HedgeConfig,
    /// P&L attribution configuration (optional)
    #[serde(default)]
    pub attribution_config: Option<AttributionConfig>,
    /// Trading costs configuration (slippage + commission)
    #[serde(default)]
    pub trading_costs: TradingCostConfig,
}

fn default_wing_width() -> f64 {
    10.0
}

fn default_post_earnings_holding_days() -> usize {
    5  // 1 trading week
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
#[serde(rename_all = "kebab-case")]
pub enum SpreadType {
    #[default]
    Calendar,
    #[serde(rename = "iron-butterfly")]
    IronButterfly,
    Straddle,
    /// Calendar straddle: short near-term straddle + long far-term straddle
    #[serde(rename = "calendar-straddle")]
    CalendarStraddle,
    /// Post-earnings straddle: enter day after earnings, hold for ~1 week
    #[serde(rename = "post-earnings-straddle")]
    PostEarningsStraddle,
}

impl SpreadType {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().replace('-', "_").as_str() {
            "iron_butterfly" | "ironbutterfly" | "butterfly" => SpreadType::IronButterfly,
            "straddle" | "long_straddle" => SpreadType::Straddle,
            "calendar_straddle" | "calendarstraddle" => SpreadType::CalendarStraddle,
            "post_earnings_straddle" | "postearningstraddle" | "post_straddle" => SpreadType::PostEarningsStraddle,
            _ => SpreadType::Calendar,
        }
    }
}

/// Selection method - HOW to select strikes/expirations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionType {
    #[default]
    #[serde(rename = "atm")]
    ATM,
    /// Fixed delta strategy (uses target_delta)
    Delta,
    /// Scanning delta strategy (scans delta_range for best opportunity)
    #[serde(rename = "delta-scan")]
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
            earnings_file: None,
            // Default to 2020-01-01 to 2020-12-31 (will be overridden by CLI)
            start_date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2020, 12, 31).unwrap(),
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
            post_earnings_holding_days: default_post_earnings_holding_days(),
            hedge_config: HedgeConfig::default(), // No hedging by default
            attribution_config: None, // No attribution by default
            trading_costs: TradingCostConfig::default(), // No costs by default (explicit opt-in)
        }
    }
}

impl BacktestConfig {
    /// Extract TradingRange (when to initiate trades)
    pub fn trading_range(&self) -> TradingRange {
        TradingRange::new(self.start_date, self.end_date)
    }

    /// Extract DataSourceConfig (infrastructure)
    pub fn data_source(&self) -> DataSourceConfig {
        DataSourceConfig {
            data_dir: self.data_dir.clone(),
            earnings_dir: self.earnings_dir.clone(),
            earnings_file: self.earnings_file.clone(),
        }
    }

    /// Extract ExecutionConfig (runtime)
    pub fn execution(&self) -> ExecutionConfig {
        ExecutionConfig {
            parallel: self.parallel,
        }
    }

    /// Extract FilterCriteria (event/trade filtering)
    pub fn filter_criteria(&self) -> FilterCriteria {
        FilterCriteria {
            symbols: self.symbols.clone(),
            min_market_cap: self.min_market_cap,
            max_entry_iv: self.max_entry_iv,
            min_notional: self.min_notional,
            min_entry_price: self.min_entry_price,
            max_entry_price: self.max_entry_price,
            min_iv_ratio: self.selection.min_iv_ratio,
        }
    }

    /// Build TradingPeriodSpec based on spread type and config
    ///
    /// This converts spread-specific timing parameters into a unified spec.
    pub fn timing_spec(&self) -> TradingPeriodSpec {
        use chrono::NaiveTime;

        match self.spread {
            SpreadType::Straddle => {
                // Pre-earnings straddle
                TradingPeriodSpec::PreEarnings {
                    entry_days_before: self.straddle_entry_days as u16,
                    exit_days_before: self.straddle_exit_days as u16,
                    entry_time: NaiveTime::from_hms_opt(
                        self.timing.entry_hour,
                        self.timing.entry_minute,
                        0,
                    )
                    .unwrap(),
                    exit_time: NaiveTime::from_hms_opt(
                        self.timing.exit_hour,
                        self.timing.exit_minute,
                        0,
                    )
                    .unwrap(),
                }
            }

            SpreadType::PostEarningsStraddle => {
                // Post-earnings straddle
                TradingPeriodSpec::PostEarnings {
                    entry_offset: 0,
                    holding_days: self.post_earnings_holding_days as u16,
                    entry_time: NaiveTime::from_hms_opt(
                        self.timing.entry_hour,
                        self.timing.entry_minute,
                        0,
                    )
                    .unwrap(),
                    exit_time: NaiveTime::from_hms_opt(
                        self.timing.exit_hour,
                        self.timing.exit_minute,
                        0,
                    )
                    .unwrap(),
                }
            }

            // Calendar, IronButterfly, CalendarStraddle - all cross earnings
            _ => TradingPeriodSpec::CrossEarnings {
                entry_days_before: 1,
                exit_days_after: 1,
                entry_time: NaiveTime::from_hms_opt(
                    self.timing.entry_hour,
                    self.timing.entry_minute,
                    0,
                )
                .unwrap(),
                exit_time: NaiveTime::from_hms_opt(
                    self.timing.exit_hour,
                    self.timing.exit_minute,
                    0,
                )
                .unwrap(),
            },
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
