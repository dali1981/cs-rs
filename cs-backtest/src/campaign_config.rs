//! Campaign configuration

use std::path::PathBuf;
use chrono::NaiveDate;

use cs_domain::{
    TimingConfig, OptionStrategy, TradeDirection,
    value_objects::{IronButterflyConfig, MultiLegStrategyConfig},
    PeriodPolicy, ExpirationPolicy,
};
use crate::config::EarningsSourceConfig;

/// Configuration for running a trading campaign
#[derive(Debug, Clone)]
pub struct CampaignConfig {
    // Data sources
    pub data_dir: PathBuf,
    /// Unified earnings source configuration (file or provider-based)
    pub earnings_source: EarningsSourceConfig,

    // Campaign parameters
    pub symbols: Vec<String>,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,

    // Strategy
    pub strategy: OptionStrategy,
    pub trade_direction: TradeDirection,

    // Timing
    pub timing: TimingConfig,

    // Period and expiration policies
    pub period_policy: PeriodPolicy,
    pub expiration_policy: ExpirationPolicy,

    // Strategy-specific config
    pub iron_butterfly_config: Option<IronButterflyConfig>,
    pub multi_leg_strategy_config: Option<MultiLegStrategyConfig>,

    // Execution options
    pub parallel: bool,
}


fn default_true() -> bool {
    true
}

impl Default for CampaignConfig {
    fn default() -> Self {
        use chrono::NaiveDate;
        use cs_domain::{RollPolicy, TradingPeriodSpec};

        Self {
            data_dir: PathBuf::from("data"),
            earnings_source: EarningsSourceConfig::default(),
            symbols: Vec::new(),
            start_date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2020, 12, 31).unwrap(),
            strategy: OptionStrategy::IronButterfly,
            trade_direction: TradeDirection::Short,
            timing: TimingConfig::default(),
            // Default: Pre-earnings trading, 14 days before earnings
            period_policy: PeriodPolicy::EarningsOnly {
                timing: TradingPeriodSpec::PreEarnings {
                    entry_days_before: 14,
                    exit_days_before: 1,
                    entry_time: chrono::NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
                    exit_time: chrono::NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
                },
            },
            // Default: First expiration after start date (will be updated by campaign)
            expiration_policy: ExpirationPolicy::FirstAfter {
                min_date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            },
            iron_butterfly_config: None,
            multi_leg_strategy_config: None,
            parallel: true,
        }
    }
}
