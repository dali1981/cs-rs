use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use chrono_tz::America::New_York;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

/// US Eastern timezone (handles EST/EDT automatically)
pub const EASTERN: Tz = New_York;

/// Convert Eastern time to UTC
///
/// Takes a date and time in US Eastern timezone and returns the equivalent UTC datetime.
/// Automatically handles EST/EDT transitions.
///
/// # Panics
/// Panics if the time is ambiguous (during DST transition). This should not happen
/// for typical market hours (9:30 AM - 4:00 PM ET).
pub fn eastern_to_utc(date: NaiveDate, time: NaiveTime) -> DateTime<Utc> {
    let naive_dt = date.and_time(time);
    EASTERN
        .from_local_datetime(&naive_dt)
        .single()
        .expect("Unambiguous Eastern time")
        .with_timezone(&Utc)
}

/// Get current time in Eastern timezone
pub fn now_eastern() -> DateTime<Tz> {
    Utc::now().with_timezone(&EASTERN)
}

/// Days since Unix epoch (1970-01-01).
/// This is the internal representation matching Polars Date type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TradingDate(i32);

/// Nanoseconds since Unix epoch (1970-01-01).
/// This is the internal representation matching Polars Datetime type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TradingTimestamp(i64);

/// Time of day for market operations (no date component)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MarketTime {
    pub hour: u32,
    pub minute: u32,
}

// Constants
const NANOS_PER_SECOND: i64 = 1_000_000_000;
const NANOS_PER_DAY: i64 = 86_400 * NANOS_PER_SECOND;

impl TradingDate {
    /// Unix epoch reference date (1970-01-01)
    fn unix_epoch() -> NaiveDate {
        NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()
    }

    /// Create from days since Unix epoch (Polars Date format)
    pub fn from_polars_date(days: i32) -> Self {
        Self(days)
    }

    /// Create from NaiveDate
    pub fn from_naive_date(date: NaiveDate) -> Self {
        let days = (date - Self::unix_epoch()).num_days() as i32;
        Self(days)
    }

    /// Create from year/month/day
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Option<Self> {
        NaiveDate::from_ymd_opt(year, month, day).map(Self::from_naive_date)
    }

    /// Convert to Polars Date format (for filtering)
    pub fn to_polars_date(&self) -> i32 {
        self.0
    }

    /// Convert to NaiveDate (for display or external APIs)
    pub fn to_naive_date(&self) -> NaiveDate {
        Self::unix_epoch() + Duration::days(self.0 as i64)
    }

    /// Days to expiry from another date (self - from)
    pub fn dte(&self, from: &TradingDate) -> i32 {
        self.0 - from.0
    }

    /// Add days
    pub fn add_days(&self, days: i32) -> Self {
        Self(self.0 + days)
    }

    /// Subtract days
    pub fn sub_days(&self, days: i32) -> Self {
        Self(self.0 - days)
    }

    /// Combine with time to create timestamp
    pub fn with_time(&self, time: &MarketTime) -> TradingTimestamp {
        // Convert MarketTime (Eastern) to UTC
        let naive_time = NaiveTime::from_hms_opt(time.hour, time.minute, 0)
            .expect("Valid market time");
        let utc_datetime = eastern_to_utc(self.to_naive_date(), naive_time);
        TradingTimestamp::from_datetime_utc(utc_datetime)
    }

    /// Combine with NaiveTime to create timestamp
    pub fn with_naive_time(&self, time: NaiveTime) -> TradingTimestamp {
        let day_nanos = (self.0 as i64) * NANOS_PER_DAY;
        let time_nanos = time.num_seconds_from_midnight() as i64 * NANOS_PER_SECOND;
        TradingTimestamp(day_nanos + time_nanos)
    }
}

impl TradingTimestamp {
    /// Create from nanoseconds since Unix epoch
    pub fn from_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    /// Create from DateTime<Utc>
    pub fn from_datetime_utc(dt: DateTime<Utc>) -> Self {
        Self(dt.timestamp_nanos_opt().unwrap_or(0))
    }

    /// Convert to nanoseconds (for Polars filtering)
    pub fn to_nanos(&self) -> i64 {
        self.0
    }

    /// Convert to DateTime<Utc> (for display)
    pub fn to_datetime_utc(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_nanos(self.0)
    }

    /// Get the date component
    pub fn date(&self) -> TradingDate {
        TradingDate((self.0 / NANOS_PER_DAY) as i32)
    }

    /// Time to expiry in years (for Black-Scholes)
    pub fn time_to_expiry(&self, expiry: &TradingDate, market_close: &MarketTime) -> f64 {
        let expiry_ts = expiry.with_time(market_close);
        let diff_nanos = expiry_ts.0 - self.0;
        let diff_seconds = diff_nanos as f64 / NANOS_PER_SECOND as f64;
        diff_seconds / (365.25 * 86400.0)
    }

