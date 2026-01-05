# Implementation Plan: Flexible Trading Period System (Option C)

**Date**: 2026-01-05
**Status**: READY FOR IMPLEMENTATION
**Approach**: Full Flexible System - Phased Implementation

---

## Overview

This plan implements a comprehensive flexible trading period system with:
- **ExpirationPolicy**: Controls how expirations are selected
- **TradingPeriod**: Decouples timing from earnings-centric model
- **RollPolicy**: Enables multi-period trades with position renewal
- **TradeStrategy**: Unified configuration combining all dimensions

---

## Design Decisions (Require User Input)

### Decision 1: Weekly Detection
**Question**: Use calendar logic or chain-based detection?

| Option | Description | Recommendation |
|--------|-------------|----------------|
| **A: Calendar Logic** | Any Friday not 3rd Friday = weekly | Simple, predictable |
| **B: Chain-Based** | Detect from available expirations | More accurate, handles irregular chains |

**Recommendation**: Start with **Option A** (calendar logic), add chain-based refinement later if needed.

### Decision 2: Roll Execution Model
**Question**: How to model position rolls?

| Option | Description | Trade-offs |
|--------|-------------|------------|
| **A: Synthetic Roll** | Close + reopen in same minute | Simpler P&L, one "trade" |
| **B: Separate Trades** | Each leg is independent | Easier accounting, cleaner results |
| **C: Single with Events** | One trade with roll events | Best for tracking, more complex |

**Recommendation**: **Option C** - Single trade with roll events. Best represents the actual strategy.

### Decision 3: Hedging During Rolls
**Question**: Close hedges on roll or carry through?

| Option | Description | Trade-offs |
|--------|-------------|------------|
| **A: Close on Roll** | Flatten hedge, restart after roll | Clean accounting, may miss moves |
| **B: Carry Through** | Keep hedge position during roll | Continuous hedge, complex tracking |

**Recommendation**: **Option A** - Close on roll. Simpler to implement and reason about.

---

## Phase 0: Immediate Bug Fix (2-4 hours)

**Goal**: Fix the straddle expiration selection bug now, enabling continued testing.

### Changes

#### File: `cs-domain/src/strike_selection/mod.rs`

```rust
// CHANGE: Update StrikeSelector trait signature (lines 175-184)

/// Select a straddle (always ATM)
///
/// # Arguments
/// * `spot` - Current spot price
/// * `surface` - IV surface with available expirations
/// * `min_expiration` - Minimum required expiration date (must be AFTER this date)
fn select_straddle(
    &self,
    _spot: &SpotPrice,
    _surface: &IVSurface,
    _min_expiration: NaiveDate,  // CHANGED from min_dte: i32
) -> Result<Straddle, SelectionError> {
    Err(SelectionError::UnsupportedStrategy(
        "Straddle not supported by this selector".to_string()
    ))
}
```

#### File: `cs-domain/src/strike_selection/atm.rs`

```rust
// CHANGE: Update select_straddle implementation (lines 345-387)

fn select_straddle(
    &self,
    spot: &SpotPrice,
    surface: &IVSurface,
    min_expiration: NaiveDate,  // CHANGED from min_dte: i32
) -> Result<Straddle, SelectionError> {
    // Get strikes from IV surface
    let strikes: Vec<Strike> = surface.strikes()
        .iter()
        .filter_map(|&s| Strike::new(s).ok())
        .collect();

    if strikes.is_empty() {
        return Err(SelectionError::NoStrikes);
    }

    // Filter expirations to those AFTER min_expiration
    let expirations: Vec<NaiveDate> = surface.expirations()
        .into_iter()
        .filter(|&exp| exp > min_expiration)  // KEY FIX
        .collect();

    if expirations.is_empty() {
        return Err(SelectionError::NoExpirations);
    }

    // Select first valid expiration (soonest after min_expiration)
    let expiration = *expirations.iter().min().unwrap();

    // Select ATM strike (closest to spot)
    let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
    let atm_strike = super::find_closest_strike(&strikes, spot_f64)?;

    // Create legs
    let symbol = surface.underlying().to_string();
    let call_leg = OptionLeg::new(symbol.clone(), atm_strike, expiration, OptionType::Call);
    let put_leg = OptionLeg::new(symbol, atm_strike, expiration, OptionType::Put);

    Straddle::new(call_leg, put_leg).map_err(Into::into)
}
```

#### File: `cs-backtest/src/unified_executor.rs`

