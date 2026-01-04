# LNT Pricing Error Investigation (2025-11-07)

## Error Summary

**Error**: `PricingError("Pricing error: Invalid IV: Cannot determine IV for put strike 67.5, expiration 2025-12-19 - no market data, put-call parity failed, and interpolation failed")`

**Context**: Calendar straddle backtest for LNT on 2025-11-07 entry time (9:30 AM)

## Root Cause Analysis

### Diagnostic Output

```
IV surface interpolation failed
  surface_points=2
  matching_type_points=0
  option_type="put"
  target_strike=67.5
  target_expiration=2025-12-19
  all_points=["C K=72.5 exp=2025-11-21 iv=0.32", "C K=67.5 exp=2025-11-21 iv=0.19"]
```

### Key Findings

1. **IV Surface is Extremely Sparse**
   - Only 2 option contracts in the entire surface
   - Both are calls (no puts available)
   - Both expire on 2025-11-21 (not 2025-12-19)

2. **Why No Puts in IV Surface?**
   - Located issue in `cs-backtest/src/iv_surface_builder.rs:167-170`
   - The minute-aligned IV surface builder requires equity spot price lookup for each option's timestamp
   - Code silently skips options where spot price is unavailable:
     ```rust
     let spot_price = match equity_repo.get_spot_price(symbol, opt_timestamp).await {
         Ok(sp) => sp,
         Err(_) => continue, // Skip if no spot price available
     };
     ```

3. **Data Availability Issue**
   - For illiquid stocks like LNT, equity minute data may not exist for all timestamps where option minute data exists
   - This causes most/all option contracts to be filtered out of the IV surface
   - Only 2 calls had matching equity spot data at their exact timestamps

### Pricing Fallback Chain (All Failed)

1. **Direct market data**: ❌ No put at strike 67.5, exp 2025-12-19 in option chain
2. **Put-call parity**: ❌ No call at strike 67.5, exp 2025-12-19 either
3. **IV interpolation**: ❌ No puts in IV surface (matching_type_points=0)

## Solutions

### Option 1: Relax Spot Price Requirement (Recommended)

Instead of requiring exact timestamp match, use nearest available spot price:

```rust
// In iv_surface_builder.rs:167-170
let spot_price = match equity_repo.get_spot_price(symbol, opt_timestamp).await {
    Ok(sp) => sp,
    Err(_) => {
        // Fall back to using the pricing_time spot if option-specific lookup fails
        // This is acceptable for IV surface construction
        continue; // For now, but should try fallback
    }
};
```

Better approach:
- Pass in a single `spot_price` parameter to `build_iv_surface_minute_aligned`
- Only use per-option spot lookups when explicitly needed for high-precision IV
- For general IV surface construction, use a single spot price for all options

### Option 2: Use EOD IV Surface for Illiquid Stocks

For illiquid stocks, minute data is too sparse. Consider:
- Detect when minute IV surface has < N points (e.g., < 10)
- Fall back to EOD-based IV surface construction
- Trade off: less precise, but more robust

### Option 3: Improve Interpolation Across Option Types

When puts are missing but calls exist:
- Use put-call parity synthetically to create put IVs from call IVs
- Add synthetic put points to IV surface before interpolation
- Requires: call, spot price, risk-free rate, expiration

### Option 4: Accept Missing Data for Illiquid Stocks

Simply skip these opportunities:
- Add a minimum IV surface size check (e.g., require at least 5 points per option type)
- Fail early with clear error message
- Document that strategy requires liquid options

## Recommended Action

**Immediate**: Option 4 (document limitation)
- Add validation: if IV surface has < 5 put points OR < 5 call points, skip the trade
- Add this to strategy selection criteria
- Document in strategy specifications

**Medium-term**: Option 1 (relax spot requirement)
- Modify `build_iv_surface_minute_aligned` to accept spot price parameter
- Use single spot price for all IV calculations (acceptable precision trade-off)
- Fall back to this when per-option lookups fail

**Long-term**: Option 3 (synthetic puts from calls)
- Implement put-call parity augmentation of IV surface
- Adds robustness for illiquid names
- More complex but handles edge cases better

## Files Modified (for diagnostics)

- `cs-backtest/src/spread_pricer.rs:203-246` - Added warning logs to show IV surface contents when interpolation fails

## Test Case

```bash
export FINQ_DATA_DIR=~/polygon/data
./target/debug/cs backtest \
    --spread calendar-straddle \
    --start 2025-11-01 \
    --end 2025-11-30 \
    --min-iv-ratio 1.5 \
    --symbols LNT
```

Expected: Pricing error for 2025-11-07 with diagnostic output showing sparse IV surface
