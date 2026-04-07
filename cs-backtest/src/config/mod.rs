use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use chrono::NaiveDate;
use cs_analytics::{PricingModel, InterpolationMode};
use cs_domain::{
    TimingConfig, TradeSelectionCriteria, StrikeMatchMode, HedgeConfig, AttributionConfig,
    TradingRange, TradingPeriodSpec, FilterCriteria, TradingCostConfig, FileRulesConfig, ReturnBasis,
    MarginConfig,
};
use thiserror::Error;

// Infrastructure config types (separated into submodules)
mod data_source;
mod earnings_source;
mod execution;

pub use data_source::DataSourceConfig;
pub use earnings_source::{EarningsSourceConfig, EarningsProvider};
pub use execution::ExecutionConfig;

#[derive(Debug, Error)]
pub enum TimingSpecError {
    #[error("Invalid {field} time: {hour:02}:{minute:02}")]
    InvalidTime {
        field: &'static str,
        hour: u32,
        minute: u32,
    },
    #[error("Unknown timing strategy: {0}")]
    UnknownStrategy(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    /// Market data source (options and equity)
    #[serde(default)]
    pub data_source: DataSourceConfig,
    /// DEPRECATED: Use data_source instead. Kept for backward compatibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<PathBuf>,
    /// Earnings calendar data source (file or provider-based)
    #[serde(default)]
    pub earnings_source: EarningsSourceConfig,
    /// Backtest start date
    pub start_date: NaiveDate,
    /// Backtest end date
    pub end_date: NaiveDate,
    pub timing: TimingConfig,

    // NEW: Generic timing specification (preferred over spread-specific params)
    /// Timing strategy: PreEarnings, PostEarnings, CrossEarnings, etc.
    #[serde(default)]
    pub timing_strategy: Option<String>,
    /// Entry days before event (for PreEarnings/CrossEarnings)
    #[serde(default)]
    pub entry_days_before: Option<u16>,
    /// Exit days before event (for PreEarnings)
    #[serde(default)]
    pub exit_days_before: Option<u16>,
    /// Days after event to enter (for PostEarnings)
    #[serde(default)]
    pub entry_offset: Option<i16>,
    /// Holding days (for PostEarnings/HoldingPeriod)
    #[serde(default)]
    pub holding_days: Option<u16>,
    /// Exit days after event (for CrossEarnings)
    #[serde(default)]
    pub exit_days_after: Option<u16>,

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
    /// Entry rules configuration (filters trades before/during execution)
    #[serde(default)]
    pub rules: FileRulesConfig,
    /// Return denominator used for capital-weighted metrics
    #[serde(default)]
    pub return_basis: ReturnBasis,
    /// Margin & buying power configuration (IBKR-like)
    #[serde(default)]
    pub margin: MarginConfig,
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
    /// Iron butterfly (short): sell ATM straddle + buy OTM wings (credit)
    #[serde(rename = "iron-butterfly")]
    IronButterfly,
    /// Long iron butterfly: buy ATM straddle + sell OTM wings (debit)
    #[serde(rename = "long-iron-butterfly")]
    LongIronButterfly,
    Straddle,
    /// Short straddle: sell ATM call + sell ATM put
    #[serde(rename = "short-straddle")]
    ShortStraddle,
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
            "iron_butterfly" | "ironbutterfly" | "butterfly" | "short_iron_butterfly" => SpreadType::IronButterfly,
            "long_iron_butterfly" | "longironbutterfly" | "reverse_butterfly" => SpreadType::LongIronButterfly,
            "straddle" | "long_straddle" => SpreadType::Straddle,
            "short_straddle" | "shortstraddle" => SpreadType::ShortStraddle,
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
            data_source: DataSourceConfig::default(),
            data_dir: None,
            earnings_source: EarningsSourceConfig::default(),
            // Default to 2020-01-01 to 2020-12-31 (will be overridden by CLI)
            start_date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2020, 12, 31).unwrap(),
            timing: TimingConfig::default(),
            // Generic timing fields (None = use legacy spread-specific params)
            timing_strategy: None,
            entry_days_before: None,
            exit_days_before: None,
            entry_offset: None,
            holding_days: None,
            exit_days_after: None,
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
            rules: FileRulesConfig::default(), // No entry rules by default
            return_basis: ReturnBasis::default(),
            margin: MarginConfig::default(),
        }
    }
}