    /// Time between two timestamps in hours
    pub fn hours_since(&self, earlier: &TradingTimestamp) -> f64 {
        let diff_nanos = self.0 - earlier.0;
        diff_nanos as f64 / (NANOS_PER_SECOND as f64 * 3600.0)
    }

    /// Days between two timestamps (fractional)
    pub fn days_since(&self, earlier: &TradingTimestamp) -> f64 {
        let diff_nanos = self.0 - earlier.0;
        diff_nanos as f64 / NANOS_PER_DAY as f64
    }
}

impl MarketTime {
    pub fn new(hour: u32, minute: u32) -> Self {
        Self { hour, minute }
    }

    pub fn to_naive_time(&self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.hour, self.minute, 0).unwrap()
    }
}

// Display implementations
impl std::fmt::Display for TradingDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_naive_date().format("%Y-%m-%d"))
    }
}

impl std::fmt::Display for TradingTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.to_datetime_utc().format("%Y-%m-%d %H:%M:%S UTC")
        )
    }
}

impl std::fmt::Display for MarketTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eastern_to_utc_est() {
        // Nov 3, 2025 is in EST (UTC-5)
        let date = NaiveDate::from_ymd_opt(2025, 11, 3).unwrap();
        let time = NaiveTime::from_hms_opt(9, 35, 0).unwrap();

        let utc = eastern_to_utc(date, time);

        // 09:35 EST = 14:35 UTC
        assert_eq!(utc.date_naive(), date);
        assert_eq!(utc.hour(), 14);
        assert_eq!(utc.minute(), 35);
    }

    #[test]
    fn test_eastern_to_utc_edt() {
        // Jul 15, 2025 is in EDT (UTC-4)
        let date = NaiveDate::from_ymd_opt(2025, 7, 15).unwrap();
        let time = NaiveTime::from_hms_opt(9, 35, 0).unwrap();

        let utc = eastern_to_utc(date, time);

        // 09:35 EDT = 13:35 UTC
        assert_eq!(utc.date_naive(), date);
        assert_eq!(utc.hour(), 13);
        assert_eq!(utc.minute(), 35);
    }

    #[test]
    fn test_roundtrip_naive_date() {
        let original = NaiveDate::from_ymd_opt(2025, 11, 3).unwrap();
        let trading = TradingDate::from_naive_date(original);
        assert_eq!(trading.to_naive_date(), original);
    }

    #[test]
    fn test_polars_date_format() {
        // Nov 3, 2025 should be ~20,395 days since 1970-01-01
        let trading = TradingDate::from_ymd(2025, 11, 3).unwrap();
        let polars_days = trading.to_polars_date();
        // Verify it's in the expected range
        assert!(polars_days > 20000 && polars_days < 21000);

        // Verify roundtrip
        let back = TradingDate::from_polars_date(polars_days);
        assert_eq!(back, trading);
    }

    #[test]
    fn test_dte_calculation() {
        let nov_3 = TradingDate::from_ymd(2025, 11, 3).unwrap();
        let nov_21 = TradingDate::from_ymd(2025, 11, 21).unwrap();
        assert_eq!(nov_21.dte(&nov_3), 18);
    }

    #[test]
    fn test_with_time() {
        let date = TradingDate::from_ymd(2025, 11, 3).unwrap();
        let time = MarketTime { hour: 9, minute: 35 };
        let ts = date.with_time(&time);
        let dt = ts.to_datetime_utc();
        // Nov 3, 2025 is EST (UTC-5), so 9:35 EST = 14:35 UTC
        assert_eq!(dt.hour(), 14);
        assert_eq!(dt.minute(), 35);
    }

    #[test]
    fn test_unix_epoch() {
        let epoch = TradingDate::from_ymd(1970, 1, 1).unwrap();
        assert_eq!(epoch.to_polars_date(), 0);
    }

    #[test]
    fn test_timestamp_roundtrip() {
        let dt = Utc::now();
        let ts = TradingTimestamp::from_datetime_utc(dt);
        let back = ts.to_datetime_utc();
        // Allow some precision loss due to nanos conversion
        assert_eq!(dt.timestamp(), back.timestamp());
    }

    #[test]
    fn test_time_to_expiry() {
        let nov_3 = TradingDate::from_ymd(2025, 11, 3).unwrap();
        let nov_21 = TradingDate::from_ymd(2025, 11, 21).unwrap();
        let entry_time = MarketTime::new(9, 35);
        let exit_time = MarketTime::new(16, 0);

        let entry_ts = nov_3.with_time(&entry_time);
        let ttm = entry_ts.time_to_expiry(&nov_21, &exit_time);

        // Should be approximately 18.27 days / 365.25 = ~0.05 years
        // (18 days + 6h25m from 09:35 to 16:00)
        assert!(ttm > 0.04 && ttm < 0.06);
    }
}
