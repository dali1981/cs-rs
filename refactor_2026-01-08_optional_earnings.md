# Refactoring: Make EarningsEvent Optional in Execution

**Date**: 2026-01-08
**Goal**: Eliminate dummy `EarningsEvent` in rolling trades by making earnings context optional

## Current Problem

`TradeExecutor::execute_rolling()` creates dummy earnings events for non-earnings trades:

```rust
// trade_executor.rs:264-269 - THE HACK
let event = EarningsEvent::new(
    symbol.to_string(),
    exit_dt.date_naive(),
    EarningsTime::AfterMarketClose,  // ❌ Meaningless for rolling
);
```

This pollutes result types with meaningless earnings data and violates domain integrity.

## Existing Infrastructure

We already have `SessionContext` (domain/campaign/session.rs:54-77):

```rust
pub enum SessionContext {
    /// Session anchored to an earnings event
    Earnings {
        event: EarningsEvent,
        timing_type: EarningsTimingType,
    },
    /// Session between two earnings dates
    InterEarnings {
        roll_number: u16,
        earnings_before: NaiveDate,
        earnings_after: NaiveDate,
    },
    /// Standalone session (no earnings reference)
    Standalone {
        note: Option<String>,
    },
}
```

This is the right abstraction but not used in execution layer.

## Solution Architecture

### Phase 1: Make Earnings Fields Optional in Result Types

All result types currently have required earnings fields:

```rust
// BEFORE (all 8 result types)
pub struct StraddleResult {
    pub symbol: String,
    pub earnings_date: NaiveDate,        // ❌ Required
    pub earnings_time: EarningsTime,     // ❌ Required
    // ...
}
```

**Change to:**

```rust
// AFTER
pub struct StraddleResult {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earnings_date: Option<NaiveDate>,    // ✅ Optional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earnings_time: Option<EarningsTime>, // ✅ Optional
    // ...
}
```

**Affected Result Types** (8 total):
- `CalendarSpreadResult` (entities.rs:770)
- `IronButterflyResult` (entities.rs:879)
- `StraddleResult` (entities.rs:987)
- `CalendarStraddleResult` (entities.rs:1075)
- `StrangleResult` (entities.rs:1169)
- `ButterflyResult` (entities.rs:1256)
- `CondorResult` (entities.rs:1330)
- `IronCondorResult` (entities.rs:1405)

### Phase 2: Update ExecutableTrade Trait

**Current trait signatures:**

```rust
// execution/traits.rs:71-87
trait ExecutableTrade {
    fn to_result(
        &self,
        entry_pricing: Self::Pricing,
        exit_pricing: Self::Pricing,
        output: &SimulationOutput,
        event: &EarningsEvent,  // ❌ Always required
    ) -> Self::Result;

    fn to_failed_result(
        &self,
        output: &SimulationOutput,
        event: &EarningsEvent,  // ❌ Always required
        error: ExecutionError,
    ) -> Self::Result;
}
```

**Change to:**

```rust
trait ExecutableTrade {
    fn to_result(
        &self,
        entry_pricing: Self::Pricing,
        exit_pricing: Self::Pricing,
        output: &SimulationOutput,
        event: Option<&EarningsEvent>,  // ✅ Optional
    ) -> Self::Result;

    fn to_failed_result(
        &self,
        output: &SimulationOutput,
        event: Option<&EarningsEvent>,  // ✅ Optional
        error: ExecutionError,
    ) -> Self::Result;
}
```

### Phase 3: Update All ExecutableTrade Implementations

**Affected files** (8 implementations):
- `execution/straddle_impl.rs`
- `execution/calendar_spread_impl.rs`
- `execution/iron_butterfly_impl.rs`
- `execution/calendar_straddle_impl.rs`
- `execution/strangle_impl.rs`
- `execution/butterfly_impl.rs`
- `execution/condor_impl.rs`
- `execution/iron_condor_impl.rs`

**Pattern for each impl:**

