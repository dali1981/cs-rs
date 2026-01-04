# Code Review Report: Strategy Pricing and PnL Consistency

**Date:** 2025-01-04
**Branch:** code/review
**Scope:** Multi-strategy pricing, PnL calculation, and code duplication analysis

---

## Executive Summary

The codebase implements 5 option strategies (CalendarSpread, IronButterfly, Straddle, CalendarStraddle, PostEarningsStraddle) with generally consistent pricing and PnL attribution patterns. However, there are several areas of code duplication and minor inconsistencies that should be addressed.

### Key Findings

| Category | Status | Severity |
|----------|--------|----------|
| Pricing Logic | Mostly Consistent | Low |
| PnL Calculation | Consistent | Low |
| PnL Attribution | Consistent | Low |
| Code Duplication | Present | Medium |
| Strategy Selection | Duplicated Logic | Medium |

---

## 1. Code Duplication Issues

### 1.1 `select_expirations` Function Duplication (HIGH PRIORITY)

**Files affected:**
- `cs-domain/src/strategies/atm.rs:212-255`
- `cs-domain/src/strategies/delta.rs:196-239`

The `select_expirations` helper function is **fully duplicated** between these two files. Both implementations are identical:

```rust
fn select_expirations(
    expirations: &[NaiveDate],
    reference_date: NaiveDate,
    min_short_dte: i32,
    max_short_dte: i32,
    min_long_dte: i32,
    max_long_dte: i32,
) -> Result<(NaiveDate, NaiveDate), StrategyError>
```

**Recommendation:** Extract to `cs-domain/src/strategies/mod.rs` as a shared utility function.

---

### 1.2 ATM Strike Finding Logic Duplication (MEDIUM PRIORITY)

The logic to find the closest strike to spot price is duplicated across multiple files:

| File | Location | Pattern |
|------|----------|---------|
| `atm.rs` | Lines 60-68, 162-170 | `min_by` with `diff.abs()` |
| `delta.rs` | Lines 242-251 | Same pattern |
| `straddle.rs` | Lines 86-93 | Same pattern |
| `iron_butterfly.rs` | Lines 110-124 | Same pattern with `partial_cmp` |

**Example from atm.rs:**
```rust
let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
let atm_strike = chain_data.strikes
    .iter()
    .min_by(|a, b| {
        let a_diff = (f64::from(**a) - spot_f64).abs();
        let b_diff = (f64::from(**b) - spot_f64).abs();
        a_diff.partial_cmp(&b_diff).unwrap()
    })
```

**Recommendation:** Add a `find_atm_strike(&[Strike], f64) -> Option<Strike>` utility function.

---

### 1.3 Executor Pattern Duplication (MEDIUM PRIORITY)

All four executors follow an identical structural pattern:

| Executor | File | Lines |
|----------|------|-------|
| TradeExecutor | `trade_executor.rs` | 371 |
| StraddleExecutor | `straddle_executor.rs` | 399 |
| IronButterflyExecutor | `iron_butterfly_executor.rs` | 432 |
| CalendarStraddleExecutor | `calendar_straddle_executor.rs` | 428 |

**Common duplicated patterns:**

1. **Executor struct definition** (same fields):
   ```rust
   struct XxxExecutor<O, E> {
       options_repo: Arc<O>,
       equity_repo: Arc<E>,
       pricer: XxxPricer,
       max_entry_iv: Option<f64>,
   }
   ```

2. **Builder methods** (identical in all):
   - `with_pricing_model()`
   - `with_max_entry_iv()`

3. **Execute flow** (same structure):
   - Get entry/exit spot
   - Get entry/exit chain
   - Build entry/exit IV surface
   - Price at entry/exit
   - Calculate PnL
   - Calculate PnL attribution

4. **Failed result creation** (same error mapping)

**Recommendation:** Consider a generic `TradeExecutor<S, P>` trait or macro to reduce duplication.

---

### 1.4 Backtest Use Case Method Duplication (MEDIUM PRIORITY)

**File:** `cs-backtest/src/backtest_use_case.rs`

The following methods have near-identical structure:
- `execute_calendar_spread()` (lines 326-445)
- `execute_iron_butterfly()` (lines 447-554)
- `execute_straddle()` (lines 556-670)
- `execute_calendar_straddle()` (lines 796-905)
- `execute_post_earnings_straddle()` (lines 677-789)

All follow the pattern:
1. Initialize result vectors
2. Create strategy/timing
3. Loop over trading days
4. Load earnings
5. Filter for entry
6. Process events (parallel or sequential)
7. Collect results
8. Return BacktestResult

**Recommendation:** Extract common loop structure to reduce ~500 lines of duplicated logic.

---

## 2. Pricing Consistency Analysis

### 2.1 Pricing Flow

All strategies use the same pricing infrastructure:

```
SpreadPricer.price_leg()
    ↓
Market data lookup
    ↓ (if missing)
Put-call parity fallback
    ↓ (if fails)
IV surface interpolation → bs_price()
```

**Status: CONSISTENT**

### 2.2 Pricing Model Configuration

| Strategy | Uses PricingModel | Default Model |
|----------|-------------------|---------------|
| CalendarSpread | Yes | StickyMoneyness |
| IronButterfly | Yes | StickyMoneyness |
| Straddle | Yes | StickyMoneyness |
| CalendarStraddle | Yes | StickyMoneyness |

**Status: CONSISTENT**

### 2.3 IV Surface Building

All strategies use `build_iv_surface_minute_aligned()` for IV surfaces:

| Executor | Uses minute-aligned IV | Location |
|----------|------------------------|----------|
| TradeExecutor | Yes | Lines 122, 185 |
| StraddleExecutor | Yes | Lines 97, 137 |
| IronButterflyExecutor | Yes | Lines 100, 142 |
| CalendarStraddleExecutor | Yes | Lines 101, 142 |

