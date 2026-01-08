// cs-domain/src/campaign/campaign.rs

use chrono::{NaiveDate, NaiveTime};
use crate::{
    EarningsEvent, OptionStrategy, ExpirationPolicy,
    TradingCalendar, TradingPeriodSpec, RollPolicy,
    value_objects::{IronButterflyConfig, TradeDirection},
};
use crate::datetime::eastern_to_utc;
use super::{TradingSession, SessionAction, SessionContext, EarningsTimingType, PeriodPolicy};

/// A campaign defines trading intent for one symbol over a date range
///
/// The campaign knows:
/// - WHAT to trade (symbol, strategy)
/// - WHEN to trade (period policy: around earnings, between earnings, fixed)
/// - HOW to roll (roll policy)
/// - WHICH expirations (expiration policy)
/// - STRATEGY-SPECIFIC CONFIG (wing mode, direction, etc.)
///
/// It generates `TradingSession`s that can be executed.
#[derive(Debug, Clone)]
pub struct TradingCampaign {
    /// Symbol to trade
    pub symbol: String,

    /// Strategy type
    pub strategy: OptionStrategy,

    /// Campaign date range
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,

    /// When to trade relative to earnings
    pub period_policy: PeriodPolicy,

    /// How to select expirations
    pub expiration_policy: ExpirationPolicy,

    /// Iron butterfly wing configuration (optional, for IronButterfly strategy)
    pub iron_butterfly_config: Option<IronButterflyConfig>,

    /// Trade direction (Short by default, Long if inverted)
    pub trade_direction: TradeDirection,
}

impl TradingCampaign {
    /// Generate all trading sessions for this campaign
    ///
    /// Requires earnings calendar for earnings-relative policies.
    pub fn generate_sessions(&self, earnings_calendar: &[EarningsEvent]) -> Vec<TradingSession> {
        // Filter to this symbol's earnings in range
        let symbol_earnings: Vec<&EarningsEvent> = earnings_calendar
            .iter()
            .filter(|e| e.symbol == self.symbol)
            .filter(|e| e.earnings_date >= self.start_date && e.earnings_date <= self.end_date)
            .collect();

        match &self.period_policy {
            PeriodPolicy::EarningsOnly { timing } => {
                self.generate_earnings_sessions(&symbol_earnings, timing)
            }

            PeriodPolicy::InterEarnings {
                entry_days_after_earnings,
                exit_days_before_earnings,
                roll_policy,
            } => {
                self.generate_inter_earnings_sessions(
                    &symbol_earnings,
                    *entry_days_after_earnings,
                    *exit_days_before_earnings,
                    roll_policy,
                )
            }

            PeriodPolicy::Continuous {
                earnings_timing,
                inter_period_roll,
            } => {
                let mut sessions = Vec::new();

                // Earnings sessions
                sessions.extend(self.generate_earnings_sessions(&symbol_earnings, earnings_timing));

                // Inter-earnings sessions
                sessions.extend(self.generate_inter_earnings_sessions(
                    &symbol_earnings,
                    2,  // Start 2 days after earnings by default
                    3,  // End 3 days before next earnings
                    inter_period_roll,
                ));

                // Sort by entry time
                sessions.sort_by_key(|s| s.entry_datetime);
                sessions
            }

            PeriodPolicy::FixedPeriod { roll_policy } => {
                self.generate_fixed_period_sessions(roll_policy)
            }
        }
    }

    /// Generate sessions for earnings-only policy
    fn generate_earnings_sessions(
        &self,
        earnings: &[&EarningsEvent],
        timing: &TradingPeriodSpec,
    ) -> Vec<TradingSession> {
        let mut sessions = Vec::new();

        for event in earnings {
            if let Ok(period) = timing.build(Some(event)) {
                let timing_type = match timing {
                    TradingPeriodSpec::PreEarnings { .. } => EarningsTimingType::PreEarnings,
                    TradingPeriodSpec::PostEarnings { .. } => EarningsTimingType::PostEarnings,
                    TradingPeriodSpec::CrossEarnings { .. } => EarningsTimingType::CrossEarnings,
                    _ => EarningsTimingType::CrossEarnings,  // Default
                };

                sessions.push(TradingSession {
                    symbol: self.symbol.clone(),
                    strategy: self.strategy,
                    entry_datetime: period.entry_datetime(),
                    exit_datetime: period.exit_datetime(),
                    action: SessionAction::OpenNew,
                    context: SessionContext::Earnings {
                        event: (*event).clone(),
                        timing_type,
                    },
                });
            }
        }

        sessions
    }

