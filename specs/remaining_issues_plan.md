# Remaining Issues - Detailed Fix Plans

After fixing the session timing bug (5% → 57% win rate), three additional issues remain:

1. **IV Interpolation** - Fixed 30% IV fallback vs proper interpolation
2. **Timezone Handling** - Naive UTC vs Eastern-aware conversion
3. **Negative Entry Cost** - Short price > Long price bug

---

## Issue 1: IV Interpolation

### Current State (Rust)

**File:** `cs-backtest/src/spread_pricer.rs:125-152`

```rust
if filtered.is_empty() {
    // No market data, use Black-Scholes with estimated IV
    let estimated_iv = 0.30; // HARDCODED 30% ← THE PROBLEM
    let price = bs_price(..., estimated_iv, ...);
    return Ok(LegPricing { ... });
}
```

**Problem:** When market data is missing for a strike/expiration, Rust uses a fixed 30% IV. This:
- Ignores IV skew (OTM puts have higher IV than ATM)
- Ignores term structure (near-term vs far-term IV differences)
- Creates pricing inconsistencies between legs
- 30% may be too high or too low depending on market conditions

### Python Approach

Python's `IVSurface` class interpolates IV using:
1. **Strike interpolation** - Linear interpolation between bracketing strikes
2. **Expiration interpolation** - Square-root time weighted interpolation
3. **Nearest neighbor fallback** - Use closest available data point

```python
# Strike interpolation (linear)
IV = lower_iv + weight * (upper_iv - lower_iv)
where weight = (target_strike - lower_strike) / (upper_strike - lower_strike)

# Expiration interpolation (sqrt-time weighted)
IV = lower_iv + weight * (upper_iv - lower_iv)
where weight = (sqrt_target_ttm - sqrt_lower_ttm) / (sqrt_upper_ttm - sqrt_lower_ttm)
```

### Rust Already Has IVSurface!

**File:** `cs-analytics/src/iv_surface.rs`

The `IVSurface` struct already exists with:
- `get_iv(strike, expiration, is_call)` - Main interpolation method
- Strike interpolation (linear)
- Expiration interpolation (sqrt-time weighted)

**The module is not being used in SpreadPricer!**

### Fix Plan

#### Step 1: Build IVSurface from Option Chain Data

**File:** `cs-backtest/src/spread_pricer.rs`

```rust
use cs_analytics::IVSurface;

impl SpreadPricer {
    /// Build IV surface from option chain DataFrame
    fn build_iv_surface(
        &self,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
    ) -> Option<IVSurface> {
        // Extract strikes, expirations, and calculate IVs from market prices
        // Build IVPoint vector
        // Return IVSurface::new(points, symbol, pricing_time, spot_price)
    }
}
```

#### Step 2: Use IVSurface for Fallback

**File:** `cs-backtest/src/spread_pricer.rs`

```rust
fn price_leg(
    &self,
    strike: &Strike,
    expiration: NaiveDate,
    option_type: OptionType,
    chain_df: &DataFrame,
    spot_price: f64,
    pricing_time: DateTime<Utc>,
    iv_surface: Option<&IVSurface>,  // NEW PARAMETER
) -> Result<LegPricing, PricingError> {
    // ... existing filtering ...

    if filtered.is_empty() {
        // Try IV surface interpolation first
        let estimated_iv = if let Some(surface) = iv_surface {
            surface.get_iv(
                Decimal::try_from(strike_f64).unwrap(),
                expiration,
                option_type == OptionType::Call,
            ).unwrap_or(0.30)  // Fall back to 30% only if surface fails
        } else {
            0.30
        };
        // ... rest of pricing ...
    }
}
```

#### Step 3: Update price_spread to Build and Pass IVSurface

```rust
pub fn price_spread(
    &self,
    spread: &CalendarSpread,
    chain_df: &DataFrame,
    spot_price: f64,
    pricing_time: DateTime<Utc>,
) -> Result<SpreadPricing, PricingError> {
    // Build IV surface once for both legs
    let iv_surface = self.build_iv_surface(chain_df, spot_price, pricing_time);

    let short_pricing = self.price_leg(
        ...,
        iv_surface.as_ref(),  // Pass surface
    )?;

    let long_pricing = self.price_leg(
        ...,
        iv_surface.as_ref(),  // Pass surface
    )?;

    // ...
}
```

### Files to Modify

