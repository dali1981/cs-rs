# Fix Plan: Straddle Expiration Selection Bug

## Status
**READY FOR IMPLEMENTATION** - 2026-01-05

## Problem Statement

Straddles are being selected with expirations **BEFORE** earnings, causing:
1. Options expire before the intended exit date
2. The new `OptionExpired` validation now correctly errors on these trades
3. Invalid backtest results (as seen in PENG October trade)

### Example of Bug
```
Symbol: PENG
Earnings Date: 2025-10-07
Entry Date: 2025-09-09 (entry_days_before = 20)
Exit Date: 2025-10-06 (exit_days_before = 1)

SELECTED EXPIRATION: 2025-09-19 (WRONG!)
  - This is 18 days BEFORE earnings
  - The option expires before the exit date
  - All pricing/hedging after Sept 19 is invalid
```

## Root Cause Analysis

### Current Code Flow

```
backtest_use_case.execute_straddle()
    │
    ├─ Build IV surface from option chain at entry_time
    │      └─ IVSurface contains all expirations in chain
    │
    └─ process_event_unified()
           └─ UnifiedExecutor::execute_with_selection()
                  └─ selector.select_straddle(spot, surface, min_short_dte)
                         │
                         └─ ATMStrategy::select_straddle()
                               │
                               └─ select_single_expiration(expirations, reference_date, min_dte)
                                      │
                                      └─ reference_date = surface.as_of_time() = ENTRY DATE
                                      └─ min_dte = min_short_dte from config (default: 7)
                                      └─ Returns FIRST expiration where DTE >= 7 from ENTRY
```

### The Bug

In `ATMStrategy::select_straddle()` (cs-domain/src/strike_selection/atm.rs:363-365):

```rust
let reference_date = surface.as_of_time().date_naive();  // ENTRY DATE!
let expiration = Self::select_single_expiration(&expirations, reference_date, min_dte)?;
```

This selects the **first expiration with DTE >= min_dte from ENTRY DATE**, not from EARNINGS or EXIT.

For PENG example:
- Entry: 2025-09-09
- min_dte: 7
- Available expirations: [2025-09-19, 2025-10-17, ...]
- 2025-09-19 has DTE = 10 from entry (>= 7), so it's selected
- But 2025-09-19 is BEFORE earnings (2025-10-07)!

### Why Legacy Flow Worked (Sometimes)

The legacy `process_straddle_event()` method had explicit filtering:

```rust
// Filter expirations to those after earnings
let valid_expirations: Vec<_> = expirations
    .iter()
    .filter(|&&exp| exp > event.earnings_date)
    .copied()
    .collect();
```

But the unified flow (`process_event_unified`) does NOT have this filter!

## Solution Design

### Approach: Add `min_expiration_date` Parameter

Add a new parameter to `select_straddle()` that specifies the **minimum required expiration date**. The caller (backtest use case) knows the exit date and can require expirations after it.

### Why This Approach

| Alternative | Pros | Cons |
|------------|------|------|
| Filter IV surface expirations before selection | Simple | IV surface is shared across trade types; filtering breaks others |
| Pass EarningsEvent to selector | Selector knows earnings date | Pollutes selector interface with earnings-specific concern |
| **Pass min_expiration_date to selector** | Clean, explicit, flexible | Requires trait change |
| Filter in UnifiedExecutor | Keeps selector simple | Logic scattered, hard to test |

The `min_expiration_date` approach is cleanest because:
1. The backtest knows the exit date (from timing strategy)
2. The selector doesn't need to know about earnings or timing
3. The constraint is explicit and testable

## Implementation Plan

### Phase 1: Update StrikeSelector Trait

**File**: `cs-domain/src/strike_selection/mod.rs`

Change `select_straddle` signature to require minimum expiration:

```rust
// BEFORE (lines 175-183)
fn select_straddle(
    &self,
    _spot: &SpotPrice,
    _surface: &IVSurface,
    _min_dte: i32,
) -> Result<Straddle, SelectionError> {
    Err(SelectionError::UnsupportedStrategy(
        "Straddle not supported by this selector".to_string()
    ))
}

// AFTER
fn select_straddle(
    &self,
    _spot: &SpotPrice,
    _surface: &IVSurface,
    _min_expiration: NaiveDate,
) -> Result<Straddle, SelectionError> {
    Err(SelectionError::UnsupportedStrategy(
        "Straddle not supported by this selector".to_string()
    ))
}
```

### Phase 2: Update ATMStrategy Implementation

**File**: `cs-domain/src/strike_selection/atm.rs`

