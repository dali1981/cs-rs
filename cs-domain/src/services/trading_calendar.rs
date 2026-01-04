use chrono::{NaiveDate, Weekday, Datelike};

/// Trading calendar utilities
pub struct TradingCalendar;

impl TradingCalendar {
    /// Check if date is a trading day (excludes weekends, not holidays)
    pub fn is_trading_day(date: NaiveDate) -> bool {
        !matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
    }

    /// Get next trading day
    pub fn next_trading_day(date: NaiveDate) -> NaiveDate {
        let mut next = date + chrono::Duration::days(1);
        while !Self::is_trading_day(next) {
            next += chrono::Duration::days(1);
        }
        next
    }

    /// Get previous trading day
    pub fn previous_trading_day(date: NaiveDate) -> NaiveDate {
        let mut prev = date - chrono::Duration::days(1);
        while !Self::is_trading_day(prev) {
            prev -= chrono::Duration::days(1);
        }
        prev
    }

    /// Iterate over trading days in range (inclusive)
    pub fn trading_days_between(
        start: NaiveDate,
        end: NaiveDate,
    ) -> impl Iterator<Item = NaiveDate> {
        let mut current = start;
        std::iter::from_fn(move || {
            while current <= end && !Self::is_trading_day(current) {
                current += chrono::Duration::days(1);
            }
            if current <= end {
                let result = current;
                current += chrono::Duration::days(1);
                Some(result)
            } else {
                None
            }
        })
    }

    /// Get N trading days before a date
    ///
    /// Example: n_trading_days_before(2025-01-10, 5)
    ///          -> 2025-01-03 (skipping weekends)
    pub fn n_trading_days_before(date: NaiveDate, n: usize) -> NaiveDate {
        let mut result = date;
        let mut count = 0;
        while count < n {
            result = Self::previous_trading_day(result);
            count += 1;
        }
        result
    }

    /// Get N trading days after a date
    ///
    /// Example: n_trading_days_after(2025-01-10, 5)
    ///          -> 2025-01-17 (skipping weekends)
    pub fn n_trading_days_after(date: NaiveDate, n: usize) -> NaiveDate {
        let mut result = date;
        let mut count = 0;
        while count < n {
            result = Self::next_trading_day(result);
            count += 1;
        }
        result
    }

