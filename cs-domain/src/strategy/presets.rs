//! Pre-configured strategy presets for common use cases

use super::{TradeStrategy, TradeStructureConfig, TradeFilters};
use crate::expiration::ExpirationPolicy;
use crate::trading_period::TradingPeriodSpec;
use crate::roll::RollPolicy;
use crate::hedging::{HedgeConfig, HedgeStrategy};
use chrono::{NaiveDate, NaiveTime};
use finq_core::OptionType;

/// Pre-earnings straddle: capture IV expansion before earnings
///
/// Entry: 20 trading days before earnings
/// Exit: 1 trading day before earnings
/// Expiration: First available after exit date
pub fn pre_earnings_straddle() -> TradeStrategy {
    TradeStrategy {
        structure: TradeStructureConfig::Straddle,
        timing: TradingPeriodSpec::PreEarnings {
            entry_days_before: 20,
            exit_days_before: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        },
        expiration_policy: ExpirationPolicy::FirstAfter {
            min_date: NaiveDate::MIN, // Will be set dynamically to exit_date
        },
        roll_policy: RollPolicy::None,
        hedge_config: HedgeConfig::default(),
        filters: TradeFilters::default(),
    }
}

/// Pre-earnings straddle with delta hedging
pub fn pre_earnings_straddle_hedged() -> TradeStrategy {
    let mut strategy = pre_earnings_straddle();
    strategy.hedge_config = HedgeConfig {
        strategy: HedgeStrategy::DeltaThreshold { threshold: 0.15 },
        max_rehedges: Some(10),
        min_hedge_size: 10,
        transaction_cost_per_share: rust_decimal::Decimal::new(1, 2), // $0.01
        contract_multiplier: 100,
    };
    strategy
}

/// Weekly roll straddle: buy weekly, roll each week
///
/// Entry: 4 weeks before earnings
/// Exit: 1 day before earnings
/// Roll: On each weekly expiration
pub fn weekly_roll_straddle() -> TradeStrategy {
    TradeStrategy {
        structure: TradeStructureConfig::Straddle,
        timing: TradingPeriodSpec::PreEarnings {
            entry_days_before: 28, // ~4 weeks
            exit_days_before: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        },
        expiration_policy: ExpirationPolicy::PreferWeekly {
            min_date: NaiveDate::MIN,
            fallback_to_monthly: true,
        },
        roll_policy: RollPolicy::OnExpiration {
            to_next: ExpirationPolicy::PreferWeekly {
                min_date: NaiveDate::MIN,
                fallback_to_monthly: true,
            },
        },
        hedge_config: HedgeConfig::default(),
        filters: TradeFilters::default(),
    }
}

/// Post-earnings straddle: capture momentum after earnings
///
/// Entry: Day after earnings
/// Exit: 5 trading days after entry
pub fn post_earnings_straddle() -> TradeStrategy {
    TradeStrategy {
        structure: TradeStructureConfig::Straddle,
        timing: TradingPeriodSpec::PostEarnings {
            entry_offset: 0,
            holding_days: 5,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        },
        expiration_policy: ExpirationPolicy::FirstAfter {
            min_date: NaiveDate::MIN,
        },
        roll_policy: RollPolicy::None,
        hedge_config: HedgeConfig::default(),
        filters: TradeFilters::default(),
    }
}

/// Earnings calendar spread: capture IV crush
///
/// Entry: 1 day before earnings
/// Exit: 1 day after earnings
/// Short: First monthly after earnings
/// Long: Second monthly after earnings
pub fn earnings_calendar_spread(option_type: OptionType) -> TradeStrategy {
    TradeStrategy {
        structure: TradeStructureConfig::CalendarSpread {
            option_type,
            strike_match: crate::strike_selection::StrikeMatchMode::SameStrike,
        },
        timing: TradingPeriodSpec::CrossEarnings {
            entry_days_before: 1,
            exit_days_after: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
        },
        expiration_policy: ExpirationPolicy::Calendar {
            short: Box::new(ExpirationPolicy::PreferMonthly {
                min_date: NaiveDate::MIN,
                months_out: 0,
            }),
            long: Box::new(ExpirationPolicy::PreferMonthly {
                min_date: NaiveDate::MIN,
                months_out: 1,
            }),
        },
        roll_policy: RollPolicy::None,
        hedge_config: HedgeConfig::default(),
        filters: TradeFilters {
            min_iv_ratio: Some(1.1), // Short IV > Long IV
            ..Default::default()
        },
    }
}

/// Monthly-only calendar spread (avoid weeklies)
pub fn monthly_calendar_spread(option_type: OptionType) -> TradeStrategy {
    earnings_calendar_spread(option_type)
}
