use chrono::{DateTime, NaiveDate, Utc};
use crate::datetime::eastern_to_utc;
use crate::entities::EarningsEvent;
use crate::value_objects::TimingConfig;
use super::{TradeTiming, TradingCalendar};

/// Calculates entry/exit timing for straddle trades around earnings
///
/// Unlike EarningsTradeTiming (which handles IV crush trades), this service
/// implements timing for IV expansion trades that profit from volatility
/// buildup BEFORE earnings.
pub struct StraddleTradeTiming {
    config: TimingConfig,
    entry_days_before: usize,  // Default: 5 (one week before)
    exit_days_before: usize,   // Default: 1 (day before earnings)
}

impl StraddleTradeTiming {
    pub fn new(config: TimingConfig) -> Self {
        Self {
            config,
            entry_days_before: 5,
            exit_days_before: 1,
        }
    }

    pub fn with_entry_days(mut self, days: usize) -> Self {
        self.entry_days_before = days;
        self
    }

    pub fn with_exit_days(mut self, days: usize) -> Self {
        self.exit_days_before = days;
        self
    }

    /// Entry date: N trading days before earnings
    pub fn entry_date(&self, event: &EarningsEvent) -> NaiveDate {
        TradingCalendar::n_trading_days_before(
            event.earnings_date,
            self.entry_days_before
        )
    }

    /// Exit date: M trading days before earnings (default: 1)
    pub fn exit_date(&self, event: &EarningsEvent) -> NaiveDate {
        TradingCalendar::n_trading_days_before(
            event.earnings_date,
            self.exit_days_before
        )
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
        self.entry_days_before - self.exit_days_before
    }
}

impl TradeTiming for StraddleTradeTiming {
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
    use crate::value_objects::EarningsTime;
    use chrono::Timelike;

    fn default_timing() -> StraddleTradeTiming {
        StraddleTradeTiming::new(TimingConfig {
            entry_hour: 9,
            entry_minute: 35,
            exit_hour: 10,
            exit_minute: 0,
        })
    }

    #[test]
    fn test_straddle_timing_entry_exit() {
        let timing = default_timing()
            .with_entry_days(5)
            .with_exit_days(1);

        let event = EarningsEvent::new(
            "AAPL".into(),
            NaiveDate::from_ymd_opt(2025, 1, 30).unwrap(),  // Thursday earnings
            EarningsTime::AfterMarketClose,
        );

        // Entry: 5 trading days before Jan 30 = Jan 23 (Thursday)
        let entry = timing.entry_date(&event);
        assert_eq!(entry, NaiveDate::from_ymd_opt(2025, 1, 23).unwrap());

        // Exit: 1 trading day before Jan 30 = Jan 29 (Wednesday)
        let exit = timing.exit_date(&event);
        assert_eq!(exit, NaiveDate::from_ymd_opt(2025, 1, 29).unwrap());

        // Holding period: 4 trading days
        assert_eq!(timing.holding_period(), 4);
    }

    #[test]
    fn test_straddle_timing_with_weekend() {
        let timing = default_timing()
            .with_entry_days(5)
            .with_exit_days(1);

        let event = EarningsEvent::new(
            "AAPL".into(),
            NaiveDate::from_ymd_opt(2025, 2, 3).unwrap(),  // Monday earnings
            EarningsTime::BeforeMarketOpen,
        );

        // Entry: 5 trading days before Mon Feb 3 = Mon Jan 27 (skip weekend backwards)
        let entry = timing.entry_date(&event);
        assert_eq!(entry, NaiveDate::from_ymd_opt(2025, 1, 27).unwrap());

        // Exit: 1 trading day before Mon Feb 3 = Fri Jan 31 (skip weekend backwards)
        let exit = timing.exit_date(&event);
        assert_eq!(exit, NaiveDate::from_ymd_opt(2025, 1, 31).unwrap());
    }

    #[test]
    fn test_entry_datetime_converts_eastern_to_utc() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 3).unwrap(),  // Nov 3 earnings
            EarningsTime::AfterMarketClose,
        );

        let entry_dt = timing.entry_datetime(&event);

        // Entry is 5 days before Nov 3 = Oct 27 (still in EDT, UTC-4)
        // Config: 09:35 ET → should be 13:35 UTC (EDT = UTC-4)
        assert_eq!(entry_dt.time().hour(), 13);
        assert_eq!(entry_dt.time().minute(), 35);
    }
}
