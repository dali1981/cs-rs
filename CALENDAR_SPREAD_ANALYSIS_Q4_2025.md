# Calendar Spread Backtest Analysis - Q4 2025

**Period:** October 1 - November 30, 2025
**Total Trades:** 2,201 (successful)
**Unique Symbols:** 2,200+ earnings events

---

## Executive Summary

Analyzed 2,201 calendar spread trades around earnings events. Found **strong predictive patterns** based on IV term structure that produce significant edge.

### Key Findings

| Metric | All Trades | IV Ratio ≥ 1.2 | IV Ratio < 1.2 |
|--------|------------|----------------|----------------|
| Trade Count | 2,201 | 786 (35.7%) | 1,144 |
| Win Rate | 46.2% | **58.0%** | 34.4% |
| Avg Return | 17.0% | **+44.1%** | -12.9% |
| Total P&L | -$143 | **+$185** | -$390 |

**EDGE: +23.7 percentage points win rate improvement when IV Ratio ≥ 1.2**

---

## IV Term Structure Analysis

### Entry Statistics

| Metric | Average | Median |
|--------|---------|--------|
| Short Leg IV | 81.5% | 68.3% |
| Long Leg IV | 69.0% | 56.4% |
| IV Ratio (Short/Long) | 1.208 | 1.188 |

**Interpretation**: On average, short-dated options trade at ~21% higher IV than long-dated options before earnings (contango term structure).

### IV Behavior Categories

| Pattern | Trades | Win Rate | Avg Return |
|---------|--------|----------|------------|
| **Ideal: Short crush, Long hold** | 347 | **89.9%** | **+150.8%** |
| Both crush | 755 | 42.4% | +9.9% |
| Other | 584 | 41.1% | -9.7% |
| Short IV rose | 515 | 28.0% | -32.5% |

**Critical Insight**: When short IV drops >5pp while long IV holds (drops <5pp), win rate is **90%** with **+150% avg return**!

---

## Entry Conditions Matrix

### Win Rate by Term Structure × IV Level

| Term Structure | Low IV (<40%) | Medium (40-60%) | High (60-80%) | Very High (80-100%) | Extreme (>100%) |
|----------------|---------------|-----------------|---------------|---------------------|-----------------|
| Backwardation (<1.0) | 21% | 21% | 21% | 12% | 19% |
| Flat (1.0-1.1) | 41% | 31% | 31% | 35% | 34% |
| Contango (1.1-1.2) | 53% | 46% | 43% | 43% | 40% |
| Steep (1.2-1.3) | **66%** | **61%** | 51% | 48% | 54% |
| Very Steep (>1.3) | **75%** | **64%** | **68%** | 56% | 56% |

### Best Entry Conditions (min 30 trades)

| Setup | Trades | Win Rate | Avg Return |
|-------|--------|----------|------------|
| Very Steep + Low IV | 56 | **75.0%** | +49.7% |
| Very Steep + High IV | 108 | **67.6%** | +58.9% |
| Steep + Low IV | 58 | **65.5%** | +68.8% |
| Very Steep + Medium IV | 116 | **63.8%** | +109.7% |
| Steep + Medium IV | 99 | **60.6%** | +43.4% |

---

## Greeks P&L Decomposition

| Greek | All Trades | Winners | Losers |
|-------|------------|---------|--------|
| Delta | -$0.0019 | +$0.0109 | -$0.0130 |
| Gamma | -$0.1980 | -$0.0825 | -$0.2982 |
| Theta | +$0.1107 | +$0.1145 | +$0.1074 |
| **Vega** | +$0.0526 | **+$0.8401** | **-$0.6312** |

### Vega is the Dominant Factor

- **Vega contributes 131% of average P&L** (more than total due to offsetting Greeks)
- Winners: Vega P&L = +$0.84 avg
- Losers: Vega P&L = -$0.63 avg
- **The trade is essentially a bet on IV term structure normalization**

---

## Pattern for Winners vs Losers

| Metric | Winners | Losers | Difference |
|--------|---------|--------|------------|
| Avg Short IV | 81.4% | 81.6% | -0.2pp |
| Avg Long IV | 63.3% | 73.9% | **-10.6pp** |
| Avg IV Ratio | **1.29** | 1.14 | **+0.14** |
| Short IV Change | **-17.2pp** | -0.1pp | **-17.0pp** |
| Long IV Change | +1.9pp | -13.1pp | **+15.0pp** |
| Avg Entry Cost | $0.98 | $1.41 | -$0.43 |

### What Makes a Winner?

1. **Higher IV Ratio at Entry (1.29 vs 1.14)** - Steeper term structure
2. **Short IV Crushes More (-17pp vs -0.1pp)** - Earnings IV premium evaporates
3. **Long IV Holds/Rises (+1.9pp vs -13pp)** - Back-month IV doesn't collapse
4. **Lower Entry Cost ($0.98 vs $1.41)** - Tighter spread at entry

---

## Optimal IV Ratio Range

