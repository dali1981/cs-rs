# Remediation Plan: Expired Option Validation

## Status
**READY FOR IMPLEMENTATION** - 2026-01-05

## Problem Summary

Options are being priced past their expiration date. The current code **silently returns intrinsic value** instead of erroring when `time_to_expiry <= 0`. This led to invalid backtest results where:

- October 2025 trade had **expiration: 2025-09-19** but **exit: 2025-10-06**
- The options expired **18 days BEFORE** the exit date
- The system priced these expired options as if they still existed
- All P&L calculations, hedging analysis, and Greeks were invalid

## Root Cause Analysis

### Current Behavior (WRONG)

| Function | Location | Behavior for TTM <= 0 |
|----------|----------|----------------------|
| `bs_price()` | `cs-analytics/src/black_scholes.rs:46-51` | Returns intrinsic value |
| `bs_implied_volatility()` | `cs-analytics/src/black_scholes.rs:78` | Returns `None` |
| `bs_greeks()` | `cs-analytics/src/black_scholes.rs:152-154` | Returns at-expiry Greeks |
| `bs_delta()` | `cs-analytics/src/black_scholes.rs:121-128` | Returns 0 or +/-1 |
| `SpreadPricer::calculate_ttm()` | `cs-backtest/src/spread_pricer.rs:426-430` | Returns negative value |
| IV Surface Builder | `cs-backtest/src/iv_surface_builder.rs:189` | Silently skips expired options |

### Why This Is Wrong

1. **No error propagation**: Silent handling means bugs go unnoticed
2. **Invalid results**: Pricing expired options produces meaningless values
3. **Cascading errors**: Hedging, Greeks, and P&L attribution are all based on invalid data
4. **No fail-fast**: The system should fail immediately when asked to price an expired option

## Proposed Solution

### Principle: Fail Fast at Lowest Level

Add explicit validation that **errors loudly** when attempting to price an option past its expiration date. The validation must occur at a level that ALL pricing paths go through.

### Architecture

```
                     +--------------------------+
                     |  Strategy Executors      |
                     |  (Straddle, Calendar,    |
                     |   IronButterfly, etc.)   |
                     +-----------+--------------+
                                 |
                                 v
                     +--------------------------+
                     |  Strategy Pricers        |
                     |  (StraddlePricer,        |
                     |   CalendarStraddlePricer)|
                     +-----------+--------------+
                                 |
                                 v
                     +--------------------------+
                     |  SpreadPricer            |  <-- VALIDATION HERE
                     |  price_leg()             |
                     |  validate_not_expired()  |
                     +-----------+--------------+
                                 |
                                 v
                     +--------------------------+
                     |  Black-Scholes Functions |
                     |  (bs_price, bs_greeks)   |
                     +--------------------------+
```

### Implementation Plan

#### Phase 1: Add Validation Error Type

**File**: `cs-backtest/src/spread_pricer.rs`