```rust
// CHANGE: Update straddle selection call (line 348)

TradeStructure::Straddle => {
    // Use exit date as minimum expiration - options must expire AFTER we exit
    let min_expiration = exit_time.date_naive();

    match selector.select_straddle(&spot, entry_surface, min_expiration) {
        // ... rest unchanged
    }
}
```

### Testing Phase 0

```bash
# Run PENG backtest - should now select October 17 expiration, not Sept 19
cargo test --package cs-backtest -- straddle

# Verify no regressions
cargo test --workspace
```

---

## Phase 1: ExpirationPolicy (1 day)

**Goal**: Introduce `ExpirationPolicy` abstraction for flexible expiration selection.

### New File: `cs-domain/src/expiration/mod.rs`

```rust
//! Expiration policy and cycle detection
//!
//! This module provides abstractions for selecting option expirations
//! based on various criteria: date constraints, cycle preferences (weekly/monthly),
//! or target DTE.

mod cycle;
mod policy;

pub use cycle::ExpirationCycle;
pub use policy::ExpirationPolicy;
```

### New File: `cs-domain/src/expiration/cycle.rs`

```rust
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
        // Sept 19, 2025 is a Friday but NOT 3rd Friday (3rd Friday is Sept 19...
        // wait, let me check: Sept 2025 has 1=Mon, so Fridays are 5, 12, 19, 26
        // 3rd Friday is Sept 19. Let's use Sept 12 instead.
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
```

### New File: `cs-domain/src/expiration/policy.rs`

```rust
use chrono::NaiveDate;
use super::ExpirationCycle;
use crate::strike_selection::SelectionError;

/// Policy for selecting option expirations
///
/// Expirations can be selected based on various criteria:
/// - Minimum date constraint (must expire after a certain date)
/// - Cycle preference (prefer weeklies or monthlies)
/// - Target DTE (days to expiration from entry)
#[derive(Debug, Clone)]
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

            Self::Calendar { short, long } => {
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
```

### Update: `cs-domain/src/lib.rs`

```rust
// Add new module
pub mod expiration;

// Add to re-exports
pub use expiration::{ExpirationCycle, ExpirationPolicy};
```

### Integration with StrikeSelector

Update `select_straddle` to accept `ExpirationPolicy`:

```rust
// In cs-domain/src/strike_selection/mod.rs

/// Select a straddle (always ATM)
fn select_straddle(
    &self,
    _spot: &SpotPrice,
    _surface: &IVSurface,
    _expiration_policy: &ExpirationPolicy,  // NEW
) -> Result<Straddle, SelectionError> {
    Err(SelectionError::UnsupportedStrategy(
        "Straddle not supported by this selector".to_string()
    ))
}
```

---

## Phase 2: TradingPeriod (1 day)

**Goal**: Decouple timing from earnings-centric model.

### New File: `cs-domain/src/trading_period/mod.rs`

```rust
//! Trading period abstractions
//!
//! This module provides flexible timing specifications that can be
//! earnings-relative, fixed-date, or holding-period based.

mod period;
mod spec;

pub use period::TradingPeriod;
pub use spec::{TradingPeriodSpec, TimingError};
```

### New File: `cs-domain/src/trading_period/period.rs`

```rust
use chrono::{NaiveDate, NaiveTime, DateTime, Utc};
use crate::datetime::eastern_to_utc;
use crate::timing::TradingCalendar;

/// A concrete trading period with resolved dates and times
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradingPeriod {
    pub entry_date: NaiveDate,
    pub exit_date: NaiveDate,
    pub entry_time: NaiveTime,
    pub exit_time: NaiveTime,
}

impl TradingPeriod {
    /// Create a new trading period
    pub fn new(
        entry_date: NaiveDate,
        exit_date: NaiveDate,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    ) -> Self {
        Self {
            entry_date,
            exit_date,
            entry_time,
            exit_time,
        }
    }

    /// Get entry datetime in UTC
    pub fn entry_datetime(&self) -> DateTime<Utc> {
        eastern_to_utc(self.entry_date, self.entry_time)
    }

    /// Get exit datetime in UTC
    pub fn exit_datetime(&self) -> DateTime<Utc> {
        eastern_to_utc(self.exit_date, self.exit_time)
    }

    /// Calculate holding period in trading days
    pub fn holding_days(&self) -> i64 {
        TradingCalendar::trading_days_between(self.entry_date, self.exit_date)
    }

    /// Minimum expiration date (options must expire AFTER this)
    pub fn min_expiration(&self) -> NaiveDate {
        self.exit_date
    }

    /// Check if a date is within the trading period
    pub fn contains_date(&self, date: NaiveDate) -> bool {
        date >= self.entry_date && date <= self.exit_date
    }
}
```

