use chrono::{DateTime, NaiveDate, Utc};
use crate::datetime::eastern_to_utc;
use crate::entities::EarningsEvent;
use crate::value_objects::{EarningsTime, TimingConfig};
use super::{TradeTiming, TradingCalendar};

/// Calculates entry/exit timing for earnings-based trades
///
/// This service implements the core session concept for earnings trades:
/// - AMC (After Market Close): Enter same day, exit NEXT trading day
/// - BMO (Before Market Open): Enter PREVIOUS trading day, exit same day
///
/// The strategy profits from IV crush after earnings announcements, so
/// trades must hold overnight through the earnings event.
#[derive(Clone, Copy)]
pub struct EarningsTradeTiming {
    config: TimingConfig,
}

impl EarningsTradeTiming {
    pub fn new(config: TimingConfig) -> Self {
        Self { config }
    }

    /// Calculate entry datetime based on earnings timing
    ///
    /// Returns the exact UTC datetime when the trade should be entered.
    /// Config times are interpreted as Eastern Time and converted to UTC.
    pub fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let entry_date = self.entry_date(event);
        eastern_to_utc(entry_date, self.config.entry_time())
    }

    /// Calculate exit datetime based on earnings timing
    ///
    /// Returns the exact UTC datetime when the trade should be exited.
    /// This will be on a DIFFERENT date than entry for earnings trades.
    /// Config times are interpreted as Eastern Time and converted to UTC.
    pub fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let exit_date = self.exit_date(event);
        eastern_to_utc(exit_date, self.config.exit_time())
    }

    /// Entry date: When we enter the trade
    ///
    /// - BMO: Previous trading day (enter day before earnings)
    /// - AMC: Same day (enter before close, earnings after close)
    /// - Unknown: Default to AMC behavior
    pub fn entry_date(&self, event: &EarningsEvent) -> NaiveDate {
        match event.earnings_time {
            EarningsTime::BeforeMarketOpen => {
                TradingCalendar::previous_trading_day(event.earnings_date)
            }
            EarningsTime::AfterMarketClose => {
                event.earnings_date
            }
            EarningsTime::Unknown => {
                // Default to AMC behavior
                event.earnings_date
            }
        }
    }

    /// Exit date: When we exit the trade (AFTER earnings)
    ///
    /// - BMO: Same day as earnings (earnings already happened before open)
    /// - AMC: Next trading day (exit morning after earnings)
    /// - Unknown: Default to next day
    pub fn exit_date(&self, event: &EarningsEvent) -> NaiveDate {
        match event.earnings_time {
            EarningsTime::BeforeMarketOpen => {
                event.earnings_date  // Exit same day
            }
            EarningsTime::AfterMarketClose => {
                TradingCalendar::next_trading_day(event.earnings_date)  // Exit next day
            }
            EarningsTime::Unknown => {
                TradingCalendar::next_trading_day(event.earnings_date)
            }
        }
    }
}

impl TradeTiming for EarningsTradeTiming {
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
    use chrono::Timelike;

    fn default_timing() -> EarningsTradeTiming {
        EarningsTradeTiming::new(TimingConfig::default())
    }

    #[test]
    fn test_amc_entry_exit_dates() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 3).unwrap(),
            EarningsTime::AfterMarketClose,
        );

        // AMC: Enter same day, exit next day
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 3).unwrap());
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 11, 4).unwrap());
    }

    #[test]
    fn test_bmo_entry_exit_dates() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 4).unwrap(),  // Earnings on Nov 4
            EarningsTime::BeforeMarketOpen,
        );

        // BMO: Enter previous day, exit same day as earnings
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 3).unwrap());
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 11, 4).unwrap());
    }

    #[test]
    fn test_amc_friday_exits_monday() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),  // Friday
            EarningsTime::AfterMarketClose,
        );

        // Friday AMC should exit Monday (skip weekend)
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 7).unwrap());
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 11, 10).unwrap());
    }

    #[test]
    fn test_bmo_monday_enters_friday() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 10).unwrap(),  // Monday
            EarningsTime::BeforeMarketOpen,
        );

        // Monday BMO should enter Friday (skip weekend backwards)
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 7).unwrap());
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 11, 10).unwrap());
    }

    #[test]
    fn test_unknown_defaults_to_amc_behavior() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 3).unwrap(),
            EarningsTime::Unknown,
        );

        // Unknown should default to AMC: enter same day, exit next day
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 3).unwrap());
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 11, 4).unwrap());
    }

    #[test]
    fn test_entry_datetime_converts_eastern_to_utc() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 3).unwrap(),  // Nov 3 is in EST (UTC-5)
            EarningsTime::AfterMarketClose,
        );

        let entry_dt = timing.entry_datetime(&event);

        // Config: 09:35 ET → should be 14:35 UTC (EST = UTC-5)
        assert_eq!(entry_dt.date_naive(), NaiveDate::from_ymd_opt(2025, 11, 3).unwrap());
        assert_eq!(entry_dt.time().hour(), 14);  // 09:35 ET = 14:35 UTC
        assert_eq!(entry_dt.time().minute(), 35);
    }

    #[test]
    fn test_exit_datetime_converts_eastern_to_utc() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 3).unwrap(),  // Nov 3 is in EST (UTC-5)
            EarningsTime::AfterMarketClose,
        );

        let exit_dt = timing.exit_datetime(&event);

        // Config: 10:00 ET on Nov 4 → should be 15:00 UTC (EST = UTC-5)
        assert_eq!(exit_dt.date_naive(), NaiveDate::from_ymd_opt(2025, 11, 4).unwrap());
        assert_eq!(exit_dt.time().hour(), 15);  // 10:00 ET = 15:00 UTC
        assert_eq!(exit_dt.time().minute(), 0);
    }

    #[test]
    fn test_entry_exit_always_different_dates_for_amc() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 3).unwrap(),
            EarningsTime::AfterMarketClose,
        );

        let entry_date = timing.entry_date(&event);
        let exit_date = timing.exit_date(&event);

        // Critical: exit must be AFTER entry for AMC
        assert!(exit_date > entry_date, "AMC trades must hold overnight");
    }

    #[test]
    fn test_entry_exit_span_earnings_for_bmo() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 4).unwrap(),
            EarningsTime::BeforeMarketOpen,
        );

        let entry_date = timing.entry_date(&event);
        let exit_date = timing.exit_date(&event);

        // BMO: Enter before earnings day, exit on earnings day
        assert!(entry_date < event.earnings_date);
        assert_eq!(exit_date, event.earnings_date);
    }
}
