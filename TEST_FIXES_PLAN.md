# Test Fixes Plan

**Created:** 2026-01-04
**Branch:** code/review
**Context:** Pre-existing test failures discovered during refactoring validation

---

## Overview

Fix 5 pre-existing test failures in cs-domain:
- 4 failures due to missing fields in test fixtures
- 1 failure due to incorrect test method call

**Estimated effort:** 15-20 minutes

---

## Issue 1: Missing `entry_surface_time` and `exit_surface_time` Fields

### Background

These fields were added to result structs to track when IV surfaces were built (for debugging timing issues with minute-aligned IV computation). Tests were not updated when the fields were added.

### Affected Structs

```rust
pub struct CalendarSpreadResult {
    // ... existing fields ...
    pub entry_surface_time: Option<DateTime<Utc>>,  // Added but not in tests
    pub exit_surface_time: Option<DateTime<Utc>>,   // Added but not in tests
}

pub struct CalendarStraddleResult {
    // ... existing fields ...
    pub entry_surface_time: Option<DateTime<Utc>>,  // Added but not in tests
    pub exit_surface_time: Option<DateTime<Utc>>,   // Added but not in tests
}
```

### Fix 1.1: `cs-domain/src/entities.rs:934` - `test_calendar_spread_result_iv_ratio`

**Location:** Line 934
**Test Purpose:** Verify IV ratio calculation for calendar spreads

**Current code:**
```rust
let result = CalendarSpreadResult {
    symbol: "AAPL".to_string(),
    // ... many fields ...
    iv_ratio_entry: Some(1.2),
    // MISSING: entry_surface_time
    // MISSING: exit_surface_time
    delta_pnl: None,
    // ... rest of fields ...
};
```

**Fix:**
Add after `exit_value` field (around line 950):
```rust
exit_value: Decimal::new(2, 0),
entry_surface_time: None,  // Add this
exit_surface_time: None,   // Add this
pnl: Decimal::new(1, 0),
```

**Reasoning:** Test fixtures don't need actual timestamps since the test is only validating IV ratio calculation logic.

---

### Fix 1.2: `cs-domain/src/entities.rs:983` - `test_calendar_spread_result_success_flag`

**Location:** Line 983
**Test Purpose:** Verify success flag behavior in result struct

**Current code:**
```rust
let mut result = CalendarSpreadResult {
    symbol: "TEST".to_string(),
    // ... many fields ...
    // MISSING: entry_surface_time
    // MISSING: exit_surface_time
};
```

**Fix:**
Add after `exit_value` field:
```rust
exit_value: Decimal::ZERO,
entry_surface_time: None,  // Add this
exit_surface_time: None,   // Add this
pnl: Decimal::ZERO,
```

---

### Fix 1.3: `cs-domain/src/entities.rs:1176` - `test_calendar_straddle_result` (or similar)

**Location:** Line 1176
**Test Purpose:** Verify CalendarStraddleResult struct behavior

**Current code:**
```rust
let result = CalendarStraddleResult {
    symbol: "AAPL".to_string(),
    // ... many fields ...
    // MISSING: entry_surface_time
    // MISSING: exit_surface_time
};
```

**Fix:**
Add after `exit_value` field:
```rust
exit_value: Decimal::new(2, 0),
entry_surface_time: None,  // Add this
exit_surface_time: None,   // Add this
pnl: Decimal::new(1, 0),
```

---

### Fix 1.4: `cs-domain/src/infrastructure/parquet_results_repo.rs:80` - Parquet conversion

**Location:** Line 80
**Test Purpose:** Verify Parquet serialization/deserialization

**Current code:**
```rust
CalendarSpreadResult {
    symbol: "TEST".to_string(),
    // ... fields ...
    // MISSING: entry_surface_time
    // MISSING: exit_surface_time
}
```

**Fix:**
Add after `exit_value` field:
```rust
exit_value: Decimal::ZERO,
entry_surface_time: None,  // Add this
exit_surface_time: None,   // Add this
pnl: Decimal::ZERO,
```

**Note:** Since this is testing Parquet I/O, verify that the schema includes these Optional DateTime fields. They should serialize as nullable columns.

---