### New File: `cs-domain/src/trading_period/spec.rs`

```rust
use chrono::{NaiveDate, NaiveTime};
use thiserror::Error;

use crate::entities::{EarningsEvent, EarningsTime};
use crate::timing::TradingCalendar;
use super::TradingPeriod;

/// Errors during trading period construction
#[derive(Error, Debug)]
pub enum TimingError {
    #[error("This timing specification requires an earnings event")]
    RequiresEarningsEvent,

    #[error("Invalid period: exit before entry")]
    ExitBeforeEntry,

    #[error("Entry date is not a trading day")]
    NonTradingDay,
}

/// Specification for a trading period
///
/// This is the "template" that gets resolved into a concrete `TradingPeriod`
/// when provided with the necessary context (e.g., earnings event).
#[derive(Debug, Clone)]
pub enum TradingPeriodSpec {
    /// Enter N days before earnings, exit M days before
    ///
    /// Used for IV expansion trades (straddles before earnings).
    PreEarnings {
        entry_days_before: u16,
        exit_days_before: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Enter after earnings, hold for N days
    ///
    /// Used for post-earnings momentum trades.
    PostEarnings {
        /// Days after earnings to enter (0 = earnings day if BMO, 1 = day after if AMC)
        entry_offset: i16,
        /// Trading days to hold
        holding_days: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Cross earnings: enter before, exit after
    ///
    /// Used for calendar spreads capturing IV crush.
    CrossEarnings {
        /// Days before earnings to enter (negative number, e.g., -1)
        entry_days_before: u16,
        /// Days after earnings to exit (positive number, e.g., 1)
        exit_days_after: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Fixed date range (not earnings-relative)
    ///
    /// Used for non-earnings trades or custom backtests.
    FixedDates {
        entry_date: NaiveDate,
        exit_date: NaiveDate,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Holding period from a start date
    ///
    /// Used when you know the entry but want to specify holding duration.
    HoldingPeriod {
        entry_date: NaiveDate,
        holding_days: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },
}

impl TradingPeriodSpec {
    // =========================================================================
    // Convenience constructors
    // =========================================================================

    /// Pre-earnings straddle timing (default: enter 20 days before, exit 1 day before)
    pub fn pre_earnings_default() -> Self {
        Self::PreEarnings {
            entry_days_before: 20,
            exit_days_before: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        }
    }

    /// Post-earnings timing (default: hold for 5 days)
    pub fn post_earnings_default() -> Self {
        Self::PostEarnings {
            entry_offset: 0,
            holding_days: 5,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        }
    }

    /// Cross-earnings timing (default: enter day before, exit day after)
    pub fn cross_earnings_default() -> Self {
        Self::CrossEarnings {
            entry_days_before: 1,
            exit_days_after: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
        }
    }

    // =========================================================================
    // Builder methods
    // =========================================================================

    /// Set entry/exit times
    pub fn with_times(self, entry_time: NaiveTime, exit_time: NaiveTime) -> Self {
        match self {
            Self::PreEarnings { entry_days_before, exit_days_before, .. } =>
                Self::PreEarnings { entry_days_before, exit_days_before, entry_time, exit_time },
            Self::PostEarnings { entry_offset, holding_days, .. } =>
                Self::PostEarnings { entry_offset, holding_days, entry_time, exit_time },
            Self::CrossEarnings { entry_days_before, exit_days_after, .. } =>
                Self::CrossEarnings { entry_days_before, exit_days_after, entry_time, exit_time },
            Self::FixedDates { entry_date, exit_date, .. } =>
                Self::FixedDates { entry_date, exit_date, entry_time, exit_time },
            Self::HoldingPeriod { entry_date, holding_days, .. } =>
                Self::HoldingPeriod { entry_date, holding_days, entry_time, exit_time },
        }
    }

    // =========================================================================
    // Resolution
    // =========================================================================

    /// Build a concrete TradingPeriod from this specification
    pub fn build(&self, event: Option<&EarningsEvent>) -> Result<TradingPeriod, TimingError> {
        match self {
            Self::PreEarnings { entry_days_before, exit_days_before, entry_time, exit_time } => {
                let event = event.ok_or(TimingError::RequiresEarningsEvent)?;

                let entry_date = TradingCalendar::n_trading_days_before(
                    event.earnings_date,
                    *entry_days_before as usize
                );
                let exit_date = TradingCalendar::n_trading_days_before(
                    event.earnings_date,
                    *exit_days_before as usize
                );

                if exit_date < entry_date {
                    return Err(TimingError::ExitBeforeEntry);
                }

                Ok(TradingPeriod::new(entry_date, exit_date, *entry_time, *exit_time))
            }

            Self::PostEarnings { entry_offset, holding_days, entry_time, exit_time } => {
                let event = event.ok_or(TimingError::RequiresEarningsEvent)?;

                // Entry depends on earnings time
                let entry_date = match event.earnings_time {
                    EarningsTime::BeforeMarketOpen => {
                        // BMO: can enter same day (offset 0) or later
                        TradingCalendar::n_trading_days_after(
                            event.earnings_date,
                            *entry_offset as usize
                        )
                    }
                    EarningsTime::AfterMarketClose | EarningsTime::Unknown => {
                        // AMC: enter next day (offset 0 = next day)
                        TradingCalendar::n_trading_days_after(
                            event.earnings_date,
                            (*entry_offset + 1) as usize
                        )
                    }
                };

                let exit_date = TradingCalendar::n_trading_days_after(
                    entry_date,
                    *holding_days as usize
                );

                Ok(TradingPeriod::new(entry_date, exit_date, *entry_time, *exit_time))
            }

            Self::CrossEarnings { entry_days_before, exit_days_after, entry_time, exit_time } => {
                let event = event.ok_or(TimingError::RequiresEarningsEvent)?;

                let entry_date = TradingCalendar::n_trading_days_before(
                    event.earnings_date,
                    *entry_days_before as usize
                );

                // Exit depends on earnings time
                let exit_date = match event.earnings_time {
                    EarningsTime::AfterMarketClose => {
                        // AMC: exit N days after earnings
                        TradingCalendar::n_trading_days_after(
                            event.earnings_date,
                            *exit_days_after as usize
                        )
                    }
                    EarningsTime::BeforeMarketOpen => {
                        // BMO: exit on earnings day (after announcement) or later
                        TradingCalendar::n_trading_days_after(
                            event.earnings_date,
                            (*exit_days_after).saturating_sub(1) as usize
                        )
                    }
                    EarningsTime::Unknown => {
                        TradingCalendar::n_trading_days_after(
                            event.earnings_date,
                            *exit_days_after as usize
                        )
                    }
                };

                Ok(TradingPeriod::new(entry_date, exit_date, *entry_time, *exit_time))
            }

            Self::FixedDates { entry_date, exit_date, entry_time, exit_time } => {
                if exit_date < entry_date {
                    return Err(TimingError::ExitBeforeEntry);
                }
                Ok(TradingPeriod::new(*entry_date, *exit_date, *entry_time, *exit_time))
            }

            Self::HoldingPeriod { entry_date, holding_days, entry_time, exit_time } => {
                let exit_date = TradingCalendar::n_trading_days_after(
                    *entry_date,
                    *holding_days as usize
                );
                Ok(TradingPeriod::new(*entry_date, exit_date, *entry_time, *exit_time))
            }
        }
    }

    /// Calculate lookahead days needed for event loading
    pub fn lookahead_days(&self) -> i64 {
        match self {
            Self::PreEarnings { entry_days_before, .. } => {
                // Add buffer for weekends: multiply by 1.5 and add 7
                ((*entry_days_before as f64 * 1.5) as i64) + 7
            }
            Self::PostEarnings { .. } => {
                // Post-earnings needs lookback, not lookahead
                -3
            }
            Self::CrossEarnings { entry_days_before, .. } => {
                (*entry_days_before as i64) + 3
            }
            Self::FixedDates { .. } | Self::HoldingPeriod { .. } => {
                0 // No earnings-based lookahead needed
            }
        }
    }

    /// Check if this spec requires an earnings event
    pub fn requires_earnings(&self) -> bool {
        !matches!(self, Self::FixedDates { .. } | Self::HoldingPeriod { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> EarningsEvent {
        EarningsEvent::new(
            "AAPL".to_string(),
            NaiveDate::from_ymd_opt(2025, 10, 30).unwrap(), // Thursday
            EarningsTime::AfterMarketClose,
        )
    }

    #[test]
    fn test_pre_earnings_period() {
        let spec = TradingPeriodSpec::PreEarnings {
            entry_days_before: 10,
            exit_days_before: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        };

        let event = sample_event();
        let period = spec.build(Some(&event)).unwrap();

        // 10 trading days before Oct 30 = Oct 16 (Wed)
        // 1 trading day before Oct 30 = Oct 29 (Wed)
        assert_eq!(period.entry_date, NaiveDate::from_ymd_opt(2025, 10, 16).unwrap());
        assert_eq!(period.exit_date, NaiveDate::from_ymd_opt(2025, 10, 29).unwrap());
    }

    #[test]
    fn test_post_earnings_period() {
        let spec = TradingPeriodSpec::PostEarnings {
            entry_offset: 0,
            holding_days: 5,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        };

        let event = sample_event(); // AMC on Oct 30
        let period = spec.build(Some(&event)).unwrap();

        // AMC: entry is next day = Oct 31
        // Exit is 5 trading days after = Nov 7
        assert_eq!(period.entry_date, NaiveDate::from_ymd_opt(2025, 10, 31).unwrap());
    }

    #[test]
    fn test_fixed_dates_no_event() {
        let spec = TradingPeriodSpec::FixedDates {
            entry_date: NaiveDate::from_ymd_opt(2025, 10, 1).unwrap(),
            exit_date: NaiveDate::from_ymd_opt(2025, 10, 15).unwrap(),
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        };

        // Should work without an earnings event
        let period = spec.build(None).unwrap();
        assert_eq!(period.entry_date, NaiveDate::from_ymd_opt(2025, 10, 1).unwrap());
    }
}
```

