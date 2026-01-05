# Research: Flexible Trading Period System

**Date**: 2026-01-05
**Status**: RESEARCH / DESIGN PHASE

## Executive Summary

This document analyzes the requirements for a flexible trading period system that supports:
1. **Pre-earnings trades** (IV expansion) - straddles, hedged straddles
2. **Weeklies renewed weekly** - rolling strategies
3. **Monthly expirations** - longer-dated trades
4. **Post-earnings trades** - momentum capture
5. **Crossing-earnings trades** - calendar spreads (current implementation)

The goal is to design abstractions that allow any combination of trade structure + timing + expiration policy.

---

## Current Architecture Analysis

### Three Orthogonal Dimensions (Already in Place)

```
WHAT (Trade Structure)     WHERE (Strike Selection)    WHEN (Timing)
├── CalendarSpread         ├── ATMStrategy             ├── EarningsTradeTiming
├── Straddle               └── DeltaStrategy           ├── StraddleTradeTiming
├── CalendarStraddle                                   └── PostEarningsStraddleTiming
└── IronButterfly
```

### Current Limitations

| Limitation | Impact |
|-----------|--------|
| Expiration selection uses DTE from entry date | Straddles can select expirations BEFORE earnings exit |
| No expiration cycle awareness | Can't prefer weeklies vs monthlies |
| No roll management | Can't renew weekly positions |
| Timing is earnings-centric | Everything anchored to `EarningsEvent` |
| No holding period flexibility | Fixed entry/exit patterns |

---

## Requirements Analysis

### Use Case 1: Pre-Earnings Straddle (IV Expansion)

**Goal**: Buy straddle N days before earnings, sell before earnings announcement
**Current Support**: Partially (StraddleTradeTiming) but expiration selection is buggy

```
Timeline:
                    Entry              Exit    Earnings
──────────────────────|─────────────────|────────|──────────→
                    D-20              D-1      D
                                             ↑
                                   Expiration must be AFTER here
```

**Requirements**:
- Entry: N trading days before earnings (configurable: 5-30)
- Exit: M trading days before earnings (configurable: 0-5)
- Expiration: MUST be after exit date (preferably just after earnings)
- Hedging: Optional delta hedging during holding period

### Use Case 2: Hedged Straddle (Pre-Earnings)

**Goal**: Same as Use Case 1, but with systematic delta hedging

**Requirements**:
- Same timing as Use Case 1
- Delta hedge at configurable intervals (time-based, threshold-based, or gamma-dollar)
- Track hedge P&L separately from options P&L

**Current Support**: HedgeConfig exists, but needs better integration

### Use Case 3: Weekly Roll Strategy

**Goal**: Buy weekly straddle, roll to next weekly each week during IV expansion period

```
Timeline:
Entry W1    Roll W2     Roll W3     Exit (before earnings)
──|──────────|───────────|────────────|──────→
  ↑          ↑           ↑
  Week1 exp  Week2 exp   Week3 exp
```

**Requirements**:
- Entry: N weeks before earnings
- Exit: M days before earnings
- Roll: On expiration day, close current, open next weekly
- Filter: Only trade if weekly expirations available

### Use Case 4: Monthly Expiration Trades

**Goal**: Use only monthly expirations for calendar spreads

**Requirements**:
- Filter expirations to 3rd Friday of month
- Avoid weeklies (higher gamma risk, wider bid-ask)

### Use Case 5: Post-Earnings Straddle (Momentum)

**Goal**: Enter after earnings move, capture continuation

**Current Support**: PostEarningsStraddleTiming exists

**Requirements**:
- Entry: Day after earnings (BMO: same day, AMC: next day)
- Exit: N days after entry (configurable: 3-10)
- Expiration: At least M days after exit

### Use Case 6: Earnings-Crossing Calendar (Current)

**Goal**: Short front-month (expires just after earnings), long back-month

**Current Support**: EarningsTradeTiming + CalendarSpread

**Requirements**:
- Entry: On/just before earnings
- Exit: Just after earnings (IV crush capture)
- Short expiration: Just after earnings
- Long expiration: 2-4 weeks after short

---

## Proposed Design

### New Abstraction 1: `ExpirationPolicy`

Controls HOW expirations are selected:

```rust
/// Defines how to select expirations for a trade
#[derive(Debug, Clone)]
pub enum ExpirationPolicy {
    /// Select first expiration after a given date (current behavior)
    AfterDate {
        min_date: NaiveDate,
        min_dte: Option<i32>,  // Optional minimum DTE from entry
    },

    /// Prefer weekly expirations
    Weekly {
        min_date: NaiveDate,
        max_weeklies_out: u8,  // e.g., 1 = first weekly, 2 = second weekly
    },

    /// Prefer monthly (3rd Friday) expirations
    Monthly {
        min_date: NaiveDate,
        months_out: u8,  // 0 = current cycle, 1 = next month, etc.
    },

    /// Calendar spread: short and long leg constraints
    Calendar {
        short_policy: Box<ExpirationPolicy>,
        long_policy: Box<ExpirationPolicy>,
    },
}

impl ExpirationPolicy {
    /// Filter and rank expirations according to policy
    pub fn select(&self, available: &[NaiveDate]) -> Result<NaiveDate, SelectionError>;

    /// For calendar spreads, select both legs
    pub fn select_pair(&self, available: &[NaiveDate])
        -> Result<(NaiveDate, NaiveDate), SelectionError>;
}
```

### New Abstraction 2: `TradingPeriod`

Decouples timing from earnings-centric model:

```rust
/// A period during which a trade is held
#[derive(Debug, Clone)]
pub struct TradingPeriod {
    pub entry_date: NaiveDate,
    pub exit_date: NaiveDate,
    pub entry_time: NaiveTime,
    pub exit_time: NaiveTime,
}

/// Builder for trading periods
pub enum TradingPeriodBuilder {
    /// Relative to an earnings event
    EarningsRelative {
        entry_days_before: i32,  // Negative = after earnings
        exit_days_before: i32,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Fixed date range
    Fixed {
        entry: NaiveDate,
        exit: NaiveDate,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Holding period from entry
    HoldingPeriod {
        entry: NaiveDate,
        holding_days: u16,  // Trading days
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },
}

impl TradingPeriodBuilder {
    pub fn build(&self, event: Option<&EarningsEvent>) -> TradingPeriod;
}
```

### New Abstraction 3: `RollPolicy`

For strategies that require renewing positions:

```rust
/// Policy for rolling positions to new expirations
#[derive(Debug, Clone)]
pub enum RollPolicy {
    /// No rolling - hold to exit
    None,

    /// Roll on each expiration to next expiration
    OnExpiration {
        to_next: ExpirationPolicy,
    },

    /// Roll at fixed intervals (regardless of expiration)
    TimeInterval {
        interval_days: u16,
        to_policy: ExpirationPolicy,
    },

    /// Roll when DTE drops below threshold
    DteThreshold {
        min_dte: i32,
        to_policy: ExpirationPolicy,
    },
}
```

### New Abstraction 4: `TradeStrategy` (Unified)

Combines all dimensions:

```rust
/// Complete trade strategy configuration
#[derive(Debug, Clone)]
pub struct TradeStrategy {
    /// What structure to trade
    pub structure: TradeStructure,

    /// How to select strikes
    pub strike_selection: StrikeSelectionMode,

    /// How to select expirations
    pub expiration_policy: ExpirationPolicy,

    /// When to enter/exit
    pub timing: TradingPeriodBuilder,

    /// Roll policy (for multi-period trades)
    pub roll_policy: RollPolicy,

    /// Hedging configuration
    pub hedge_config: HedgeConfig,

    /// Entry filters
    pub filters: TradeFilters,
}

#[derive(Debug, Clone)]
pub struct TradeFilters {
    pub min_iv: Option<f64>,
    pub max_iv: Option<f64>,
    pub min_iv_ratio: Option<f64>,  // For calendar spreads
    pub min_volume: Option<u64>,
    pub max_bid_ask_pct: Option<f64>,
}
```

---

## Implementation Recommendations

### Option A: Minimal Fix (Original FIX_PLAN)

**Scope**: Just fix the straddle expiration bug

**Changes**:
1. Add `min_expiration: NaiveDate` parameter to `select_straddle()`
2. Pass `exit_date` from timing to selection
3. Filter expirations to those after exit date

**Pros**: Quick fix, minimal disruption
**Cons**: Doesn't enable new strategies

**Estimated effort**: 2-4 hours

### Option B: Expiration Policy Only