impl BacktestConfig {
    /// Convert to a `RunBacktestCommand`, stripping infrastructure concerns.
    ///
    /// `data_source` and `earnings_source` are NOT included in the command —
    /// they are passed separately to the factory. The deprecated `data_dir`
    /// field is never transferred.
    ///
    /// This is the explicit Application-layer mapping described in ADR-0003.
    pub fn to_run_command(&self) -> crate::commands::RunBacktestCommand {
        crate::commands::RunBacktestCommand {
            start_date: self.start_date,
            end_date: self.end_date,
            spread: self.spread,
            selection_strategy: self.selection_strategy,
            selection: self.selection.clone(),
            timing: self.timing,
            timing_strategy: self.timing_strategy.clone(),
            entry_days_before: self.entry_days_before,
            exit_days_before: self.exit_days_before,
            entry_offset: self.entry_offset,
            holding_days: self.holding_days,
            exit_days_after: self.exit_days_after,
            symbols: self.symbols.clone(),
            min_market_cap: self.min_market_cap,
            max_entry_iv: self.max_entry_iv,
            min_notional: self.min_notional,
            min_entry_price: self.min_entry_price,
            max_entry_price: self.max_entry_price,
            parallel: self.parallel,
            pricing_model: self.pricing_model,
            vol_model: self.vol_model,
            target_delta: self.target_delta,
            delta_range: self.delta_range,
            delta_scan_steps: self.delta_scan_steps,
            strike_match_mode: self.strike_match_mode,
            wing_width: self.wing_width,
            straddle_entry_days: self.straddle_entry_days,
            straddle_exit_days: self.straddle_exit_days,
            min_straddle_dte: self.min_straddle_dte,
            post_earnings_holding_days: self.post_earnings_holding_days,
            return_basis: self.return_basis,
            margin: self.margin.clone(),
            rules: self.rules.clone(),
            hedge_config: self.hedge_config.clone(),
            attribution_config: self.attribution_config.clone(),
            trading_costs: self.trading_costs.clone(),
        }
    }

    /// Extract TradingRange (when to initiate trades)
    pub fn trading_range(&self) -> TradingRange {
        TradingRange::new(self.start_date, self.end_date)
    }