```rust
// BEFORE
fn to_result(
    &self,
    entry_pricing: CompositePricing,
    exit_pricing: CompositePricing,
    output: &SimulationOutput,
    event: &EarningsEvent,
) -> StraddleResult {
    StraddleResult {
        symbol: self.symbol().to_string(),
        earnings_date: event.earnings_date,  // ❌ Always present
        earnings_time: event.earnings_time,
        // ...
    }
}

// AFTER
fn to_result(
    &self,
    entry_pricing: CompositePricing,
    exit_pricing: CompositePricing,
    output: &SimulationOutput,
    event: Option<&EarningsEvent>,  // ✅ Optional
) -> StraddleResult {
    StraddleResult {
        symbol: self.symbol().to_string(),
        earnings_date: event.map(|e| e.earnings_date),  // ✅ Optional
        earnings_time: event.map(|e| e.earnings_time),
        // ...
    }
}
```

### Phase 4: Update TradeExecutor

**File**: `cs-backtest/src/trade_executor.rs`

#### 4a. Change `execute()` signature

```rust
// BEFORE (line 153)
pub async fn execute(
    &self,
    trade: &T,
    event: &EarningsEvent,  // ❌ Required
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
) -> <T as ExecutableTrade>::Result

// AFTER
pub async fn execute(
    &self,
    trade: &T,
    event: Option<&EarningsEvent>,  // ✅ Optional
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
) -> <T as ExecutableTrade>::Result
```

#### 4b. Update result construction (line 171-172)

```rust
// BEFORE
let mut result = match simulator.run(trade, &self.pricer).await {
    Ok(raw) => trade.to_result(raw.entry_pricing, raw.exit_pricing, &raw.output, event),
    Err(err) => trade.to_failed_result(&simulator.failed_output(), event, err),
};

// AFTER
let mut result = match simulator.run(trade, &self.pricer).await {
    Ok(raw) => trade.to_result(raw.entry_pricing, raw.exit_pricing, &raw.output, event),
    Err(err) => trade.to_failed_result(&simulator.failed_output(), event, err),
};
// (No change needed - event is already Option<&EarningsEvent>)
```

#### 4c. Fix `execute_rolling()` (line 264-269)

```rust
// BEFORE - Creating dummy event
let event = EarningsEvent::new(
    symbol.to_string(),
    exit_dt.date_naive(),
    EarningsTime::AfterMarketClose,
);
let result = self.execute(&trade, &event, entry_dt, exit_dt).await;

// AFTER - Pass None
let result = self.execute(&trade, None, entry_dt, exit_dt).await;
```

### Phase 5: Update SessionExecutor

**File**: `cs-backtest/src/session_executor.rs`

Update all strategy execution methods to pass `Some(&event)`:

```rust
// BEFORE (example: execute_straddle, line 585-589)
let result = executor.execute(
    &trade,
    earnings_event,  // &EarningsEvent
    session.entry_datetime,
    session.exit_datetime,
).await;

// AFTER
let result = executor.execute(
    &trade,
    Some(earnings_event),  // Option<&EarningsEvent>
    session.entry_datetime,
    session.exit_datetime,
).await;
```

**Affected methods** (8):
- `execute_calendar_spread()` (line 494)
- `execute_straddle()` (line 585)
- `execute_iron_butterfly()` (line 698)
- `execute_strangle()` (line 800)
- `execute_butterfly()` (line 903)
- `execute_condor()` (line 1006)
- `execute_iron_condor()` (line 1109)

### Phase 6: Update Other Callers

**File**: `cs-backtest/src/trade_strategy.rs`

Update all direct trade execution calls (5 occurrences at lines 171, 259, 318, 376, 427):

```rust
// BEFORE
Ok(raw) => trade.to_result(raw.entry_pricing, raw.exit_pricing, &raw.output, event),
Err(err) => trade.to_failed_result(&simulator.failed_output(), event, err),

// AFTER
Ok(raw) => trade.to_result(raw.entry_pricing, raw.exit_pricing, &raw.output, Some(event)),
Err(err) => trade.to_failed_result(&simulator.failed_output(), Some(event), err),
```

## Migration Steps

### Step 1: Domain Layer (No Breaking Changes Yet)

1. **Make earnings fields optional in all 8 result structs**
   - Add `Option<T>` wrapper
   - Add `#[serde(skip_serializing_if = "Option::is_none")]`
   - This is backwards compatible: `Some(value)` serializes identically

**Validation**: Existing tests pass, serialization unchanged for Some() values

### Step 2: Trait Layer (Breaking Change)

2. **Update `ExecutableTrade` trait**
   - Change `event: &EarningsEvent` → `event: Option<&EarningsEvent>`
   - **This breaks all implementations**

### Step 3: Implementation Layer