**Scope**: Introduce `ExpirationPolicy` abstraction

**Changes**:
1. Define `ExpirationPolicy` enum in cs-domain
2. Modify `StrikeSelector` to accept policy
3. Update `UnifiedExecutor` to pass policy from timing
4. Add weekly/monthly detection utilities

**Pros**: Enables expiration preferences, fixes bug
**Cons**: Still earnings-centric, no roll support

**Estimated effort**: 1-2 days

### Option C: Full Flexible System (Recommended)

**Scope**: Implement all new abstractions

**Phases**:

#### Phase 1: ExpirationPolicy
- `ExpirationPolicy` enum
- Weekly/monthly detection
- Migrate current selection to use policy

#### Phase 2: TradingPeriod
- `TradingPeriod` and `TradingPeriodBuilder`
- Decouple from `EarningsEvent` (make it optional)
- Support arbitrary date ranges

#### Phase 3: RollPolicy
- `RollPolicy` enum
- Roll execution in executor
- Multi-leg trade tracking

#### Phase 4: TradeStrategy
- Unified configuration object
- Strategy presets (pre-earnings straddle, etc.)
- Serialization for config files

**Pros**: Maximum flexibility, clean architecture, testable
**Cons**: Larger scope, more testing needed

**Estimated effort**: 1-2 weeks

---

## Design Details: Option C

### ExpirationPolicy Implementation

```rust
// cs-domain/src/expiration_policy.rs

use chrono::{NaiveDate, Datelike, Weekday};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpirationCycle {
    Weekly,   // Any Friday that's not 3rd Friday
    Monthly,  // 3rd Friday of month
    Quarterly, // 3rd Friday of Mar/Jun/Sep/Dec
    Leaps,    // Jan expirations 1+ year out
}

impl ExpirationCycle {
    /// Classify an expiration date
    pub fn classify(date: NaiveDate) -> Self {
        let weekday = date.weekday();
        if weekday != Weekday::Fri {
            // Not a standard expiration, treat as weekly
            return Self::Weekly;
        }

        let day = date.day();
        let is_third_friday = day >= 15 && day <= 21;

        if !is_third_friday {
            return Self::Weekly;
        }

        let month = date.month();
        if month == 1 && date.year() > chrono::Utc::now().year() {
            return Self::Leaps;
        }

        if matches!(month, 3 | 6 | 9 | 12) {
            return Self::Quarterly;
        }

        Self::Monthly
    }
}

#[derive(Debug, Clone)]
pub enum ExpirationPolicy {
    /// First expiration >= min_date with DTE >= min_dte
    FirstAvailable {
        min_date: NaiveDate,
        min_dte: i32,
    },

    /// First weekly expiration >= min_date
    PreferWeekly {
        min_date: NaiveDate,
        fallback_to_monthly: bool,
    },

    /// First monthly expiration >= min_date
    PreferMonthly {
        min_date: NaiveDate,
        months_out: u8,  // 0 = first monthly >= min_date
    },

    /// Specific DTE target (find closest)
    TargetDte {
        target_dte: i32,
        tolerance: i32,  // +/- days
        entry_date: NaiveDate,
    },
}

impl ExpirationPolicy {
    pub fn select(&self, expirations: &[NaiveDate]) -> Result<NaiveDate, SelectionError> {
        let mut sorted: Vec<_> = expirations.iter().copied().collect();
        sorted.sort();

        match self {
            Self::FirstAvailable { min_date, min_dte } => {
                sorted.into_iter()
                    .find(|&exp| {
                        exp >= *min_date &&
                        (exp - *min_date).num_days() as i32 >= *min_dte
                    })
                    .ok_or(SelectionError::NoExpirations)
            }

            Self::PreferWeekly { min_date, fallback_to_monthly } => {
                // First, try to find a weekly
                let weekly = sorted.iter()
                    .find(|&&exp| {
                        exp >= *min_date &&
                        ExpirationCycle::classify(exp) == ExpirationCycle::Weekly
                    })
                    .copied();

                if weekly.is_some() {
                    return Ok(weekly.unwrap());
                }

                if *fallback_to_monthly {
                    sorted.into_iter()
                        .find(|&exp| exp >= *min_date)
                        .ok_or(SelectionError::NoExpirations)
                } else {
                    Err(SelectionError::NoExpirations)
                }
            }

            Self::PreferMonthly { min_date, months_out } => {
                let monthlies: Vec<_> = sorted.iter()
                    .filter(|&&exp| {
                        exp >= *min_date &&
                        matches!(
                            ExpirationCycle::classify(exp),
                            ExpirationCycle::Monthly | ExpirationCycle::Quarterly
                        )
                    })
                    .copied()
                    .collect();

                monthlies.get(*months_out as usize)
                    .copied()
                    .ok_or(SelectionError::NoExpirations)
            }

            Self::TargetDte { target_dte, tolerance, entry_date } => {
                sorted.into_iter()
                    .filter(|&exp| {
                        let dte = (exp - *entry_date).num_days() as i32;
                        (dte - target_dte).abs() <= *tolerance
                    })
                    .min_by_key(|&exp| {
                        let dte = (exp - *entry_date).num_days() as i32;
                        (dte - target_dte).abs()
                    })
                    .ok_or(SelectionError::NoExpirations)
            }
        }
    }
}
```