## Issue 2: Incorrect Test Method Call

### Fix 2.1: `cs-domain/src/strategies/straddle.rs:124` - `test_select_expiration`

**Location:** Line 124
**Test Purpose:** Verify expiration selection for straddle strategy

**Problem:** Test calls method with 2 arguments, but method only takes 1

**Current code:**
```rust
let selected = strategy.select_expiration(&expirations, earnings_date);
//                                                        ^^^^^^^^^^^^
//                                                        EXTRA ARGUMENT
```

**Method signature:**
```rust
fn select_expiration(&self, expirations: &[NaiveDate]) -> Option<NaiveDate>
```

**Root cause analysis:**

The `StraddleStrategy` struct stores `entry_date`:
```rust
pub struct StraddleStrategy {
    pub min_dte: i32,
    pub entry_date: NaiveDate,  // <-- Used internally
}
```

The `select_expiration` method uses `self.entry_date`:
```rust
fn select_expiration(&self, expirations: &[NaiveDate]) -> Option<NaiveDate> {
    expirations
        .iter()
        .filter(|&&exp| (exp - self.entry_date).num_days() >= self.min_dte as i64)
        //                    ^^^^^^^^^^^^^^^^^
        //                    Uses stored entry_date
        .min()
        .copied()
}
```

**Fix:**

Remove the second argument from the test call:

```rust
// Before
let selected = strategy.select_expiration(&expirations, earnings_date);

// After
let selected = strategy.select_expiration(&expirations);
```

**Verification:**
The test already creates the strategy with the correct entry date:
```rust
let strategy = StraddleStrategy::with_entry_date(
    earnings_date,  // This is stored in strategy.entry_date
    7  // min_dte
);
```

So the method will use `strategy.entry_date` (which is `earnings_date`) internally.

---

## Implementation Order

### Step 1: Fix entity tests (batch together)
1. Read each test file to identify exact line numbers
2. Add `entry_surface_time: None,` and `exit_surface_time: None,` to all 4 locations
3. Run `cargo test --package cs-domain --lib entities` to verify

### Step 2: Fix parquet test
1. Add missing fields to parquet test fixture
2. Run `cargo test --package cs-domain --lib infrastructure::parquet_results_repo` to verify

### Step 3: Fix straddle test
1. Remove extra argument from `select_expiration` call
2. Run `cargo test --package cs-domain --lib strategies::straddle` to verify

### Step 4: Full validation
1. Run `cargo test --package cs-domain` to verify all tests pass
2. Run `cargo test --workspace` to ensure no regressions elsewhere

---

## Expected Changes

| File | Lines Changed | Type |
|------|---------------|------|
| `cs-domain/src/entities.rs` | +6 | Add missing fields (3 tests) |
| `cs-domain/src/infrastructure/parquet_results_repo.rs` | +2 | Add missing fields |
| `cs-domain/src/strategies/straddle.rs` | -1 | Remove extra argument |
| **Total** | **+7** | Minimal changes |

---

## Commit Message

```
Fix pre-existing test failures in cs-domain

- Add missing entry_surface_time and exit_surface_time fields to test fixtures
  - test_calendar_spread_result_iv_ratio
  - test_calendar_spread_result_success_flag
  - test_calendar_straddle_result
  - parquet_results_repo test
- Remove incorrect extra argument from straddle test

These fields were added to result structs in a previous commit but
tests were not updated. The straddle test was calling a method with
the wrong number of arguments.
```

---

## Risk Assessment

**Risk Level:** Very Low

**Reasoning:**
- Only touching test code, not production code
- Adding `None` values for optional fields (no behavior change)
- Fixing obvious test bugs (wrong argument count)
- All changes are in test modules or test fixtures

**Validation:**
- All tests should pass after fixes
- No production code changes
- No API changes

---

## Post-Fix Validation

After completing all fixes, verify:

```bash
# 1. All cs-domain tests pass
cargo test --package cs-domain

# 2. All workspace tests pass
cargo test --workspace

# 3. Build succeeds
cargo build --workspace

# 4. No new warnings introduced
cargo clippy --workspace
```

Expected result: All tests pass, build succeeds, no new warnings.
