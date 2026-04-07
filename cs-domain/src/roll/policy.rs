use chrono::{Datelike, NaiveDate, Weekday};
use crate::expiration::ExpirationPolicy;
use serde::{Deserialize, Serialize};

/// Policy for rolling (renewing) positions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RollPolicy {
    /// No rolling - hold position until exit
    None,

    /// Roll every week on specified day
    ///
    /// Typical usage: Roll every Friday to reset ATM position
    Weekly {
        /// Day of week to roll on
        #[serde(with = "weekday_serde")]
        roll_day: Weekday,
    },

    /// Roll on monthly expiration cycle
    ///
    /// Tracks the monthly expiration calendar (3rd Friday).
    Monthly {
        /// Week relative to 3rd Friday to roll:
        /// - 0: Roll ON 3rd Friday (expiration day)
        /// - 1: Roll 1 week after 3rd Friday
        /// - -1: Roll 1 week before 3rd Friday
        roll_week_offset: i8,
    },

    /// Roll every N trading days
    ///
    /// Simpler than weekly - just counts trading days
    TradingDays {
        /// Number of trading days between rolls
        interval: u16,
    },

    /// Roll when current position expires
    ///
    /// On expiration day, close current position and open new one
    /// using the specified expiration policy.
    OnExpiration {
        /// Policy to select next expiration after roll
        to_next: ExpirationPolicy,
    },

    /// Roll when DTE drops below threshold
    ///
    /// Roll before expiration to avoid gamma risk.
    DteThreshold {
        /// Minimum DTE - roll when position DTE < this
        min_dte: i32,
        /// Policy to select next expiration
        to_policy: ExpirationPolicy,
    },

    /// Roll at fixed time intervals
    ///
    /// Roll every N days regardless of expiration.
    TimeInterval {
        /// Days between rolls
        interval_days: u16,
        /// Policy to select next expiration
        to_policy: ExpirationPolicy,
    },
}

// Helper module for serializing Weekday
mod weekday_serde {
    use chrono::Weekday;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(weekday: &Weekday, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&weekday.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Weekday, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "monday" | "mon" => Ok(Weekday::Mon),
            "tuesday" | "tue" => Ok(Weekday::Tue),
            "wednesday" | "wed" => Ok(Weekday::Wed),
            "thursday" | "thu" => Ok(Weekday::Thu),
            "friday" | "fri" => Ok(Weekday::Fri),
            "saturday" | "sat" => Ok(Weekday::Sat),
            "sunday" | "sun" => Ok(Weekday::Sun),
            _ => Err(serde::de::Error::custom(format!("Invalid weekday: {}", s))),
        }
    }
}

impl RollPolicy {
    /// Check if rolling is enabled
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Find next monthly 3rd Friday
    fn next_third_friday(from: NaiveDate) -> NaiveDate {
        let mut date = from;
        loop {
            // Move to next month if past 21st (can't be 3rd Friday)
            if date.day() > 21 {
                date = NaiveDate::from_ymd_opt(
                    if date.month() == 12 { date.year() + 1 } else { date.year() },
                    if date.month() == 12 { 1 } else { date.month() + 1 },
                    1,
                ).unwrap();
            }

            // Find 3rd Friday of this month
            let first_of_month = NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap();
            let first_friday = (0..7)
                .map(|d| first_of_month + chrono::Duration::days(d))
                .find(|d| d.weekday() == Weekday::Fri)
                .unwrap();
            let third_friday = first_friday + chrono::Duration::weeks(2);

            if third_friday > from {
                return third_friday;
            }

            // Move to next month
            date = NaiveDate::from_ymd_opt(
                if date.month() == 12 { date.year() + 1 } else { date.year() },
                if date.month() == 12 { 1 } else { date.month() + 1 },
                1,
            ).unwrap();
        }
    }

