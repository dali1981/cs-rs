# Refactoring Plan: Campaign + Session Architecture

**Date**: 2026-01-06
**Status**: Planning
**Author**: Architecture Review

---

## Executive Summary

Introduce a two-layer scheduling architecture that separates **intent** (what to trade) from **execution** (when to trade):

1. **TradingCampaign** - Stock-centric: defines trading intent for one symbol
2. **SessionSchedule** - Date-centric: groups sessions by execution date

This solves the fundamental tension between:
- Single-stock view: "For PENG, show me earnings → inter-period → earnings"
- Multi-stock view: "On 2025-04-01, which stocks have sessions to execute?"

---

## Motivating Use Cases

### Use Case 1: Calendar Spread Around Earnings

**Strategy**: Sell near-term ATM options, buy far-term ATM options to capture IV crush.

```
PENG Earnings: 2025-04-02 AMC

Timeline:
    Mar 31      Apr 1       Apr 2       Apr 3
      │           │           │           │
      │        ENTRY         │         EXIT
      │      (day before)    │      (day after)
      │           │           │           │
                         EARNINGS
                           (AMC)
```

**Configuration**:
```rust
TradingCampaign {
    symbol: "PENG",
    strategy: OptionStrategy::CalendarSpread,
    period_policy: PeriodPolicy::EarningsOnly {
        timing: TradingPeriodSpec::CrossEarnings {
            entry_days_before: 1,
            exit_days_after: 1,
            entry_time: time!(09:35),
            exit_time: time!(09:35),
        },
    },
    roll_policy: RollPolicy::None,  // No rolling within earnings trade
    expiration_policy: ExpirationPolicy::Calendar {
        short: Box::new(ExpirationPolicy::first_after(earnings_date)),
        long: Box::new(ExpirationPolicy::prefer_monthly(earnings_date, 1)),
    },
}
```

### Use Case 2: Pre-Earnings Straddle (2 Weeks Before)

**Strategy**: Buy ATM straddle to capture IV expansion leading into earnings.

```
PENG Earnings: 2025-04-02 AMC

Timeline:
    Mar 19      Mar 26      Apr 1       Apr 2
      │           │           │           │
    ENTRY         │         EXIT          │
  (14 days)       │      (1 day before)   │
      │           │           │           │
      └───────────┴───────────┘       EARNINGS
           IV EXPANSION PERIOD
```

**Configuration**:
```rust
TradingCampaign {
    symbol: "PENG",
    strategy: OptionStrategy::Straddle,
    period_policy: PeriodPolicy::EarningsOnly {
        timing: TradingPeriodSpec::PreEarnings {
            entry_days_before: 14,  // 2 weeks before
            exit_days_before: 1,    // Exit day before earnings
            entry_time: time!(09:35),
            exit_time: time!(15:55),
        },
    },
    roll_policy: RollPolicy::None,  // Hold entire period
    expiration_policy: ExpirationPolicy::prefer_monthly(
        earnings_date,
        0,  // First monthly >= earnings
    ),
}
```

### Use Case 3: Weekly Rolling Straddle Between Earnings

**Strategy**: Roll ATM straddle weekly between earnings periods.

```
PENG: Q1 earnings 2025-01-08 → Q2 earnings 2025-04-02

Timeline:
    Jan 10      Jan 17      Jan 24   ...   Mar 28      Apr 1
      │           │           │              │           │
    ENTRY       ROLL        ROLL           ROLL        EXIT
      │           │           │              │           │
      └─────┬─────┴─────┬─────┴──── ... ────┴─────┬─────┘
         Week 1      Week 2                    Week N
```

**Configuration**:
```rust
TradingCampaign {
    symbol: "PENG",
    strategy: OptionStrategy::Straddle,
    period_policy: PeriodPolicy::InterEarnings {
        entry_days_after_earnings: 2,   // Start 2 days after Q1 earnings
        exit_days_before_earnings: 3,   // Stop 3 days before Q2 earnings
        roll_policy: RollPolicy::Weekly { roll_day: Weekday::Fri },
    },
    roll_policy: RollPolicy::Weekly { roll_day: Weekday::Fri },
    expiration_policy: ExpirationPolicy::PreferMonthly {
        min_date: entry_date,
        months_out: 0,  // Current monthly
    },
}
```

