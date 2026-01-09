use chrono::NaiveDate;
use crate::{EarningsEvent, TradingPeriodSpec};
use super::TradableEvent;

/// A date range during which we want to INITIATE trades
///
/// This is different from TradingPeriod which is a resolved single trade period.
/// TradingRange represents "when do we want to start trades" (e.g., all of Q1 2025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TradingRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl TradingRange {
    pub fn new(start: NaiveDate, end: NaiveDate) -> Self {
        Self { start, end }
    }

    /// Check if a date falls within this range
    pub fn contains(&self, date: NaiveDate) -> bool {
        date >= self.start && date <= self.end
    }

    /// Number of calendar days in this range
    pub fn duration_days(&self) -> i64 {
        (self.end - self.start).num_days() + 1
    }

    /// Discover events whose ENTRY DATE falls within this range
    ///
    /// This is the core discovery logic: given a set of earnings events and
    /// a timing specification, find which events would have their entry date
    /// (as resolved by the timing spec) fall within our trading range.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Want to initiate trades in January 2025
    /// let range = TradingRange::new(
    ///     NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
    ///     NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
    /// );
    ///
    /// // Pre-earnings strategy: enter 14 days before earnings
    /// let timing = TradingPeriodSpec::PreEarnings { entry_days_before: 14, ... };
    ///
    /// // Event: AAPL earnings on Feb 15
    /// // Entry would be Feb 1 (14 trading days before Feb 15)
    /// // Feb 1 is in January → include this event
    /// ```
    pub fn discover_tradable_events(
        &self,
        events: &[EarningsEvent],
        timing: &TradingPeriodSpec,
    ) -> Vec<TradableEvent> {
        events
            .iter()
            .filter_map(|event| {
                // Resolve timing spec for this event
                let resolved = timing.build(Some(event)).ok()?;

                // Include only if entry falls in our range
                if self.contains(resolved.entry_date) {
                    Some(TradableEvent::new(
                        event.clone(),
                        resolved.entry_date,
                        resolved.exit_date,
                        resolved.entry_time,
                        resolved.exit_time,
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
}