3. **Update all 8 `ExecutableTrade` implementations**
   - Change signature to accept `Option<&EarningsEvent>`
   - Use `.map()` for optional field extraction
   - Run tests after each impl

**Pattern**:
```rust
earnings_date: event.map(|e| e.earnings_date),
earnings_time: event.map(|e| e.earnings_time),
```

### Step 4: Execution Layer

4. **Update `TradeExecutor`**
   - Change `execute()` signature
   - Fix `execute_rolling()` to pass `None`

5. **Update `SessionExecutor`**
   - Wrap event references with `Some()`

6. **Update `trade_strategy.rs`**
   - Wrap event references with `Some()`

### Step 5: Test & Validate

7. **Run full test suite**
   - Unit tests for each result type
   - Integration tests for executors
   - Verify serialization works for both Some/None

8. **Verify output files**
   - Check that earnings-based results still have earnings fields
   - Check that rolling results now omit earnings fields
   - Validate Parquet/JSON output format

## Affected Code Locations

### Domain Layer (cs-domain)
- `src/entities.rs` - 8 result structs (lines 770, 879, 987, 1075, 1169, 1256, 1330, 1405)

### Execution Layer (cs-backtest)
- `src/execution/traits.rs` - `ExecutableTrade` trait (lines 71-87)
- `src/execution/straddle_impl.rs` - `to_result()`, `to_failed_result()`
- `src/execution/calendar_spread_impl.rs` - `to_result()`, `to_failed_result()`
- `src/execution/iron_butterfly_impl.rs` - `to_result()`, `to_failed_result()`
- `src/execution/calendar_straddle_impl.rs` - `to_result()`, `to_failed_result()`
- `src/execution/strangle_impl.rs` - `to_result()`, `to_failed_result()`
- `src/execution/butterfly_impl.rs` - `to_result()`, `to_failed_result()`
- `src/execution/condor_impl.rs` - `to_result()`, `to_failed_result()`
- `src/execution/iron_condor_impl.rs` - `to_result()`, `to_failed_result()`

### Orchestration Layer (cs-backtest)
- `src/trade_executor.rs` - `execute()`, `execute_rolling()` (lines 153, 171, 264)
- `src/session_executor.rs` - 7 execution methods (lines 494, 585, 698, 800, 903, 1006, 1109)
- `src/trade_strategy.rs` - 5 direct calls (lines 171, 259, 318, 376, 427)

## Benefits

1. **Domain Integrity**: No fake earnings dates in rolling results
2. **Type Safety**: `Option<NaiveDate>` communicates "may not apply"
3. **Cleaner Data**: Parquet/JSON output omits irrelevant fields
4. **Future-Proof**: Supports non-earnings strategies naturally

## Risks & Mitigation

### Risk 1: Downstream Code Expects Required Fields

**Mitigation**:
- Audit all code that reads `earnings_date`/`earnings_time`
- Use `.unwrap_or_default()` or pattern matching
- Add helper methods: `result.earnings_date_or_exit_date()`

### Risk 2: Breaking Serialization Format

**Mitigation**:
- Existing results with `Some(date)` serialize identically
- New rolling results omit field (JSON) or use null (Parquet)
- Version output schema if needed

### Risk 3: Large Refactor Surface Area

**Mitigation**:
- Follow incremental steps (domain → trait → impls → callers)
- Run tests after each layer
- Use compiler to find all call sites

## Alternative: SessionContext-Based Execution

**Future Enhancement** (not in this refactor):

Instead of `Option<&EarningsEvent>`, accept `&SessionContext`:

```rust
pub async fn execute(
    &self,
    trade: &T,
    context: &SessionContext,  // Full context
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
) -> <T as ExecutableTrade>::Result
```

This would allow results to include richer context:
- Earnings timing type (pre/cross/post)
- Inter-earnings roll number
- Standalone notes

**Decision**: Defer to future refactor. Current approach (Option<EarningsEvent>) is sufficient and less invasive.

## Summary

This refactoring eliminates the dummy `EarningsEvent` hack by:
1. Making earnings fields optional in result types
2. Updating trait signatures to accept `Option<&EarningsEvent>`
3. Fixing `execute_rolling()` to pass `None`
4. Updating all callers to wrap with `Some()` where needed

The change is semantically correct: rolling trades don't have earnings events, and the type system should reflect this.