---

## Domain Model Changes

### New File: `cs-domain/src/campaign/mod.rs`

```rust
//! Trading campaign and session scheduling
//!
//! A campaign defines the trading intent for one symbol.
//! Sessions are the atomic execution units generated from campaigns.

mod campaign;
mod session;
mod schedule;
mod period_policy;

pub use campaign::TradingCampaign;
pub use session::{TradingSession, SessionAction, SessionContext};
pub use schedule::SessionSchedule;
pub use period_policy::PeriodPolicy;
```

### New Type: `TradingSession`

```rust
// cs-domain/src/campaign/session.rs

use chrono::{DateTime, NaiveDate, Utc};
use crate::{EarningsEvent, OptionStrategy};

/// A session is the atomic unit of trading
///
/// One session = one entry-exit period for one symbol.
/// Generated from campaigns, consumed by executors.
#[derive(Debug, Clone)]
pub struct TradingSession {
    /// Symbol to trade
    pub symbol: String,

    /// Strategy type (determines which executor handles this)
    pub strategy: OptionStrategy,

    /// When to enter
    pub entry_datetime: DateTime<Utc>,

    /// When to exit
    pub exit_datetime: DateTime<Utc>,

    /// What action this session represents
    pub action: SessionAction,

    /// Context for understanding this session
    pub context: SessionContext,
}

/// What action this session represents in a campaign
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    /// First entry of a campaign or period
    OpenNew,

    /// Roll: close current position, open new one
    RollToNext,

    /// Final exit (end of campaign or period)
    CloseOnly,
}

/// Context that explains WHY this session exists
#[derive(Debug, Clone)]
pub enum SessionContext {
    /// Session anchored to an earnings event
    Earnings {
        event: EarningsEvent,
        timing_type: EarningsTimingType,
    },

    /// Session between two earnings dates
    InterEarnings {
        /// Which roll this is (1 = first, 2 = second, etc.)
        roll_number: u16,
        /// Previous earnings date
        earnings_before: NaiveDate,
        /// Next earnings date
        earnings_after: NaiveDate,
    },

    /// Standalone session (no earnings reference)
    Standalone {
        /// Optional description
        note: Option<String>,
    },
}

/// Type of earnings-relative timing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EarningsTimingType {
    /// Enter before earnings, exit before earnings
    PreEarnings,
    /// Enter before earnings, exit after earnings
    CrossEarnings,
    /// Enter after earnings, hold for period
    PostEarnings,
}

impl TradingSession {
    /// Entry date (convenience)
    pub fn entry_date(&self) -> NaiveDate {
        self.entry_datetime.date_naive()
    }

    /// Exit date (convenience)
    pub fn exit_date(&self) -> NaiveDate {
        self.exit_datetime.date_naive()
    }

    /// Duration in trading days (approximate)
    pub fn duration_days(&self) -> i64 {
        (self.exit_datetime - self.entry_datetime).num_days()
    }

    /// Is this an earnings-related session?
    pub fn is_earnings_session(&self) -> bool {
        matches!(self.context, SessionContext::Earnings { .. })
    }
}
```

### New Type: `TradingCampaign`

