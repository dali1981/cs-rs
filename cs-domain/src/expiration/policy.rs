use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use super::ExpirationCycle;
use crate::strike_selection::SelectionError;

/// Policy for selecting option expirations
///
/// Expirations can be selected based on various criteria:
/// - Minimum date constraint (must expire after a certain date)
/// - Cycle preference (prefer weeklies or monthlies)
/// - Target DTE (days to expiration from entry)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExpirationPolicy {
    /// Select first expiration >= min_date
    ///
    /// This is the simplest policy - just find the soonest valid expiration.
    FirstAfter {
        /// Minimum expiration date (expiration must be > this date)
        min_date: NaiveDate,
    },

    /// Prefer weekly expirations
    ///
    /// Useful for weekly roll strategies or capturing short-term theta.
    PreferWeekly {
        /// Minimum expiration date
        min_date: NaiveDate,
        /// If no weekly found, fall back to monthly?
        fallback_to_monthly: bool,
    },

    /// Prefer monthly (3rd Friday) expirations
    ///
    /// Avoids weekly pin risk and typically has tighter spreads.
    PreferMonthly {
        /// Minimum expiration date
        min_date: NaiveDate,
        /// Which monthly: 0 = first monthly >= min_date, 1 = second, etc.
        months_out: u8,
    },

    /// Target a specific DTE from entry date
    ///
    /// Finds the expiration closest to target_dte days from entry_date.
    TargetDte {
        /// The target days to expiration
        target_dte: i32,
        /// Acceptable tolerance (+/- days)
        tolerance: i32,
        /// Entry date for DTE calculation
        entry_date: NaiveDate,
    },

    /// Calendar spread: separate policies for short and long legs
    Calendar {
        /// Policy for short (near-term) leg
        short: Box<ExpirationPolicy>,
        /// Policy for long (far-term) leg
        long: Box<ExpirationPolicy>,
    },
}

impl ExpirationPolicy {
    // =========================================================================
    // Convenience constructors
    // =========================================================================

    /// Create a policy that selects the first expiration after `min_date`
    pub fn first_after(min_date: NaiveDate) -> Self {
        Self::FirstAfter { min_date }
    }

    /// Create a policy that prefers weekly expirations
    pub fn prefer_weekly(min_date: NaiveDate) -> Self {
        Self::PreferWeekly {
            min_date,
            fallback_to_monthly: true,
        }
    }

    /// Create a policy that prefers monthly expirations
    pub fn prefer_monthly(min_date: NaiveDate, months_out: u8) -> Self {
        Self::PreferMonthly { min_date, months_out }
    }

    /// Create a policy targeting a specific DTE
    pub fn target_dte(entry_date: NaiveDate, target_dte: i32, tolerance: i32) -> Self {
        Self::TargetDte {
            target_dte,
            tolerance,
            entry_date,
        }
    }

    // =========================================================================
    // Selection logic
    // =========================================================================

    /// Select a single expiration from available dates
    pub fn select(&self, expirations: &[NaiveDate]) -> Result<NaiveDate, SelectionError> {
        let mut sorted: Vec<NaiveDate> = expirations.to_vec();
        sorted.sort();

        match self {
            Self::FirstAfter { min_date } => {
                sorted.into_iter()
                    .find(|&exp| exp > *min_date)
                    .ok_or(SelectionError::NoExpirations)
            }

            Self::PreferWeekly { min_date, fallback_to_monthly } => {
                // First, try to find a weekly after min_date
                let weekly = sorted.iter()
                    .filter(|&&exp| exp > *min_date)
                    .find(|&&exp| ExpirationCycle::classify(exp).is_weekly())
                    .copied();

                if let Some(w) = weekly {
                    return Ok(w);
                }

                // No weekly found - fallback?
                if *fallback_to_monthly {
                    sorted.into_iter()
                        .find(|&exp| exp > *min_date)
                        .ok_or(SelectionError::NoExpirations)
                } else {
                    Err(SelectionError::NoExpirations)
                }
            }

            Self::PreferMonthly { min_date, months_out } => {
                let monthlies: Vec<NaiveDate> = sorted.into_iter()
                    .filter(|&exp| exp > *min_date)
                    .filter(|&exp| ExpirationCycle::classify(exp).is_monthly_or_longer())
                    .collect();

                monthlies.get(*months_out as usize)
                    .copied()
                    .ok_or(SelectionError::NoExpirations)
            }

            Self::TargetDte { target_dte, tolerance, entry_date } => {
                sorted.into_iter()
                    .filter(|&exp| {
                        let dte = (exp - *entry_date).num_days() as i32;
                        dte > 0 && (dte - target_dte).abs() <= *tolerance
                    })
                    .min_by_key(|&exp| {
                        let dte = (exp - *entry_date).num_days() as i32;
                        (dte - target_dte).abs()
                    })
                    .ok_or(SelectionError::NoExpirations)
            }

            Self::Calendar { short, long: _ } => {
                // For Calendar policy, select() returns the short leg
                // Use select_pair() for both legs
                short.select(expirations)
            }
        }
    }