    /// Count trading days between two dates (exclusive of start, inclusive of end)
    pub fn trading_days_count(start: NaiveDate, end: NaiveDate) -> usize {
        Self::trading_days_between(start, end).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_trading_day_weekday() {
        // Monday, June 2, 2025
        let monday = NaiveDate::from_ymd_opt(2025, 6, 2).unwrap();
        assert!(TradingCalendar::is_trading_day(monday));
    }

    #[test]
    fn test_is_trading_day_saturday() {
        // Saturday, June 7, 2025
        let saturday = NaiveDate::from_ymd_opt(2025, 6, 7).unwrap();
        assert!(!TradingCalendar::is_trading_day(saturday));
    }

    #[test]
    fn test_is_trading_day_sunday() {
        // Sunday, June 8, 2025
        let sunday = NaiveDate::from_ymd_opt(2025, 6, 8).unwrap();
        assert!(!TradingCalendar::is_trading_day(sunday));
    }

    #[test]
    fn test_next_trading_day_weekday() {
        // Monday -> Tuesday
        let monday = NaiveDate::from_ymd_opt(2025, 6, 2).unwrap();
        let next = TradingCalendar::next_trading_day(monday);
        assert_eq!(next, NaiveDate::from_ymd_opt(2025, 6, 3).unwrap());
    }

    #[test]
    fn test_next_trading_day_friday() {
        // Friday -> Monday (skip weekend)
        let friday = NaiveDate::from_ymd_opt(2025, 6, 6).unwrap();
        let next = TradingCalendar::next_trading_day(friday);
        assert_eq!(next, NaiveDate::from_ymd_opt(2025, 6, 9).unwrap());
    }

    #[test]
    fn test_next_trading_day_saturday() {
        // Saturday -> Monday
        let saturday = NaiveDate::from_ymd_opt(2025, 6, 7).unwrap();
        let next = TradingCalendar::next_trading_day(saturday);
        assert_eq!(next, NaiveDate::from_ymd_opt(2025, 6, 9).unwrap());
    }

    #[test]
    fn test_previous_trading_day_weekday() {
        // Tuesday -> Monday
        let tuesday = NaiveDate::from_ymd_opt(2025, 6, 3).unwrap();
        let prev = TradingCalendar::previous_trading_day(tuesday);
        assert_eq!(prev, NaiveDate::from_ymd_opt(2025, 6, 2).unwrap());
    }

    #[test]
    fn test_previous_trading_day_monday() {
        // Monday -> Friday (skip weekend)
        let monday = NaiveDate::from_ymd_opt(2025, 6, 9).unwrap();
        let prev = TradingCalendar::previous_trading_day(monday);
        assert_eq!(prev, NaiveDate::from_ymd_opt(2025, 6, 6).unwrap());
    }

    #[test]
    fn test_previous_trading_day_sunday() {
        // Sunday -> Friday
        let sunday = NaiveDate::from_ymd_opt(2025, 6, 8).unwrap();
        let prev = TradingCalendar::previous_trading_day(sunday);
        assert_eq!(prev, NaiveDate::from_ymd_opt(2025, 6, 6).unwrap());
    }

    #[test]
    fn test_trading_days_between_same_week() {
        let start = NaiveDate::from_ymd_opt(2025, 6, 2).unwrap(); // Monday
        let end = NaiveDate::from_ymd_opt(2025, 6, 6).unwrap();   // Friday

        let days: Vec<_> = TradingCalendar::trading_days_between(start, end).collect();
        assert_eq!(days.len(), 5); // Mon, Tue, Wed, Thu, Fri
    }

    #[test]
    fn test_trading_days_between_with_weekend() {
        let start = NaiveDate::from_ymd_opt(2025, 6, 6).unwrap(); // Friday
        let end = NaiveDate::from_ymd_opt(2025, 6, 10).unwrap();  // Tuesday

        let days: Vec<_> = TradingCalendar::trading_days_between(start, end).collect();
        assert_eq!(days.len(), 3); // Fri, Mon, Tue (skip Sat, Sun)

        assert_eq!(days[0], NaiveDate::from_ymd_opt(2025, 6, 6).unwrap());
        assert_eq!(days[1], NaiveDate::from_ymd_opt(2025, 6, 9).unwrap());
        assert_eq!(days[2], NaiveDate::from_ymd_opt(2025, 6, 10).unwrap());
    }

    #[test]
    fn test_trading_days_between_starting_on_weekend() {
        let start = NaiveDate::from_ymd_opt(2025, 6, 7).unwrap(); // Saturday
        let end = NaiveDate::from_ymd_opt(2025, 6, 10).unwrap();  // Tuesday

        let days: Vec<_> = TradingCalendar::trading_days_between(start, end).collect();
        assert_eq!(days.len(), 2); // Mon, Tue (skip Sat, Sun)
    }

    #[test]
    fn test_trading_days_between_single_day() {
        let date = NaiveDate::from_ymd_opt(2025, 6, 2).unwrap(); // Monday

        let days: Vec<_> = TradingCalendar::trading_days_between(date, date).collect();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0], date);
    }

    #[test]
    fn test_trading_days_between_weekend_only() {
        let start = NaiveDate::from_ymd_opt(2025, 6, 7).unwrap(); // Saturday
        let end = NaiveDate::from_ymd_opt(2025, 6, 8).unwrap();   // Sunday

        let days: Vec<_> = TradingCalendar::trading_days_between(start, end).collect();
        assert_eq!(days.len(), 0); // No trading days on weekend
    }

    #[test]
    fn test_n_trading_days_after_same_week() {
        // Monday + 4 days = Friday
        let monday = NaiveDate::from_ymd_opt(2025, 6, 2).unwrap();
        let result = TradingCalendar::n_trading_days_after(monday, 4);
        assert_eq!(result, NaiveDate::from_ymd_opt(2025, 6, 6).unwrap());
    }

    #[test]
    fn test_n_trading_days_after_with_weekend() {
        // Friday + 3 days = Wednesday (skip weekend)
        let friday = NaiveDate::from_ymd_opt(2025, 6, 6).unwrap();
        let result = TradingCalendar::n_trading_days_after(friday, 3);
        assert_eq!(result, NaiveDate::from_ymd_opt(2025, 6, 11).unwrap());
    }
}