### Update: `cs-domain/src/lib.rs`

```rust
pub mod trading_period;
pub use trading_period::{TradingPeriod, TradingPeriodSpec, TimingError};
```

---

## Phase 3: RollPolicy (1-2 days)

**Goal**: Enable multi-period trades with position renewal.

### New File: `cs-domain/src/roll/mod.rs`

```rust
//! Roll policy for multi-period trades
//!
//! Defines when and how positions should be renewed/rolled.

mod policy;
mod event;

pub use policy::RollPolicy;
pub use event::RollEvent;
```

### New File: `cs-domain/src/roll/policy.rs`

```rust
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
```

### New File: `cs-domain/src/roll/event.rs`

```rust
use chrono::{NaiveDate, DateTime, Utc};
use rust_decimal::Decimal;

/// Record of a roll event
#[derive(Debug, Clone)]
pub struct RollEvent {
    /// When the roll occurred
    pub timestamp: DateTime<Utc>,

    /// Expiration of position being closed
    pub old_expiration: NaiveDate,

    /// Expiration of new position
    pub new_expiration: NaiveDate,

    /// Value received for closing old position
    pub close_value: Decimal,

    /// Cost of opening new position
    pub open_cost: Decimal,

    /// Net credit/debit of roll
    pub net_credit: Decimal,

    /// Spot price at roll time
    pub spot_at_roll: f64,
}

impl RollEvent {
    pub fn new(
        timestamp: DateTime<Utc>,
        old_expiration: NaiveDate,
        new_expiration: NaiveDate,
        close_value: Decimal,
        open_cost: Decimal,
        spot_at_roll: f64,
    ) -> Self {
        Self {
            timestamp,
            old_expiration,
            new_expiration,
            close_value,
            open_cost,
            net_credit: close_value - open_cost,
            spot_at_roll,
        }
    }
}
```

