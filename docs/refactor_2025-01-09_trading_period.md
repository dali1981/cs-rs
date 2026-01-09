# Trading Period Refactoring Plan

**Date**: 2025-01-09
**Focus**: Backtest execution model - trading-period-centric approach
**Scope**: cs-backtest, cs-domain (not campaign unification yet)

---

## 1. Current State (After Recent Refactoring)

### What's Been Done

```
Strategy structs are now lean:
├── CalendarSpreadStrategy { timing, option_type }
├── IronButterflyStrategy { timing, wing_width }
├── StraddleStrategy { timing }
├── PostEarningsStraddleStrategy { timing }
└── CalendarStraddleStrategy { timing }

Timing factories exist:
├── TimingStrategy::for_earnings(config)
├── TimingStrategy::for_straddle(config, entry_days, exit_days)
└── TimingStrategy::for_post_earnings(config, holding_days)

Concerns extracted:
├── Validation → ExecutionConfig, passed to execute_trade()
└── Filters → min_iv_ratio passed to apply_filter()
```

### Current Weakness in Backtest

```rust
// backtest_use_case.rs - iterates by DATE, not by TRADE
for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
    let events = load_events_for_date_with_lookahead(...);
    // process events for this date
}
```

**Problems**:
1. Date-centric, not trade-centric
2. Lookahead logic is ad-hoc per strategy
3. Event discovery doesn't account for timing spec properly

---

## 2. Target Model

### Core Concept

**TradingPeriod** = window during which we INITIATE trades (not when events occur)

```
Given:
  - TradingPeriod: Jan 1 - Jan 31
  - TimingSpec: PreEarnings { entry_days_before: 14 }
  - Event: AAPL earnings Feb 15

Resolution:
  - entry_date = Feb 15 - 14 trading days ≈ Jan 27
  - Jan 27 ∈ [Jan 1, Jan 31] ✓
  - → Include this event, trade starts Jan 27
```

### New Flow

```
┌─────────────────────────────────────────────────────────────────┐
│  1. TradingPeriod (start, end)                                  │
│     "When do we want to INITIATE trades?"                       │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  2. TimingSpec.event_search_range(trading_period)               │
│     "What event dates should we look at?"                       │
│                                                                 │
│     PreEarnings(14 days) + period [Jan 1-31]                    │
│       → search events [Jan 15 - Feb 28] (entry could be Jan 1+) │
│                                                                 │
│     PostEarnings(5 days) + period [Jan 1-31]                    │
│       → search events [Dec 25 - Jan 30] (entry is day after)    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  3. Load events in search range                                 │
│     earnings_repo.load_range(search_start, search_end)          │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  4. TradingPeriod.discover_tradable_events(events, timing)      │
│     For each event:                                             │
│       entry_date = timing.resolve_entry(event)                  │
│       if entry_date ∈ trading_period → include                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  5. Apply FilterCriteria (symbols, market_cap, iv, etc.)        │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  6. Execute trade-by-trade (not date-by-date)                   │
│     for tradable_event in filtered_events {                     │
│         simulate_trade(tradable_event)                          │
│     }                                                           │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. Domain Types to Add/Modify

### 3.1 TradingPeriod (new)

```rust
// cs-domain/src/trading_period/period.rs

