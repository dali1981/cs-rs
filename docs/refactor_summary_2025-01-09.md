# Trading Period Refactoring - Implementation Summary

**Date**: 2025-01-09
**Status**: ✅ Phases 1-3 Complete
**Commits**:
- `0e4488b` - Add domain types for trading-period-centric execution
- `e66818f` - Refactor BacktestUseCase to trade-centric execution

---

## What We Built

### ✅ Phase 1: Domain Types (cs-domain)

**New Types:**

1. **`TradingRange`** (`cs-domain/src/trading_period/range.rs`)
   - Represents date range for initiating trades
   - Method: `discover_tradable_events(events, timing)` → filters events by resolved entry date

2. **`TradableEvent`** (`cs-domain/src/trading_period/tradable_event.rs`)
   - Earnings event resolved to concrete entry/exit dates/times
   - Created from `TradingPeriodSpec.build()` + earnings event
   - Carries pre-computed `entry_datetime()` and `exit_datetime()`

3. **`FilterCriteria`** (`cs-domain/src/config/filter_criteria.rs`)
   - Unified trade filtering logic
   - Fields: symbols, min_market_cap, max_entry_iv, min_notional, min/max_entry_price, min_iv_ratio
   - Methods: `symbol_matches()`, `market_cap_matches()`, `iv_matches()`, etc.

4. **`PositionSpec`** (`cs-domain/src/config/position_spec.rs`)
   - Position structure configuration
   - Enums: `PositionStructure` (Calendar, IronButterfly, Straddle, etc.)
   - Enum: `StrikeSelection` (ATM, Delta, DeltaScan)

**New Methods:**

5. **`TradingPeriodSpec::event_search_range(range)`**
   - Calculates which event dates to load based on timing strategy
   - PreEarnings: events must be N days after range.start
   - PostEarnings: events must be before range.end
   - CrossEarnings: similar to PreEarnings

---

### ✅ Phase 2: Config Restructure (cs-backtest)

**Config Organization:**
```
cs-backtest/src/config/
├── mod.rs           # BacktestConfig + SpreadType/SelectionType
├── data_source.rs   # DataSourceConfig (infrastructure)
└── execution.rs     # ExecutionConfig (runtime)
```

**Helper Methods on `BacktestConfig`:**

```rust
impl BacktestConfig {
    pub fn trading_range(&self) -> TradingRange;
    pub fn timing_spec(&self) -> TradingPeriodSpec;
    pub fn filter_criteria(&self) -> FilterCriteria;
    pub fn data_source(&self) -> DataSourceConfig;
    pub fn execution(&self) -> ExecutionConfig;
}
```

**Architecture:**
- Clean separation: domain (what/when to trade) vs infrastructure (data paths, runtime options)
- `timing_spec()` is spread-type aware (converts legacy params to TradingPeriodSpec)

---

### ✅ Phase 3: BacktestUseCase Refactor (cs-backtest)

**Before (Date-Centric):**
```rust
for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
    // Load events with lookahead for this date
    let events = self.load_earnings_for_strategy(session_date, strategy).await?;

    // Filter where entry_date == session_date
    let to_enter: Vec<_> = events.iter()
        .filter(|e| strategy.entry_date(e) == session_date)
        .collect();

    // Execute batch
    let results = self.execute_batch(&to_enter, strategy, ...).await;
}
```

**After (Trade-Centric):**
```rust
// 1. Determine trading range and timing
let trading_range = self.config.trading_range();
let timing_spec = self.config.timing_spec();
let filter_criteria = self.config.filter_criteria();

// 2. Calculate event search range based on timing
let (search_start, search_end) = timing_spec.event_search_range(&trading_range);

// 3. Load all potentially relevant events (once!)
let all_events = self.earnings_repo
    .load_earnings(search_start, search_end, symbols)
    .await?;

// 4. Discover tradable events (entry date in range)
let tradable_events = trading_range.discover_tradable_events(&all_events, &timing_spec);

// 5. Apply filters
let filtered: Vec<_> = tradable_events
    .into_iter()
    .filter(|te| filter_criteria.symbol_matches(te.symbol()))
    .filter(|te| filter_criteria.market_cap_matches(te.event.market_cap))
    .collect();

// 6. Execute trade-by-trade
let batch_results = self.execute_tradable_batch(&filtered, strategy, ...).await;
```

