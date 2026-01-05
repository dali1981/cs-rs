# Timing Strategy Refactor - Option B

**Date:** 2026-01-04
**Issue:** Straddle backtest uses wrong timing (enters on earnings day instead of N days before)

## Problem Summary

`process_event_unified` hardcodes `self.earnings_timing` (calendar spread timing) for ALL strategies. Straddles and post-earnings straddles create correct timing objects but they're ignored.

### Current Bug
```rust
// Line 1316-1317 in backtest_use_case.rs
let entry_time = self.earnings_timing.entry_datetime(event);  // ← Wrong for straddles!
let exit_time = self.earnings_timing.exit_datetime(event);
```

Result: 10-day straddle entry enters ON earnings day, buying at IV peak and selling after crush.

---

## Solution Design

### Architecture
```
┌─────────────────┐     ┌──────────────────────┐
│ TradeStructure  │────▶│ TradeTiming (trait)  │
└─────────────────┘     └──────────────────────┘
                                  ▲
                                  │
        ┌─────────────────────────┼─────────────────────────┐
        │                         │                         │
┌───────┴───────┐    ┌────────────┴────────────┐   ┌───────┴────────┐
│ EarningsTrade │    │ StraddleTradeTiming     │   │ PostEarnings   │
│ Timing        │    │ (N days before)         │   │ StraddleTiming │
└───────────────┘    └─────────────────────────┘   └────────────────┘
```

---

## Implementation Steps

### Step 1: Verify TradeTiming Trait
**File:** `cs-domain/src/timing/mod.rs`

Ensure trait exists with:
- `entry_date(&self, event) -> NaiveDate`
- `exit_date(&self, event) -> NaiveDate`
- `entry_datetime(&self, event) -> DateTime<Utc>`
- `exit_datetime(&self, event) -> DateTime<Utc>`

---

### Step 2: Create TimingStrategy Enum
**File:** `cs-backtest/src/timing_strategy.rs` (new)

```rust
pub enum TimingStrategy {
    Earnings(EarningsTradeTiming),
    Straddle(StraddleTradeTiming),
    PostEarnings(PostEarningsStraddleTiming),
}

impl TimingStrategy {
    pub fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> { ... }
    pub fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> { ... }
    pub fn entry_date(&self, event: &EarningsEvent) -> NaiveDate { ... }

    // Lookahead calculation based on timing type
    pub fn lookahead_days(&self) -> i64 { ... }
}
```

---

### Step 3: Modify `process_event_unified` Signature
**File:** `cs-backtest/src/backtest_use_case.rs`

**Before:**
```rust
pub async fn process_event_unified(
    &self,
    event: &EarningsEvent,
    selector: &dyn StrikeSelector,
    structure: TradeStructure,
) -> TradeResult
```

**After:**
```rust
pub async fn process_event_unified(
    &self,
    event: &EarningsEvent,
    selector: &dyn StrikeSelector,
    structure: TradeStructure,
    timing: &TimingStrategy,  // ← NEW
) -> TradeResult
```

**Inside:** Replace `self.earnings_timing` with `timing`

---

### Step 4: Update Execute Methods
**File:** `cs-backtest/src/backtest_use_case.rs`

#### 4a. `execute_calendar_spread`
```rust
let timing = TimingStrategy::Earnings(
    EarningsTradeTiming::new(self.config.timing)
);
// Pass to process_event_unified
```

#### 4b. `execute_iron_butterfly`
Same as 4a - use Earnings timing.

#### 4c. `execute_straddle`
```rust
let timing_impl = StraddleTradeTiming::new(self.config.timing)
    .with_entry_days(self.config.straddle_entry_days)
    .with_exit_days(self.config.straddle_exit_days);
let timing = TimingStrategy::Straddle(timing_impl);

// Fix lookahead using timing.lookahead_days()
let lookahead = timing.lookahead_days();
```

#### 4d. `execute_post_earnings_straddle`
```rust
let timing_impl = PostEarningsStraddleTiming::new(self.config.timing)
    .with_holding_days(self.config.post_earnings_holding_days);
let timing = TimingStrategy::PostEarnings(timing_impl);
```

