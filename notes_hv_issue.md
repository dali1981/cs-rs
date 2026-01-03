# IV Data Generation Issues

## Issue 1: Historical Volatility in EOD Mode (RESOLVED)

### Problem
The `--with-hv` flag in EOD mode did not generate HV columns.

### Status: RESOLVED
- Minute-aligned mode DOES generate HV columns (`hv_10d`, `hv_20d`, `hv_30d`, `iv_hv_spread_30d`)
- EOD mode does NOT generate HV columns

### Workaround
Use `--minute-aligned` flag to get HV data (though see Issue 2 below).

---

## Issue 2: Minute-Aligned Mode Missing Data (FIXED)

### Problem
The `--minute-aligned` flag produced NaN values for critical dates around earnings events.

### Root Cause
**Premature filtering** in `cs-backtest/src/minute_aligned_iv_use_case.rs:189-201`

The code filtered option contracts BEFORE computing IVs, rejecting any expiration not within tolerance of target maturities. This broke constant-maturity interpolation which requires ALL available expirations.

For USAU on Nov 28, 2025:
- Available expirations: 21, 49, 84, 168 DTE
- Target 7d (0-14 DTE): ❌ no match
- Target 30d (23-37 DTE): ❌ no match
- Result: ALL contracts rejected → 0 observations

But constant-maturity SHOULD interpolate between 21 and 49 DTE to estimate 30 DTE!

### Fix Applied
**Deleted lines 189-201** in `minute_aligned_iv_use_case.rs`

Removed the premature target-maturity matching filter. Now all valid options are processed, and filtering happens later during interpolation where it belongs.

### Verification
```bash
# BEFORE FIX
./target/release/cs atm-iv --symbols USAU --start 2025-11-28 --end 2025-12-04 \
  --maturities 7,30 --constant-maturity --minute-aligned --output ./test
# Result: 0 observations

# AFTER FIX
./target/release/cs atm-iv --symbols USAU --start 2025-11-28 --end 2025-12-04 \
  --maturities 7,30 --constant-maturity --minute-aligned --output ./test
# Result: 5 observations ✓
```

### Status: RESOLVED
Minute-aligned mode now works correctly. Historical volatility is also computed successfully with `--with-hv` flag.

---

## Issue 3: Single Expiration Problem

When `cm_num_expirations = 1`, constant-maturity interpolation cannot work:
- 7d and 30d IVs become identical (no term structure)
- This is expected behavior but reduces data quality

### Workaround
Use wider date ranges to ensure multiple expirations are always available.

---

## Recommendation for Earnings Analysis

✅ **Minute-aligned mode is now fixed and recommended** for earnings analysis:
- Generates complete term structures with constant-maturity interpolation
- Includes historical volatility computation (`--with-hv`)
- Processes all available option expirations correctly
- Suitable for analyzing IV evolution around earnings events

**Note**: Minute-aligned mode still uses EOD-level data (16:00 ET). For true intraday analysis at specific times (e.g., 14:35 entry), you would need minute-level option chain snapshots at those exact times.

**Usage**:
```bash
./target/release/cs atm-iv --symbols SYMBOL \
  --start START_DATE --end END_DATE \
  --maturities 7,30 --constant-maturity --minute-aligned \
  --with-hv --hv-windows 10,20,30 \
  --output ./output_dir
```

---
Date: 2026-01-03 (Updated)
Status: Minute-aligned mode bug FIXED