| File | Changes |
|------|---------|
| `cs-backtest/src/spread_pricer.rs` | Add `build_iv_surface()`, update `price_leg()` signature |
| `cs-analytics/src/iv_surface.rs` | May need minor updates for integration |

### Expected Impact

- More accurate pricing when exact contract data missing
- Better IV estimates respecting term structure
- Reduced pricing anomalies (negative entry costs)

---

## Issue 2: Timezone Handling

### Current State (Rust)

**File:** `cs-domain/src/value_objects.rs:76-82`

```rust
impl TimingConfig {
    pub fn entry_datetime(&self, date: NaiveDate) -> DateTime<Utc> {
        date.and_time(self.entry_time()).and_utc()  // NAIVE UTC!
    }
}
```

**Problem:** Rust treats 09:35 as 09:35 **UTC**, but Python treats it as 09:35 **Eastern** then converts to UTC.

**Time difference:**
- Rust: 09:35 UTC (actual)
- Python: 09:35 ET = 14:35 UTC (during EST) or 13:35 UTC (during EDT)

This is a **5 hour difference** in when trades are priced!

### Python Approach

**File:** `calendar_spread_backtest/domain/time_utils.py`

```python
from zoneinfo import ZoneInfo
EASTERN_TZ = ZoneInfo("America/New_York")

def ensure_utc_datetime(value: datetime) -> datetime:
    if value.tzinfo is not None:
        return value.astimezone(timezone.utc)  # Convert to UTC
    raise ValueError("Naive datetime not allowed")
```

**File:** `calendar_spread_backtest/domain/earnings_timing.py`

```python
def entry_dt(self, event: EarningsEvent) -> datetime:
    entry_date = ...
    # Create Eastern-aware datetime, then convert to UTC
    entry_eastern = datetime.combine(
        entry_date,
        self.config.get_entry_time(),  # e.g., time(15, 0)
        tzinfo=EASTERN_TZ
    )
    return ensure_utc_datetime(entry_eastern)  # → UTC
```

### Fix Plan

#### Step 1: Add chrono-tz Dependency

**File:** `cs-domain/Cargo.toml`

```toml
[dependencies]
chrono-tz = "0.10"
```

#### Step 2: Create Timezone Utilities

**File:** `cs-domain/src/timezone.rs` (NEW)

```rust
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use chrono_tz::America::New_York;

/// US Eastern timezone constant
pub const EASTERN: chrono_tz::Tz = New_York;

/// Create a UTC datetime from Eastern time components
pub fn eastern_to_utc(date: NaiveDate, time: NaiveTime) -> DateTime<Utc> {
    date.and_time(time)
        .and_local_timezone(EASTERN)
        .single()
        .expect("Unambiguous Eastern time")
        .with_timezone(&Utc)
}

/// Get current time in Eastern timezone
pub fn now_eastern() -> DateTime<chrono_tz::Tz> {
    Utc::now().with_timezone(&EASTERN)
}
```

#### Step 3: Update EarningsTradeTiming

**File:** `cs-domain/src/services/earnings_timing.rs`

```rust
use crate::timezone::eastern_to_utc;

impl EarningsTradeTiming {
    pub fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let entry_date = self.entry_date(event);
        // Convert Eastern time to UTC
        eastern_to_utc(entry_date, self.config.entry_time())
    }

    pub fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let exit_date = self.exit_date(event);
        eastern_to_utc(exit_date, self.config.exit_time())
    }
}
```

#### Step 4: Update TimingConfig (Optional)

Keep the existing methods for backward compatibility, but add Eastern-aware variants:

```rust
impl TimingConfig {
    /// Entry datetime in UTC (from Eastern time)
    pub fn entry_datetime_eastern(&self, date: NaiveDate) -> DateTime<Utc> {
        eastern_to_utc(date, self.entry_time())
    }

    /// Exit datetime in UTC (from Eastern time)
    pub fn exit_datetime_eastern(&self, date: NaiveDate) -> DateTime<Utc> {
        eastern_to_utc(date, self.exit_time())
    }
}
```

### Files to Create/Modify

| File | Changes |
|------|---------|
| `cs-domain/Cargo.toml` | Add `chrono-tz = "0.10"` |
| `cs-domain/src/timezone.rs` | Create (new module) |
| `cs-domain/src/lib.rs` | Add `pub mod timezone;` |
| `cs-domain/src/services/earnings_timing.rs` | Use `eastern_to_utc()` |

### Expected Impact

- Entry/exit times will match Python exactly
- Correct market data lookups (3 PM ET vs 9:35 UTC)
- Better price alignment between Python and Rust