/// When we want to INITIATE trades
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TradingPeriod {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl TradingPeriod {
    pub fn new(start: NaiveDate, end: NaiveDate) -> Self {
        Self { start, end }
    }

    pub fn contains(&self, date: NaiveDate) -> bool {
        date >= self.start && date <= self.end
    }

    /// Discover events whose resolved entry date falls in this period
    pub fn discover_tradable_events(
        &self,
        events: &[EarningsEvent],
        timing: &TradingPeriodSpec,
    ) -> Vec<TradableEvent> {
        events
            .iter()
            .filter_map(|event| {
                let resolved = timing.build(Some(event)).ok()?;
                if self.contains(resolved.entry_date) {
                    Some(TradableEvent {
                        event: event.clone(),
                        entry_date: resolved.entry_date,
                        exit_date: resolved.exit_date,
                        entry_time: resolved.entry_time,
                        exit_time: resolved.exit_time,
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}
```

### 3.2 TradableEvent (new)

```rust
// cs-domain/src/trading_period/tradable_event.rs

/// An event resolved to concrete trading dates
#[derive(Debug, Clone)]
pub struct TradableEvent {
    pub event: EarningsEvent,
    pub entry_date: NaiveDate,
    pub exit_date: NaiveDate,
    pub entry_time: NaiveTime,
    pub exit_time: NaiveTime,
}

impl TradableEvent {
    pub fn entry_datetime(&self) -> DateTime<Utc> {
        // Combine date + time → UTC datetime
    }

    pub fn exit_datetime(&self) -> DateTime<Utc> {
        // Combine date + time → UTC datetime
    }

    pub fn symbol(&self) -> &str {
        &self.event.symbol
    }
}
```

### 3.3 TradingPeriodSpec - Add event_search_range()

```rust
// cs-domain/src/trading_period/spec.rs (add method)

impl TradingPeriodSpec {
    /// Calculate the event date range to search given a trading period
    ///
    /// Returns (search_start, search_end) for events whose entry would
    /// fall within the trading period.
    pub fn event_search_range(&self, period: &TradingPeriod) -> (NaiveDate, NaiveDate) {
        match self {
            Self::PreEarnings { entry_days_before, .. } => {
                // Entry is N days BEFORE event
                // To have entry in [period.start, period.end]:
                //   event_date - N = entry_date ∈ period
                //   event_date ∈ [period.start + N, period.end + N]
                let buffer = (*entry_days_before as i64 * 7 / 5) + 5; // trading→calendar
                let search_start = period.start + Duration::days(buffer);
                let search_end = period.end + Duration::days(buffer + 30);
                (search_start, search_end)
            }

            Self::PostEarnings { entry_offset, .. } => {
                // Entry is day(s) AFTER event
                // To have entry in period:
                //   event_date + offset = entry_date ∈ period
                //   event_date ∈ [period.start - offset - buffer, period.end]
                let buffer = 5; // safety margin
                let search_start = period.start - Duration::days(*entry_offset as i64 + buffer);
                let search_end = period.end;
                (search_start, search_end)
            }

            Self::CrossEarnings { entry_days_before, .. } => {
                // Similar to PreEarnings
                let buffer = (*entry_days_before as i64 * 7 / 5) + 5;
                let search_start = period.start + Duration::days(buffer);
                let search_end = period.end + Duration::days(buffer + 30);
                (search_start, search_end)
            }

            Self::FixedDates { .. } | Self::HoldingPeriod { .. } => {
                // No event dependency
                (period.start, period.end)
            }
        }
    }
}
```

---

## 4. Config Separation

### 4.1 Current BacktestConfig (mixed concerns)

```rust
pub struct BacktestConfig {
    // Data sources (infrastructure)
    pub data_dir: PathBuf,
    pub earnings_dir: PathBuf,
    pub earnings_file: Option<PathBuf>,

    // Trading period (domain)
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,

    // Timing (domain) - scattered
    pub timing: TimingConfig,  // entry/exit hours
    pub straddle_entry_days: usize,
    pub straddle_exit_days: usize,
    pub post_earnings_holding_days: usize,

    // Position spec (domain) - scattered
    pub spread: SpreadType,
    pub selection_strategy: SelectionType,
    pub wing_width: f64,
    pub target_delta: f64,
    // ...

    // Filters (domain) - scattered
    pub symbols: Option<Vec<String>>,
    pub min_market_cap: Option<u64>,
    pub max_entry_iv: Option<f64>,
    pub min_notional: Option<f64>,
    // ...

    // Execution (runtime)
    pub parallel: bool,
}
```

### 4.2 Proposed Structure

```rust
// cs-backtest/src/config.rs

pub struct BacktestConfig {
    /// When to initiate trades
    pub period: TradingPeriod,

    /// How to time entry/exit relative to events
    pub timing_spec: TradingPeriodSpec,

    /// What position structure to build
    pub position: PositionSpec,

    /// Which events to include
    pub filter: FilterCriteria,

    /// Data source paths
    pub data_source: DataSourceConfig,

    /// Execution options
    pub execution: ExecutionConfig,
}
```

```rust
// cs-domain/src/config/position_spec.rs

/// What option structure to trade
#[derive(Debug, Clone)]
pub struct PositionSpec {
    pub structure: SpreadType,        // Calendar, IB, Straddle...
    pub selection: SelectionType,     // ATM, Delta, DeltaScan
    pub direction: TradeDirection,    // Long, Short
    pub expiration_policy: ExpirationPolicy,

    // Structure-specific params
    pub wing_width: Option<f64>,      // IB only
    pub target_delta: Option<f64>,    // Delta selection only
    pub delta_range: Option<(f64, f64)>,  // DeltaScan only
}
```

```rust
// cs-domain/src/config/filter_criteria.rs

/// Criteria for filtering tradable events
#[derive(Debug, Clone, Default)]
pub struct FilterCriteria {
    pub symbols: Option<Vec<String>>,
    pub min_market_cap: Option<u64>,
    pub max_entry_iv: Option<f64>,
    pub min_notional: Option<f64>,
    pub min_entry_price: Option<f64>,
    pub max_entry_price: Option<f64>,
}

impl FilterCriteria {
    pub fn matches(&self, event: &TradableEvent, market_data: &MarketData) -> bool {
        // Check all criteria
    }
}
```

```rust
// cs-backtest/src/config/data_source.rs (infrastructure)

pub struct DataSourceConfig {
    pub data_dir: PathBuf,
    pub earnings_dir: PathBuf,
    pub earnings_file: Option<PathBuf>,
}

pub struct ExecutionConfig {
    pub parallel: bool,
}
```

---

## 5. Refactored BacktestUseCase

```rust
// cs-backtest/src/backtest_use_case.rs

impl BacktestUseCase {
    pub async fn execute(&self) -> Result<Vec<TradeResult>> {
        // 1. Determine event search range based on timing
        let (search_start, search_end) = self.config.timing_spec
            .event_search_range(&self.config.period);

        // 2. Load events in that range
        let all_events = self.earnings_repo
            .load_range(search_start, search_end)?;

        // 3. Discover tradable events (entry date in period)
        let tradable = self.config.period
            .discover_tradable_events(&all_events, &self.config.timing_spec);

        // 4. Apply filters
        let filtered: Vec<_> = tradable
            .into_iter()
            .filter(|e| self.config.filter.matches_event(e))
            .collect();

        // 5. Sort by entry date (trade execution order)
        let mut sorted = filtered;
        sorted.sort_by_key(|e| e.entry_date);

        // 6. Execute trade-by-trade
        let strategy = self.create_strategy();
        let exec_config = self.create_execution_config();

        let results = if self.config.execution.parallel {
            self.execute_parallel(&sorted, &strategy, &exec_config).await
        } else {
            self.execute_sequential(&sorted, &strategy, &exec_config).await
        };

        results
    }

    async fn execute_sequential(
        &self,
        events: &[TradableEvent],
        strategy: &dyn TradeStrategy,
        exec_config: &ExecutionConfig,
    ) -> Result<Vec<TradeResult>> {
        let mut results = Vec::new();

        for tradable in events {
            let result = strategy.execute_trade(
                &tradable.event,
                tradable.entry_datetime(),
                tradable.exit_datetime(),
                exec_config,
                &self.repos,
            ).await;

            // Apply post-trade filter
            if let Ok(ref trade) = result {
                if strategy.apply_filter(trade, &self.config.filter) {
                    results.push(result?);
                }
            }
        }

        Ok(results)
    }
}
```

---

## 6. Implementation Steps

### Phase 1: Domain Types (cs-domain)

- [ ] Add `TradingPeriod` struct in `cs-domain/src/trading_period/period.rs`
- [ ] Add `TradableEvent` struct in `cs-domain/src/trading_period/tradable_event.rs`
- [ ] Add `event_search_range()` method to `TradingPeriodSpec`
- [ ] Add `discover_tradable_events()` method to `TradingPeriod`
- [ ] Add `FilterCriteria` struct in `cs-domain/src/config/filter_criteria.rs`
- [ ] Add `PositionSpec` struct in `cs-domain/src/config/position_spec.rs`
- [ ] Update `cs-domain/src/lib.rs` exports

### Phase 2: Backtest Config Restructure (cs-backtest)

- [ ] Create `DataSourceConfig` and `ExecutionConfig` structs
- [ ] Refactor `BacktestConfig` to compose the new types
- [ ] Update `BacktestConfig::default()` and builders
- [ ] Update CLI argument parsing to build new config structure

### Phase 3: BacktestUseCase Refactor (cs-backtest)

- [ ] Replace date-iteration with trade-iteration
- [ ] Use `TradingPeriod.discover_tradable_events()` for event discovery
- [ ] Use `TradingPeriodSpec.event_search_range()` for loading
- [ ] Update strategy execution to use `TradableEvent`

### Phase 4: Strategy Cleanup (cs-backtest)

- [ ] Ensure strategies use passed timing (already done via factories)
- [ ] Remove any remaining hardcoded timing logic
- [ ] Ensure `execute_trade` works with `TradableEvent` entry/exit times

---

## 7. Campaign Considerations (Future)

Campaign already uses a trade-centric model:
- `TradingCampaign` → generates `TradingSession` objects
- `SessionExecutor` executes sessions

**Not unifying yet** because:
1. Campaign has rolling/inter-earnings complexity
2. Different execution model (session-based vs trade-based)
3. Different result types

Future alignment could share:
- `TradingPeriod` for date ranges
- `TradingPeriodSpec` for timing resolution
- `FilterCriteria` for event filtering

But campaign uses `TradingSession` as its unit, backtest will use `TradableEvent`.
Both are valid - TradingSession is richer (includes action, context for rolling).

---

## 8. Notes

- Keep `TimingStrategy` factories (for_earnings, for_straddle, etc.) - they work well
- `TradingPeriodSpec` is the better abstraction for timing resolution
- `TimingStrategy` wraps the concrete timing implementations for backtest execution
- Don't break existing CLI - config restructure should be internal

---

## 9. Open Questions

1. Should `TradableEvent` include the resolved `TradingPeriod` or just dates?
2. Should `FilterCriteria.matches()` need market data, or just event metadata?
3. How to handle events that span trading period boundary (entry in, exit out)?

---

*This plan focuses on backtest. Campaign alignment is a separate future effort.*
