use chrono::{NaiveDate, NaiveTime};
use thiserror::Error;

use crate::entities::EarningsEvent;
use crate::value_objects::EarningsTime;
use crate::timing::TradingCalendar;
use super::TradingPeriod;

/// Errors during trading period construction
#[derive(Error, Debug)]
pub enum TimingError {
    #[error("This timing specification requires an earnings event")]
    RequiresEarningsEvent,

    #[error("Invalid period: exit before entry")]
    ExitBeforeEntry,

    #[error("Entry date is not a trading day")]
    NonTradingDay,
}

/// Specification for a trading period
///
/// This is the "template" that gets resolved into a concrete `TradingPeriod`
/// when provided with the necessary context (e.g., earnings event).
#[derive(Debug, Clone)]
pub enum TradingPeriodSpec {
    /// Enter N days before earnings, exit M days before
    ///
    /// Used for IV expansion trades (straddles before earnings).
    PreEarnings {
        entry_days_before: u16,
        exit_days_before: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Enter after earnings, hold for N days
    ///
    /// Used for post-earnings momentum trades.
    PostEarnings {
        /// Days after earnings to enter (0 = earnings day if BMO, 1 = day after if AMC)
        entry_offset: i16,
        /// Trading days to hold
        holding_days: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Cross earnings: enter before, exit after
    ///
    /// Used for calendar spreads capturing IV crush.
    CrossEarnings {
        /// Days before earnings to enter
        entry_days_before: u16,
        /// Days after earnings to exit
        exit_days_after: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Fixed date range (not earnings-relative)
    ///
    /// Used for non-earnings trades or custom backtests.
    FixedDates {
        entry_date: NaiveDate,
        exit_date: NaiveDate,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Holding period from a start date
    ///
    /// Used when you know the entry but want to specify holding duration.
    HoldingPeriod {
        entry_date: NaiveDate,
        holding_days: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },
}

impl TradingPeriodSpec {
    // =========================================================================
    // Convenience constructors
    // =========================================================================

    /// Pre-earnings straddle timing (default: enter 20 days before, exit 1 day before)
    pub fn pre_earnings_default() -> Self {
        Self::PreEarnings {
            entry_days_before: 20,
            exit_days_before: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        }
    }

    /// Post-earnings timing (default: hold for 5 days)
    pub fn post_earnings_default() -> Self {
        Self::PostEarnings {
            entry_offset: 0,
            holding_days: 5,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        }
    }

    /// Cross-earnings timing (default: enter day before, exit day after)
    pub fn cross_earnings_default() -> Self {
        Self::CrossEarnings {
            entry_days_before: 1,
            exit_days_after: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
        }
    }

    // =========================================================================
    // Builder methods
    // =========================================================================

    /// Set entry/exit times
    pub fn with_times(self, entry_time: NaiveTime, exit_time: NaiveTime) -> Self {
        match self {
            Self::PreEarnings { entry_days_before, exit_days_before, .. } =>
                Self::PreEarnings { entry_days_before, exit_days_before, entry_time, exit_time },
            Self::PostEarnings { entry_offset, holding_days, .. } =>
                Self::PostEarnings { entry_offset, holding_days, entry_time, exit_time },
            Self::CrossEarnings { entry_days_before, exit_days_after, .. } =>
                Self::CrossEarnings { entry_days_before, exit_days_after, entry_time, exit_time },
            Self::FixedDates { entry_date, exit_date, .. } =>
                Self::FixedDates { entry_date, exit_date, entry_time, exit_time },
            Self::HoldingPeriod { entry_date, holding_days, .. } =>
                Self::HoldingPeriod { entry_date, holding_days, entry_time, exit_time },
        }
    }

    // =========================================================================
    // Resolution
    // =========================================================================

    /// Build a concrete TradingPeriod from this specification
    pub fn build(&self, event: Option<&EarningsEvent>) -> Result<TradingPeriod, TimingError> {
        match self {
            Self::PreEarnings { entry_days_before, exit_days_before, entry_time, exit_time } => {
                let event = event.ok_or(TimingError::RequiresEarningsEvent)?;

                let entry_date = TradingCalendar::n_trading_days_before(
                    event.earnings_date,
                    *entry_days_before as usize
                );
                let exit_date = TradingCalendar::n_trading_days_before(
                    event.earnings_date,
                    *exit_days_before as usize
                );

                if exit_date < entry_date {
                    return Err(TimingError::ExitBeforeEntry);
                }

                Ok(TradingPeriod::new(entry_date, exit_date, *entry_time, *exit_time))
            }

            Self::PostEarnings { entry_offset, holding_days, entry_time, exit_time } => {
                let event = event.ok_or(TimingError::RequiresEarningsEvent)?;

                // Entry depends on earnings time
                let entry_date = match event.earnings_time {
                    EarningsTime::BeforeMarketOpen => {
                        // BMO: can enter same day (offset 0) or later
                        TradingCalendar::n_trading_days_after(
                            event.earnings_date,
                            (*entry_offset).max(0) as usize
                        )
                    }
                    EarningsTime::AfterMarketClose | EarningsTime::Unknown => {
                        // AMC: enter next day (offset 0 = next day)
                        TradingCalendar::n_trading_days_after(
                            event.earnings_date,
                            ((*entry_offset).max(0) + 1) as usize
                        )
                    }
                };

                let exit_date = TradingCalendar::n_trading_days_after(
                    entry_date,
                    *holding_days as usize
                );

                Ok(TradingPeriod::new(entry_date, exit_date, *entry_time, *exit_time))
            }

            Self::CrossEarnings { entry_days_before, exit_days_after, entry_time, exit_time } => {
                let event = event.ok_or(TimingError::RequiresEarningsEvent)?;

                let entry_date = TradingCalendar::n_trading_days_before(
                    event.earnings_date,
                    *entry_days_before as usize
                );

                // Exit depends on earnings time
                let exit_date = match event.earnings_time {
                    EarningsTime::AfterMarketClose => {
                        // AMC: exit N days after earnings
                        TradingCalendar::n_trading_days_after(
                            event.earnings_date,
                            *exit_days_after as usize
                        )
                    }
                    EarningsTime::BeforeMarketOpen => {
                        // BMO: exit on earnings day (after announcement) or later
                        if *exit_days_after == 0 {
                            event.earnings_date
                        } else {
                            TradingCalendar::n_trading_days_after(
                                event.earnings_date,
                                (*exit_days_after - 1) as usize
                            )
                        }
                    }
                    EarningsTime::Unknown => {
                        TradingCalendar::n_trading_days_after(
                            event.earnings_date,
                            *exit_days_after as usize
                        )
                    }
                };

                Ok(TradingPeriod::new(entry_date, exit_date, *entry_time, *exit_time))
            }

            Self::FixedDates { entry_date, exit_date, entry_time, exit_time } => {
                if exit_date < entry_date {
                    return Err(TimingError::ExitBeforeEntry);
                }
                Ok(TradingPeriod::new(*entry_date, *exit_date, *entry_time, *exit_time))
            }

            Self::HoldingPeriod { entry_date, holding_days, entry_time, exit_time } => {
                let exit_date = TradingCalendar::n_trading_days_after(
                    *entry_date,
                    *holding_days as usize
                );
                Ok(TradingPeriod::new(*entry_date, exit_date, *entry_time, *exit_time))
            }
        }
    }

    /// Calculate lookahead days needed for event loading
    pub fn lookahead_days(&self) -> i64 {
        match self {
            Self::PreEarnings { entry_days_before, .. } => {
                // Add buffer for weekends: multiply by 1.5 and add 7
                ((*entry_days_before as f64 * 1.5) as i64) + 7
            }
            Self::PostEarnings { .. } => {
                // Post-earnings needs lookback, not lookahead
                -3
            }
            Self::CrossEarnings { entry_days_before, .. } => {
                (*entry_days_before as i64) + 3
            }
            Self::FixedDates { .. } | Self::HoldingPeriod { .. } => {
                0 // No earnings-based lookahead needed
            }
        }
    }

    /// Check if this spec requires an earnings event
    pub fn requires_earnings(&self) -> bool {
        !matches!(self, Self::FixedDates { .. } | Self::HoldingPeriod { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> EarningsEvent {
        EarningsEvent::new(
            "AAPL".to_string(),
            NaiveDate::from_ymd_opt(2025, 10, 30).unwrap(), // Thursday
            EarningsTime::AfterMarketClose,
        )
    }

    #[test]
    fn test_pre_earnings_period() {
        let spec = TradingPeriodSpec::PreEarnings {
            entry_days_before: 10,
            exit_days_before: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        };

        let event = sample_event();
        let period = spec.build(Some(&event)).unwrap();

        // 10 trading days before Oct 30 = Oct 16 (Wed)
        // 1 trading day before Oct 30 = Oct 29 (Wed)
        assert_eq!(period.entry_date, NaiveDate::from_ymd_opt(2025, 10, 16).unwrap());
        assert_eq!(period.exit_date, NaiveDate::from_ymd_opt(2025, 10, 29).unwrap());
    }

    #[test]
    fn test_post_earnings_period() {
        let spec = TradingPeriodSpec::PostEarnings {
            entry_offset: 0,
            holding_days: 5,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        };

        let event = sample_event(); // AMC on Oct 30
        let period = spec.build(Some(&event)).unwrap();

        // AMC: entry is next day = Oct 31
        assert_eq!(period.entry_date, NaiveDate::from_ymd_opt(2025, 10, 31).unwrap());
    }

    #[test]
    fn test_fixed_dates_no_event() {
        let spec = TradingPeriodSpec::FixedDates {
            entry_date: NaiveDate::from_ymd_opt(2025, 10, 1).unwrap(),
            exit_date: NaiveDate::from_ymd_opt(2025, 10, 15).unwrap(),
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        };

        // Should work without an earnings event
        let period = spec.build(None).unwrap();
        assert_eq!(period.entry_date, NaiveDate::from_ymd_opt(2025, 10, 1).unwrap());
    }
}