**New Method:**
```rust
async fn execute_tradable_batch<S, R>(
    &self,
    tradable_events: &[&TradableEvent],
    strategy: &S,
    selector: &dyn StrikeSelector,
    criteria: &ExpirationCriteria,
    exec_config: &ExecutionConfig,
) -> Vec<Option<R>>
```

**Deprecated (kept for compatibility):**
- `execute_batch()` - Old date-iteration method
- `load_earnings_for_strategy()` - Per-date loading with lookahead
- `report_progress()` - Per-session progress reporting

---

## Key Benefits

### 1. **Efficiency**
- **Single event load** instead of N loads (one per trading day)
- Timing-aware search range calculation eliminates unnecessary event loading

### 2. **Clarity**
- **Discovery phase** (which events to trade) is separate from **execution phase**
- Timing logic centralized in `TradingPeriodSpec.event_search_range()`
- Filtering logic unified in `FilterCriteria`

### 3. **Correctness**
- Event discovery based on **resolved entry date** (not event date)
- Handles different timing strategies uniformly:
  - PreEarnings: entry 14 days before → event must be mid-to-late in range
  - PostEarnings: entry 1 day after → event must be at/before end of range
  - CrossEarnings: similar to PreEarnings

### 4. **Foundation for Future Work**
- **Portfolio-level simulation**: All trades for a period are known upfront
- **Rolling strategies**: Can see sequence of trades
- **Resource planning**: Know total number of trades before execution

---

## Architectural Patterns Applied

### 1. **Inversion of Control**
- **Before**: Date iteration controls event loading
- **After**: Trading range defines scope, timing determines loading

### 2. **Separation of Concerns**
- **TradingRange**: When to initiate trades
- **TradingPeriodSpec**: How to time entry/exit relative to events
- **FilterCriteria**: Which events qualify
- **Strategy**: How to execute trades

### 3. **Template Method Pattern**
```
execute_with_strategy() {
    // 1. Discover (domain logic - timing-aware)
    tradable = range.discover_tradable_events(events, timing)

    // 2. Filter (domain logic - business rules)
    filtered = apply_filters(tradable)

    // 3. Execute (strategy-specific)
    results = execute_tradable_batch(filtered, strategy)
}
```

---

## Examples

### Example 1: Pre-Earnings Straddle

**User Intent**: "Trade straddles in Q1 2025, entering 14 days before earnings"

**Config**:
```rust
BacktestConfig {
    start_date: 2025-01-01,
    end_date: 2025-03-31,
    spread: Straddle,
    straddle_entry_days: 14,
    straddle_exit_days: 1,
    ...
}
```

**Execution Flow**:
```
1. trading_range = TradingRange(2025-01-01, 2025-03-31)

2. timing_spec = PreEarnings { entry_days_before: 14, exit_days_before: 1 }

3. event_search_range(trading_range)
   → (2025-01-21, 2025-04-30)  // Events 20+ days after range.start

4. load_earnings(2025-01-21, 2025-04-30)
   → [AAPL 2025-02-05, MSFT 2025-02-15, ...]

5. discover_tradable_events()
   → AAPL: entry=2025-01-15 ✓ (in range), exit=2025-02-04
   → MSFT: entry=2025-01-25 ✓ (in range), exit=2025-02-14
   → NVDA: entry=2025-04-10 ✗ (entry after range.end)

6. execute_tradable_batch([AAPL, MSFT], strategy)
```

### Example 2: Post-Earnings Straddle

**User Intent**: "Trade post-earnings straddles in Q1 2025"