---

## Issue 3: Negative Entry Cost Bug

### Current State

**Example from backtest output:**
```
QRVO - short 6.1, long 4.06, cost -2.04  ← NEGATIVE!
```

**Expected:** Long price > Short price for calendar spreads (you PAY to enter)

### Root Causes

1. **30% IV Fallback Mismatch**
   - Short leg (near-term) has market data, uses actual IV
   - Long leg (far-term) missing, uses 30% IV
   - If actual short IV > 30%, short price > long price
   - Result: negative entry cost

2. **Term Structure Ignored**
   - Near-term options typically have HIGHER IV around earnings
   - Far-term options have lower IV
   - 30% flat IV doesn't capture this dynamic

3. **No Validation**
   - No check to reject invalid spreads
   - Negative costs flow through to P&L calculation
   - Distorts win rate and total P&L

### Fix Plan

#### Step 1: Add Entry Cost Validation

**File:** `cs-backtest/src/trade_executor.rs`

```rust
impl<O, E> TradeExecutor<O, E> {
    async fn try_execute_trade(
        &self,
        spread: &CalendarSpread,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Result<CalendarSpreadResult, ExecutionError> {
        // ... existing pricing ...

        // VALIDATE: Calendar spread should have positive entry cost
        if entry_pricing.net_cost <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(format!(
                "Negative entry cost: {} (short={}, long={})",
                entry_pricing.net_cost,
                entry_pricing.short_leg.price,
                entry_pricing.long_leg.price,
            )));
        }

        // ... rest of execution ...
    }
}
```

#### Step 2: Add New Error Variant

**File:** `cs-backtest/src/trade_executor.rs`

```rust
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    // ... existing variants ...
    #[error("Invalid spread structure: {0}")]
    InvalidSpread(String),
}
```

#### Step 3: Map to Failure Reason

**File:** `cs-backtest/src/trade_executor.rs`

```rust
fn create_failed_result(...) -> CalendarSpreadResult {
    let failure_reason = match error {
        // ... existing cases ...
        ExecutionError::InvalidSpread(_) => FailureReason::DegenerateSpread,
    };
    // ...
}
```

#### Step 4: Add Warning Log for Debugging

```rust
if entry_pricing.net_cost <= Decimal::ZERO {
    tracing::warn!(
        symbol = %spread.symbol(),
        short_price = %entry_pricing.short_leg.price,
        long_price = %entry_pricing.long_leg.price,
        short_iv = ?entry_pricing.short_leg.iv,
        long_iv = ?entry_pricing.long_leg.iv,
        "Negative entry cost detected - spread may be mispriced"
    );
}
```

### Files to Modify

| File | Changes |
|------|---------|
| `cs-backtest/src/trade_executor.rs` | Add validation, new error variant |
| `cs-domain/src/value_objects.rs` | May need new `FailureReason` variant |

### Expected Impact

- Invalid spreads rejected early
- Clearer debugging information
- More accurate win rate (excludes mispriced trades)

---

## Implementation Priority

| Issue | Impact | Complexity | Priority |
|-------|--------|------------|----------|
| **Timezone Handling** | High - 5hr time offset | Low | 1st |
| **IV Interpolation** | Medium - pricing accuracy | Medium | 2nd |
| **Negative Entry Cost** | Low - data quality | Low | 3rd |

### Recommended Order

1. **Timezone** - Fixes fundamental timing mismatch, low risk
2. **IV Interpolation** - Improves pricing accuracy, uses existing code
3. **Negative Entry Cost** - Adds validation, can be done anytime

---

## Testing Strategy

### For Timezone Fix

```bash
# Compare entry/exit times between Python and Rust for same event
# Should match within milliseconds after fix
```

### For IV Interpolation

```bash
# Compare IV values for trades with missing data
# Rust should interpolate instead of using 30%
```

### For Negative Entry Cost

```bash
# Run backtest, count trades with negative entry_cost
# Should be 0 after validation added
# Trades that would have been negative should appear in dropped_events
```

---

## Summary

| Issue | Root Cause | Fix | Files |
|-------|------------|-----|-------|
| **IV Interpolation** | Fixed 30% IV | Use existing IVSurface | `spread_pricer.rs` |
| **Timezone** | Naive UTC | Eastern → UTC conversion | New `timezone.rs` |
| **Negative Cost** | No validation | Reject invalid spreads | `trade_executor.rs` |