    /// Get market data source configuration
    pub fn market_data_source(&self) -> &DataSourceConfig {
        &self.data_source
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

    /// Build RulesConfig from file config + legacy filters
    ///
    /// If new-style rules are defined in `[rules]` section, uses those.
    /// Otherwise, falls back to legacy filter fields for backward compatibility.
    pub fn build_rules_config(&self) -> cs_domain::RulesConfig {
        use cs_domain::{RulesConfig, EventRule, MarketRule, TradeRule};

        // Start with defaults (empty)
        let mut config = RulesConfig::default();

        // Apply file-based rules if any
        config = config.apply_file(self.rules.clone());

        // If no event rules from file, migrate legacy filters
        if config.event.is_empty() {
            if let Some(ref symbols) = self.symbols {
                if !symbols.is_empty() {
                    config.event.push(EventRule::Symbols {
                        include: symbols.clone(),
                    });
                }
            }
            if let Some(min_cap) = self.min_market_cap {
                config.event.push(EventRule::MinMarketCap {
                    threshold: min_cap,
                });
            }
        }

        // If no market rules from file, migrate legacy filters
        if config.market.is_empty() {
            if let Some(max_iv) = self.max_entry_iv {
                config.market.push(MarketRule::MaxEntryIv {
                    threshold: max_iv,
                });
            }
            if let Some(min_ratio) = self.selection.min_iv_ratio {
                config.market.push(MarketRule::MinIvRatio {
                    short_dte: 7,
                    long_dte: 30,
                    threshold: min_ratio,
                });
            }
            if let Some(min_notional) = self.min_notional {
                config.market.push(MarketRule::MinNotional {
                    threshold: min_notional,
                });
            }
        }

        // If no trade rules from file, migrate legacy filters
        if config.trade.is_empty() {
            if self.min_entry_price.is_some() || self.max_entry_price.is_some() {
                config.trade.push(TradeRule::EntryPriceRange {
                    min: self.min_entry_price,
                    max: self.max_entry_price,
                });
            }
        }

        config
    }

    /// Build TradingPeriodSpec based on config
    ///
    /// Priority order (for backward compatibility):
    /// 1. NEW PATH: Use generic timing_strategy if provided
    /// 2. LEGACY PATH: Convert spread-specific params to timing spec
    pub fn timing_spec(&self) -> Result<TradingPeriodSpec, TimingSpecError> {
        use chrono::NaiveTime;

        // Helper to get entry/exit times from config
        let entry_time = NaiveTime::from_hms_opt(
            self.timing.entry_hour,
            self.timing.entry_minute,
            0,
        )
        .ok_or(TimingSpecError::InvalidTime {
            field: "entry_time",
            hour: self.timing.entry_hour,
            minute: self.timing.entry_minute,
        })?;
        let exit_time = NaiveTime::from_hms_opt(
            self.timing.exit_hour,
            self.timing.exit_minute,
            0,
        )
        .ok_or(TimingSpecError::InvalidTime {
            field: "exit_time",
            hour: self.timing.exit_hour,
            minute: self.timing.exit_minute,
        })?;

        // 1. NEW PATH: Use generic timing_strategy if provided
        if let Some(strategy) = &self.timing_strategy {
            return match strategy.as_str() {
                "PreEarnings" => Ok(TradingPeriodSpec::PreEarnings {
                    entry_days_before: self.entry_days_before.unwrap_or(5),
                    exit_days_before: self.exit_days_before.unwrap_or(1),
                    entry_time,
                    exit_time,
                }),
                "PostEarnings" => Ok(TradingPeriodSpec::PostEarnings {
                    entry_offset: self.entry_offset.unwrap_or(0),
                    holding_days: self.holding_days.unwrap_or(5),
                    entry_time,
                    exit_time,
                }),
                "CrossEarnings" => Ok(TradingPeriodSpec::CrossEarnings {
                    entry_days_before: self.entry_days_before.unwrap_or(1),
                    exit_days_after: self.exit_days_after.unwrap_or(1),
                    entry_time,
                    exit_time,
                }),
                _ => Err(TimingSpecError::UnknownStrategy(strategy.clone())),
            };
        }

        // 2. LEGACY PATH: Convert spread-specific params (backward compatible)
        Ok(match self.spread {
            SpreadType::Straddle | SpreadType::ShortStraddle => {
                // Pre-earnings straddle (long or short)
                TradingPeriodSpec::PreEarnings {
                    entry_days_before: self.straddle_entry_days as u16,
                    exit_days_before: self.straddle_exit_days as u16,
                    entry_time,
                    exit_time,
                }
            }

            SpreadType::PostEarningsStraddle => {
                // Post-earnings straddle
                TradingPeriodSpec::PostEarnings {
                    entry_offset: 0,
                    holding_days: self.post_earnings_holding_days as u16,
                    entry_time,
                    exit_time,
                }
            }

            // Calendar, IronButterfly, CalendarStraddle - all cross earnings
            _ => TradingPeriodSpec::CrossEarnings {
                entry_days_before: 1,
                exit_days_after: 1,
                entry_time,
                exit_time,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backtest_config_default() {
        let config = BacktestConfig::default();
        assert!(config.parallel);
        assert!(matches!(config.spread, SpreadType::Calendar));
        assert!(matches!(config.selection_strategy, SelectionType::ATM));
        // Earnings source defaults to Provider with TradingView
        assert!(!config.earnings_source.is_file());
    }

    #[test]
    fn test_backtest_config_serialization() {
        let config = BacktestConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: BacktestConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.parallel, deserialized.parallel);
        assert!(!deserialized.earnings_source.is_file());
    }
}