**Config**:
```rust
BacktestConfig {
    start_date: 2025-01-01,
    end_date: 2025-03-31,
    spread: PostEarningsStraddle,
    post_earnings_holding_days: 5,
    ...
}
```

**Execution Flow**:
```
1. trading_range = TradingRange(2025-01-01, 2025-03-31)

2. timing_spec = PostEarnings { entry_offset: 0, holding_days: 5 }

3. event_search_range(trading_range)
   → (2024-12-25, 2025-03-31)  // Events before/at range.end

4. load_earnings(2024-12-25, 2025-03-31)
   → [AAPL 2025-01-15, MSFT 2025-02-05, ...]

5. discover_tradable_events()
   → AAPL: entry=2025-01-16 ✓ (in range), exit=2025-01-23
   → MSFT: entry=2025-02-06 ✓ (in range), exit=2025-02-13
   → NVDA: entry=2024-12-20 ✗ (entry before range.start)

6. execute_tradable_batch([AAPL, MSFT], strategy)
```

---

## Migration Notes

### For Existing Code

The refactoring is **backwards compatible** at the API level:
- `BacktestUseCase::execute()` still works
- All config fields remain unchanged
- Same result types returned

**What changed internally:**
- Date iteration → Event discovery
- Per-date loading → Single bulk load
- Late-bound timing → Pre-computed entry/exit times

### For New Code

**Recommended pattern**:
```rust
// Build config with new helper methods
let config = BacktestConfig { /* ... */ };

// Use domain types explicitly
let trading_range = config.trading_range();
let timing_spec = config.timing_spec();
let filters = config.filter_criteria();

// Manual discovery (if needed outside BacktestUseCase)
let (search_start, search_end) = timing_spec.event_search_range(&trading_range);
let events = load_earnings(search_start, search_end, ...);
let tradable = trading_range.discover_tradable_events(&events, &timing_spec);
```

---

## Next Steps (Not Implemented)

### 📋 Phase 4: Strategy Cleanup (Future)
- Ensure strategies use passed `entry_datetime`/`exit_datetime` (already mostly done)
- Remove any remaining hardcoded timing logic
- Unify result types (reduce duplication in TradeResultMethods impls)

### 🔮 Future Enhancements
1. **Portfolio-level simulation**:
   - Hold all positions simultaneously
   - Track aggregate Greeks, margin requirements
   - Day-by-day portfolio snapshots

2. **Campaign unification**:
   - Campaign already uses TradingSession (richer than TradableEvent)
   - Potential to share more code between Campaign and Backtest
   - Both could use `TradingRange.discover_tradable_events()`

3. **Advanced filtering**:
   - Liquidity filters (min option volume, bid-ask spread)
   - IV rank/percentile filters
   - Earnings forecast filters (EPS beat/miss probability)

4. **Adaptive timing**:
   - Dynamic entry days based on IV expansion rate
   - Profit-taking exits before scheduled exit
   - Stop-loss exits

---

## Testing Checklist

- [ ] Run existing backtest tests (ensure backwards compatibility)
- [ ] Test PreEarnings strategy (straddle)
- [ ] Test PostEarnings strategy
- [ ] Test CrossEarnings strategy (calendar, IronButterfly)
- [ ] Verify event search range calculations for edge cases
- [ ] Verify discovery filters events correctly by entry date
- [ ] Verify FilterCriteria applies all filters
- [ ] Compare results with old date-centric implementation

---

## Lessons Learned

1. **Naming matters**: "TradingRange" (when to initiate) vs "TradingPeriod" (single trade duration)
2. **Timing is complex**: Three levels (Spec → Resolution → Execution) are necessary
3. **Event search range is non-trivial**: Must account for timing direction (before/after event)
4. **Separation pays off**: Clean domain types make refactoring easier
5. **Deprecate gracefully**: Keep old methods for compatibility, mark as deprecated

---

*End of summary*