---

## Phase 4: TradeStrategy (1 day)

**Goal**: Unified configuration combining all dimensions.

### New File: `cs-domain/src/strategy/mod.rs`

```rust
//! Trade strategy configuration
//!
//! Combines trade structure, timing, expiration policy, and rolling
//! into a single unified configuration.

mod config;
mod presets;

pub use config::{TradeStrategy, TradeFilters};
pub use presets::*;
```

### New File: `cs-domain/src/strategy/config.rs`

```rust
use crate::expiration::ExpirationPolicy;
use crate::trading_period::TradingPeriodSpec;
use crate::roll::RollPolicy;
use crate::hedging::HedgeConfig;
use crate::strike_selection::StrikeMatchMode;
use finq_core::OptionType;

/// Complete trade strategy configuration
#[derive(Debug, Clone)]
pub struct TradeStrategy {
    /// Trade structure (straddle, calendar, etc.)
    pub structure: TradeStructureConfig,

    /// Timing specification (when to enter/exit)
    pub timing: TradingPeriodSpec,

    /// Expiration selection policy
    pub expiration_policy: ExpirationPolicy,

    /// Roll policy for multi-period trades
    pub roll_policy: RollPolicy,

    /// Delta hedging configuration
    pub hedge_config: HedgeConfig,

    /// Entry/exit filters
    pub filters: TradeFilters,
}

/// Trade structure configuration
#[derive(Debug, Clone)]
pub enum TradeStructureConfig {
    /// Long straddle (ATM call + ATM put)
    Straddle,

    /// Calendar spread
    CalendarSpread {
        option_type: OptionType,
        strike_match: StrikeMatchMode,
    },

    /// Calendar straddle (4 legs)
    CalendarStraddle,

    /// Iron butterfly
    IronButterfly {
        wing_width: rust_decimal::Decimal,
    },
}

/// Filters for trade entry
#[derive(Debug, Clone, Default)]
pub struct TradeFilters {
    /// Minimum IV to enter
    pub min_iv: Option<f64>,

    /// Maximum IV to enter
    pub max_iv: Option<f64>,

    /// Minimum IV ratio (short/long) for calendars
    pub min_iv_ratio: Option<f64>,

    /// Minimum option volume
    pub min_volume: Option<u64>,

    /// Maximum bid-ask spread percentage
    pub max_bid_ask_pct: Option<f64>,
}

impl Default for TradeStrategy {
    fn default() -> Self {
        Self {
            structure: TradeStructureConfig::Straddle,
            timing: TradingPeriodSpec::pre_earnings_default(),
            expiration_policy: ExpirationPolicy::FirstAfter {
                min_date: chrono::NaiveDate::MIN,
            },
            roll_policy: RollPolicy::None,
            hedge_config: HedgeConfig::default(),
            filters: TradeFilters::default(),
        }
    }
}

impl TradeStrategy {
    /// Create a new strategy with the given structure
    pub fn new(structure: TradeStructureConfig) -> Self {
        Self {
            structure,
            ..Default::default()
        }
    }

    /// Set timing specification
    pub fn with_timing(mut self, timing: TradingPeriodSpec) -> Self {
        self.timing = timing;
        self
    }

    /// Set expiration policy
    pub fn with_expiration_policy(mut self, policy: ExpirationPolicy) -> Self {
        self.expiration_policy = policy;
        self
    }

    /// Set roll policy
    pub fn with_roll_policy(mut self, policy: RollPolicy) -> Self {
        self.roll_policy = policy;
        self
    }

    /// Set hedge config
    pub fn with_hedge_config(mut self, config: HedgeConfig) -> Self {
        self.hedge_config = config;
        self
    }

    /// Set filters
    pub fn with_filters(mut self, filters: TradeFilters) -> Self {
        self.filters = filters;
        self
    }
}
```

