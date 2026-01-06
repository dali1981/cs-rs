// cs-domain/src/campaign/period_policy.rs

use chrono::{NaiveTime, Weekday};
use crate::{RollPolicy, TradingPeriodSpec};

/// Policy for when to trade within a campaign
#[derive(Debug, Clone)]
pub enum PeriodPolicy {
    /// Only trade around earnings announcements
    ///
    /// Use for: Calendar spreads, earnings straddles
    EarningsOnly {
        /// How to time entry/exit relative to earnings
        timing: TradingPeriodSpec,
    },

    /// Trade between earnings dates with rolling
    ///
    /// Use for: Theta harvesting between earnings, weekly premium selling
    InterEarnings {
        /// Days after earnings to start trading
        entry_days_after_earnings: u16,
        /// Days before next earnings to stop trading
        exit_days_before_earnings: u16,
        /// How often to roll positions
        roll_policy: RollPolicy,
    },

    /// Both earnings and inter-period trading
    ///
    /// Use for: Continuous premium collection with earnings plays
    Continuous {
        /// How to time earnings trades
        earnings_timing: TradingPeriodSpec,
        /// How to roll between earnings
        inter_period_roll: RollPolicy,
    },

    /// Fixed date range, ignore earnings calendar
    ///
    /// Use for: Backtests, specific date ranges
    FixedPeriod {
        /// How often to roll
        roll_policy: RollPolicy,
    },
}

impl PeriodPolicy {
    // =========================================================================
    // Convenience constructors
    // =========================================================================

    /// Trade only around earnings with cross-earnings timing (calendar spread default)
    pub fn cross_earnings() -> Self {
        Self::EarningsOnly {
            timing: TradingPeriodSpec::cross_earnings_default(),
        }
    }

    /// Trade only before earnings (straddle IV expansion)
    pub fn pre_earnings(days_before: u16) -> Self {
        Self::EarningsOnly {
            timing: TradingPeriodSpec::PreEarnings {
                entry_days_before: days_before,
                exit_days_before: 1,
                entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
                exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
            },
        }
    }

    /// Trade between earnings with weekly rolling
    pub fn weekly_between_earnings() -> Self {
        Self::InterEarnings {
            entry_days_after_earnings: 2,
            exit_days_before_earnings: 3,
            roll_policy: RollPolicy::Weekly { roll_day: Weekday::Fri },
        }
    }

    /// Trade between earnings with monthly rolling
    pub fn monthly_between_earnings() -> Self {
        Self::InterEarnings {
            entry_days_after_earnings: 2,
            exit_days_before_earnings: 5,
            roll_policy: RollPolicy::Monthly { roll_week_offset: 0 },
        }
    }
}