### TradingPeriod Implementation

```rust
// cs-domain/src/trading_period.rs

use chrono::{NaiveDate, NaiveTime, DateTime, Utc};
use crate::entities::EarningsEvent;
use crate::timing::TradingCalendar;

#[derive(Debug, Clone)]
pub struct TradingPeriod {
    pub entry_date: NaiveDate,
    pub exit_date: NaiveDate,
    pub entry_time: NaiveTime,
    pub exit_time: NaiveTime,
}

impl TradingPeriod {
    pub fn entry_datetime(&self) -> DateTime<Utc> {
        crate::datetime::eastern_to_utc(self.entry_date, self.entry_time)
    }

    pub fn exit_datetime(&self) -> DateTime<Utc> {
        crate::datetime::eastern_to_utc(self.exit_date, self.exit_time)
    }

    pub fn holding_days(&self) -> i64 {
        TradingCalendar::trading_days_between(self.entry_date, self.exit_date)
    }
}

#[derive(Debug, Clone)]
pub enum TradingPeriodSpec {
    /// Entry N days before earnings, exit M days before
    PreEarnings {
        entry_days_before: u16,
        exit_days_before: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Entry after earnings, hold for N days
    PostEarnings {
        entry_offset: i16,  // 0 = earnings day (if BMO), 1 = day after
        holding_days: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Cross earnings (enter before, exit after)
    CrossEarnings {
        entry_offset: i16,  // Days before earnings (negative = before)
        exit_offset: i16,   // Days after earnings (positive = after)
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },

    /// Fixed dates (not earnings-relative)
    FixedDates {
        entry_date: NaiveDate,
        exit_date: NaiveDate,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },
}

impl TradingPeriodSpec {
    /// Build concrete TradingPeriod from spec
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
                Ok(TradingPeriod {
                    entry_date,
                    exit_date,
                    entry_time: *entry_time,
                    exit_time: *exit_time,
                })
            }
            // ... other variants
        }
    }

    /// Minimum expiration date (options must not expire before this)
    pub fn min_expiration(&self, event: Option<&EarningsEvent>) -> Result<NaiveDate, TimingError> {
        let period = self.build(event)?;
        // Expiration must be AFTER exit date
        Ok(period.exit_date + chrono::Duration::days(1))
    }
}
```

### Strategy Presets

```rust
// cs-domain/src/strategy_presets.rs

/// Pre-configured strategies for common use cases
pub mod presets {
    use super::*;

    /// Pre-earnings straddle: buy 20 days out, sell day before
    pub fn pre_earnings_straddle() -> TradeStrategy {
        TradeStrategy {
            structure: TradeStructure::Straddle,
            timing: TradingPeriodSpec::PreEarnings {
                entry_days_before: 20,
                exit_days_before: 1,
                entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
                exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
            },
            expiration_policy: ExpirationPolicy::FirstAvailable {
                min_date: NaiveDate::MIN, // Will be set to exit_date + 1
                min_dte: 3,
            },
            roll_policy: RollPolicy::None,
            hedge_config: HedgeConfig::default(),
            ..Default::default()
        }
    }

    /// Weekly straddle roll: buy weekly, roll each week
    pub fn weekly_roll_straddle() -> TradeStrategy {
        TradeStrategy {
            structure: TradeStructure::Straddle,
            timing: TradingPeriodSpec::PreEarnings {
                entry_days_before: 28,  // 4 weeks
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
            ..Default::default()
        }
    }

    /// Monthly calendar spread: use monthly expirations only
    pub fn monthly_calendar_spread() -> TradeStrategy {
        TradeStrategy {
            structure: TradeStructure::CalendarSpread(OptionType::Call),
            timing: TradingPeriodSpec::CrossEarnings {
                entry_offset: -1,  // Day before
                exit_offset: 1,    // Day after
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
            ..Default::default()
        }
    }
}
```

