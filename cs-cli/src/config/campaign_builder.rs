//! CampaignConfig builder from CLI args

use anyhow::{Context, Result};
use chrono::NaiveDate;

use cs_backtest::CampaignConfig;
use cs_domain::{PeriodPolicy, TradingPeriodSpec, ExpirationPolicy, RollPolicy};
use crate::args::{CampaignArgs, GlobalArgs};

/// Builder for CampaignConfig from CLI args
pub struct CampaignConfigBuilder {
    global: Option<GlobalArgs>,
    args: Option<CampaignArgs>,
}

impl CampaignConfigBuilder {
    /// Create builder from campaign args
    pub fn from_args(args: &CampaignArgs) -> Self {
        Self {
            global: None,
            args: Some(args.clone()),
        }
    }

    /// Apply global args
    pub fn with_global(mut self, global: &GlobalArgs) -> Self {
        self.global = Some(global.clone());
        self
    }

    /// Build and validate the config
    pub fn build(self) -> Result<CampaignConfig> {
        let args = self.args.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing campaign args"))?;

        // Parse dates
        let start_date = Self::parse_date(&args.start)?;
        let end_date = Self::parse_date(&args.end)?;

        // Determine data directory
        let data_dir = self.global.as_ref()
            .and_then(|g| g.data_dir.clone())
            .or_else(|| {
                std::env::var("FINQ_DATA_DIR")
                    .ok()
                    .map(std::path::PathBuf::from)
            })
            .unwrap_or_else(|| {
                use crate::config::PathsConfig;
                PathsConfig::default().data_dir
            });

        // Earnings directory (use default if no file specified)
        let earnings_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("trading_project/nasdaq_earnings/data");

        // Build period policy (simplified - just use args directly)
        let period_policy = PeriodPolicy::EarningsOnly {
            timing: TradingPeriodSpec::PreEarnings {
                entry_days_before: 14, // TODO: Get from args
                exit_days_before: 1,
                entry_time: chrono::NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
                exit_time: chrono::NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
            },
        };

        // Build expiration policy
        let expiration_policy = ExpirationPolicy::FirstAfter {
            min_date: start_date,
        };

        // Parse strategy from string
        let strategy = Self::parse_strategy(&args.strategy)?;

        // Parse direction from string
        let trade_direction = Self::parse_direction(&args.direction)?;

        let config = CampaignConfig {
            data_dir,
            earnings_dir,
            earnings_file: args.earnings_file.clone(),
            symbols: args.symbols.clone(),
            start_date,
            end_date,
            strategy,
            trade_direction,
            timing: cs_domain::TimingConfig::default(), // TODO: Get from timing args
            period_policy,
            expiration_policy,
            iron_butterfly_config: None, // TODO: Parse from args if needed
            multi_leg_strategy_config: None,
            parallel: true,
        };

        Ok(config)
    }

    /// Parse strategy from string
    fn parse_strategy(s: &str) -> Result<cs_domain::OptionStrategy> {
        use cs_domain::OptionStrategy;

        match s.to_lowercase().as_str() {
            "calendar" | "calendar-spread" => Ok(OptionStrategy::CalendarSpread),
            "iron-butterfly" => Ok(OptionStrategy::IronButterfly),
            "straddle" => Ok(OptionStrategy::Straddle),
            "calendar-straddle" => Ok(OptionStrategy::CalendarStraddle),
            "strangle" => Ok(OptionStrategy::Strangle),
            "butterfly" => Ok(OptionStrategy::Butterfly),
            "condor" => Ok(OptionStrategy::Condor),
            "iron-condor" => Ok(OptionStrategy::IronCondor),
            _ => anyhow::bail!("Invalid strategy: {}. Use calendar, iron-butterfly, straddle, etc.", s),
        }
    }

    /// Parse trade direction from string
    fn parse_direction(s: &str) -> Result<cs_domain::TradeDirection> {
        use cs_domain::TradeDirection;

        match s.to_lowercase().as_str() {
            "short" => Ok(TradeDirection::Short),
            "long" => Ok(TradeDirection::Long),
            _ => anyhow::bail!("Invalid direction: {}. Use 'short' or 'long'", s),
        }
    }

    /// Parse date string to NaiveDate
    fn parse_date(s: &str) -> Result<NaiveDate> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .with_context(|| format!("Invalid date format: {}. Use YYYY-MM-DD", s))
    }
}

impl Default for CampaignConfigBuilder {
    fn default() -> Self {
        Self {
            global: None,
            args: None,
        }
    }
}
