use chrono::{DateTime, Duration, NaiveDate, Timelike, Utc};
use cs_domain::{EarningsEvent, EarningsTradeTiming, StraddleTradeTiming, PostEarningsStraddleTiming, HedgeStrategy, TradingCalendar};

/// Timing strategy enum that wraps all timing implementations
///
/// This enum allows different trade structures to use different timing strategies
/// without requiring dynamic dispatch or trait objects.
#[derive(Clone)]
pub enum TimingStrategy {
    /// Calendar spread / Iron butterfly timing: enter on/before earnings day
    Earnings(EarningsTradeTiming),

    /// Straddle timing: enter N days before earnings to capture IV expansion
    Straddle(StraddleTradeTiming),

    /// Post-earnings straddle timing: enter day after earnings
    PostEarnings(PostEarningsStraddleTiming),
}

// Factory functions for creating timing strategies
//
// These extract the hardcoded timing defaults from strategy structs,
// making timing composable and independent of trade type.
impl TimingStrategy {
    /// Create earnings-based timing (default for calendar spreads, iron butterflies, calendar straddles)
    ///
    /// Entry: Day before earnings (BMO) or on earnings day (AMC)
    /// Exit: Day after earnings
    pub fn for_earnings(config: cs_domain::TimingConfig) -> Self {
        TimingStrategy::Earnings(EarningsTradeTiming::new(config))
    }

    /// Create straddle timing (enter N days before earnings)
    ///
    /// Entry: N trading days before earnings
    /// Exit: M trading days before earnings (or on earnings day if M=0)
    pub fn for_straddle(config: cs_domain::TimingConfig, entry_days: usize, exit_days: usize) -> Self {
        let timing = StraddleTradeTiming::new(config)
            .with_entry_days(entry_days)
            .with_exit_days(exit_days);
        TimingStrategy::Straddle(timing)
    }

    /// Create post-earnings straddle timing (enter after earnings)
    ///
    /// Entry: Day after earnings announcement
    /// Exit: N trading days after entry
    pub fn for_post_earnings(config: cs_domain::TimingConfig, holding_days: usize) -> Self {
        let timing = PostEarningsStraddleTiming::new(config)
            .with_holding_days(holding_days);
        TimingStrategy::PostEarnings(timing)
    }
}