```rust
// cs-domain/src/campaign/campaign.rs

use chrono::NaiveDate;
use crate::{
    EarningsEvent, OptionStrategy, RollPolicy, ExpirationPolicy,
    TradingCalendar, TradingPeriodSpec,
};
use super::{TradingSession, SessionAction, SessionContext, EarningsTimingType, PeriodPolicy};

/// A campaign defines trading intent for one symbol over a date range
///
/// The campaign knows:
/// - WHAT to trade (symbol, strategy)
/// - WHEN to trade (period policy: around earnings, between earnings, fixed)
/// - HOW to roll (roll policy)
/// - WHICH expirations (expiration policy)
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
        let mut last_roll_date: Option<NaiveDate> = None;

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

            let entry_dt = TradingCalendar::to_datetime(current_date, time!(09:35));
            let exit_dt = TradingCalendar::to_datetime(next_roll, time!(15:55));

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
            last_roll_date = Some(next_roll);
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

            let entry_dt = TradingCalendar::to_datetime(current_date, time!(09:35));
            let exit_dt = TradingCalendar::to_datetime(next_roll, time!(15:55));

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
```

### New Type: `PeriodPolicy`

```rust
// cs-domain/src/campaign/period_policy.rs

use crate::{RollPolicy, TradingPeriodSpec};

/// Policy for when to trade within a campaign
#[derive(Debug, Clone)]
pub enum PeriodPolicy {
    /// Only trade around earnings announcements
    ///
    /// Use for: Calendar spreads, earnings straddles
    EarningsOnly {
        /// How to time entry/exit relative to earnings
        timing: TradingPeriodSpec,
    },

    /// Trade between earnings dates with rolling
    ///
    /// Use for: Theta harvesting between earnings, weekly premium selling
    InterEarnings {
        /// Days after earnings to start trading
        entry_days_after_earnings: u16,
        /// Days before next earnings to stop trading
        exit_days_before_earnings: u16,
        /// How often to roll positions
        roll_policy: RollPolicy,
    },

    /// Both earnings and inter-period trading
    ///
    /// Use for: Continuous premium collection with earnings plays
    Continuous {
        /// How to time earnings trades
        earnings_timing: TradingPeriodSpec,
        /// How to roll between earnings
        inter_period_roll: RollPolicy,
    },

    /// Fixed date range, ignore earnings calendar
    ///
    /// Use for: Backtests, specific date ranges
    FixedPeriod {
        /// How often to roll
        roll_policy: RollPolicy,
    },
}

impl PeriodPolicy {
    // =========================================================================
    // Convenience constructors
    // =========================================================================

    /// Trade only around earnings with cross-earnings timing (calendar spread default)
    pub fn cross_earnings() -> Self {
        Self::EarningsOnly {
            timing: TradingPeriodSpec::cross_earnings_default(),
        }
    }

    /// Trade only before earnings (straddle IV expansion)
    pub fn pre_earnings(days_before: u16) -> Self {
        Self::EarningsOnly {
            timing: TradingPeriodSpec::PreEarnings {
                entry_days_before: days_before,
                exit_days_before: 1,
                entry_time: chrono::NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
                exit_time: chrono::NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
            },
        }
    }

    /// Trade between earnings with weekly rolling
    pub fn weekly_between_earnings() -> Self {
        Self::InterEarnings {
            entry_days_after_earnings: 2,
            exit_days_before_earnings: 3,
            roll_policy: RollPolicy::Weekly { roll_day: chrono::Weekday::Fri },
        }
    }

    /// Trade between earnings with monthly rolling
    pub fn monthly_between_earnings() -> Self {
        Self::InterEarnings {
            entry_days_after_earnings: 2,
            exit_days_before_earnings: 5,
            roll_policy: RollPolicy::Monthly { roll_week: 0 },
        }
    }
}
```

### New Type: `SessionSchedule`

