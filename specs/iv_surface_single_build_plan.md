# IV Surface Single Build Plan

## Problem

Delta strategies require an IVSurface for strike selection, but:
1. `backtest_use_case.rs` sets `iv_surface: None` at line 416
2. `spread_pricer.rs` builds IV surface separately for pricing (line 88)

This causes:
- Delta strategies fail with `NoDeltaData` error
- If we add IV surface building at selection, it would be built twice per trade

## Current Flow

```
process_event()
  ├── get_option_bars()      ← Load option chain DataFrame
  ├── build OptionChainData with iv_surface: None
  ├── strategy.select()      ← FAILS for delta (no iv_surface)
  │
  └── executor.execute_trade()
        ├── get_option_bars() again ← Duplicate load!
        └── pricer.price_spread()
              └── build_iv_surface() ← Builds IV surface here
```

## Proposed Flow

```
process_event()
  ├── get_option_bars()           ← Load option chain DataFrame (ONCE)
  ├── build_iv_surface(chain_df)  ← Build IV surface (ONCE)
  ├── build OptionChainData with iv_surface: Some(surface)
  ├── strategy.select()           ← NOW WORKS with iv_surface
  │
  └── executor.execute_trade_with_chain()
        ├── Receive pre-loaded chain_df and iv_surface
        └── pricer.price_spread_with_surface()
              └── Use existing IV surface, don't rebuild
```

## Implementation Steps

### 1. Extract `build_iv_surface` to a shared location

Move `SpreadPricer::build_iv_surface()` to a standalone function or service that can be called from both places:

**Option A**: Move to `cs-analytics` (IVSurface is already there)
- Add `IVSurface::from_option_chain(chain_df, spot, pricing_time, symbol)`

**Option B**: Keep in `spread_pricer.rs` but make public
- Make `SpreadPricer::build_iv_surface()` public

### 2. Update `process_event()` to build IV surface

```rust
// In backtest_use_case.rs process_event()

// Get option chain (already happening)
let chain_df = self.options_repo.get_option_bars(&event.symbol, session_date).await?;

// Build IV surface ONCE
let pricer = SpreadPricer::new();
let iv_surface = pricer.build_iv_surface(
    &chain_df,
    spot.to_f64(),
    entry_time,
    &event.symbol,
);

// Pass to strategy
let chain_data = OptionChainData {
    expirations,
    strikes,
    deltas: None,
    volumes: None,
    iv_ratios: None,
    iv_surface,  // NOW POPULATED
};
```

### 3. Pass chain data to trade executor

Option 1: Pass IV surface to executor directly
```rust
let result = executor.execute_trade_with_surface(
    &spread,
    event,
    entry_time,
    exit_time,
    &entry_chain_df,  // Already loaded
    iv_surface.as_ref(),
).await;
```

Option 2: Executor rebuilds for pricing (simpler but less efficient)
- Keep current executor interface
- Accept that pricing might rebuild surface (but it's fast)

### 4. Update SpreadPricer to accept pre-built surface

```rust
pub fn price_spread_with_surface(
    &self,
    spread: &CalendarSpread,
    chain_df: &DataFrame,
    spot_price: f64,
    pricing_time: DateTime<Utc>,
    iv_surface: Option<&IVSurface>,  // Use pre-built if provided
) -> Result<SpreadPricing, PricingError>
```

## Decision: Minimal Change Approach

For minimal code change, we can:

1. Make `SpreadPricer::build_iv_surface()` a public static method
2. Call it in `process_event()` to populate `iv_surface` field
3. Let the executor rebuild for exit pricing (different timestamp anyway)

This approach:
- Fixes delta strategy (iv_surface populated at selection)
- Keeps executor interface unchanged
- Accepts rebuild at pricing (fast, O(n) where n = chain size)

## Files to Modify

1. **cs-backtest/src/spread_pricer.rs**
   - Make `build_iv_surface()` public (or static method)

2. **cs-backtest/src/backtest_use_case.rs**
   - Build IV surface in `process_event()`
   - Pass to `OptionChainData`

## Testing

After implementation:
```bash
./target/release/cs backtest \
  --start 2025-11-01 \
  --end 2025-11-30 \
  --strategy delta-scan \
  --delta-range "0.25,0.75" \
  --delta-scan-steps 5
```

Should see trades being entered (no more STRATEGY_SELECTION_FAILED).
