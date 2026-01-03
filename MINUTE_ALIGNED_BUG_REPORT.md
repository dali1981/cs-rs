# Minute-Aligned IV Mode Bug Report

## Summary

Minute-aligned mode fails to generate observations due to premature filtering of option contracts based on target maturity matching. This breaks constant-maturity interpolation which requires all available expirations.

## Impact

**CRITICAL**: Minute-aligned mode produces NaN for dates with option data, making earnings analysis impossible.

## Root Cause: Premature Filtering

### The Bug (cs-backtest/src/minute_aligned_iv_use_case.rs:189-201)

```rust
// Check if this expiration is within tolerance of a target maturity
let mut matched_target_dte: Option<i64> = None;
for &target_dte in &config.maturity_targets {
    let diff = (dte - target_dte as i64).abs();
    if diff <= config.maturity_tolerance as i64 {
        matched_target_dte = Some(dte);
        break;
    }
}

if matched_target_dte.is_none() {
    continue; // Not within tolerance of any target maturity ← PREMATURE FILTER
}
```

**Problem**: Filtering happens during IV computation, before interpolation can occur.

### Evidence: USAU 2025-11-28

**Available expirations**: 21, 49, 84, 168 DTE

**With targets [7, 30] and tolerance 7**:
- 7d target accepts 0-14 DTE: ❌ none (closest is 21)
- 30d target accepts 23-37 DTE: ❌ none (21 too short, 49 too long)

**Result**: ALL contracts rejected → NO IVs computed → NaN

**But**: Constant-maturity should interpolate between 21 and 49 DTE to produce 30 DTE estimate!

## Correct Architecture: Filter Only at Interpolation Stage

### EOD Mode (Working)

```
1. Extract ALL options from DataFrame
2. Compute IVs for ALL options (no filtering by target DTE)
3. Pass all IVs to compute_constant_maturity_ivs()
   └─ Calls atm_computer.compute_all_atm_ivs()
      └─ Filters by min_dte only
      └─ Returns full term structure
4. Interpolate to target maturities
   └─ ConstantMaturityInterpolator handles target matching
```

**Key**: No filtering during IV computation. Only quality filters (IV bounds, min_dte) applied inside interpolator.

### Minute-Aligned Mode (Broken)

```
1. Extract timestamped options
2. FOR EACH option:
   ├─ Filter by target DTE ± tolerance  ← WRONG: Premature filtering
   ├─ Compute IV
   └─ Store in iv_results
3. Build term structure from filtered iv_results
   └─ May be empty if no matches!
4. Interpolate (but term structure is incomplete)
```

**Problem**: Filtering before IV computation prevents interpolation from seeing all available expirations.

## Proposed Fix

### Remove ALL Filtering from IV Computation Loop

In `minute_aligned_iv_use_case.rs`, remove lines 189-201 entirely:

```rust
// BEFORE (lines 174-230):
for opt in &options_with_timestamps {
    let spot_price = ...;
    let dte = ...;

    if dte <= 0 {
        continue;
    }

    // ❌ DELETE THIS BLOCK (lines 189-201)
    // Check if this expiration is within tolerance of a target maturity
    let mut matched_target_dte: Option<i64> = None;
    for &target_dte in &config.maturity_targets {
        ...
    }
    if matched_target_dte.is_none() {
        continue; // ← Premature filtering
    }

    // Compute IV
    let iv = cs_analytics::bs_implied_volatility(...);

    if let Some(iv_value) = iv {
        // Skip unreasonable IVs (quality filter - OK)
        if iv_value < config.iv_min_bound || iv_value > config.iv_max_bound {
            continue;
        }

        // Store
        iv_results.entry((dte, opt.expiration))
                  .or_default()
                  .push((opt.strike, iv_value, opt.is_call));
    }
}
```

```rust
// AFTER (simplified):
for opt in &options_with_timestamps {
    let spot_price = ...;
    let dte = ...;

    if dte <= 0 {
        continue; // Only skip expired options
    }

    // Compute IV for ALL valid expirations
    let iv = cs_analytics::bs_implied_volatility(...);

    if let Some(iv_value) = iv {
        // Quality filter only
        if iv_value < config.iv_min_bound || iv_value > config.iv_max_bound {
            continue;
        }

        // Store ALL valid IVs - no DTE filtering
        iv_results.entry((dte, opt.expiration))
                  .or_default()
                  .push((opt.strike, iv_value, opt.is_call));
    }
}
```

### Let build_term_structure Handle Filtering

`build_term_structure` (line 509-562) already filters by `min_dte` (line 520):

```rust
fn build_term_structure(...) -> Vec<ExpirationIv> {
    let mut term_structure = Vec::new();

    for ((dte, expiration), contracts) in iv_results {
        // Filter out expirations below min_dte
        if *dte <= config.min_dte {
            continue; // ← This is the ONLY filter we need
        }

        // Process all remaining expirations
        ...
    }

    term_structure.sort_by_key(|e| e.dte);
    term_structure
}
```

This returns a complete term structure with all available expirations > min_dte.

### Let Interpolator Handle Target Matching

`ConstantMaturityInterpolator::interpolate_many` receives the full term structure and interpolates/extrapolates to target maturities:

```rust
// This already works correctly - no changes needed
ConstantMaturityInterpolator::interpolate_many(
    term_structure,  // All available expirations
    &[7, 30],        // Target maturities
)
// Returns interpolated IVs at exactly 7 and 30 DTE
```

## Result After Fix

For USAU 2025-11-28 with expirations at 21, 49, 84, 168 DTE:

1. Compute IVs for all 4 expirations ✓
2. Build term structure with 4 points ✓
3. Interpolate:
   - 7 DTE: Extrapolate from 21 DTE
   - 30 DTE: Interpolate between 21 and 49 DTE ✓

## Testing

```bash
./target/release/cs atm-iv --symbols USAU \
  --start 2025-11-28 --end 2025-12-04 \
  --maturities 7,30 --constant-maturity --minute-aligned \
  --output ./test_fix
```

Expected: 5 observations (not 0), matching EOD mode.

## Summary of Changes

| Component | Before | After |
|-----------|--------|-------|
| **IV computation loop** | Filter by target ± tolerance | NO filtering (except expired) |
| **build_term_structure** | Filter by min_dte | Filter by min_dte (unchanged) |
| **Interpolator** | Receives filtered data | Receives complete term structure |

## Architectural Principle

**Separate Concerns:**
- **Data collection** (IV computation): Collect ALL valid data
- **Data quality** (term structure building): Filter by quality metrics (min_dte, IV bounds)
- **Data selection** (interpolation): Apply domain logic (target matching) at the end

Filtering should be a **late-stage concern**, not mixed with data collection.

---
**Date**: 2026-01-03
**Files**: `cs-backtest/src/minute_aligned_iv_use_case.rs:189-201` (DELETE)
**Priority**: P0 - Critical bug blocking earnings analysis
