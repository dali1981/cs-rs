use chrono::{NaiveDate, NaiveTime, DateTime, Utc};
use crate::datetime::eastern_to_utc;
use crate::timing::TradingCalendar;

/// A concrete trading period with resolved dates and times
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradingPeriod {
    pub entry_date: NaiveDate,
    pub exit_date: NaiveDate,
    pub entry_time: NaiveTime,
    pub exit_time: NaiveTime,
}

impl TradingPeriod {
    /// Create a new trading period
    pub fn new(
        entry_date: NaiveDate,
        exit_date: NaiveDate,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    ) -> Self {
        Self {
            entry_date,
            exit_date,
            entry_time,
            exit_time,
        }
    }

    /// Get entry datetime in UTC
    pub fn entry_datetime(&self) -> DateTime<Utc> {
        eastern_to_utc(self.entry_date, self.entry_time)
    }

    /// Get exit datetime in UTC
    pub fn exit_datetime(&self) -> DateTime<Utc> {
        eastern_to_utc(self.exit_date, self.exit_time)
    }

    /// Calculate holding period in trading days
    pub fn holding_days(&self) -> i64 {
        TradingCalendar::trading_days_between(self.entry_date, self.exit_date).count() as i64
    }

    /// Minimum expiration date (options must expire AFTER this)
    pub fn min_expiration(&self) -> NaiveDate {
        self.exit_date
    }

    /// Check if a date is within the trading period
    pub fn contains_date(&self, date: NaiveDate) -> bool {
        date >= self.entry_date && date <= self.exit_date
    }
}