### New File: `cs-domain/src/strategy/presets.rs`

```rust
//! Pre-configured strategy presets for common use cases

use super::{TradeStrategy, TradeStructureConfig, TradeFilters};
use crate::expiration::ExpirationPolicy;
use crate::trading_period::TradingPeriodSpec;
use crate::roll::RollPolicy;
use crate::hedging::{HedgeConfig, HedgeStrategy};
use chrono::{NaiveDate, NaiveTime, Duration};
use finq_core::OptionType;

/// Pre-earnings straddle: capture IV expansion before earnings
///
/// Entry: 20 trading days before earnings
/// Exit: 1 trading day before earnings
/// Expiration: First available after exit date
pub fn pre_earnings_straddle() -> TradeStrategy {
    TradeStrategy {
        structure: TradeStructureConfig::Straddle,
        timing: TradingPeriodSpec::PreEarnings {
            entry_days_before: 20,
            exit_days_before: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        },
        expiration_policy: ExpirationPolicy::FirstAfter {
            min_date: NaiveDate::MIN, // Will be set dynamically to exit_date
        },
        roll_policy: RollPolicy::None,
        hedge_config: HedgeConfig::default(),
        filters: TradeFilters::default(),
    }
}

/// Pre-earnings straddle with delta hedging
pub fn pre_earnings_straddle_hedged() -> TradeStrategy {
    let mut strategy = pre_earnings_straddle();
    strategy.hedge_config = HedgeConfig {
        strategy: HedgeStrategy::DeltaThreshold { threshold: 0.15 },
        max_rehedges: Some(10),
        min_hedge_size: 10,
        transaction_cost_per_share: rust_decimal::Decimal::new(1, 2), // $0.01
        contract_multiplier: 100,
    };
    strategy
}

/// Weekly roll straddle: buy weekly, roll each week
///
/// Entry: 4 weeks before earnings
/// Exit: 1 day before earnings
/// Roll: On each weekly expiration
pub fn weekly_roll_straddle() -> TradeStrategy {
    TradeStrategy {
        structure: TradeStructureConfig::Straddle,
        timing: TradingPeriodSpec::PreEarnings {
            entry_days_before: 28, // ~4 weeks
            exit_days_before: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        },
        expiration_policy: ExpirationPolicy::PreferWeekly {
            min_date: NaiveDate::MIN,
            fallback_to_monthly: true,
        },
        roll_policy: RollPolicy::OnExpiration {
            to_next: ExpirationPolicy::PreferWeekly {
                min_date: NaiveDate::MIN,
                fallback_to_monthly: true,
            },
        },
        hedge_config: HedgeConfig::default(),
        filters: TradeFilters::default(),
    }
}

/// Post-earnings straddle: capture momentum after earnings
///
/// Entry: Day after earnings
/// Exit: 5 trading days after entry
pub fn post_earnings_straddle() -> TradeStrategy {
    TradeStrategy {
        structure: TradeStructureConfig::Straddle,
        timing: TradingPeriodSpec::PostEarnings {
            entry_offset: 0,
            holding_days: 5,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
        },
        expiration_policy: ExpirationPolicy::FirstAfter {
            min_date: NaiveDate::MIN,
        },
        roll_policy: RollPolicy::None,
        hedge_config: HedgeConfig::default(),
        filters: TradeFilters::default(),
    }
}

/// Earnings calendar spread: capture IV crush
///
/// Entry: 1 day before earnings
/// Exit: 1 day after earnings
/// Short: First monthly after earnings
/// Long: Second monthly after earnings
pub fn earnings_calendar_spread(option_type: OptionType) -> TradeStrategy {
    TradeStrategy {
        structure: TradeStructureConfig::CalendarSpread {
            option_type,
            strike_match: crate::strike_selection::StrikeMatchMode::SameStrike,
        },
        timing: TradingPeriodSpec::CrossEarnings {
            entry_days_before: 1,
            exit_days_after: 1,
            entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
            exit_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
        },
        expiration_policy: ExpirationPolicy::Calendar {
            short: Box::new(ExpirationPolicy::PreferMonthly {
                min_date: NaiveDate::MIN,
                months_out: 0,
            }),
            long: Box::new(ExpirationPolicy::PreferMonthly {
                min_date: NaiveDate::MIN,
                months_out: 1,
            }),
        },
        roll_policy: RollPolicy::None,
        hedge_config: HedgeConfig::default(),
        filters: TradeFilters {
            min_iv_ratio: Some(1.1), // Short IV > Long IV
            ..Default::default()
        },
    }
}

/// Monthly-only calendar spread (avoid weeklies)
pub fn monthly_calendar_spread(option_type: OptionType) -> TradeStrategy {
    earnings_calendar_spread(option_type)
}
```