```rust
// BEFORE (lines 345-387)
fn select_straddle(
    &self,
    spot: &SpotPrice,
    surface: &IVSurface,
    min_dte: i32,
) -> Result<Straddle, SelectionError> {
    // ...
    let reference_date = surface.as_of_time().date_naive();
    let expiration = Self::select_single_expiration(&expirations, reference_date, min_dte)?;
    // ...
}

// AFTER
fn select_straddle(
    &self,
    spot: &SpotPrice,
    surface: &IVSurface,
    min_expiration: NaiveDate,
) -> Result<Straddle, SelectionError> {
    // Get strikes and expirations from IV surface
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
        .filter(|&exp| exp > min_expiration)
        .collect();

    if expirations.is_empty() {
        return Err(SelectionError::NoExpirations);
    }

    // Select FIRST valid expiration (soonest after min_expiration)
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

### Phase 3: Update DeltaStrategy Implementation

**File**: `cs-domain/src/strike_selection/delta.rs`

If DeltaStrategy also implements `select_straddle`, update it similarly.

### Phase 4: Update UnifiedExecutor

**File**: `cs-backtest/src/unified_executor.rs`

Change the call to `select_straddle` to pass the exit date:

```rust
// BEFORE (line 348)
match selector.select_straddle(&spot, entry_surface, criteria.min_short_dte) {

// AFTER - need exit_time parameter
TradeStructure::Straddle => {
    // Calculate minimum required expiration (exit date + 1 day buffer)
    let min_expiration = exit_time.date_naive();

    match selector.select_straddle(&spot, entry_surface, min_expiration) {
```

But wait - `execute_with_selection` already receives `exit_time`! We just need to use it:

```rust
pub async fn execute_with_selection(
    &self,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,  // Already available!
    entry_surface: &IVSurface,
    selector: &dyn StrikeSelector,
    structure: TradeStructure,
    criteria: &ExpirationCriteria,
) -> TradeResult {
    // ...
    TradeStructure::Straddle => {
        let min_expiration = exit_time.date_naive();  // Use exit date
        match selector.select_straddle(&spot, entry_surface, min_expiration) {
```

### Phase 5: Update BacktestConfig (Optional)

If we want a buffer between exit and expiration, add config:

```rust
// In config.rs
pub struct BacktestConfig {
    // ... existing fields ...
    /// Minimum days between exit and expiration (default: 0)
    pub min_days_to_expiration_after_exit: i32,
}
```

Then in UnifiedExecutor:
```rust
let min_expiration = exit_time.date_naive() + chrono::Duration::days(config.min_days_to_expiration_after_exit as i64);
```

### Phase 6: Add Tests

**File**: `cs-domain/src/strike_selection/atm.rs` (add tests)

```rust
#[test]
fn test_select_straddle_filters_expired_expirations() {
    let strategy = ATMStrategy::default();

    // Build IV surface with multiple expirations
    // Entry: 2025-09-09
    // Expirations: [2025-09-19, 2025-10-17, 2025-11-21]
    // min_expiration: 2025-10-06 (exit date)

    // Should skip 2025-09-19 (before exit) and select 2025-10-17
}

#[test]
fn test_select_straddle_no_valid_expiration() {
    // All expirations before min_expiration
    // Should return NoExpirations error
}
```

## Files to Modify

| File | Change |
|------|--------|
| `cs-domain/src/strike_selection/mod.rs` | Change `select_straddle` trait signature |
| `cs-domain/src/strike_selection/atm.rs` | Update implementation to filter by min_expiration |
| `cs-domain/src/strike_selection/delta.rs` | Update if implements select_straddle |
| `cs-backtest/src/unified_executor.rs` | Pass exit_date as min_expiration |
| `cs-backtest/src/backtest_use_case.rs` | Remove redundant min_straddle_dte usage |

## Validation Strategy

### 1. Unit Tests
- Test that expirations before min_expiration are filtered
- Test that NoExpirations error is returned when no valid expiration exists
- Test that the soonest valid expiration is selected

### 2. Integration Test
Re-run PENG backtest and verify:
```
Entry: 2025-09-09
Exit: 2025-10-06
Earnings: 2025-10-07

Expected: Expiration >= 2025-10-07 (first monthly after exit)
```

### 3. Regression Test
Run full backtest suite to ensure no valid trades are broken.

## Expected Outcome

After fix, PENG trade would:
1. Skip 2025-09-19 expiration (before exit date)
2. Select 2025-10-17 expiration (first after exit)
3. Price options correctly through exit
4. Produce valid P&L and hedging analytics

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| Breaking existing trades | All valid trades already have expiration > exit |
| Fewer trade opportunities | Expected - invalid opportunities should be filtered |
| Trait change breaks callers | Limited callers, all in this repo |

## Implementation Order

1. **Phase 1-3**: Update trait and implementations (domain layer)
2. **Phase 4**: Update UnifiedExecutor caller
3. **Phase 5**: Optional config enhancement
4. **Phase 6**: Add tests
5. **Validate**: Run PENG backtest, verify correct expiration selected

## Estimated Changes

- Lines changed: ~50
- Files modified: 4-5
- Risk: Low (fixing a clear bug, not changing behavior for valid trades)
