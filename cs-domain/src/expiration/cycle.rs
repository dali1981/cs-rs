use chrono::{NaiveDate, Datelike, Weekday};

/// Classification of option expiration cycles
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpirationCycle {
    /// Weekly expiration (any Friday except 3rd Friday)
    Weekly,
    /// Monthly expiration (3rd Friday of month)
    Monthly,
    /// Quarterly expiration (3rd Friday of Mar/Jun/Sep/Dec)
    Quarterly,
    /// LEAPS (January expiration 1+ year out)
    Leaps,
    /// Non-standard expiration (not a Friday, or unusual date)
    NonStandard,
}

impl ExpirationCycle {
    /// Classify an expiration date into its cycle type
    pub fn classify(date: NaiveDate) -> Self {
        // Non-Friday expirations are non-standard (e.g., some ETF expirations)
        if date.weekday() != Weekday::Fri {
            return Self::NonStandard;
        }

        let day = date.day();
        let is_third_friday = (15..=21).contains(&day);

        if !is_third_friday {
            return Self::Weekly;
        }

        // It's a 3rd Friday - determine if monthly, quarterly, or LEAPS
        let month = date.month();
        let current_year = chrono::Utc::now().year();

        // January expiration 1+ year out is LEAPS
        if month == 1 && date.year() > current_year {
            return Self::Leaps;
        }

        // Quarterly: Mar, Jun, Sep, Dec
        if matches!(month, 3 | 6 | 9 | 12) {
            return Self::Quarterly;
        }

        Self::Monthly
    }

    /// Check if this is a "standard" monthly-or-better cycle
    pub fn is_monthly_or_longer(&self) -> bool {
        matches!(self, Self::Monthly | Self::Quarterly | Self::Leaps)
    }

    /// Check if this is a weekly expiration
    pub fn is_weekly(&self) -> bool {
        matches!(self, Self::Weekly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weekly_detection() {
        // Sept 12, 2025 is a Friday but NOT 3rd Friday (Sept 19 is 3rd)
        let weekly = NaiveDate::from_ymd_opt(2025, 9, 12).unwrap();
        assert_eq!(ExpirationCycle::classify(weekly), ExpirationCycle::Weekly);
    }

    #[test]
    fn test_monthly_detection() {
        // Oct 17, 2025 is 3rd Friday of October
        let monthly = NaiveDate::from_ymd_opt(2025, 10, 17).unwrap();
        assert_eq!(ExpirationCycle::classify(monthly), ExpirationCycle::Monthly);
    }

    #[test]
    fn test_quarterly_detection() {
        // Dec 19, 2025 is 3rd Friday of December
        let quarterly = NaiveDate::from_ymd_opt(2025, 12, 19).unwrap();
        assert_eq!(ExpirationCycle::classify(quarterly), ExpirationCycle::Quarterly);
    }

    #[test]
    fn test_non_friday() {
        // Wednesday expiration (non-standard)
        let wed = NaiveDate::from_ymd_opt(2025, 10, 15).unwrap();
        assert_eq!(ExpirationCycle::classify(wed), ExpirationCycle::NonStandard);
    }
}