    /// Generate sessions between earnings dates
    fn generate_inter_earnings_sessions(
        &self,
        earnings: &[&EarningsEvent],
        entry_days_after: u16,
        exit_days_before: u16,
        roll_policy: &RollPolicy,
    ) -> Vec<TradingSession> {
        let mut sessions = Vec::new();

        // Process each earnings-to-earnings window
        for window in earnings.windows(2) {
            let prev_earnings = window[0].earnings_date;
            let next_earnings = window[1].earnings_date;

            // Calculate inter-period boundaries
            let period_start = TradingCalendar::n_trading_days_after(
                prev_earnings,
                entry_days_after as usize,
            );
            let period_end = TradingCalendar::n_trading_days_before(
                next_earnings,
                exit_days_before as usize,
            );

            if period_end <= period_start {
                continue;  // Period too short
            }

            // Generate rolling sessions within this inter-period
            let inter_sessions = self.generate_rolling_sessions_in_range(
                period_start,
                period_end,
                roll_policy,
                prev_earnings,
                next_earnings,
            );

            sessions.extend(inter_sessions);
        }

        sessions
    }

    /// Generate rolling sessions within a date range
    fn generate_rolling_sessions_in_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        roll_policy: &RollPolicy,
        earnings_before: NaiveDate,
        earnings_after: NaiveDate,
    ) -> Vec<TradingSession> {
        let mut sessions = Vec::new();
        let mut current_date = start;
        let mut roll_number = 1u16;

        // Default times
        let entry_time = NaiveTime::from_hms_opt(9, 35, 0).unwrap();
        let exit_time = NaiveTime::from_hms_opt(15, 55, 0).unwrap();

        while current_date < end {
            // Determine exit date based on roll policy
            let next_roll = roll_policy.next_roll_date(current_date)
                .unwrap_or(end)
                .min(end);

            // Determine action
            let action = if roll_number == 1 {
                SessionAction::OpenNew
            } else if next_roll >= end {
                SessionAction::CloseOnly
            } else {
                SessionAction::RollToNext
            };

            let entry_dt = eastern_to_utc(current_date, entry_time);
            let exit_dt = eastern_to_utc(next_roll, exit_time);

            sessions.push(TradingSession {
                symbol: self.symbol.clone(),
                strategy: self.strategy,
                entry_datetime: entry_dt,
                exit_datetime: exit_dt,
                action,
                context: SessionContext::InterEarnings {
                    roll_number,
                    earnings_before,
                    earnings_after,
                },
            });

            // Move to next period
            current_date = TradingCalendar::next_trading_day(next_roll);
            roll_number += 1;
        }

        sessions
    }

    /// Generate sessions for fixed period (no earnings reference)
    fn generate_fixed_period_sessions(&self, roll_policy: &RollPolicy) -> Vec<TradingSession> {
        let mut sessions = Vec::new();
        let mut current_date = self.start_date;
        let mut roll_number = 1u16;

        // Default times
        let entry_time = NaiveTime::from_hms_opt(9, 35, 0).unwrap();
        let exit_time = NaiveTime::from_hms_opt(15, 55, 0).unwrap();

        while current_date < self.end_date {
            let next_roll = roll_policy.next_roll_date(current_date)
                .unwrap_or(self.end_date)
                .min(self.end_date);

            let action = if roll_number == 1 {
                SessionAction::OpenNew
            } else if next_roll >= self.end_date {
                SessionAction::CloseOnly
            } else {
                SessionAction::RollToNext
            };

            let entry_dt = eastern_to_utc(current_date, entry_time);
            let exit_dt = eastern_to_utc(next_roll, exit_time);

            sessions.push(TradingSession {
                symbol: self.symbol.clone(),
                strategy: self.strategy,
                entry_datetime: entry_dt,
                exit_datetime: exit_dt,
                action,
                context: SessionContext::Standalone { note: None },
            });

            current_date = TradingCalendar::next_trading_day(next_roll);
            roll_number += 1;
        }

        sessions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EarningsEvent, EarningsTime, SessionSchedule};
    use chrono::{Weekday, Datelike};

    fn sample_earnings() -> Vec<EarningsEvent> {
        vec![
            EarningsEvent::new(
                "PENG".to_string(),
                NaiveDate::from_ymd_opt(2025, 1, 8).unwrap(),
                EarningsTime::AfterMarketClose,
            ),
            EarningsEvent::new(
                "PENG".to_string(),
                NaiveDate::from_ymd_opt(2025, 4, 2).unwrap(),
                EarningsTime::AfterMarketClose,
            ),
            EarningsEvent::new(
                "PENG".to_string(),
                NaiveDate::from_ymd_opt(2025, 7, 8).unwrap(),
                EarningsTime::AfterMarketClose,
            ),
            EarningsEvent::new(
                "PENG".to_string(),
                NaiveDate::from_ymd_opt(2025, 10, 7).unwrap(),
                EarningsTime::AfterMarketClose,
            ),
        ]
    }

    #[test]
    fn test_calendar_spread_session_generation() {
        let campaign = TradingCampaign {
            symbol: "PENG".to_string(),
            strategy: OptionStrategy::CalendarSpread,
            start_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            period_policy: PeriodPolicy::cross_earnings(),
            expiration_policy: ExpirationPolicy::FirstAfter {
                min_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            },
            iron_butterfly_config: None,
            trade_direction: TradeDirection::Short,
        };

        let earnings = sample_earnings();
        let sessions = campaign.generate_sessions(&earnings);

        // Should have 4 earnings sessions (one per quarter)
        assert_eq!(sessions.len(), 4);

        // Verify first session is around Q1 earnings
        assert_eq!(sessions[0].entry_date(), NaiveDate::from_ymd_opt(2025, 1, 7).unwrap());
        assert_eq!(sessions[0].exit_date(), NaiveDate::from_ymd_opt(2025, 1, 9).unwrap());
        assert_eq!(sessions[0].action, SessionAction::OpenNew);
        assert!(sessions[0].is_earnings_session());
    }

    #[test]
    fn test_pre_earnings_straddle_14_days() {
        let campaign = TradingCampaign {
            symbol: "PENG".to_string(),
            strategy: OptionStrategy::Straddle,
            start_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            period_policy: PeriodPolicy::pre_earnings(14),
            expiration_policy: ExpirationPolicy::FirstAfter {
                min_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            },
        };

        let earnings = sample_earnings();
        let sessions = campaign.generate_sessions(&earnings);

        // Should have 4 sessions
        assert_eq!(sessions.len(), 4);

        // First session should enter 14 trading days before Jan 8
        // and exit 1 day before Jan 8
        let first_entry = sessions[0].entry_date();
        let first_exit = sessions[0].exit_date();

        // Exit should be 1 day before earnings
        assert_eq!(first_exit, NaiveDate::from_ymd_opt(2025, 1, 7).unwrap());

        // Entry should be 14 trading days before earnings
        // This is approximately Dec 18, 2024 (need to account for weekends)
        assert!(first_entry < first_exit);
        assert!(first_entry.year() == 2024 || first_entry.month() == 12);
    }

    #[test]
    fn test_weekly_inter_earnings_sessions() {
        let campaign = TradingCampaign {
            symbol: "PENG".to_string(),
            strategy: OptionStrategy::Straddle,
            start_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            period_policy: PeriodPolicy::weekly_between_earnings(),
            expiration_policy: ExpirationPolicy::FirstAfter {
                min_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            },
        };

        let earnings = sample_earnings();
        let sessions = campaign.generate_sessions(&earnings);

        // Should have multiple inter-earnings sessions
        // Between Q1 (Jan 8) and Q2 (Apr 2) is ~84 days, ~60 trading days
        // With weekly rolls, that's ~8-9 sessions per inter-period
        assert!(sessions.len() >= 24); // At least ~8 per quarter x 3 quarters

        // First session should be inter-earnings
        let first = &sessions[0];
        match &first.context {
            SessionContext::InterEarnings { roll_number, .. } => {
                assert_eq!(*roll_number, 1);
            }
            _ => panic!("Expected InterEarnings context"),
        }

        // First session should be OpenNew
        assert_eq!(first.action, SessionAction::OpenNew);

        // Second session should be a roll
        if sessions.len() > 1 {
            assert_eq!(sessions[1].action, SessionAction::RollToNext);
        }
    }

    #[test]
    fn test_session_schedule_from_campaigns() {
        let campaign1 = TradingCampaign {
            symbol: "PENG".to_string(),
            strategy: OptionStrategy::CalendarSpread,
            start_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            period_policy: PeriodPolicy::cross_earnings(),
            expiration_policy: ExpirationPolicy::FirstAfter {
                min_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            },
        };

        let campaign2 = TradingCampaign {
            symbol: "AAPL".to_string(),
            strategy: OptionStrategy::CalendarSpread,
            start_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            period_policy: PeriodPolicy::cross_earnings(),
            expiration_policy: ExpirationPolicy::FirstAfter {
                min_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            },
        };

        // Create AAPL earnings (different dates from PENG)
        let mut earnings = sample_earnings();
        earnings.extend(vec![
            EarningsEvent::new(
                "AAPL".to_string(),
                NaiveDate::from_ymd_opt(2025, 1, 30).unwrap(),
                EarningsTime::AfterMarketClose,
            ),
            EarningsEvent::new(
                "AAPL".to_string(),
                NaiveDate::from_ymd_opt(2025, 5, 1).unwrap(),
                EarningsTime::AfterMarketClose,
            ),
        ]);

        let schedule = SessionSchedule::from_campaigns(&[campaign1, campaign2], &earnings);

        // Should have sessions for both symbols
        let symbols = schedule.symbols();
        assert!(symbols.contains(&"PENG".to_string()));
        assert!(symbols.contains(&"AAPL".to_string()));

        // Total sessions should be from both campaigns
        assert!(schedule.session_count() >= 6); // At least 4 PENG + 2 AAPL
    }

    #[test]
    fn test_monthly_roll_policy() {
        let policy = RollPolicy::Monthly { roll_week_offset: 0 };

        // Test next_roll_date for a date in January
        let start = NaiveDate::from_ymd_opt(2025, 1, 10).unwrap();
        let next = policy.next_roll_date(start);

        assert!(next.is_some());
        let next_date = next.unwrap();

        // Should be on a Friday (3rd Friday of month)
        assert_eq!(next_date.weekday(), Weekday::Fri);

        // Should be in January or later
        assert!(next_date >= start);
    }
}
