# Straddle Strategy Liquidity Filters

## Overview

This document describes two filters to improve straddle strategy quality by excluding illiquid stocks with sparse or untradeable option chains.

## Current Issue

After enabling IV surface interpolation (commit fixing line 868 in backtest_use_case.rs), pricing errors decreased significantly:
- **Q4 2025 Results**: 2132 successful straddle trades
- **Remaining Issues**: ~100-150 pricing errors from extremely illiquid stocks

Example remaining errors:
```
PricingError("Cannot determine IV for put strike 2.5, expiration 2025-12-19 -
             no market data and interpolation failed"): 10 events
  ↳ QRHC, CGEN, PLBY (resolved by interpolation fix)

PricingError("Cannot determine IV for put strike 17.5, expiration 2025-11-21"):
  ↳ BWB, BNL, MNRO, ... (15 events - still need filtering)
```

These errors occur because:
1. **ATM strike doesn't exist or wasn't traded** - Cannot get market price or IV
2. **Extremely low notional** - Penny stocks with minimal option activity
3. **Sparse option chains** - Insufficient data points for IV interpolation

---

## Filter 1: ATM Strike Must Have Traded at Entry Date

### Requirement

**Reject straddles where the ATM strike has zero volume on the entry date.**

### Rationale

- **Market data dependency**: Need actual traded options to get IV and fair prices
- **Interpolation fails**: Even with PricingModel, need nearby strikes with valid IV
- **Execution realism**: Cannot execute a straddle at strikes that don't trade

### Implementation Location

**File**: `cs-backtest/src/straddle_strategy.rs` (or create new validator)

**Method**: `validate_straddle_liquidity()`

### Logic

```rust
fn validate_atm_strike_traded(
    chain_df: &DataFrame,
    strike: Strike,
    expiration: NaiveDate,
) -> Result<(), ValidationError> {
    // Filter to ATM strike and expiration
    let atm_options = chain_df.filter(
        col("strike").eq(strike.value()) &
        col("expiration").eq(expiration)
    )?;

    // Check if EITHER call or put has volume > 0
    let call_volume = atm_options.filter(col("option_type").eq("call"))
        .select("volume")
        .sum()
        .unwrap_or(0);

    let put_volume = atm_options.filter(col("option_type").eq("put"))
        .select("volume")
        .sum()
        .unwrap_or(0);

    if call_volume == 0 && put_volume == 0 {
        return Err(ValidationError::NoATMVolume {
            strike,
            expiration,
            details: "ATM strike has zero volume - cannot price or execute"
        });
    }

    Ok(())
}
```

### Where to Apply

**Option A: During strategy selection** (recommended)
- In `StraddleStrategy::select_straddle()`
- Validate before creating Straddle entity
- Fail fast, clear error message

**Option B: During trade execution**
- In `StraddleExecutor::try_execute_trade()`
- After getting chain data, before pricing
- More defensive but later in pipeline

### Configuration

Add to `BacktestConfig`:
```rust
pub struct BacktestConfig {
    // ... existing fields

    /// Require ATM strike to have traded at entry (default: true)
    pub require_atm_volume: bool,

    /// Minimum volume for ATM strike validation (default: 1)
    pub min_atm_volume: i64,
}
```

### Expected Impact

**Symbols filtered**: ~50-100 illiquid penny stocks per quarter
**Examples**: QRHC ($1.41 spot, only 2 call contracts), BWB, BNL, FTEK

**Trades lost**: Minimal - these would have failed pricing anyway
**Quality improvement**: High - removes unpriceable/unexecutable trades

---

## Filter 2: Minimum Notional Filter

### Requirement

**Reject straddles where the underlying notional (100 × stock_price) is below a threshold.**

### Rationale

- **Transaction costs**: Small notional → high cost % → unrealistic returns
- **Bid-ask spread**: Penny stocks have wide spreads (20-50% of price)
- **Liquidity risk**: Cannot execute at advertised prices
- **Data quality**: Penny stocks often have stale/unreliable option quotes

### Notional Calculation

```
Notional per contract = 100 shares × Stock Price

Example:
- PLBY at $1.36: Notional = 100 × $1.36 = $136
- AAPL at $180:  Notional = 100 × $180 = $18,000
```

### Implementation Location

**File**: `cs-backtest/src/backtest_use_case.rs`

**Method**: Filter during `load_earnings_window()` or add to `passes_market_cap_filter()`

### Logic

```rust
fn passes_notional_filter(
    &self,
    event: &EarningsEvent,
    entry_spot: Decimal,
) -> bool {
    // Default minimum: $500 notional ($5 stock)
    let min_notional = self.config.min_notional.unwrap_or(Decimal::new(500, 0));

    let notional = entry_spot * Decimal::from(100); // 100 shares per contract

    if notional < min_notional {
        debug!(
            symbol = %event.symbol,
            spot = %entry_spot,
            notional = %notional,
            min_required = %min_notional,
            "Rejected: notional below minimum"
        );
        return false;
    }

    true
}
```

### Where to Apply

**Early filtering** (recommended):
```rust
// In execute_straddle() loop
for session_date in TradingCalendar::trading_days_between(...) {
    let events = self.load_earnings_window(session_date).await?;

    let to_enter: Vec<_> = events
        .iter()
        .filter(|e| timing.entry_date(e) == session_date)
        .filter(|e| self.passes_market_cap_filter(e))
        .filter(|e| {
            // Get spot price at entry
            let spot = self.equity_repo
                .get_spot_price(&e.symbol, entry_time)
                .await
                .ok()?;

            self.passes_notional_filter(e, spot)
        })
        .collect();
}
```