```rust
// cs-domain/src/campaign/schedule.rs

use std::collections::BTreeMap;
use chrono::NaiveDate;
use crate::EarningsEvent;
use super::{TradingCampaign, TradingSession};

/// A schedule of trading sessions organized by date
///
/// This is the "date-centric" view for multi-stock execution.
/// Use this when you want to know "what trades happen on each date?"
#[derive(Debug, Clone)]
pub struct SessionSchedule {
    /// Sessions grouped by entry date
    by_entry_date: BTreeMap<NaiveDate, Vec<TradingSession>>,

    /// Sessions grouped by exit date (for tracking closes)
    by_exit_date: BTreeMap<NaiveDate, Vec<TradingSession>>,
}

impl SessionSchedule {
    /// Create an empty schedule
    pub fn new() -> Self {
        Self {
            by_entry_date: BTreeMap::new(),
            by_exit_date: BTreeMap::new(),
        }
    }

    /// Build schedule from multiple campaigns
    ///
    /// This is the primary constructor for multi-stock backtests.
    pub fn from_campaigns(
        campaigns: &[TradingCampaign],
        earnings_calendar: &[EarningsEvent],
    ) -> Self {
        let mut schedule = Self::new();

        for campaign in campaigns {
            let sessions = campaign.generate_sessions(earnings_calendar);
            for session in sessions {
                schedule.add_session(session);
            }
        }

        schedule
    }

    /// Add a session to the schedule
    pub fn add_session(&mut self, session: TradingSession) {
        self.by_entry_date
            .entry(session.entry_date())
            .or_default()
            .push(session.clone());

        self.by_exit_date
            .entry(session.exit_date())
            .or_default()
            .push(session);
    }

    /// Get sessions that ENTER on a given date
    pub fn entries_on(&self, date: NaiveDate) -> &[TradingSession] {
        self.by_entry_date.get(&date).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Get sessions that EXIT on a given date
    pub fn exits_on(&self, date: NaiveDate) -> &[TradingSession] {
        self.by_exit_date.get(&date).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Iterate over all entry dates in order
    pub fn iter_entry_dates(&self) -> impl Iterator<Item = (NaiveDate, &[TradingSession])> {
        self.by_entry_date.iter().map(|(d, s)| (*d, s.as_slice()))
    }

    /// Get all unique symbols in the schedule
    pub fn symbols(&self) -> Vec<String> {
        let mut symbols: Vec<String> = self.by_entry_date
            .values()
            .flat_map(|sessions| sessions.iter().map(|s| s.symbol.clone()))
            .collect();
        symbols.sort();
        symbols.dedup();
        symbols
    }

    /// Total number of sessions
    pub fn session_count(&self) -> usize {
        self.by_entry_date.values().map(Vec::len).sum()
    }

    /// Date range of the schedule
    pub fn date_range(&self) -> Option<(NaiveDate, NaiveDate)> {
        let first = self.by_entry_date.keys().next()?;
        let last = self.by_exit_date.keys().last()?;
        Some((*first, *last))
    }

    /// Filter to single symbol
    pub fn filter_symbol(&self, symbol: &str) -> Self {
        let mut filtered = Self::new();

        for sessions in self.by_entry_date.values() {
            for session in sessions.iter().filter(|s| s.symbol == symbol) {
                filtered.add_session(session.clone());
            }
        }

        filtered
    }

    /// Print summary
    pub fn summary(&self) -> String {
        let symbols = self.symbols();
        let (start, end) = self.date_range().unwrap_or_default();

        format!(
            "SessionSchedule: {} sessions for {} symbols ({} to {})",
            self.session_count(),
            symbols.len(),
            start,
            end,
        )
    }
}

impl Default for SessionSchedule {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## RollPolicy Enhancement: Monthly Support

```rust
// Update cs-domain/src/roll/policy.rs

pub enum RollPolicy {
    None,

    Weekly { roll_day: Weekday },

    /// NEW: Roll on monthly expiration cycle
    ///
    /// Tracks the monthly expiration calendar (3rd Friday).
    Monthly {
        /// Week relative to 3rd Friday to roll:
        /// - 0: Roll ON 3rd Friday (expiration day)
        /// - 1: Roll 1 week after 3rd Friday
        /// - -1: Roll 1 week before 3rd Friday
        roll_week_offset: i8,
    },

    TradingDays { interval: u16 },

    OnExpiration { to_next: ExpirationPolicy },

    DteThreshold { min_dte: i32, to_policy: ExpirationPolicy },

    TimeInterval { interval_days: u16, to_policy: ExpirationPolicy },
}

impl RollPolicy {
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