---

## Migration Path

### Phase 1: Fix Current Bug (Immediate)

1. Modify `select_straddle()` to accept `min_expiration: NaiveDate`
2. Pass `exit_date` from `UnifiedExecutor` to selector
3. Add validation test for PENG case

**Files changed**:
- `cs-domain/src/strike_selection/mod.rs`
- `cs-domain/src/strike_selection/atm.rs`
- `cs-backtest/src/unified_executor.rs`

### Phase 2: ExpirationPolicy (Week 1)

1. Add `cs-domain/src/expiration_policy.rs`
2. Add `ExpirationCycle` detection
3. Add `ExpirationPolicy` enum
4. Migrate `StrikeSelector` to use policy
5. Add tests for weekly/monthly classification

### Phase 3: TradingPeriod (Week 1-2)

1. Add `cs-domain/src/trading_period.rs`
2. Add `TradingPeriodSpec` enum
3. Refactor existing timing implementations
4. Update `TimingStrategy` in backtest

### Phase 4: RollPolicy (Week 2)

1. Add `cs-domain/src/roll_policy.rs`
2. Add roll execution to `UnifiedExecutor`
3. Add multi-leg tracking in results

### Phase 5: Strategy Presets (Week 2)

1. Add strategy preset configurations
2. Add CLI/config support
3. Documentation

---

## Files To Create/Modify

### New Files
- `cs-domain/src/expiration_policy.rs`
- `cs-domain/src/trading_period.rs`
- `cs-domain/src/roll_policy.rs`
- `cs-domain/src/strategy_presets.rs`

### Modified Files
- `cs-domain/src/lib.rs` - exports
- `cs-domain/src/strike_selection/mod.rs` - policy integration
- `cs-domain/src/strike_selection/atm.rs` - policy-based selection
- `cs-backtest/src/unified_executor.rs` - period/policy usage
- `cs-backtest/src/timing_strategy.rs` - period integration

---

## Testing Strategy

### Unit Tests

1. `ExpirationCycle::classify()` - verify weekly/monthly/quarterly detection
2. `ExpirationPolicy::select()` - various policy scenarios
3. `TradingPeriodSpec::build()` - date calculations
4. `TradingPeriodSpec::min_expiration()` - expiration constraints

### Integration Tests

1. PENG case: Verify straddle selects expiration after exit
2. Weekly roll: Verify correct weekly expirations selected
3. Monthly calendar: Verify both legs use monthly expirations

### Regression Tests

1. Run full backtest suite with new code
2. Verify no change in valid trade results
3. Verify invalid trades now properly rejected

---

## Recommendation

**Implement Option C (Full Flexible System)** in phases:

1. **Immediate (today)**: Apply Phase 1 fix from original FIX_PLAN to unblock testing
2. **This week**: Implement Phase 2 (ExpirationPolicy) and Phase 3 (TradingPeriod)
3. **Next week**: Implement Phase 4 (RollPolicy) and Phase 5 (Presets)

This approach:
- Fixes the immediate bug
- Provides a clean foundation for all future strategies
- Maintains backward compatibility
- Enables all the user's desired trading patterns

---

## Questions for User

1. **Weekly detection**: Should we use pure calendar logic (any Friday not 3rd Friday) or detect based on available expirations in the chain?

2. **Roll execution**: When rolling, should we:
   - Close and re-open in same minute (synthetic roll)?
   - Model as two separate trades?
   - Track as single trade with roll events?

3. **Hedging during rolls**: Should hedges be closed and re-opened on roll, or carried through?

4. **Priority**: Which use cases are most important?
   - Pre-earnings straddle with correct expiration (immediate need)
   - Weekly roll strategy
   - Monthly-only calendar spreads
   - Other?