impl TimingStrategy {
    /// Get entry datetime for the trade
    pub fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        match self {
            TimingStrategy::Earnings(t) => t.entry_datetime(event),
            TimingStrategy::Straddle(t) => t.entry_datetime(event),
            TimingStrategy::PostEarnings(t) => t.entry_datetime(event),
        }
    }

    /// Get exit datetime for the trade
    pub fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        match self {
            TimingStrategy::Earnings(t) => t.exit_datetime(event),
            TimingStrategy::Straddle(t) => t.exit_datetime(event),
            TimingStrategy::PostEarnings(t) => t.exit_datetime(event),
        }
    }

    /// Get entry date for the trade
    pub fn entry_date(&self, event: &EarningsEvent) -> NaiveDate {
        match self {
            TimingStrategy::Earnings(t) => t.entry_date(event),
            TimingStrategy::Straddle(t) => t.entry_date(event),
            TimingStrategy::PostEarnings(t) => t.entry_date(event),
        }
    }

    /// Get exit date for the trade
    pub fn exit_date(&self, event: &EarningsEvent) -> NaiveDate {
        match self {
            TimingStrategy::Earnings(t) => t.exit_date(event),
            TimingStrategy::Straddle(t) => t.exit_date(event),
            TimingStrategy::PostEarnings(t) => t.exit_date(event),
        }
    }

    /// Calculate lookahead days needed for event loading
    ///
    /// This determines how far ahead to look when loading earnings events
    /// for a given session date. Different timing strategies need different
    /// lookahead windows:
    ///
    /// - Earnings: Small lookahead (2-3 days) since entry is on/near earnings
    /// - Straddle: Large lookahead (entry_days * 1.5 + 7) to account for weekends
    /// - PostEarnings: Negative lookahead (lookback) since we enter after earnings
    pub fn lookahead_days(&self) -> i64 {
        match self {
            TimingStrategy::Earnings(_) => {
                // Calendar spreads/butterflies enter on earnings day (AMC) or day before (BMO)
                // Need small lookahead for AMC events
                3
            }
            TimingStrategy::Straddle(t) => {
                // Entry is N days before earnings
                // Add buffer for weekends/holidays: multiply by 1.5 and add 7 days
                let entry_days = t.entry_days_before();
                ((entry_days as f64 * 1.5) as i64) + 7
            }
            TimingStrategy::PostEarnings(_) => {
                // Post-earnings enters AFTER the event, so we need lookback, not lookahead
                // Return negative to signal lookback
                -3
            }
        }
    }

    /// Compute rehedge timestamps between entry and exit
    ///
    /// Returns a vector of timestamps when delta hedge should be checked/executed.
    /// The actual decision to rehedge is made by HedgeConfig::should_rehedge().
    pub fn rehedge_times(
        &self,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        strategy: &HedgeStrategy,
    ) -> Vec<DateTime<Utc>> {
        match strategy {
            HedgeStrategy::None => vec![],
            HedgeStrategy::TimeBased { interval } => {
                // Include entry_time as first hedge to ensure delta-neutral entry
                let mut times = vec![entry_time];
                let mut current = entry_time + *interval;
                while current < exit_time {
                    // Only include trading days (skip weekends/holidays)
                    if TradingCalendar::is_trading_day(current.date_naive()) {
                        times.push(current);
                    }
                    current = current + *interval;
                }
                times
            }
            HedgeStrategy::DeltaThreshold { .. } | HedgeStrategy::GammaDollar { .. } => {
                // For threshold-based strategies, check at regular intervals
                // but only actually hedge if threshold exceeded
                self.generate_check_times(entry_time, exit_time)
            }
        }
    }

    /// Generate times to check delta (for threshold strategies)
    ///
    /// Checks every hour during market hours (14:30 - 21:00 UTC = 9:30 - 16:00 ET)
    /// Includes entry_time as first check to ensure delta-neutral entry.
    fn generate_check_times(
        &self,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Vec<DateTime<Utc>> {
        let check_interval = Duration::hours(1);

        // Include entry_time as first check
        let mut times = vec![entry_time];
        let mut current = entry_time + check_interval;

        while current < exit_time {
            // Only include times during market hours (14:30 - 21:00 UTC = 9:30 - 16:00 ET)
            let hour = current.hour();
            if hour >= 14 && hour < 21 {
                times.push(current);
            }
            current = current + check_interval;
        }
        times
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_domain::value_objects::{EarningsTime, TimingConfig};
    use chrono::NaiveDate;

    #[test]
    fn test_earnings_timing_lookahead() {
        let config = TimingConfig {
            entry_hour: 9,
            entry_minute: 30,
            exit_hour: 15,
            exit_minute: 55,
        };
        let timing = TimingStrategy::Earnings(EarningsTradeTiming::new(config));
        assert_eq!(timing.lookahead_days(), 3);
    }

    #[test]
    fn test_straddle_timing_lookahead() {
        let config = TimingConfig {
            entry_hour: 15,
            entry_minute: 45,
            exit_hour: 15,
            exit_minute: 55,
        };
        let straddle_timing = StraddleTradeTiming::new(config)
            .with_entry_days(10)
            .with_exit_days(2);
        let timing = TimingStrategy::Straddle(straddle_timing);

        // 10 entry days * 1.5 + 7 = 22
        assert_eq!(timing.lookahead_days(), 22);
    }

    #[test]
    fn test_straddle_timing_entry_date() {
        let config = TimingConfig {
            entry_hour: 15,
            entry_minute: 45,
            exit_hour: 15,
            exit_minute: 55,
        };
        let straddle_timing = StraddleTradeTiming::new(config)
            .with_entry_days(10)
            .with_exit_days(2);
        let timing = TimingStrategy::Straddle(straddle_timing);

        let event = EarningsEvent::new(
            "PENG".to_string(),
            NaiveDate::from_ymd_opt(2025, 4, 2).unwrap(),
            EarningsTime::AfterMarketClose,
        );

        // Entry should be 10 trading days before Apr 2
        let entry_date = timing.entry_date(&event);

        // Apr 2 is Wednesday
        // 10 trading days back: Mar 19 (Wed)
        assert_eq!(entry_date, NaiveDate::from_ymd_opt(2025, 3, 19).unwrap());
    }
}
