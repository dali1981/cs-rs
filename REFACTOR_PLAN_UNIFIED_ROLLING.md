# Unified Rolling Framework Refactoring Plan

**Created**: 2026-01-05
**Status**: Planning
**Scope**: Long-term architectural refactoring

---

## Executive Summary

Consolidate the fragmented backtest execution paths into a single, extensible framework that:
1. Eliminates scattered hardcoded defaults (09:35 bug)
2. Enables rolling strategies for ANY trade type (not just straddles)
3. Unifies earnings-based and calendar-based scheduling
4. Reduces code duplication between standard and rolling paths

---

## Current State Analysis

### Code Paths Today

```
CLI
├── --roll-strategy provided
│   └── run_rolling_straddle()
│       └── RollingStraddleExecutor
│           └── UnifiedExecutor.execute_straddle()
│
└── no --roll-strategy
    └── BacktestUseCase.execute()
        ├── execute_straddle()      → UnifiedExecutor
        ├── execute_calendar_spread() → UnifiedExecutor
        ├── execute_iron_butterfly()  → UnifiedExecutor
        └── execute_calendar_straddle() → UnifiedExecutor
```

### Problems

| Problem | Impact | Files Affected |
|---------|--------|----------------|
| 6+ hardcoded 09:35 defaults | Entry time inconsistency | 6 files in cs-domain, cs-backtest |
| Rolling only works for straddles | Can't roll calendars, butterflies | cs-backtest/rolling_straddle_executor.rs |
| Two parallel timing systems | Confusing, bug-prone | timing/*.rs, timing_strategy.rs |
| No earnings-aware rolling | Can't do "monthly pre-earnings" | N/A (missing feature) |
| TradeFactory vs StraddleStrategy | Duplicate trade construction | unified_executor.rs, rolling_straddle_executor.rs |

---

## Target Architecture

### Layer Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              CLI Layer                                   │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  Single entry point: run_backtest()                              │   │
│  │  • Parses strategy config (trade type, scheduling, hedging)     │   │
│  │  • Delegates to BacktestOrchestrator                            │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        Application Layer                                 │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  BacktestOrchestrator                                            │   │
│  │  • Owns TradeScheduler (determines WHEN to trade)               │   │
│  │  • Owns RollingExecutor<T> (executes trades)                    │   │
│  │  • Aggregates results into RollingResult or SingleTradeResult   │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                    │                                    │
│         ┌──────────────────────────┼──────────────────────────┐        │
│         ▼                          ▼                          ▼        │
│  ┌──────────────┐         ┌──────────────┐         ┌──────────────┐   │
│  │TradeScheduler│         │RollingExecutor│        │ ResultAggregator│ │
│  │  (trait)     │         │   <T>        │         │               │   │
│  └──────────────┘         └──────────────┘         └──────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                          Domain Layer                                    │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  RollableTrade (trait)                                           │   │
│  │  ├── Straddle                                                    │   │
│  │  ├── CalendarSpread                                              │   │
│  │  ├── IronButterfly                                               │   │
│  │  └── CalendarStraddle                                            │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  MarketTime constants (single source of truth)                   │   │
│  │  • DEFAULT_ENTRY = 10:00                                         │   │
│  │  • DEFAULT_EXIT = 15:45                                          │   │
│  │  • MARKET_OPEN = 09:30                                           │   │
│  │  • MARKET_CLOSE = 16:00                                          │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                       Infrastructure Layer                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                  │
│  │OptionsRepo   │  │ EquityRepo   │  │ EarningsRepo │                  │
│  └──────────────┘  └──────────────┘  └──────────────┘                  │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Phase 1: Foundation (Fix Immediate Bugs)

**Goal**: Eliminate hardcoded defaults, fix Saturday bug

### Task 1.1: Centralize MarketTime Constants

**Files to modify**:
- `cs-domain/src/datetime.rs` - Already has constants (verified)
- `cs-domain/src/value_objects.rs` - Line 65-66
- `cs-domain/src/timing/straddle.rs` - Line 106-107
- `cs-domain/src/timing/earnings.rs` - Line 109-110
- `cs-domain/src/timing/post_earnings.rs` - Line 95-96
- `cs-backtest/src/timing_strategy.rs` - Line 154
- `cs-domain/src/strategy/presets.rs` - Lines 23, 59, 87, 114-115
- `cs-domain/src/trading_period/spec.rs` - Lines 93, 103, 113-114, 292, 310, 326

**Change pattern**:
```rust
// BEFORE (scattered in 6+ files)
impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            entry_hour: 9,
            entry_minute: 35,
            ...
        }
    }
}

// AFTER (all reference single source)
impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            entry_hour: MarketTime::DEFAULT_ENTRY.hour,
            entry_minute: MarketTime::DEFAULT_ENTRY.minute,
            exit_hour: MarketTime::DEFAULT_HEDGE_CHECK.hour,
            exit_minute: MarketTime::DEFAULT_HEDGE_CHECK.minute,
        }
    }
}
```

**Verification**:
```bash
# Should return 0 results after fix
grep -r "entry_hour.*9" --include="*.rs" | grep -v "test" | grep -v "DEFAULT"
grep -r "entry_minute.*35" --include="*.rs" | grep -v "test"
```

### Task 1.2: Fix Saturday Scheduling Bug

**Investigation needed**: Verify `TradingCalendar::subtract_trading_days()` behavior

**Files to check**:
- `cs-domain/src/trading_calendar.rs`

**Expected behavior**:
```rust
// earnings_date = Monday 2025-03-31
// entry_days = 5
// Expected entry_date = Monday 2025-03-24 (5 trading days back)
// NOT Saturday 2025-03-29
```

**Test to add**:
```rust
#[test]
fn test_subtract_trading_days_skips_weekends() {
    let monday = NaiveDate::from_ymd_opt(2025, 3, 31).unwrap();
    let result = TradingCalendar::subtract_trading_days(monday, 5);
    assert_eq!(result, NaiveDate::from_ymd_opt(2025, 3, 24).unwrap());
    assert_ne!(result.weekday(), chrono::Weekday::Sat);
    assert_ne!(result.weekday(), chrono::Weekday::Sun);
}
```

### Task 1.3: Add Trading Calendar Validation

**Location**: `cs-backtest/src/backtest_use_case.rs`

**Change**:
```rust
// In filter_for_entry() or execute_straddle()
let entry_date = timing.entry_date(event);

// ADD: Validate entry_date is a trading day
if !TradingCalendar::is_trading_day(entry_date) {
    warn!("Entry date {} is not a trading day, skipping", entry_date);
    continue;
}
```

---

## Phase 2: RollableTrade Trait

**Goal**: Enable generic rolling for any trade type

### Task 2.1: Define RollableTrade Trait

**New file**: `cs-domain/src/trade/rollable.rs`

```rust
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

/// A trade that can be constructed, executed, and rolled
#[async_trait]
pub trait RollableTrade: Sized + Send + Sync {
    /// Result type returned by execution
    type Result: TradeResult;

    /// Construct trade at given datetime
    ///
    /// # Arguments
    /// * `factory` - Trade factory for option chain queries
    /// * `symbol` - Underlying symbol
    /// * `dt` - Entry datetime (for spot/IV lookup)
    /// * `min_expiration` - Earliest acceptable expiration date
    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError>;

    /// Get expiration date (for roll scheduling)
    fn expiration(&self) -> NaiveDate;

    /// Get strike (for logging/display)
    fn strike(&self) -> Decimal;

    /// Get symbol
    fn symbol(&self) -> &str;
}

/// Common interface for trade results
pub trait TradeResult: Send + Sync {
    fn pnl(&self) -> Decimal;
    fn entry_cost(&self) -> Decimal;
    fn exit_value(&self) -> Decimal;
    fn success(&self) -> bool;
    fn hedge_pnl(&self) -> Option<Decimal>;
}

#[derive(Debug, thiserror::Error)]
pub enum TradeConstructionError {
    #[error("No options data available: {0}")]
    NoOptionsData(String),
    #[error("No valid expiration found: {0}")]
    NoExpiration(String),
    #[error("No ATM strike found: {0}")]
    NoStrike(String),
    #[error("Factory error: {0}")]
    FactoryError(String),
}
```

### Task 2.2: Implement RollableTrade for Straddle

**File**: `cs-domain/src/trade/straddle.rs` (modify existing)

```rust
#[async_trait]
impl RollableTrade for Straddle {
    type Result = StraddleResult;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        factory
            .create_atm_straddle(symbol, dt, min_expiration)
            .await
            .map_err(|e| TradeConstructionError::FactoryError(e.to_string()))
    }

    fn expiration(&self) -> NaiveDate {
        self.expiration()
    }

    fn strike(&self) -> Decimal {
        self.strike().value()
    }

    fn symbol(&self) -> &str {
        self.symbol()
    }
}

impl TradeResult for StraddleResult {
    fn pnl(&self) -> Decimal { self.pnl }
    fn entry_cost(&self) -> Decimal { self.entry_debit }
    fn exit_value(&self) -> Decimal { self.exit_credit }
    fn success(&self) -> bool { self.success }
    fn hedge_pnl(&self) -> Option<Decimal> { self.hedge_pnl }
}
```

### Task 2.3: Implement RollableTrade for CalendarSpread

**File**: `cs-domain/src/trade/calendar_spread.rs` (modify existing)

```rust
#[async_trait]
impl RollableTrade for CalendarSpread {
    type Result = CalendarSpreadResult;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        factory
            .create_calendar_spread(symbol, dt, min_expiration)
            .await
            .map_err(|e| TradeConstructionError::FactoryError(e.to_string()))
    }

    fn expiration(&self) -> NaiveDate {
        self.short_leg.expiration  // Roll based on short leg
    }

    fn strike(&self) -> Decimal {
        self.short_leg.strike.value()
    }

    fn symbol(&self) -> &str {
        &self.symbol
    }
}
```

### Task 2.4: Extend TradeFactory Trait

**File**: `cs-domain/src/trade/factory.rs`

```rust
#[async_trait]
pub trait TradeFactory: Send + Sync {
    /// Create ATM straddle (existing)
    async fn create_atm_straddle(
        &self,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Straddle, String>;

    /// Create ATM calendar spread (NEW)
    async fn create_calendar_spread(
        &self,
        symbol: &str,
        dt: DateTime<Utc>,
        min_short_expiration: NaiveDate,
    ) -> Result<CalendarSpread, String>;

    /// Create ATM iron butterfly (NEW)
    async fn create_iron_butterfly(
        &self,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
        wing_width: Decimal,
    ) -> Result<IronButterfly, String>;
}
```

---

## Phase 3: Generic Rolling Executor

**Goal**: Single executor that rolls any RollableTrade

### Task 3.1: Create Generic RollingExecutor

**New file**: `cs-backtest/src/rolling_executor.rs`

```rust
use std::marker::PhantomData;
use std::sync::Arc;
use chrono::{DateTime, NaiveDate, Utc};

use cs_domain::{
    EquityDataRepository, OptionsDataRepository, MarketTime,
    RollPolicy, RollPeriod, RollReason, RollingResult,
    TradeFactory, TradingCalendar,
    trade::{RollableTrade, TradeResult},
};

use crate::unified_executor::UnifiedExecutor;

/// Generic executor for rolling any trade type
pub struct RollingExecutor<T, O, E>
where
    T: RollableTrade,
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    unified_executor: UnifiedExecutor<O, E>,
    trade_factory: Arc<dyn TradeFactory>,
    roll_policy: RollPolicy,
    _phantom: PhantomData<T>,
}

impl<T, O, E> RollingExecutor<T, O, E>
where
    T: RollableTrade,
    O: OptionsDataRepository + 'static,
    E: EquityDataRepository + 'static,
{
    pub fn new(
        unified_executor: UnifiedExecutor<O, E>,
        trade_factory: Arc<dyn TradeFactory>,
        roll_policy: RollPolicy,
    ) -> Self {
        Self {
            unified_executor,
            trade_factory,
            roll_policy,
            _phantom: PhantomData,
        }
    }

    /// Execute rolling strategy for any trade type
    pub async fn execute_rolling(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        entry_time: MarketTime,
        exit_time: MarketTime,
    ) -> RollingResult {
        let mut rolls = Vec::new();
        let mut current_date = start_date;

        // Ensure we start on a trading day
        if !TradingCalendar::is_trading_day(current_date) {
            current_date = TradingCalendar::next_trading_day(current_date);
        }

        while current_date < end_date {
            // Construct trade using trait method
            let entry_dt = self.to_datetime(current_date, entry_time);
            let min_expiration = current_date + chrono::Duration::days(1);

            let trade = match T::create(
                self.trade_factory.as_ref(),
                symbol,
                entry_dt,
                min_expiration,
            ).await {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Failed to create trade at {}: {}", current_date, e);
                    current_date = TradingCalendar::next_trading_day(current_date);
                    continue;
                }
            };

            // Determine exit
            let (exit_date, roll_reason) = self.determine_exit_date(
                current_date,
                end_date,
                trade.expiration(),
            );

            let exit_dt = self.to_datetime(exit_date, exit_time);

            // Execute using unified executor (dispatch based on type)
            let result = self.execute_trade(&trade, entry_dt, exit_dt).await;

            // Convert to RollPeriod
            let roll_period = self.to_roll_period(&trade, result, roll_reason);
            rolls.push(roll_period);

            // Next roll
            current_date = TradingCalendar::next_trading_day(exit_date);
        }

        RollingResult::from_rolls(
            symbol.to_string(),
            start_date,
            end_date,
            self.roll_policy.description(),
            std::any::type_name::<T>().to_string(),
            rolls,
        )
    }

    /// Execute trade - dispatches to correct UnifiedExecutor method
    async fn execute_trade(
        &self,
        trade: &T,
        entry: DateTime<Utc>,
        exit: DateTime<Utc>,
    ) -> T::Result {
        // This requires a way to dispatch based on T
        // Option 1: Add execute method to RollableTrade trait
        // Option 2: Use type_id matching
        // Option 3: Create TradeExecutor trait

        // For now, we'll add to RollableTrade trait (see Task 3.2)
        todo!("Implement dispatch")
    }

    fn determine_exit_date(
        &self,
        entry_date: NaiveDate,
        campaign_end: NaiveDate,
        option_expiration: NaiveDate,
    ) -> (NaiveDate, RollReason) {
        // Same logic as current RollingStraddleExecutor
        if campaign_end <= entry_date {
            return (entry_date, RollReason::EndOfCampaign);
        }

        let next_roll = self.roll_policy
            .next_roll_date(entry_date)
            .unwrap_or(campaign_end);

        let exit_date = next_roll.min(option_expiration).min(campaign_end);

        let reason = if exit_date >= campaign_end {
            RollReason::EndOfCampaign
        } else if exit_date >= option_expiration {
            RollReason::Expiry
        } else {
            RollReason::Scheduled
        };

        (exit_date, reason)
    }

    fn to_roll_period(
        &self,
        trade: &T,
        result: T::Result,
        roll_reason: RollReason,
    ) -> RollPeriod {
        // Generic conversion using TradeResult trait
        RollPeriod {
            entry_date: todo!(),
            exit_date: todo!(),
            strike: trade.strike(),
            expiration: trade.expiration(),
            entry_debit: result.entry_cost(),
            exit_credit: result.exit_value(),
            pnl: result.pnl(),
            hedge_pnl: result.hedge_pnl(),
            roll_reason,
            // ... other fields from result
        }
    }

    fn to_datetime(&self, date: NaiveDate, time: MarketTime) -> DateTime<Utc> {
        use chrono::NaiveTime;
        use cs_domain::datetime::eastern_to_utc;

        let naive_time = NaiveTime::from_hms_opt(
            time.hour as u32,
            time.minute as u32,
            0
        ).unwrap();

        eastern_to_utc(date, naive_time)
    }
}
```

### Task 3.2: Add Execute Method to RollableTrade

**Update**: `cs-domain/src/trade/rollable.rs`

```rust
#[async_trait]
pub trait RollableTrade: Sized + Send + Sync {
    type Result: TradeResult;

    // ... existing methods ...

    /// Execute the trade using provided executor
    async fn execute<O, E>(
        &self,
        executor: &UnifiedExecutor<O, E>,
        entry: DateTime<Utc>,
        exit: DateTime<Utc>,
    ) -> Self::Result
    where
        O: OptionsDataRepository + 'static,
        E: EquityDataRepository + 'static;
}

// Implementation for Straddle
#[async_trait]
impl RollableTrade for Straddle {
    // ... existing ...

    async fn execute<O, E>(
        &self,
        executor: &UnifiedExecutor<O, E>,
        entry: DateTime<Utc>,
        exit: DateTime<Utc>,
    ) -> StraddleResult
    where
        O: OptionsDataRepository + 'static,
        E: EquityDataRepository + 'static,
    {
        // Create dummy earnings event (not used for rolling)
        let event = EarningsEvent::new(
            self.symbol().to_string(),
            exit.date_naive(),
            EarningsTime::AfterMarketClose,
        );

        executor.execute_straddle(self, &event, entry, exit).await
    }
}
```

### Task 3.3: Migrate RollingStraddleExecutor

**File**: `cs-backtest/src/rolling_straddle_executor.rs`

```rust
// BEFORE: Specific implementation
pub struct RollingStraddleExecutor<O, E> { ... }

// AFTER: Type alias to generic
pub type RollingStraddleExecutor<O, E> = RollingExecutor<Straddle, O, E>;

// Keep for backwards compatibility, but deprecate
#[deprecated(since = "0.2.0", note = "Use RollingExecutor<Straddle, O, E> instead")]
pub fn new_straddle_executor<O, E>(
    unified: UnifiedExecutor<O, E>,
    factory: Arc<dyn TradeFactory>,
    policy: RollPolicy,
) -> RollingExecutor<Straddle, O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    RollingExecutor::new(unified, factory, policy)
}
```

---

## Phase 4: TradeScheduler Abstraction

**Goal**: Unify calendar-based and earnings-based scheduling

### Task 4.1: Define TradeScheduler Trait

**New file**: `cs-domain/src/scheduling/scheduler.rs`

```rust
use chrono::NaiveDate;

/// Determines when trades should be entered and exited
pub trait TradeScheduler: Send + Sync {
    /// Get next entry date on or after `from`
    /// Returns None if no more entries in the schedule
    fn next_entry_date(&self, from: NaiveDate) -> Option<NaiveDate>;

    /// Get exit date for a trade entered on `entry_date`
    fn exit_date(&self, entry_date: NaiveDate) -> NaiveDate;

    /// Human-readable description
    fn description(&self) -> String;
}
```

### Task 4.2: Implement CalendarScheduler

**New file**: `cs-domain/src/scheduling/calendar.rs`

```rust
/// Schedule based on calendar roll policy (weekly, monthly, etc.)
pub struct CalendarScheduler {
    roll_policy: RollPolicy,
    end_date: NaiveDate,
}

impl CalendarScheduler {
    pub fn new(roll_policy: RollPolicy, end_date: NaiveDate) -> Self {
        Self { roll_policy, end_date }
    }
}

impl TradeScheduler for CalendarScheduler {
    fn next_entry_date(&self, from: NaiveDate) -> Option<NaiveDate> {
        let candidate = if TradingCalendar::is_trading_day(from) {
            from
        } else {
            TradingCalendar::next_trading_day(from)
        };

        if candidate >= self.end_date {
            None
        } else {
            Some(candidate)
        }
    }

    fn exit_date(&self, entry_date: NaiveDate) -> NaiveDate {
        self.roll_policy
            .next_roll_date(entry_date)
            .unwrap_or(self.end_date)
            .min(self.end_date)
    }

    fn description(&self) -> String {
        format!("Calendar: {}", self.roll_policy.description())
    }
}
```

### Task 4.3: Implement EarningsScheduler

**New file**: `cs-domain/src/scheduling/earnings.rs`

```rust
use std::sync::Arc;

/// Schedule based on earnings events
pub struct EarningsScheduler {
    earnings_repo: Arc<dyn EarningsRepository>,
    symbols: Option<Vec<String>>,
    entry_days_before: i32,
    exit_days_before: i32,
    end_date: NaiveDate,
    // Cache of upcoming earnings
    earnings_cache: Vec<EarningsEvent>,
}

impl EarningsScheduler {
    pub async fn new(
        earnings_repo: Arc<dyn EarningsRepository>,
        symbols: Option<Vec<String>>,
        entry_days_before: i32,
        exit_days_before: i32,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Self, String> {
        // Pre-load all earnings in range
        let lookahead = entry_days_before as i64 + 30; // Buffer
        let earnings_end = end_date + chrono::Duration::days(lookahead);

        let earnings_cache = earnings_repo
            .load_earnings(start_date, earnings_end, symbols.as_deref())
            .await
            .map_err(|e| e.to_string())?;

        Ok(Self {
            earnings_repo,
            symbols,
            entry_days_before,
            exit_days_before,
            end_date,
            earnings_cache,
        })
    }

    fn find_next_earnings(&self, from: NaiveDate) -> Option<&EarningsEvent> {
        // Entry date must be >= from
        // Entry = earnings - entry_days_before
        // So earnings >= from + entry_days_before

        let min_earnings_date = from + chrono::Duration::days(self.entry_days_before as i64);

        self.earnings_cache
            .iter()
            .filter(|e| e.earnings_date >= min_earnings_date)
            .filter(|e| e.earnings_date <= self.end_date)
            .min_by_key(|e| e.earnings_date)
    }
}

impl TradeScheduler for EarningsScheduler {
    fn next_entry_date(&self, from: NaiveDate) -> Option<NaiveDate> {
        let event = self.find_next_earnings(from)?;
        let entry = TradingCalendar::subtract_trading_days(
            event.earnings_date,
            self.entry_days_before as usize,
        );

        if entry >= from && entry < self.end_date {
            Some(entry)
        } else {
            None
        }
    }

    fn exit_date(&self, entry_date: NaiveDate) -> NaiveDate {
        // Find the earnings event this entry corresponds to
        let earnings_date = TradingCalendar::add_trading_days(
            entry_date,
            self.entry_days_before as usize,
        );

        TradingCalendar::subtract_trading_days(
            earnings_date,
            self.exit_days_before as usize,
        )
    }

    fn description(&self) -> String {
        format!(
            "Earnings: entry {} days before, exit {} days before",
            self.entry_days_before,
            self.exit_days_before
        )
    }
}
```

### Task 4.4: Implement MonthlyEarningsScheduler (Your Use Case)

**New file**: `cs-domain/src/scheduling/monthly_earnings.rs`

```rust
/// Monthly trades aligned with earnings cycles
///
/// For stocks with only monthly options, this scheduler:
/// 1. Enters after previous earnings (or campaign start)
/// 2. Exits before next earnings
/// 3. Rolls monthly if earnings are far apart
pub struct MonthlyEarningsScheduler {
    earnings_repo: Arc<dyn EarningsRepository>,
    symbol: String,
    entry_days_after_earnings: i32,  // Enter N days after previous earnings
    exit_days_before_earnings: i32,   // Exit N days before next earnings
    max_holding_days: i32,            // Roll if no earnings within this period
    end_date: NaiveDate,
    earnings_cache: Vec<EarningsEvent>,
}

impl MonthlyEarningsScheduler {
    pub async fn new(
        earnings_repo: Arc<dyn EarningsRepository>,
        symbol: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Self, String> {
        // Load earnings for entire period
        let earnings_cache = earnings_repo
            .load_earnings(
                start_date - chrono::Duration::days(30), // Look back for previous
                end_date + chrono::Duration::days(30),   // Look ahead for next
                Some(&[symbol.clone()]),
            )
            .await
            .map_err(|e| e.to_string())?;

        Ok(Self {
            earnings_repo,
            symbol,
            entry_days_after_earnings: 1,   // Day after earnings
            exit_days_before_earnings: 1,    // Day before next earnings
            max_holding_days: 30,            // Roll monthly if no earnings
            end_date,
            earnings_cache,
        })
    }

    /// Find previous earnings on or before date
    fn previous_earnings(&self, date: NaiveDate) -> Option<&EarningsEvent> {
        self.earnings_cache
            .iter()
            .filter(|e| e.earnings_date <= date)
            .max_by_key(|e| e.earnings_date)
    }

    /// Find next earnings after date
    fn next_earnings(&self, date: NaiveDate) -> Option<&EarningsEvent> {
        self.earnings_cache
            .iter()
            .filter(|e| e.earnings_date > date)
            .min_by_key(|e| e.earnings_date)
    }
}

impl TradeScheduler for MonthlyEarningsScheduler {
    fn next_entry_date(&self, from: NaiveDate) -> Option<NaiveDate> {
        // Find previous earnings to start after
        if let Some(prev) = self.previous_earnings(from) {
            let entry = TradingCalendar::add_trading_days(
                prev.earnings_date,
                self.entry_days_after_earnings as usize,
            );

            if entry >= from && entry < self.end_date {
                return Some(entry);
            }
        }

        // No previous earnings, start from beginning
        if TradingCalendar::is_trading_day(from) && from < self.end_date {
            Some(from)
        } else if from < self.end_date {
            Some(TradingCalendar::next_trading_day(from))
        } else {
            None
        }
    }

    fn exit_date(&self, entry_date: NaiveDate) -> NaiveDate {
        // Exit before next earnings OR after max_holding_days
        let max_exit = entry_date + chrono::Duration::days(self.max_holding_days as i64);

        if let Some(next) = self.next_earnings(entry_date) {
            let earnings_exit = TradingCalendar::subtract_trading_days(
                next.earnings_date,
                self.exit_days_before_earnings as usize,
            );

            earnings_exit.min(max_exit).min(self.end_date)
        } else {
            max_exit.min(self.end_date)
        }
    }

    fn description(&self) -> String {
        format!(
            "Monthly Earnings: {} (enter {} days after, exit {} days before)",
            self.symbol,
            self.entry_days_after_earnings,
            self.exit_days_before_earnings
        )
    }
}
```

---

## Phase 5: Unified BacktestOrchestrator

**Goal**: Single entry point for all backtest types

### Task 5.1: Create BacktestOrchestrator

**New file**: `cs-backtest/src/orchestrator.rs`

```rust
use std::sync::Arc;

/// Unified orchestrator for all backtest types
pub struct BacktestOrchestrator<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    trade_factory: Arc<dyn TradeFactory>,
    config: BacktestConfig,
}

impl<O, E> BacktestOrchestrator<O, E>
where
    O: OptionsDataRepository + 'static,
    E: EquityDataRepository + 'static,
{
    /// Run backtest with any trade type and scheduler
    pub async fn run<T, S>(
        &self,
        scheduler: S,
        entry_time: MarketTime,
        exit_time: MarketTime,
    ) -> BacktestResult
    where
        T: RollableTrade,
        S: TradeScheduler,
    {
        let unified_executor = UnifiedExecutor::new(
            Arc::clone(&self.options_repo),
            Arc::clone(&self.equity_repo),
        )
        .with_pricing_model(self.config.pricing_model)
        .with_hedge_config(self.config.hedge_config.clone());

        let rolling_executor = RollingExecutor::<T, O, E>::new(
            unified_executor,
            Arc::clone(&self.trade_factory),
            scheduler,  // Now uses TradeScheduler instead of RollPolicy
        );

        // Execute
        rolling_executor.execute(&self.config.symbols, entry_time, exit_time).await
    }

    /// Convenience: Rolling straddles with calendar schedule
    pub async fn rolling_straddles(
        &self,
        symbol: &str,
        roll_policy: RollPolicy,
    ) -> BacktestResult {
        let scheduler = CalendarScheduler::new(roll_policy, self.config.end_date);
        self.run::<Straddle, _>(scheduler, self.config.entry_time, self.config.exit_time).await
    }

    /// Convenience: Earnings-based straddles
    pub async fn earnings_straddles(
        &self,
        symbol: &str,
        entry_days_before: i32,
        exit_days_before: i32,
    ) -> BacktestResult {
        let scheduler = EarningsScheduler::new(
            self.config.earnings_repo.clone(),
            Some(vec![symbol.to_string()]),
            entry_days_before,
            exit_days_before,
            self.config.start_date,
            self.config.end_date,
        ).await.expect("Failed to create earnings scheduler");

        self.run::<Straddle, _>(scheduler, self.config.entry_time, self.config.exit_time).await
    }

    /// Convenience: Monthly earnings straddles (your use case!)
    pub async fn monthly_earnings_straddles(
        &self,
        symbol: &str,
    ) -> BacktestResult {
        let scheduler = MonthlyEarningsScheduler::new(
            self.config.earnings_repo.clone(),
            symbol.to_string(),
            self.config.start_date,
            self.config.end_date,
        ).await.expect("Failed to create monthly earnings scheduler");

        self.run::<Straddle, _>(scheduler, self.config.entry_time, self.config.exit_time).await
    }

    /// Rolling calendar spreads
    pub async fn rolling_calendars(
        &self,
        symbol: &str,
        roll_policy: RollPolicy,
    ) -> BacktestResult {
        let scheduler = CalendarScheduler::new(roll_policy, self.config.end_date);
        self.run::<CalendarSpread, _>(scheduler, self.config.entry_time, self.config.exit_time).await
    }
}
```

### Task 5.2: Update CLI to Use Orchestrator

**File**: `cs-cli/src/main.rs`

```rust
// BEFORE: Two separate paths
if let Some(roll_policy) = roll_policy_opt {
    run_rolling_straddle(...).await?;
} else {
    BacktestUseCase::new(...).execute(...).await?;
}

// AFTER: Single path through orchestrator
let orchestrator = BacktestOrchestrator::new(
    options_repo,
    equity_repo,
    trade_factory,
    config,
);

let result = match (spread_type, scheduling_mode) {
    // Rolling straddles
    (SpreadType::Straddle, Scheduling::Calendar(policy)) => {
        orchestrator.rolling_straddles(&symbol, policy).await
    }

    // Earnings-based straddles (current standard)
    (SpreadType::Straddle, Scheduling::Earnings { entry_days, exit_days }) => {
        orchestrator.earnings_straddles(&symbol, entry_days, exit_days).await
    }

    // Monthly earnings straddles (NEW!)
    (SpreadType::Straddle, Scheduling::MonthlyEarnings) => {
        orchestrator.monthly_earnings_straddles(&symbol).await
    }

    // Rolling calendars (NEW!)
    (SpreadType::Calendar, Scheduling::Calendar(policy)) => {
        orchestrator.rolling_calendars(&symbol, policy).await
    }

    // ... other combinations
};
```

---

## Phase 6: Testing & Migration

### Task 6.1: Unit Tests for RollableTrade

```rust
#[tokio::test]
async fn test_straddle_implements_rollable_trade() {
    let factory = MockTradeFactory::new();
    let straddle = Straddle::create(
        &factory,
        "AAPL",
        Utc::now(),
        NaiveDate::from_ymd_opt(2025, 4, 1).unwrap(),
    ).await.unwrap();

    assert_eq!(straddle.symbol(), "AAPL");
    assert!(straddle.expiration() > Utc::now().date_naive());
}

#[tokio::test]
async fn test_calendar_spread_implements_rollable_trade() {
    let factory = MockTradeFactory::new();
    let spread = CalendarSpread::create(
        &factory,
        "AAPL",
        Utc::now(),
        NaiveDate::from_ymd_opt(2025, 4, 1).unwrap(),
    ).await.unwrap();

    assert_eq!(spread.symbol(), "AAPL");
}
```

### Task 6.2: Integration Tests for Schedulers

```rust
#[tokio::test]
async fn test_earnings_scheduler_finds_next_entry() {
    let earnings = vec![
        EarningsEvent::new("PENG", date(2025, 3, 15), AMC),
        EarningsEvent::new("PENG", date(2025, 6, 15), AMC),
    ];
    let repo = MockEarningsRepo::with_events(earnings);

    let scheduler = EarningsScheduler::new(
        Arc::new(repo),
        Some(vec!["PENG".to_string()]),
        5,  // entry_days_before
        1,  // exit_days_before
        date(2025, 1, 1),
        date(2025, 12, 31),
    ).await.unwrap();

    // From Jan 1, next entry should be 5 days before Mar 15
    let entry = scheduler.next_entry_date(date(2025, 1, 1)).unwrap();
    assert_eq!(entry, date(2025, 3, 10)); // Approximately
}

#[tokio::test]
async fn test_monthly_earnings_scheduler() {
    // Similar test for MonthlyEarningsScheduler
}
```

### Task 6.3: Backward Compatibility Tests

```rust
#[tokio::test]
async fn test_old_rolling_straddle_executor_still_works() {
    // Ensure deprecated type alias works
    #[allow(deprecated)]
    let executor: RollingStraddleExecutor<_, _> = RollingExecutor::new(...);

    let result = executor.execute_rolling("PENG", ...).await;
    assert!(result.num_rolls > 0);
}
```

### Task 6.4: Migration Checklist

- [ ] All 09:35 defaults replaced with `MarketTime::DEFAULT_ENTRY`
- [ ] Saturday bug fixed with test coverage
- [ ] `RollableTrade` trait implemented for all trade types
- [ ] `TradeFactory` extended with new methods
- [ ] `RollingExecutor<T>` generic executor working
- [ ] `RollingStraddleExecutor` deprecated and aliased
- [ ] `TradeScheduler` trait and implementations complete
- [ ] `BacktestOrchestrator` unified entry point complete
- [ ] CLI updated to use orchestrator
- [ ] All existing tests passing
- [ ] New integration tests added
- [ ] Documentation updated

---

## CLI Interface (Final State)

```bash
# Rolling straddles (calendar-based)
cs backtest --symbols PENG --spread straddle --roll-strategy weekly

# Earnings straddles (current standard)
cs backtest --symbols PENG --spread straddle --entry-days 5 --exit-days 1

# Monthly earnings straddles (NEW - your use case!)
cs backtest --symbols PENG --spread straddle --schedule monthly-earnings

# Rolling calendar spreads (NEW!)
cs backtest --symbols AAPL --spread calendar --roll-strategy monthly

# Rolling iron butterflies (NEW!)
cs backtest --symbols SPY --spread iron-butterfly --roll-strategy weekly
```

---

## Estimated Effort

| Phase | Tasks | Complexity | Dependencies |
|-------|-------|------------|--------------|
| Phase 1 | 1.1-1.3 | Low | None |
| Phase 2 | 2.1-2.4 | Medium | Phase 1 |
| Phase 3 | 3.1-3.3 | Medium | Phase 2 |
| Phase 4 | 4.1-4.4 | Medium | Phase 2 |
| Phase 5 | 5.1-5.2 | High | Phase 3, 4 |
| Phase 6 | 6.1-6.4 | Medium | Phase 5 |

**Recommended approach**: Complete Phase 1 first (bug fixes), then Phase 2-4 can be done in parallel, Phase 5 integrates everything, Phase 6 validates.

---

## Appendix: File Change Summary

### New Files
- `cs-domain/src/trade/rollable.rs`
- `cs-domain/src/scheduling/mod.rs`
- `cs-domain/src/scheduling/scheduler.rs`
- `cs-domain/src/scheduling/calendar.rs`
- `cs-domain/src/scheduling/earnings.rs`
- `cs-domain/src/scheduling/monthly_earnings.rs`
- `cs-backtest/src/rolling_executor.rs`
- `cs-backtest/src/orchestrator.rs`

### Modified Files
- `cs-domain/src/datetime.rs` - Already done
- `cs-domain/src/value_objects.rs` - Use MarketTime constants
- `cs-domain/src/timing/*.rs` - Use MarketTime constants
- `cs-domain/src/trade/factory.rs` - Extend trait
- `cs-domain/src/trade/straddle.rs` - Implement RollableTrade
- `cs-domain/src/trade/calendar_spread.rs` - Implement RollableTrade
- `cs-backtest/src/rolling_straddle_executor.rs` - Deprecate, alias
- `cs-backtest/src/lib.rs` - Export new modules
- `cs-cli/src/main.rs` - Use orchestrator

### Deleted Files (after migration)
- None (maintain backwards compatibility)