---

## Phase 5: Integration (1-2 days)

**Goal**: Update executors to use new abstractions.

### Updates to `cs-backtest/src/unified_executor.rs`

```rust
// Add new imports
use cs_domain::{
    TradingPeriod, ExpirationPolicy, RollPolicy,
    TradeStrategy, TradeStructureConfig,
};

impl<O, E> UnifiedExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    /// Execute a trade using the new TradeStrategy configuration
    pub async fn execute_strategy(
        &self,
        strategy: &TradeStrategy,
        event: Option<&EarningsEvent>,
        entry_surface: &IVSurface,
    ) -> TradeResult {
        // 1. Build trading period
        let period = match strategy.timing.build(event) {
            Ok(p) => p,
            Err(e) => return TradeResult::Failed(FailedTrade {
                symbol: event.map(|e| e.symbol.clone()).unwrap_or_default(),
                earnings_date: event.map(|e| e.earnings_date).unwrap_or(NaiveDate::MIN),
                earnings_time: event.map(|e| e.earnings_time).unwrap_or(EarningsTime::Unknown),
                trade_structure: self.to_trade_structure(&strategy.structure),
                reason: FailureReason::PricingError(e.to_string()),
                phase: "timing".to_string(),
                details: Some(e.to_string()),
            }),
        };

        // 2. Update expiration policy with actual min_date
        let exp_policy = strategy.expiration_policy
            .clone()
            .with_min_date(period.min_expiration());

        // 3. Select and execute based on structure
        match &strategy.structure {
            TradeStructureConfig::Straddle => {
                self.execute_straddle_with_policy(
                    event,
                    &period,
                    entry_surface,
                    &exp_policy,
                    &strategy.roll_policy,
                    &strategy.hedge_config,
                ).await
            }
            TradeStructureConfig::CalendarSpread { option_type, .. } => {
                self.execute_calendar_with_policy(
                    event,
                    &period,
                    entry_surface,
                    *option_type,
                    &exp_policy,
                ).await
            }
            // ... other structures
        }
    }

    async fn execute_straddle_with_policy(
        &self,
        event: Option<&EarningsEvent>,
        period: &TradingPeriod,
        entry_surface: &IVSurface,
        exp_policy: &ExpirationPolicy,
        roll_policy: &RollPolicy,
        hedge_config: &HedgeConfig,
    ) -> TradeResult {
        let spot = SpotPrice::new(entry_surface.spot_price(), period.entry_datetime());

        // Select straddle with expiration policy
        let selector = ATMStrategy::default();
        let straddle = match selector.select_straddle_with_policy(&spot, entry_surface, exp_policy) {
            Ok(s) => s,
            Err(e) => return self.failed_trade(event, TradeStructure::Straddle, "selection", e),
        };

        // Check if rolling is needed
        if roll_policy.is_enabled() {
            return self.execute_straddle_with_rolls(
                event,
                period,
                &straddle,
                roll_policy,
                hedge_config,
            ).await;
        }

        // Execute without rolling
        let mut result = self.straddle_executor
            .execute_trade(&straddle, event.unwrap(), period.entry_datetime(), period.exit_datetime())
            .await;

        // Apply hedging if enabled
        if hedge_config.is_enabled() {
            // ... hedging logic
        }

        if result.success {
            TradeResult::Straddle(result)
        } else {
            // ... error handling
        }
    }
}
```