    pub fn next_roll_date(&self, from: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::None => None,

            Self::Weekly { roll_day } => Some(next_weekday(from, *roll_day)),

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
                let calendar_days = (*interval as f64 * 1.4).round() as i64;
                Some(from + chrono::Duration::days(calendar_days))
            }

            Self::OnExpiration { .. } |
            Self::DteThreshold { .. } |
            Self::TimeInterval { .. } => None,  // Need expiration context
        }
    }
}
```

---

## CLI Usage Examples

### Example 1: Calendar Spread Around PENG Earnings

```bash
# Single stock with custom earnings calendar
cs backtest calendar-spread \
    --symbol PENG \
    --earnings-file custom_earnings/PENG_2025.parquet \
    --period-policy cross-earnings \
    --entry-days-before 1 \
    --exit-days-after 1 \
    --start-date 2025-01-01 \
    --end-date 2025-12-31

# Output:
# Session Schedule for PENG:
#   Q1: Entry 2025-01-07, Exit 2025-01-09 (around 2025-01-08 AMC)
#   Q2: Entry 2025-04-01, Exit 2025-04-03 (around 2025-04-02 AMC)
#   Q3: Entry 2025-07-07, Exit 2025-07-09 (around 2025-07-08 AMC)
#   Q4: Entry 2025-10-06, Exit 2025-10-08 (around 2025-10-07 AMC)
```

### Example 2: Pre-Earnings Straddle (14 Days Before)

```bash
# Straddle entering 2 weeks before earnings
cs backtest straddle \
    --symbol PENG \
    --earnings-file custom_earnings/PENG_2025.parquet \
    --period-policy pre-earnings \
    --entry-days-before 14 \
    --exit-days-before 1 \
    --start-date 2025-01-01 \
    --end-date 2025-12-31

# Output:
# Session Schedule for PENG:
#   Q1: Entry 2024-12-18, Exit 2025-01-07 (14d before 2025-01-08)
#   Q2: Entry 2025-03-13, Exit 2025-04-01 (14d before 2025-04-02)
#   Q3: Entry 2025-06-18, Exit 2025-07-07 (14d before 2025-07-08)
#   Q4: Entry 2025-09-17, Exit 2025-10-06 (14d before 2025-10-07)
```

### Example 3: Multi-Stock Backtest on Universe

```bash
# Run calendar spreads on entire universe for Q1 2025
cs backtest calendar-spread \
    --universe sp500 \
    --period-policy cross-earnings \
    --start-date 2025-01-01 \
    --end-date 2025-03-31 \
    --output results/q1_calendar_backtest.parquet

# This uses SessionSchedule internally to:
# 1. Generate campaigns for each symbol
# 2. Build date-centric schedule
# 3. Execute all sessions for each date in parallel
```

### Example 4: Weekly Rolling Straddle Between Earnings

```bash
# Weekly rolling straddle on PENG between earnings
cs backtest straddle \
    --symbol PENG \
    --earnings-file custom_earnings/PENG_2025.parquet \
    --period-policy inter-earnings \
    --roll-policy weekly \
    --roll-day friday \
    --start-date 2025-01-01 \
    --end-date 2025-12-31