    /// Calculate next roll date from given start date
    ///
    /// Returns None for RollPolicy::None
    pub fn next_roll_date(&self, from: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::None => None,

            Self::Weekly { roll_day } => {
                Some(next_weekday(from, *roll_day))
            }

            Self::Monthly { roll_week_offset } => {
                let third_friday = Self::next_third_friday(from);
                let roll_date = third_friday + chrono::Duration::weeks(*roll_week_offset as i64);

                // Ensure roll date is after 'from'
                if roll_date <= from {
                    // Get next month's 3rd Friday
                    let next_third = Self::next_third_friday(third_friday + chrono::Duration::days(1));
                    Some(next_third + chrono::Duration::weeks(*roll_week_offset as i64))
                } else {
                    Some(roll_date)
                }
            }

            Self::TradingDays { interval } => {
                // Simple approximation: 1.4 calendar days per trading day
                let calendar_days = (*interval as f64 * 1.4).round() as i64;
                Some(from + chrono::Duration::days(calendar_days))
            }

            Self::OnExpiration { .. } |
            Self::DteThreshold { .. } |
            Self::TimeInterval { .. } => {
                // These need expiration/last_roll context, handled elsewhere
                None
            }
        }
    }

    /// Determine if a roll is needed at the given date
    pub fn should_roll(
        &self,
        current_date: NaiveDate,
        current_expiration: NaiveDate,
        last_roll_date: Option<NaiveDate>,
    ) -> bool {
        match self {
            Self::None => false,

            Self::Weekly { roll_day } => {
                // Roll if today is the roll day and we haven't rolled yet this week
                if current_date.weekday() == *roll_day {
                    match last_roll_date {
                        Some(last) => (current_date - last).num_days() >= 7,
                        None => true, // First roll
                    }
                } else {
                    false
                }
            }

            Self::Monthly { roll_week_offset: _ } => {
                // Check if current_date is a monthly roll date
                if let Some(next_roll) = self.next_roll_date(current_date - chrono::Duration::days(1)) {
                    if next_roll == current_date {
                        match last_roll_date {
                            Some(last) => (current_date - last).num_days() >= 28, // At least ~1 month
                            None => true, // First roll
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }

            Self::TradingDays { interval } => {
                match last_roll_date {
                    Some(last) => {
                        // Approximate trading days
                        let calendar_days = (current_date - last).num_days();
                        let est_trading_days = (calendar_days as f64 / 1.4) as u16;
                        est_trading_days >= *interval
                    }
                    None => true, // First roll
                }
            }

            Self::OnExpiration { .. } => {
                // Roll on or after expiration
                current_date >= current_expiration
            }

            Self::DteThreshold { min_dte, .. } => {
                let dte = (current_expiration - current_date).num_days() as i32;
                dte < *min_dte
            }

            Self::TimeInterval { interval_days, .. } => {
                match last_roll_date {
                    Some(last) => {
                        let days_since = (current_date - last).num_days();
                        days_since >= *interval_days as i64
                    }
                    None => false // First position, no roll yet
                }
            }
        }
    }

    /// Get the expiration policy for rolling
    pub fn expiration_policy(&self) -> Option<&ExpirationPolicy> {
        match self {
            Self::None => None,
            Self::Weekly { .. } => None,  // No expiration policy, just rolls weekly
            Self::Monthly { .. } => None,  // No expiration policy, just rolls monthly
            Self::TradingDays { .. } => None,  // No expiration policy, just rolls by days
            Self::OnExpiration { to_next } => Some(to_next),
            Self::DteThreshold { to_policy, .. } => Some(to_policy),
            Self::TimeInterval { to_policy, .. } => Some(to_policy),
        }
    }

    /// Get a human-readable description of the policy
    pub fn description(&self) -> String {
        match self {
            Self::None => "No rolling".to_string(),
            Self::Weekly { roll_day } => format!("Weekly on {:?}", roll_day),
            Self::Monthly { roll_week_offset } => {
                match roll_week_offset {
                    0 => "Monthly on 3rd Friday".to_string(),
                    1 => "Monthly 1 week after 3rd Friday".to_string(),
                    -1 => "Monthly 1 week before 3rd Friday".to_string(),
                    n if *n > 0 => format!("Monthly {} weeks after 3rd Friday", n),
                    n => format!("Monthly {} weeks before 3rd Friday", n.abs()),
                }
            }
            Self::TradingDays { interval } => format!("Every {} trading days", interval),
            Self::OnExpiration { .. } => "On expiration".to_string(),
            Self::DteThreshold { min_dte, .. } => format!("When DTE < {}", min_dte),
            Self::TimeInterval { interval_days, .. } => format!("Every {} days", interval_days),
        }
    }
}

/// Find the next occurrence of a given weekday
fn next_weekday(from: NaiveDate, target: Weekday) -> NaiveDate {
    let current = from.weekday();
    let days_ahead = (target.num_days_from_monday() + 7 - current.num_days_from_monday()) % 7;
    let days_ahead = if days_ahead == 0 { 7 } else { days_ahead };  // If today is the target, next week
    from + chrono::Duration::days(days_ahead as i64)
}

impl Default for RollPolicy {
    fn default() -> Self {
        Self::None
    }
}