    /// Select a pair of expirations (for calendar spreads)
    pub fn select_pair(
        &self,
        expirations: &[NaiveDate],
    ) -> Result<(NaiveDate, NaiveDate), SelectionError> {
        match self {
            Self::Calendar { short, long } => {
                let short_exp = short.select(expirations)?;

                // For long leg, filter out short expiration and earlier
                let long_candidates: Vec<NaiveDate> = expirations.iter()
                    .filter(|&&exp| exp > short_exp)
                    .copied()
                    .collect();

                let long_exp = long.select(&long_candidates)?;

                Ok((short_exp, long_exp))
            }
            _ => {
                // Non-calendar policy: just use same expiration for both
                let exp = self.select(expirations)?;
                Ok((exp, exp))
            }
        }
    }

    /// Update the min_date constraint (useful for building policies dynamically)
    pub fn with_min_date(self, new_min_date: NaiveDate) -> Self {
        match self {
            Self::FirstAfter { .. } => Self::FirstAfter { min_date: new_min_date },
            Self::PreferWeekly { fallback_to_monthly, .. } =>
                Self::PreferWeekly { min_date: new_min_date, fallback_to_monthly },
            Self::PreferMonthly { months_out, .. } =>
                Self::PreferMonthly { min_date: new_min_date, months_out },
            Self::TargetDte { target_dte, tolerance, .. } =>
                Self::TargetDte { target_dte, tolerance, entry_date: new_min_date },
            Self::Calendar { short, long } => Self::Calendar {
                short: Box::new(short.with_min_date(new_min_date)),
                long: Box::new(long.with_min_date(new_min_date)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_expirations() -> Vec<NaiveDate> {
        vec![
            NaiveDate::from_ymd_opt(2025, 9, 12).unwrap(),  // Weekly
            NaiveDate::from_ymd_opt(2025, 9, 19).unwrap(),  // Monthly (3rd Fri)
            NaiveDate::from_ymd_opt(2025, 9, 26).unwrap(),  // Weekly
            NaiveDate::from_ymd_opt(2025, 10, 17).unwrap(), // Monthly
            NaiveDate::from_ymd_opt(2025, 10, 24).unwrap(), // Weekly
        ]
    }

    #[test]
    fn test_first_after() {
        let exps = sample_expirations();
        let min = NaiveDate::from_ymd_opt(2025, 9, 15).unwrap();
        let policy = ExpirationPolicy::first_after(min);

        let result = policy.select(&exps).unwrap();
        assert_eq!(result, NaiveDate::from_ymd_opt(2025, 9, 19).unwrap());
    }

    #[test]
    fn test_prefer_weekly() {
        let exps = sample_expirations();
        let min = NaiveDate::from_ymd_opt(2025, 9, 15).unwrap();
        let policy = ExpirationPolicy::prefer_weekly(min);

        let result = policy.select(&exps).unwrap();
        // Should skip Sept 19 (monthly) and pick Sept 26 (weekly)
        assert_eq!(result, NaiveDate::from_ymd_opt(2025, 9, 26).unwrap());
    }

    #[test]
    fn test_prefer_monthly() {
        let exps = sample_expirations();
        let min = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let policy = ExpirationPolicy::prefer_monthly(min, 0);

        let result = policy.select(&exps).unwrap();
        // First monthly is Sept 19
        assert_eq!(result, NaiveDate::from_ymd_opt(2025, 9, 19).unwrap());
    }

    #[test]
    fn test_prefer_monthly_second() {
        let exps = sample_expirations();
        let min = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let policy = ExpirationPolicy::prefer_monthly(min, 1);

        let result = policy.select(&exps).unwrap();
        // Second monthly is Oct 17
        assert_eq!(result, NaiveDate::from_ymd_opt(2025, 10, 17).unwrap());
    }

    #[test]
    fn test_calendar_policy() {
        let exps = sample_expirations();
        let min = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();

        let policy = ExpirationPolicy::Calendar {
            short: Box::new(ExpirationPolicy::prefer_monthly(min, 0)),
            long: Box::new(ExpirationPolicy::prefer_monthly(min, 1)),
        };

        let (short, long) = policy.select_pair(&exps).unwrap();
        assert_eq!(short, NaiveDate::from_ymd_opt(2025, 9, 19).unwrap());
        assert_eq!(long, NaiveDate::from_ymd_opt(2025, 10, 17).unwrap());
    }
}
