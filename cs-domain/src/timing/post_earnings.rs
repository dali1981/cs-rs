use chrono::{DateTime, NaiveDate, Utc};
use crate::datetime::eastern_to_utc;
use crate::entities::EarningsEvent;
use crate::value_objects::{EarningsTime, TimingConfig};
use super::{TradeTiming, TradingCalendar};

/// Calculates entry/exit timing for post-earnings straddle trades
///
/// Unlike EarningsTradeTiming (which enters BEFORE earnings for IV crush plays),
/// this service enters AFTER earnings to capture continued momentum while
/// benefiting from lower IV entry prices.
#[derive(Clone, Copy)]
pub struct PostEarningsStraddleTiming {
    config: TimingConfig,
    holding_days: usize,  // Default: 5 (one trading week)
}

impl PostEarningsStraddleTiming {
    pub fn new(config: TimingConfig) -> Self {
        Self {
            config,
            holding_days: 5,
        }
    }

    pub fn with_holding_days(mut self, days: usize) -> Self {
        self.holding_days = days;
        self
    }

    /// Entry date: Day AFTER earnings announcement
    ///
    /// - BMO: Same day as earnings (earnings already happened before open)
    /// - AMC: Next trading day (earnings happened after previous close)
    /// - Unknown: Default to AMC behavior (next day)
    pub fn entry_date(&self, event: &EarningsEvent) -> NaiveDate {
        match event.earnings_time {
            EarningsTime::BeforeMarketOpen => {
                // Earnings happened before market open, can enter same day
                event.earnings_date
            }
            EarningsTime::AfterMarketClose | EarningsTime::Unknown => {
                // Earnings happened after close, enter next day
                TradingCalendar::next_trading_day(event.earnings_date)
            }
        }
    }

    /// Exit date: N trading days after entry (default: 5 = 1 week)
    pub fn exit_date(&self, event: &EarningsEvent) -> NaiveDate {
        let entry = self.entry_date(event);
        TradingCalendar::n_trading_days_after(entry, self.holding_days)
    }

    pub fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let entry_date = self.entry_date(event);
        eastern_to_utc(entry_date, self.config.entry_time())
    }

    pub fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let exit_date = self.exit_date(event);
        eastern_to_utc(exit_date, self.config.exit_time())
    }

    /// Get holding period in trading days
    pub fn holding_period(&self) -> usize {
        self.holding_days
    }
}

impl TradeTiming for PostEarningsStraddleTiming {
    fn entry_date(&self, event: &EarningsEvent) -> NaiveDate {
        self.entry_date(event)
    }

    fn exit_date(&self, event: &EarningsEvent) -> NaiveDate {
        self.exit_date(event)
    }

    fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        self.entry_datetime(event)
    }

    fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        self.exit_datetime(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_timing() -> PostEarningsStraddleTiming {
        PostEarningsStraddleTiming::new(TimingConfig::default())
    }

    #[test]
    fn test_amc_entry_next_day() {
        let timing = default_timing().with_holding_days(5);
        let event = EarningsEvent::new(
            "AAPL".into(),
            NaiveDate::from_ymd_opt(2025, 1, 30).unwrap(),  // Thursday AMC
            EarningsTime::AfterMarketClose,
        );

        // Entry: Next day (Friday Jan 31)
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 1, 31).unwrap());

        // Exit: 5 trading days later = Friday Feb 7
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 2, 7).unwrap());

        assert_eq!(timing.holding_period(), 5);
    }

    #[test]
    fn test_bmo_entry_same_day() {
        let timing = default_timing().with_holding_days(5);
        let event = EarningsEvent::new(
            "AAPL".into(),
            NaiveDate::from_ymd_opt(2025, 2, 3).unwrap(),  // Monday BMO
            EarningsTime::BeforeMarketOpen,
        );

        // Entry: Same day (Monday Feb 3)
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 2, 3).unwrap());

        // Exit: 5 trading days later = Monday Feb 10
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 2, 10).unwrap());
    }

    #[test]
    fn test_friday_amc_enters_monday() {
        let timing = default_timing().with_holding_days(5);
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),  // Friday AMC
            EarningsTime::AfterMarketClose,
        );

        // Entry: Monday Nov 10 (skip weekend)
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 10).unwrap());

        // Exit: Monday Nov 17 (5 trading days: Nov 11, 12, 13, 14, 17)
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 11, 17).unwrap());
    }

    #[test]
    fn test_unknown_defaults_to_amc() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 3).unwrap(),
            EarningsTime::Unknown,
        );

        // Should behave like AMC: entry next day
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 4).unwrap());
    }
}