Add new error variant to `PricingError`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    // ... existing variants ...

    #[error("Option expired: expiration {expiration} is before pricing time {pricing_time}. TTM = {ttm:.6} years")]
    OptionExpired {
        expiration: NaiveDate,
        pricing_time: DateTime<Utc>,
        ttm: f64,
    },
}
```

#### Phase 2: Add Validation Function

**File**: `cs-backtest/src/spread_pricer.rs`

Add a new validation method to `SpreadPricer`:

```rust
impl SpreadPricer {
    /// Validate that an option has not expired
    ///
    /// # Errors
    /// Returns `PricingError::OptionExpired` if the option has expired
    /// (time to expiry is negative or zero)
    fn validate_not_expired(
        &self,
        expiration: NaiveDate,
        pricing_time: DateTime<Utc>,
    ) -> Result<f64, PricingError> {
        let ttm = self.calculate_ttm(pricing_time, expiration);

        if ttm <= 0.0 {
            return Err(PricingError::OptionExpired {
                expiration,
                pricing_time,
                ttm,
            });
        }

        Ok(ttm)
    }
}
```

#### Phase 3: Integrate Validation into price_leg()

**File**: `cs-backtest/src/spread_pricer.rs`

Modify `price_leg()` to call validation BEFORE any pricing logic:

```rust
pub fn price_leg(
    &self,
    strike: &Strike,
    expiration: NaiveDate,
    option_type: OptionType,
    chain_df: &DataFrame,
    spot_price: f64,
    pricing_time: DateTime<Utc>,
    iv_surface: Option<&IVSurface>,
    pricing_provider: &dyn PricingIVProvider,
) -> Result<LegPricing, PricingError> {
    // CRITICAL: Validate option has not expired BEFORE any pricing
    let ttm = self.validate_not_expired(expiration, pricing_time)?;

    // ... rest of existing implementation, using ttm instead of calling calculate_ttm again
}
```

#### Phase 4: Add Tracing/Logging

Add clear error logging so failures are visible:

```rust
fn validate_not_expired(
    &self,
    expiration: NaiveDate,
    pricing_time: DateTime<Utc>,
) -> Result<f64, PricingError> {
    let ttm = self.calculate_ttm(pricing_time, expiration);

    if ttm <= 0.0 {
        tracing::error!(
            expiration = %expiration,
            pricing_time = %pricing_time,
            ttm = ttm,
            "FATAL: Attempted to price expired option. This indicates a bug in expiration selection."
        );
        return Err(PricingError::OptionExpired {
            expiration,
            pricing_time,
            ttm,
        });
    }

    Ok(ttm)
}
```

### Files to Modify

| File | Change |
|------|--------|
| `cs-backtest/src/spread_pricer.rs` | Add `OptionExpired` error variant, add `validate_not_expired()`, modify `price_leg()` |

### Files That Inherit the Fix (No Changes Needed)

These pricers all call `SpreadPricer::price_leg()` and will automatically get the validation:

| File | Notes |
|------|-------|
| `cs-backtest/src/straddle_pricer.rs` | Uses `spread_pricer.price_leg()` |
| `cs-backtest/src/iron_butterfly_pricer.rs` | Uses `spread_pricer.price_leg()` |
| `cs-backtest/src/calendar_straddle_pricer.rs` | Uses `spread_pricer.price_leg()` |

### Testing Strategy

#### 1. Unit Test: Expired Option Validation

```rust
#[test]
fn test_price_leg_errors_on_expired_option() {
    let pricer = SpreadPricer::new();
    let expiration = NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
    let pricing_time = DateTime::parse_from_rfc3339("2025-01-20T14:30:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // Expiration is 5 days BEFORE pricing time
    let result = pricer.validate_not_expired(expiration, pricing_time);

    assert!(result.is_err());
    match result.unwrap_err() {
        PricingError::OptionExpired { ttm, .. } => {
            assert!(ttm < 0.0);
        }
        _ => panic!("Expected OptionExpired error"),
    }
}
```

#### 2. Unit Test: Valid Option Passes

```rust
#[test]
fn test_validate_not_expired_passes_for_valid_option() {
    let pricer = SpreadPricer::new();
    let expiration = NaiveDate::from_ymd_opt(2025, 1, 20).unwrap();
    let pricing_time = DateTime::parse_from_rfc3339("2025-01-15T14:30:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // Expiration is 5 days AFTER pricing time
    let result = pricer.validate_not_expired(expiration, pricing_time);

    assert!(result.is_ok());
    assert!(result.unwrap() > 0.0);
}
```

#### 3. Integration Test: Full Trade Flow

Re-run the PENG backtest after fixing the straddle expiration selection bug and confirm:
1. No `OptionExpired` errors occur for valid trades
2. All expirations are AFTER earnings dates
3. Hedge P&L analysis produces valid results

### Related Issue

This validation fix should be implemented AFTER (or in conjunction with) the expiration selection bug fix documented in:
- `ISSUE_straddle_expiration_before_earnings.md`

The expiration selection bug is the **root cause** - it selects expirations before earnings.
This validation is a **safety net** - it prevents silent failures if similar bugs occur in the future.

### Benefits of This Approach

1. **Fail-fast**: Bugs are caught immediately at pricing time
2. **Clear errors**: Error message explains exactly what went wrong
3. **Single point of validation**: All pricing flows through `price_leg()`
4. **Backward compatible**: No API changes, just stricter validation
5. **Observable**: Tracing logs make failures visible in production

### Alternative Approaches Considered

| Approach | Pros | Cons | Decision |
|----------|------|------|----------|
| Validate in Black-Scholes functions | Catches all callers | Too low-level, would break existing at-expiry handling | Rejected |
| Validate in strategy executors | Close to business logic | Would need to add validation to every executor | Rejected |
| **Validate in SpreadPricer** | Single point, all strategies inherit | Requires careful test coverage | **Selected** |
| Validate in calculate_ttm() | Very low level | Changes return type, high impact | Rejected |

## Implementation Checklist

- [ ] Add `PricingError::OptionExpired` variant
- [ ] Add `validate_not_expired()` method to `SpreadPricer`
- [ ] Modify `price_leg()` to call validation first
- [ ] Add tracing for expired option errors
- [ ] Add unit tests for validation
- [ ] Fix straddle expiration selection bug (separate PR)
- [ ] Run full backtest suite to verify no regressions
- [ ] Update documentation

## Estimated Impact

- **Code changes**: ~50 lines
- **Test changes**: ~30 lines
- **Risk**: Low (adds validation, doesn't change happy path)
- **Performance**: Negligible (one comparison per option leg)

## Timeline

1. **Phase 1-4**: 1 hour implementation
2. **Testing**: 30 minutes
3. **Validation**: Run full PENG backtest after expiration fix

## Success Criteria

1. Any attempt to price an expired option produces a clear error
2. Error message includes: expiration date, pricing time, and TTM value
3. All existing valid trades continue to work
4. PENG backtest with fixed expiration selection produces valid results