### Configuration

Add to `BacktestConfig`:
```rust
pub struct BacktestConfig {
    // ... existing fields

    /// Minimum notional per contract (100 × stock_price)
    /// None = no filter, Some(500.0) = $500 minimum (default)
    pub min_notional: Option<Decimal>,
}
```

Add CLI parameter:
```rust
#[arg(long)]
/// Minimum notional per contract: 100 × stock_price (e.g., 500 for $500 minimum)
pub min_notional: Option<f64>,
```

### Recommended Thresholds

| Threshold | Stock Price | Use Case |
|-----------|-------------|----------|
| **$500** (default) | $5.00 | General backtesting, exclude penny stocks |
| **$1,000** | $10.00 | Higher quality, institutional-tradeable |
| **$2,000** | $20.00 | Very liquid, tight spreads |
| **$5,000** | $50.00 | Large-cap only |

### Expected Impact

**With $500 minimum**:
- **Symbols filtered**: All stocks under $5
- **Examples filtered**: QRHC ($1.41), CGEN ($1.67), PLBY ($1.36), SMSI ($1.00)
- **Q4 impact**: ~5-10% of opportunities removed
- **Quality**: Significantly improved - realistic execution assumptions

**With $1,000 minimum**:
- **Symbols filtered**: All stocks under $10
- **More aggressive**: Removes small/micro-caps
- **Q4 impact**: ~15-20% of opportunities removed

---

## Implementation Priority

### Phase 1: Notional Filter (Easier, High Impact)

1. Add `min_notional` to `BacktestConfig`
2. Add `--min-notional` CLI parameter
3. Implement `passes_notional_filter()` in backtest_use_case.rs
4. Apply filter in `execute_straddle()` event loop
5. Test: Run Q4 backtest with `--min-notional 500`

**Estimated effort**: 30 minutes

### Phase 2: ATM Strike Volume Filter (More Complex)

1. Add `require_atm_volume` and `min_atm_volume` to `BacktestConfig`
2. Create validation function in straddle_strategy.rs or separate validator
3. Call validation after fetching chain_df, before strategy selection
4. Return meaningful error (not just "PricingError")
5. Test: Verify QRHC, BWB, BNL are filtered before pricing

**Estimated effort**: 1-2 hours

---

## Testing Plan

### Test Case 1: Notional Filter

```bash
# Baseline: No filter
./target/release/cs backtest --spread straddle \
    --start 2025-10-01 --end 2025-12-31 \
    --output /tmp/baseline.json

# Filter: $500 minimum
./target/release/cs backtest --spread straddle \
    --start 2025-10-01 --end 2025-12-31 \
    --min-notional 500 \
    --output /tmp/notional_500.json

# Filter: $1000 minimum
./target/release/cs backtest --spread straddle \
    --start 2025-10-01 --end 2025-12-31 \
    --min-notional 1000 \
    --output /tmp/notional_1000.json
```

**Expected**:
- Baseline: 2132 trades, ~100 pricing errors
- $500 filter: 2000-2100 trades, <50 pricing errors (all penny stocks filtered)
- $1000 filter: 1700-1900 trades, ~10 pricing errors

### Test Case 2: ATM Volume Filter

```bash
# Enable ATM volume check
./target/release/cs backtest --spread straddle \
    --start 2025-10-01 --end 2025-12-31 \
    --require-atm-volume \
    --min-atm-volume 1 \
    --output /tmp/atm_volume.json
```

**Expected**:
- Trades: 2100-2130 (minimal impact - only extreme cases filtered)
- Pricing errors: 0-5 (should eliminate "no market data" errors)
- New error type: "ATMVolumeValidationFailed" (clear reason)

### Test Case 3: Combined Filters

```bash
# Both filters together
./target/release/cs backtest --spread straddle \
    --start 2025-10-01 --end 2025-12-31 \
    --min-notional 500 \
    --require-atm-volume \
    --output /tmp/combined.json
```

**Expected**:
- Trades: 2000-2100
- Pricing errors: <10
- Clean, realistic backtest results

---

## Alternative: Post-Execution Filtering

Instead of preventing trade creation, filter results after execution:

```rust
// In save_results()
let filtered_results: Vec<_> = all_results
    .into_iter()
    .filter(|r| {
        // Only keep trades with notional > $500
        let notional = r.spot_at_entry * 100.0;
        notional >= 500.0
    })
    .collect();
```

**Pros**: Simpler implementation, see what was filtered
**Cons**: Still attempts pricing (wasted compute), less clear why trade was rejected

**Recommendation**: Use pre-filtering for performance and clarity

---

## Summary

### Filter 1: ATM Strike Volume
- **Purpose**: Ensure ATM strike actually traded at entry
- **Impact**: ~30-50 trades filtered (extreme illiquid stocks)
- **Benefit**: Eliminates "no market data" pricing errors
- **Complexity**: Medium (requires chain_df validation)

### Filter 2: Minimum Notional
- **Purpose**: Exclude penny stocks with unrealistic execution
- **Impact**: ~100-200 trades filtered at $500 threshold
- **Benefit**: More realistic backtest, better quality results
- **Complexity**: Low (simple spot price check)

### Recommended Default Configuration

```toml
[backtest]
min_notional = 500.0           # $5 stock minimum
require_atm_volume = true      # ATM must have traded
min_atm_volume = 1             # At least 1 contract
```

These filters will significantly improve backtest quality while removing only unrealistic or unpriceable trades.