---

## Testing Strategy

### Unit Tests

| Test | Location | Description |
|------|----------|-------------|
| `ExpirationCycle::classify` | `cs-domain/src/expiration/cycle.rs` | Verify weekly/monthly detection |
| `ExpirationPolicy::select` | `cs-domain/src/expiration/policy.rs` | Various selection scenarios |
| `TradingPeriodSpec::build` | `cs-domain/src/trading_period/spec.rs` | Date calculations |
| `RollPolicy::should_roll` | `cs-domain/src/roll/policy.rs` | Roll trigger conditions |

### Integration Tests

| Test | Description |
|------|-------------|
| PENG straddle | Verify Oct 17 expiration selected (not Sept 19) |
| Weekly roll | Verify weekly expirations, 4 rolls over 4 weeks |
| Monthly calendar | Verify both legs use monthly expirations |
| Post-earnings | Verify entry after earnings announcement |

### Regression Tests

```bash
# Full backtest suite
cargo test --workspace

# Specific scenarios
cargo test --package cs-backtest -- straddle
cargo test --package cs-backtest -- calendar
```

---

## File Summary

### New Files (cs-domain)

| File | Lines | Description |
|------|-------|-------------|
| `src/expiration/mod.rs` | ~10 | Module exports |
| `src/expiration/cycle.rs` | ~80 | Expiration cycle detection |
| `src/expiration/policy.rs` | ~200 | Expiration policy selection |
| `src/trading_period/mod.rs` | ~10 | Module exports |
| `src/trading_period/period.rs` | ~60 | Concrete trading period |
| `src/trading_period/spec.rs` | ~250 | Period specification |
| `src/roll/mod.rs` | ~10 | Module exports |
| `src/roll/policy.rs` | ~100 | Roll policy |
| `src/roll/event.rs` | ~50 | Roll event record |
| `src/strategy/mod.rs` | ~10 | Module exports |
| `src/strategy/config.rs` | ~120 | TradeStrategy config |
| `src/strategy/presets.rs` | ~150 | Strategy presets |

### Modified Files

| File | Changes |
|------|---------|
| `cs-domain/src/lib.rs` | Add module exports |
| `cs-domain/src/strike_selection/mod.rs` | Update trait signature |
| `cs-domain/src/strike_selection/atm.rs` | Implement policy-based selection |
| `cs-backtest/src/unified_executor.rs` | Add `execute_strategy` method |
| `cs-backtest/src/timing_strategy.rs` | Integrate with `TradingPeriodSpec` |

---

## Timeline

| Phase | Duration | Deliverable |
|-------|----------|-------------|
| **Phase 0** | 2-4 hours | Bug fix (immediate) |
| **Phase 1** | 1 day | ExpirationPolicy |
| **Phase 2** | 1 day | TradingPeriod |
| **Phase 3** | 1-2 days | RollPolicy |
| **Phase 4** | 1 day | TradeStrategy |
| **Phase 5** | 1-2 days | Integration |
| **Total** | ~1 week | Full flexible system |

---

## Next Steps

1. **Confirm design decisions** (weekly detection, roll model, hedge handling)
2. **Implement Phase 0** (immediate bug fix)
3. **Proceed with Phases 1-5** in order

Ready to begin implementation when you confirm the design decisions.
