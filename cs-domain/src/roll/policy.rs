use chrono::NaiveDate;
use crate::expiration::ExpirationPolicy;

/// Policy for rolling (renewing) positions
#[derive(Debug, Clone)]
pub enum RollPolicy {
    /// No rolling - hold position until exit
    None,

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

impl RollPolicy {
    /// Check if rolling is enabled
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
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
            Self::OnExpiration { to_next } => Some(to_next),
            Self::DteThreshold { to_policy, .. } => Some(to_policy),
            Self::TimeInterval { to_policy, .. } => Some(to_policy),
        }
    }
}

impl Default for RollPolicy {
    fn default() -> Self {
        Self::None
    }
}
