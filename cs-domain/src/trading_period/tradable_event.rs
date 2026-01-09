use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use crate::{EarningsEvent, datetime::eastern_to_utc};

/// An earnings event resolved to concrete trading dates/times
///
/// This represents an event that has been processed through a TimingSpec
/// to determine exact entry and exit datetimes for trading.
#[derive(Debug, Clone)]
pub struct TradableEvent {
    /// The underlying earnings event
    pub event: EarningsEvent,

    /// Resolved entry date
    pub entry_date: NaiveDate,

    /// Resolved exit date
    pub exit_date: NaiveDate,

    /// Entry time of day
    pub entry_time: NaiveTime,

    /// Exit time of day
    pub exit_time: NaiveTime,
}

impl TradableEvent {
    pub fn new(
        event: EarningsEvent,
        entry_date: NaiveDate,
        exit_date: NaiveDate,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    ) -> Self {
        Self {
            event,
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

    /// Symbol being traded
    pub fn symbol(&self) -> &str {
        &self.event.symbol
    }

    /// Earnings date
    pub fn earnings_date(&self) -> NaiveDate {
        self.event.earnings_date
    }

    /// Number of calendar days from entry to exit
    pub fn duration_days(&self) -> i64 {
        (self.exit_date - self.entry_date).num_days()
    }
}
