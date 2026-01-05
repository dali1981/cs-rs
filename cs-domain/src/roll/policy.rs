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

    /// Calculate next roll date from given start date
    ///
    /// Returns None for RollPolicy::None
    pub fn next_roll_date(&self, from: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::None => None,

            Self::Weekly { roll_day } => {
                Some(next_weekday(from, *roll_day))
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