**Status: CONSISTENT**

### 2.4 Minor Inconsistency: Risk-Free Rate

The pricers use different default risk-free rates when creating pricing providers:

| Pricer | Risk-Free Rate |
|--------|----------------|
| SpreadPricer | `self.bs_config.risk_free_rate` (from BSConfig) |
| StraddlePricer | `0.0` hardcoded |
| IronButterflyPricer | `0.0` hardcoded |
| CalendarStraddlePricer | `0.0` hardcoded |

**Location:**
- `straddle_pricer.rs:63`
- `iron_butterfly_pricer.rs:61`
- `calendar_straddle_pricer.rs:71`

**Impact:** Low (rate is primarily used for delta-to-strike conversion in StickyDelta mode)

**Recommendation:** Pass through the configured rate from SpreadPricer.

---

## 3. PnL Calculation Consistency

### 3.1 Raw PnL Calculation

All strategies calculate PnL identically:

| Strategy | Formula | Verified |
|----------|---------|----------|
| CalendarSpread | `exit_value - entry_cost` | Yes |
| IronButterfly | `entry_credit - exit_cost` | Yes |
| Straddle | `exit_value - entry_cost` | Yes |
| CalendarStraddle | `exit_value - entry_cost` | Yes |

**Note:** IronButterfly reverses the formula because it's a credit strategy (receive premium at entry).

**Status: CONSISTENT**

### 3.2 PnL Percentage Calculation

All strategies use the same formula:

```rust
pnl_pct = (pnl / entry_cost.abs()) * 100
```

**Status: CONSISTENT**

---

## 4. PnL Attribution Consistency

### 4.1 Attribution Function

All strategies use the same domain function for per-leg attribution:

```rust
cs_domain::calculate_option_leg_pnl(
    entry_greeks,
    entry_iv,
    exit_iv,
    spot_change,
    days_held,
    position_sign,  // +1.0 long, -1.0 short
)
```

**Status: CONSISTENT**

### 4.2 Position Signs

| Strategy | Leg | Sign |
|----------|-----|------|
| CalendarSpread | Short leg | -1.0 |
| CalendarSpread | Long leg | +1.0 |
| Straddle | Call | +1.0 |
| Straddle | Put | +1.0 |
| IronButterfly | Short call | -1.0 |
| IronButterfly | Short put | -1.0 |
| IronButterfly | Long call | +1.0 |
| IronButterfly | Long put | +1.0 |
| CalendarStraddle | Short call | -1.0 |
| CalendarStraddle | Short put | -1.0 |
| CalendarStraddle | Long call | +1.0 |
| CalendarStraddle | Long put | +1.0 |

**Status: CONSISTENT**

### 4.3 Days Held Calculation

All executors calculate days held identically:

```rust
let days_held = (exit_time - entry_time).num_hours() as f64 / 24.0;
```

**Status: CONSISTENT**

### 4.4 Unexplained PnL

All strategies calculate unexplained PnL the same way:

```rust
let unexplained = total_pnl - (delta_pnl + gamma_pnl + theta_pnl + vega_pnl);
```

**Status: CONSISTENT**

---

## 5. Strategy Selection Duplication

### 5.1 Straddle vs Post-Earnings Straddle

`process_straddle_event()` (lines 907-1096) and `process_post_earnings_straddle_event()` (lines 1098-1291) are nearly identical (~200 lines each), differing only in:

1. Timing service used (`StraddleTradeTiming` vs `PostEarningsStraddleTiming`)
2. Expiration filtering logic

**Recommendation:** Extract common logic, parameterize the timing strategy.

---

## 6. Deprecated Code

### 6.1 Deprecated Function in iron_butterfly_executor.rs

```rust
#[deprecated(note = "Use cs_domain::calculate_option_leg_pnl instead")]
fn calculate_leg_attribution(...) -> (f64, f64, f64, f64)
```

**Location:** `iron_butterfly_executor.rs:346-366`

This function wraps `cs_domain::calculate_option_leg_pnl` but is marked deprecated. It's still being called within the same file (line 383-413).

**Recommendation:** Remove deprecated wrapper, call domain function directly.

---

## 7. Recommendations Summary

### High Priority
1. **Extract `select_expirations()`** to shared module (removes ~45 lines duplication)

### Medium Priority
2. **Extract ATM strike finder** to shared utility function
3. **Consider executor trait/macro** to reduce structural duplication
4. **Consolidate straddle/post-earnings-straddle processing** logic
5. **Remove deprecated `calculate_leg_attribution`** function

### Low Priority
6. **Fix risk-free rate inconsistency** in pricers
7. **Consider generic backtest executor loop** to reduce ~500 lines

---

## 8. Consistency Matrix

| Aspect | Calendar | IronButterfly | Straddle | CalStraddle |
|--------|----------|---------------|----------|-------------|
| Uses SpreadPricer.price_leg() | Yes | Yes | Yes | Yes |
| Minute-aligned IV surface | Yes | Yes | Yes | Yes |
| PnL = exit - entry | Yes | Yes* | Yes | Yes |
| Uses calculate_option_leg_pnl | Yes | Yes | Yes | Yes |
| Position signs correct | Yes | Yes | Yes | Yes |
| Days held formula | Same | Same | Same | Same |
| Unexplained PnL formula | Same | Same | Same | Same |

*IronButterfly: `entry_credit - exit_cost` (credit strategy)

---

## Conclusion

The codebase demonstrates **good consistency** in pricing and PnL calculation across all strategies. The main concern is **code duplication** which inflates maintenance burden. The recommended refactoring would reduce the codebase by an estimated 500-800 lines while improving maintainability.

No critical bugs or calculation inconsistencies were found.