| IV Ratio Range | Trades | Win Rate | Avg Return |
|----------------|--------|----------|------------|
| 0.8-1.0 | 228 | 20.2% | -29.6% |
| 0.9-1.1 | 552 | 31.2% | -19.5% |
| **1.1-1.3** | 800 | **50.7%** | **+18.3%** |
| 1.2-1.3 | 406 | **45.8%** | -0.5% |
| ≥1.3 | 663 | **58.9%** | +49.3% |

**Sweet Spot: IV Ratio 1.1-1.3 with 50.7% win rate and +18.3% avg return**

---

## Trade Setup Criteria

### Recommended Entry Criteria

```
IF iv_ratio >= 1.2 AND short_iv >= 0.50 THEN
    ENTER calendar spread
```

### Expected Results

| Filter | Trades | Win Rate | Avg Return | Total P&L |
|--------|--------|----------|------------|-----------|
| No filter | 2,201 | 46.2% | +17.0% | -$143 |
| IV Ratio ≥ 1.2 | 1,057 | 58.9% | +49.3% | +$247 |
| IV Ratio ≥ 1.2 + IV ≥ 50% | 786 | 58.0% | +44.1% | +$185 |

**Filtering improves Total P&L from -$143 to +$185** (swing of +$328)

---

## Missing Features for Better Analysis

### Currently Available
- ✅ IV at entry and exit (short and long legs)
- ✅ IV ratio (term structure indicator)
- ✅ Greeks P&L attribution (delta, gamma, theta, vega)
- ✅ Spot price at entry and exit
- ✅ Calendar spread P&L

### Would Improve Analysis
- ❌ **Historical Volatility (HV)** at entry - to compare IV vs realized
- ❌ **IV Percentile/Rank** - is this IV high or low relative to history?
- ❌ **Time Series IV Before Earnings** - how did IV evolve in days leading up?
- ❌ **Expected Move from Straddle** - ATM straddle price at entry
- ❌ **Post-Earnings Actual Move** - to validate IV crush magnitude
- ❌ **Sector/Industry** - to find sector-specific patterns
- ❌ **Market Regime** - VIX level, SPX returns context

### To Generate Time Series Analysis

Need to run:
```bash
./target/release/cs atm-iv \
    --symbols AAPL,MSFT,... \
    --start 2025-10-01 \
    --end 2025-11-30 \
    --minute-aligned \
    --constant-maturity \
    --with-hv \
    --output ./output/iv_timeseries_q4
```

Then join with earnings dates to analyze IV evolution.

---

## Trading Strategy Recommendations

### Primary Strategy: Steep Contango Calendar Spreads

**Entry Criteria:**
1. IV Ratio ≥ 1.2 (Short IV / Long IV)
2. Short Leg IV ≥ 50%
3. Enter 1 day before earnings
4. Select ATM strikes

**Expected Performance:**
- Win Rate: ~58%
- Avg Return: ~44%
- Edge vs Random: +24pp in win rate

**Risk Management:**
- Max 1-2% of capital per trade
- Know that 42% of trades still lose
- Average loser: -77% of premium paid

### Secondary Strategy: Ideal IV Behavior Targeting

**Look for setups where:**
1. Short IV is likely to crush significantly (>10pp)
2. Long IV is likely to hold steady (<5pp change)

**Indicators of this pattern:**
- Very steep contango (IV Ratio > 1.3)
- Short leg near expiration (< 7 DTE)
- Long leg has multiple catalysts ahead

---

## Visualization Files

1. **`./output/calendar_spread_analysis.png`** - 6-panel basic analysis
   - IV Ratio vs Return scatter
   - Short IV vs Return scatter
   - Win rate by IV ratio bucket
   - P&L distribution
   - IV change scatter (short vs long)
   - Greeks P&L attribution

2. **`./output/iv_evolution_analysis.png`** - 6-panel IV deep dive
   - IV Ratio distribution (winners vs losers)
   - Short IV change distribution
   - Long IV change distribution
   - Win rate heatmap (IV ratio × IV level)
   - Cumulative P&L by IV ratio filter
   - Return distribution by IV ratio bucket

---

## Data Files

- **Backtest Results:** `./output/backtest_q4_2025.json`
- **Analysis Scripts:**
  - `analyze_calendar_spreads.py`
  - `analyze_iv_evolution.py`

---

## Conclusions

1. **IV Ratio is the Key Predictor** - Steeper term structure (higher ratio) leads to better outcomes

2. **Vega Dominates P&L** - Calendar spreads are fundamentally vega trades; delta/gamma/theta are secondary

3. **Filter for Edge** - Using IV Ratio ≥ 1.2 filter turns a losing strategy (-$143) into a winning one (+$185)

4. **Ideal Pattern is Rare but Profitable** - When short IV crushes and long IV holds, 90% win rate with 150% avg return

5. **Not All Earnings Are Equal** - Win rate ranges from 20% (backwardation) to 75% (steep contango + low IV)

---

**Analysis Date:** 2026-01-03
**Data Source:** finq (Polygon) + nasdaq-earnings
