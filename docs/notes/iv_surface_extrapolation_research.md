# IV Surface Extrapolation Research Notes

**Date**: 2025-01-04
**Context**: CRBG strike 32 pricing failure investigation

## Problem Statement

When pricing options at strikes outside the available market data range, the current implementation attempts to:
1. Interpolate in strike/moneyness dimension
2. Interpolate across expirations (term structure)

This fails when only one expiration has data, even though flat extrapolation in strike space should work.

## Academic Literature Summary

### Flat Extrapolation Concerns

From Le Floc'h (2024) and others:
- Flat IV extrapolation is "flawed" - introduces boundary instability
- Creates "unrealistically narrow tails at extreme strikes"
- Can introduce static arbitrage at wing boundaries

### Recommended Strike Extrapolation: SVI / Roger Lee

**Roger Lee's Moment Formula**:
> "Any implied variance extrapolation must be at most linear in the wings"

**SVI Parameterization** (Gatheral):
```
w(k) = a + b(ρ(k-m) + √((k-m)² + σ²))
```
- 5 parameters fit per expiration slice
- Guarantees linear wings (σ² ∝ |k| as |k| → ∞)
- Consistent with no-arbitrage at extreme strikes

### Term Structure Interpolation

**Calendar Arbitrage Condition**:
> Total variance σ²T must be monotonically increasing in T for fixed moneyness

**Standard Approach**:
- √T weighted interpolation for variance
- Monotonic spline (Stineman) preferred over cubic
- Ensures no calendar spread arbitrage

## Our Use Case: Calendar Spread Arbitrage

**Key Insight**: We are TRADING the term structure (calendar spreads around earnings).

If we impose no-arbitrage constraints on the term structure:
- We would smooth out the very inefficiencies we're trying to capture
- IV term structure dislocations (short-term IV spike vs long-term) are our alpha source
- Earnings IV crush is fundamentally a calendar arbitrage opportunity

**Therefore**: We should NOT interpolate across expirations for pricing.

## Decision

### What We Should Do

1. **Strike/Moneyness Dimension**: Use available data at target expiration only
   - If target strike is between available strikes → linear interpolation
   - If target strike is outside available strikes → flat extrapolation (use nearest)
   - Acceptable for backtesting; captures most of the value

2. **Expiry Dimension**: NO cross-expiry interpolation
   - Price each expiration slice independently
   - If no data at target expiration → FAIL (don't guess from other expirations)
   - Preserves term structure information for calendar spread trades

### What We Should NOT Do

- Don't impose calendar arbitrage-free constraints
- Don't interpolate IV across expirations
- Don't use SSVI (surface SVI) which ties expirations together

## Future Enhancement (Optional)

If flat extrapolation proves too crude:
- Implement SVI per-slice fitting (5 params per expiration)
- Better wing behavior
- Still no cross-expiry constraints

## CRBG Investigation (2025-01-04)

### Root Cause: Exit Time Liquidity

Investigation of CRBG strike 32 pricing failure revealed it's a **liquidity issue**, not a code bug:

| Time | Data Available |
|------|----------------|
| Entry 15:55 ET (Nov 3) | ✅ Dec 19 calls [34, 35] with valid IVs |
| Exit 9:45 ET (Nov 4) | ❌ **0 bars** - no data yet |

**Key Finding:**
- Exit time: 9:45 AM ET = 14:45 UTC
- First CRBG trade on Nov 4: 14:47 UTC (2 minutes LATE)
- First Dec 19 call on Nov 4: 15:01 UTC (16 minutes LATE)

The flat extrapolation code exists and works correctly, but requires at least ONE data point at the target expiration. For illiquid stocks like CRBG, early morning exit has no data.

### Implications

1. **Not a code bug**: Flat extrapolation works when data exists
2. **Filter for liquidity**: Skip symbols with insufficient trading activity
3. **Consider later exit time**: 10:00 AM or 10:30 AM would have data

## Sources

- [Gatheral & Jacquier - Arbitrage-free SVI (2012)](https://arxiv.org/pdf/1204.0646)
- [Le Floc'h - Arbitrages in Vol Surface Interpolation](https://www.ssrn.com/abstract=2175001)
- [Roger Lee - Moment Formula at Extreme Strikes](https://math.uchicago.edu/~rl/moment.pdf)
- [Gatheral - The Volatility Surface (Bloomberg 2013)](https://mfe.baruch.cuny.edu/wp-content/uploads/2013/04/BloombergSVI2013.pdf)