# Output shows rolling sessions between each earnings pair:
# Q1-Q2 inter-period: 12 weekly rolls (Jan 10 - Mar 28)
# Q2-Q3 inter-period: 13 weekly rolls (Apr 4 - Jul 4)
# etc.
```

---

## Implementation Order

### Phase 1: Core Domain Types (cs-domain)

1. Add `cs-domain/src/campaign/mod.rs` with:
   - `TradingSession`
   - `SessionAction`
   - `SessionContext`
   - `EarningsTimingType`

2. Add `TradingCampaign` with session generation logic

3. Add `PeriodPolicy` enum

4. Add `SessionSchedule` with date-centric grouping

5. Extend `RollPolicy` with `Monthly { roll_week_offset }`

### Phase 2: Execution Layer (cs-backtest)

1. Add `SessionExecutor` that consumes `TradingSession`

2. Update `TradeExecutor<T>` to accept sessions

3. Add batch execution for multi-stock

### Phase 3: CLI Integration

1. Add `--period-policy` flag with variants
2. Add `--roll-policy monthly` support
3. Support `--earnings-file` for custom calendars
4. Add `--universe` for multi-stock runs

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_calendar_spread_session_generation() {
    let campaign = TradingCampaign {
        symbol: "PENG".to_string(),
        strategy: OptionStrategy::CalendarSpread,
        start_date: date!(2025-01-01),
        end_date: date!(2025-12-31),
        period_policy: PeriodPolicy::cross_earnings(),
        expiration_policy: ExpirationPolicy::Calendar { .. },
    };

    let earnings = vec![
        EarningsEvent::new("PENG", date!(2025-01-08), EarningsTime::AfterMarketClose),
        EarningsEvent::new("PENG", date!(2025-04-02), EarningsTime::AfterMarketClose),
    ];

    let sessions = campaign.generate_sessions(&earnings);

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].entry_date(), date!(2025-01-07));
    assert_eq!(sessions[0].exit_date(), date!(2025-01-09));
}

#[test]
fn test_pre_earnings_straddle_14_days() {
    let campaign = TradingCampaign {
        symbol: "PENG".to_string(),
        strategy: OptionStrategy::Straddle,
        period_policy: PeriodPolicy::pre_earnings(14),
        ..
    };

    let sessions = campaign.generate_sessions(&earnings);

    // 14 trading days before Jan 8 = Dec 18
    assert_eq!(sessions[0].entry_date(), date!(2024-12-18));
    assert_eq!(sessions[0].exit_date(), date!(2025-01-07));
}

#[test]
fn test_weekly_inter_earnings_sessions() {
    let campaign = TradingCampaign {
        symbol: "PENG".to_string(),
        strategy: OptionStrategy::Straddle,
        period_policy: PeriodPolicy::weekly_between_earnings(),
        ..
    };

    // Q1 earnings Jan 8, Q2 earnings Apr 2
    // Inter-period: Jan 10 - Mar 28 (approx 11 weeks)
    let sessions = campaign.generate_sessions(&earnings);

    let inter_sessions: Vec<_> = sessions.iter()
        .filter(|s| matches!(s.context, SessionContext::InterEarnings { .. }))
        .collect();

    assert!(inter_sessions.len() >= 10);  // ~11 weeks of rolling
}
```

### Integration Test with PENG

```rust
#[tokio::test]
async fn test_peng_full_year_calendar_spread() {
    let earnings = load_parquet("custom_earnings/PENG_2025.parquet");

    let campaign = TradingCampaign::builder()
        .symbol("PENG")
        .strategy(OptionStrategy::CalendarSpread)
        .period_policy(PeriodPolicy::cross_earnings())
        .date_range(date!(2025-01-01), date!(2025-12-31))
        .build();

    let sessions = campaign.generate_sessions(&earnings);

    // Should have 4 earnings sessions
    assert_eq!(sessions.len(), 4);

    // Execute and verify
    let executor = SessionExecutor::new(..);
    let results = executor.run_sessions(&sessions).await;

    assert!(results.iter().all(|r| r.is_ok()));
}
```

---

## Migration Path

1. **No breaking changes**: `TradeExecutor<T>` continues to work as-is
2. **New path**: `TradingCampaign` → `SessionSchedule` → `SessionExecutor`
3. **Gradual adoption**: CLI can use either path based on flags

---

## Summary

| Before | After |
|--------|-------|
| Loop over dates for one stock | Generate sessions from campaign |
| Hard to see multi-stock schedule | `SessionSchedule` groups by date |
| Weekly roll without weeklies = confusing | Explicit ATM reset vs actual roll |
| No monthly cycle tracking | `RollPolicy::Monthly` with offset |
| Timing coupled to strategy | `PeriodPolicy` decouples timing |

The key insight: **Campaigns express intent, sessions are the execution plan.**