#### 4e. `execute_calendar_straddle`
Same as 4a - use Earnings timing.

---

### Step 5: Fix Event Loading Logic
**File:** `cs-backtest/src/backtest_use_case.rs`

Update lookahead calculation in each execute method:

**Before:**
```rust
let lookahead = self.config.straddle_entry_days as i64 + 5;  // BUG!
```

**After:**
```rust
let lookahead = timing.lookahead_days();  // Timing-aware
```

---

### Step 6: Remove `self.earnings_timing` Field
**File:** `cs-backtest/src/backtest_use_case.rs`

**Remove:**
```rust
struct BacktestUseCase {
    earnings_timing: EarningsTradeTiming,  // ← Delete this
    // ... rest
}
```

**In constructor:**
```rust
impl BacktestUseCase {
    pub fn new(...) -> Self {
        // Remove: let earnings_timing = EarningsTradeTiming::new(config.timing);
        Self {
            // Remove: earnings_timing,
            // ... rest
        }
    }
}
```

---

### Step 7: Update Helper Methods
**File:** `cs-backtest/src/backtest_use_case.rs`

#### `should_enter_today`
**Before:**
```rust
fn should_enter_today(&self, event: &EarningsEvent, session_date: NaiveDate) -> bool {
    self.earnings_timing.entry_date(event) == session_date
}
```

**After:** Remove this method - each execute method now filters using its own timing

#### `filter_for_entry`
**Before:**
```rust
fn filter_for_entry(&self, events: &[EarningsEvent], session_date: NaiveDate) -> Vec<EarningsEvent> {
    events.filter(|e| self.should_enter_today(e, session_date))
}
```

**After:** Inline into each execute method with appropriate timing

---

### Step 8: Testing
**File:** `cs-backtest/tests/test_unified_executor.rs`

Add test cases:
1. Straddle with 10-day entry → verify entry is 10 trading days before earnings
2. Straddle with 5-day entry → verify entry is 5 trading days before
3. Post-earnings straddle → verify entry is day after earnings
4. Calendar spread → verify entry is on/before earnings day

---

## File Change Summary

| File | Changes |
|------|---------|
| `cs-backtest/src/timing_strategy.rs` | **NEW** - TimingStrategy enum |
| `cs-backtest/src/lib.rs` | Add `mod timing_strategy;` |
| `cs-backtest/src/backtest_use_case.rs` | Update signature, 5 execute methods, remove field, update helpers |
| `cs-domain/src/timing/mod.rs` | Ensure trait is public |
| `cs-backtest/tests/test_unified_executor.rs` | Add timing tests |

---

## Validation

### Before Fix
```bash
./target/debug/cs backtest \
  --earnings-file ./custom_earnings/PENG_2025.parquet \
  --symbols PENG --spread straddle \
  --straddle-entry-days 10 --straddle-exit-days 2 \
  --start 2024-12-01 --end 2025-12-31

Expected: Entry 10 days before earnings
Actual: Entry ON earnings day (bug)
Result: -$2.06 P&L (buying at IV peak)
```

### After Fix
```bash
Same command as above

Expected: Entry 10 days before earnings, exit 2 days before
Actual: Should match expected
Result: Positive P&L from IV expansion
```

---

## Rollback Plan

If issues arise:
1. Revert `process_event_unified` signature change
2. Restore `self.earnings_timing` field
3. Revert execute method changes
4. Delete `timing_strategy.rs`

All changes isolated to backtest crate, domain timing unchanged.

---

## Benefits

| Benefit | Description |
|---------|-------------|
| **Type Safety** | Compiler ensures timing matches structure |
| **Extensibility** | Easy to add new timing strategies |
| **Single Responsibility** | Timing logic fully encapsulated |
| **Testability** | Unit test timing in isolation |
| **No Magic Numbers** | Lookahead lives with timing |

---

## Estimated Effort

- Step 1-3: 30 minutes
- Step 4: 30 minutes
- Steps 5-7: 30 minutes
- Step 8: 30 minutes

**Total:** ~2 hours
