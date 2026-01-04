# Plan: IV Slice-Only Extrapolation

**Date**: 2025-01-04
**Status**: Ready for implementation
**Rationale**: See `docs/notes/iv_surface_extrapolation_research.md`

## Goal

Fix IV interpolation to work within a single expiration slice only, without cross-expiry interpolation. This preserves term structure information needed for calendar spread trading.

## Current Behavior (Broken)

```
price_leg() → iv_surface.get_iv() or pricing_provider.get_iv()
    ↓
interpolate_strike() at target expiration
    ↓
if fails → interpolate_expiration() across ALL expirations  ← PROBLEM
    ↓
if only 1 expiration has data → returns None  ← BUG
```

**CRBG Example**:
- Target: Strike 32, Dec 19 expiration
- Available Dec 19 strikes: [34, 35]
- Flat extrapolation in strike space WORKS (returns IV from strike 34)
- But code then tries to interpolate across expirations
- Only Dec 19 has call data → `expiry_ivs.len() == 1` → tries to bracket → fails

## Desired Behavior

```
price_leg() → iv_surface.get_iv() or pricing_provider.get_iv()
    ↓
interpolate_strike() at target expiration ONLY
    ↓
if target expiration has data → return interpolated/extrapolated IV
    ↓
if target expiration has NO data → return None (fail cleanly)
```

**No cross-expiry interpolation**. Each expiration slice is independent.

## Implementation

### File 1: `cs-analytics/src/iv_surface.rs`

#### Change `get_iv()` (lines 57-114)

**Current logic**:
```rust
// Try exact expiration first
if let Some(points) = by_expiry.get(&expiration) {
    if let Some(iv) = self.interpolate_strike(points, strike) {
        return Some(iv);
    }
}

// Interpolate across expirations  ← REMOVE THIS
if let Some(iv) = self.interpolate_expiration(&by_expiry, strike, expiration) {
    return Some(iv);
}
```

**New logic**:
```rust
// Only use target expiration - no cross-expiry interpolation
if let Some(points) = by_expiry.get(&expiration) {
    return self.interpolate_strike(points, strike);
}

// No data at target expiration - don't guess from other expirations
None
```

#### Simplify or remove `interpolate_expiration()` (lines 205-262)

Either:
- Remove the function entirely (if unused elsewhere)
- Keep but don't call from `get_iv()`

### File 2: `cs-analytics/src/iv_model.rs`

#### Change `interpolate_by_moneyness()` (lines 98-129)

**Current logic**:
```rust
// Try exact expiration first
if let Some(points) = by_expiry.get(&expiration) {
    if let Some(iv) = interpolate_moneyness_at_expiry(points, target_moneyness) {
        return Some(iv);
    }
}

// Interpolate across expirations using sqrt(T) weighting  ← REMOVE THIS
interpolate_expiration_by_moneyness(surface, &by_expiry, target_moneyness, expiration)
```

**New logic**:
```rust
// Only use target expiration - no cross-expiry interpolation
if let Some(points) = by_expiry.get(&expiration) {
    return interpolate_moneyness_at_expiry(points, target_moneyness);
}

// No data at target expiration
None
```

#### Simplify or remove `interpolate_expiration_by_moneyness()` (lines 175-220)

Either:
- Remove the function entirely
- Keep but don't call from `interpolate_by_moneyness()`

### File 3: No changes needed to `spread_pricer.rs`

The existing fallback cascade remains:
1. Market data → use directly
2. Put-call parity → derive from opposite type
3. IV interpolation → now slice-only
4. Fail with error

## Verification

### Test 1: CRBG Strike 32

```bash
./target/release/cs backtest \
  --spread calendar-straddle \
  --start 2025-11-03 \
  --end 2025-11-03 \
  --symbols CRBG
```

**Expected**:
- Strike 32 call (Dec 19) prices using IV from strike 34 (flat extrapolation)
- No "interpolation failed" error

### Test 2: Python Comparison

```bash
uv run python scripts/analysis/compare_extrapolation_methods.py
```

**Expected**:
- Rust price ≈ $1.45 (flat IV method)
- Matches Python flat IV calculation

### Test 3: Existing Tests Pass

```bash
cargo test -p cs-analytics
cargo test -p cs-backtest
```

**Expected**: All existing tests pass (no regression)

## Edge Cases

| Case | Available Data | Behavior |
|------|---------------|----------|
| Exact strike exists | Dec 19 @ 32 | Return market IV |
| Strike between available | Dec 19 @ [30, 35] target 32 | Linear interpolation |
| Strike below all | Dec 19 @ [34, 35] target 32 | Flat extrapolation (use 34's IV) |
| Strike above all | Dec 19 @ [30, 32] target 35 | Flat extrapolation (use 32's IV) |
| No data at expiration | Only Nov 21 data | Return None (fail) |
| No data at all | Empty surface | Return None (fail) |

## Rollback Plan

If issues arise:
```bash
git checkout cs-analytics/src/iv_surface.rs
git checkout cs-analytics/src/iv_model.rs
```

## Timeline

- Implementation: ~30 minutes
- Testing: ~15 minutes
- Total: ~45 minutes
