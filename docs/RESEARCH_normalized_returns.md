# Research Report: Normalized Returns for Options Trading

## Executive Summary

This document analyzes the problem of misleading return metrics in options backtesting and proposes a solution using exposure-normalized returns. The key insight is that **simple percentage returns are meaningless for options** because position sizes vary dramatically based on premium levels, not risk exposure.

## The Problem

### Current Backtest Output Shows Contradictory Metrics

```
Mean Return (simple)   | 1279.61%      ← Extremely positive!
Return on Capital      | -9.30%        ← Negative!
Capital-Weighted Return| -9.31%        ← Negative!
Std Dev                | 35304.81%     ← Extreme outliers!
```

**Root Cause:** Simple mean return treats all trades equally regardless of:
1. Capital deployed (a $50 trade vs a $500 trade)
2. Risk exposure (vega, delta, gamma)
3. Notional value (underlying price × 100)

### Why This Happens

Consider two straddle trades:

| Trade | Premium | Vega | Notional | P&L | % Return |
|-------|---------|------|----------|-----|----------|
| A (GME) | $50 | 0.08 | $2,000 | +$200 | **+400%** |
| B (AAPL) | $800 | 0.45 | $17,500 | -$150 | **-19%** |

**Simple mean:** `(400 + (-19)) / 2 = +190.5%` — Looks great!

**Reality:** You lost $50 net (`200 - 150 - 100 = -50` including another trade)

The problem: Trade A had 1/10th the capital but gets equal weight in the average.

## Industry Standard Solutions

### 1. Vega-Weighted Return (Primary for Volatility Strategies)

**Rationale:** Straddles are volatility bets. The "size" of your bet is your vega exposure, not your premium.

**Formula:**
```
Vega-Weighted Return = Σ(Dollar_Vega_i × Return_i) / Σ(Dollar_Vega_i)

Where:
  Dollar_Vega = Net_Vega × Contract_Multiplier (typically 100)
  Return_i = P&L_i / Capital_i (return on capital for trade i)
```

**Interpretation:** "What was my average return, weighted by how much volatility exposure I had?"

**Example:**
```
Trade A: Vega=0.08, Return=+400%  → Weight = 8
Trade B: Vega=0.45, Return=-19%   → Weight = 45

Vega-Weighted = (8 × 4.00 + 45 × (-0.19)) / (8 + 45)
              = (32 - 8.55) / 53
              = 44.2%
```

Still positive, but **much more realistic** than +190.5%.

### 2. Notional-Weighted Return (For Cross-Symbol Comparison)

**Rationale:** Normalizes by underlying value, enabling comparison across different-priced underlyings.

**Formula:**
```
Notional-Weighted Return = Σ(Notional_i × Return_i) / Σ(Notional_i)

Where:
  Notional = Strike × 100 (for straddles at the strike)
```

**Use Case:** Comparing AAPL ($175) straddles vs GME ($20) straddles fairly.

### 3. Variance Risk Premium (The True "Edge" Metric)

**Rationale:** For volatility strategies, your edge is whether realized volatility exceeds implied volatility.

**Formula:**
```
VRP_Captured = (IV_Entry² - RV_Realized²) × (DTE / 365)
```

**Interpretation:** Positive = short vol won, Negative = long vol won.

## Comparison of Return Metrics

| Metric | Formula | Denominator | Best For |
|--------|---------|-------------|----------|
| Simple Mean | `Σ(return_i) / N` | Count | Never (misleading) |
| Capital-Weighted | `Σ(capital_i × return_i) / Σ(capital_i)` | Premium | Capital efficiency |
| **Vega-Weighted** | `Σ(vega_i × return_i) / Σ(vega_i)` | Vol exposure | Straddles, vol strategies |
| **Notional-Weighted** | `Σ(notional_i × return_i) / Σ(notional_i)` | Underlying value | Cross-symbol comparison |
| Delta-Weighted | `Σ(delta_i × return_i) / Σ(delta_i)` | Directional exp. | Directional strategies |

## Academic and Industry References

1. **Basel III Capital Adequacy** — Uses delta-adjusted notional for regulatory capital
2. **Taleb, "Dynamic Hedging"** — Dollar gamma as exposure unit for gamma scalping
3. **Natenberg, "Option Volatility & Pricing"** — Vega exposure for vol strategies
4. **CBOE Volatility Indexes** — Variance-weighted (vega²) for VIX calculation

## Recommendations

### For Straddle/Strangle Strategies

1. **Primary Metric:** Vega-Weighted Return
   - Captures "return per unit of volatility bet"
   - Comparable across different premium levels
   - Aligns with the actual bet being made

2. **Secondary Metric:** Notional-Weighted Return
   - Enables cross-symbol comparison
   - Regulatory-compatible (Basel III)
   - Intuitive: "% of underlying value captured"

3. **Keep:** Capital-Weighted Return
   - Still valuable for capital efficiency analysis
   - Required for portfolio management

4. **Deprecate:** Simple Mean Return
   - Actively misleading
   - Should be removed from primary display or labeled as "informational only"

### Additional Metrics to Add

1. **Total Vega Exposure:** Sum of dollar vega across all trades
2. **Average Vega per Trade:** Total vega / trade count
3. **Vega-Weighted Sharpe:** Sharpe ratio using vega-weighted returns
4. **Notional Coverage:** Total notional / total capital (leverage ratio)

## Implementation Notes

### Data Requirements

The following data is already available in trade results:
- `net_vega: Option<f64>` — Vega at entry
- `strike: Decimal` — Strike price for notional calculation
- `realized_pnl: Decimal` — P&L for return calculation
- `capital_required: Decimal` — Capital for return on capital

### Calculation Considerations

1. **Missing Vega Data:** Some trades may have `None` for vega. Options:
   - Exclude from vega-weighted calculation
   - Use implied vega from straddle approximation: `Vega ≈ S × sqrt(T/365) / (σ × 25)`

2. **Aggregation:** Use the same pattern as capital-weighted return:
   ```rust
   let weighted_sum = trades.iter()
       .filter_map(|t| t.net_vega.map(|v| v * t.return_on_capital()))
       .sum();
   let total_vega = trades.iter()
       .filter_map(|t| t.net_vega)
       .sum();
   vega_weighted_return = weighted_sum / total_vega;
   ```

3. **Display:** Add new section "Exposure-Normalized Metrics" in console output.

## Conclusion

Simple percentage returns are fundamentally flawed for options analysis. By implementing vega-weighted and notional-weighted returns, we align our metrics with:

1. The actual bet being made (volatility exposure)
2. Industry standards (Basel III, institutional trading desks)
3. Academic best practices (Taleb, Natenberg)

The capital-weighted return already implemented is a step in the right direction, but vega-weighted return is the correct primary metric for volatility strategies like straddles.
