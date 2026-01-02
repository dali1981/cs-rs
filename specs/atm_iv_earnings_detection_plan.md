# ATM IV Earnings Detection - Research & Implementation Plan

## Objective

Create a script/use case that computes daily ATM IV time series for stocks to detect earnings dates from IV patterns. This will help identify earnings events for stocks where we lack explicit earnings data.

## Background: IV Behavior Around Earnings

### Key Patterns to Detect

1. **Pre-Earnings IV Ramp**: IV typically climbs days/weeks before earnings, accelerating in final 24-48 hours
2. **IV Crush**: Post-earnings, IV drops 30-40%+ as uncertainty resolves
3. **Term Structure Inversion**: Near-term IV exceeds longer-term IV (backwardation) before earnings
4. **Earnings Effect Concentration**: Options expiring just after earnings show largest IV elevation

### Why ATM IV?

- ATM options have highest vega sensitivity and liquidity
- Least affected by skew distortions (smile effects)
- Best representation of "at-the-money" market uncertainty
- Averaging calls + puts reduces noise from directional positioning

---

## Implementation Architecture

### Phase 1: Core ATM IV Computation

**Domain Service**: `AtmIvComputer`

```
Input:
  - symbol: String
  - date: NaiveDate (EOD observation date)
  - spot_price: Decimal
  - option_chain: DataFrame (from FinqOptionsRepository)

Output:
  - AtmIvObservation {
      symbol: String,
      date: NaiveDate,
      spot: Decimal,
      atm_iv_30d: Option<f64>,  // ~30 DTE
      atm_iv_60d: Option<f64>,  // ~60 DTE
      atm_iv_90d: Option<f64>,  // ~90 DTE
      term_spread_30_60: Option<f64>,  // IV_30 - IV_60
      term_spread_30_90: Option<f64>,  // IV_30 - IV_90
    }
```

**ATM Strike Selection Logic**:
1. Get spot price at EOD
2. Find strike closest to spot (minimize |strike - spot|)
3. If equidistant, prefer lower strike (conservative)

**IV Averaging Strategy**:
1. Get call IV at ATM strike
2. Get put IV at ATM strike
3. ATM IV = (call_iv + put_iv) / 2
4. This cancels out skew bias and directional positioning effects

**Maturity Selection** (configurable):
- Target DTEs: [30, 60, 90] days (default)
- Tolerance window: +/- 7 days for each target
- Select closest available expiration within window
- If multiple expirations in window, use closest to target DTE

### Phase 2: Time Series Generation

**Use Case**: `GenerateIvTimeSeriesUseCase`

```
Input:
  - symbols: Vec<String> (or all available)
  - start_date: NaiveDate
  - end_date: NaiveDate
  - maturity_targets: Vec<u32> = [30, 60, 90]
  - maturity_tolerance: u32 = 7

Output:
  - DataFrame with columns:
    [symbol, date, spot, atm_iv_30d, atm_iv_60d, atm_iv_90d,
     term_spread_30_60, term_spread_30_90]
```

**Processing Flow**:
```
1. For each symbol:
   a. Get list of available trading days in date range
   b. For each trading day:
      - Load option chain (FinqOptionsRepository)
      - Load spot price (FinqEquityRepository)
      - Compute ATM IV for each maturity target
      - Calculate term spreads
      - Store observation
   c. Aggregate into DataFrame
2. Save as parquet: {output_dir}/atm_iv_timeseries_{symbol}.parquet
```

### Phase 3: Earnings Detection Algorithm

**Heuristic Approach** (configurable thresholds):

```
Signals for potential earnings:
1. IV Spike: 30d ATM IV increases > X% over Y days
2. IV Crush: 30d ATM IV drops > Z% in 1 day
3. Term Structure Inversion: 30d IV > 60d IV (backwardation)
4. Absolute IV Elevation: 30d IV > historical mean + N stddev

Default thresholds (tunable):
- X = 20% (spike threshold)
- Y = 5 days (lookback window)
- Z = 15% (crush threshold)
- N = 2.0 (standard deviations)
```

**Detection Logic**:
```python
def detect_earnings_candidates(iv_series):
    candidates = []
    for i in range(len(iv_series)):
        # Check for IV crush (post-earnings signal)
        if iv_series[i] / iv_series[i-1] < 0.85:  # 15% drop
            candidates.append((date[i], "iv_crush"))

        # Check for IV spike (pre-earnings signal)
        if i >= 5:
            five_day_change = iv_series[i] / iv_series[i-5]
            if five_day_change > 1.20:  # 20% rise
                candidates.append((date[i], "iv_spike"))

        # Check for term structure inversion
        if term_spread_30_60[i] > 0.05:  # 5% backwardation
            candidates.append((date[i], "backwardation"))

    return candidates
```

### Phase 4: Visualization

**Plot Types**:

1. **ATM IV Time Series Plot**:
   - X-axis: Date
   - Y-axis: ATM IV (%)
   - Multiple lines for different maturities (30d, 60d, 90d)
   - Vertical lines for known earnings dates (if available)
   - Markers for detected earnings candidates

2. **Term Structure Plot**:
   - X-axis: Date
   - Y-axis: Term spread (30d - 60d)
   - Highlight periods of backwardation
   - Known earnings dates as vertical lines

3. **Earnings Detection Overlay**:
   - Combine IV time series with detection markers
   - Color-coded: green=spike, red=crush, orange=backwardation
   - Ground truth overlay if earnings data available

**Output Format**:
- PNG files: `{output_dir}/plots/{symbol}_atm_iv.png`
- Interactive HTML (optional): Using plotly

---

## Data Requirements

### Input Data Sources

| Data | Source | Repository |
|------|--------|------------|
| Option chains | finq_flatfiles | `FinqOptionsRepository` |
| Spot prices | finq_flatfiles | `FinqEquityRepository` |
| Known earnings | earnings-rs | `EarningsReaderAdapter` |

### Required Columns from Option Chain

```
strike: f64
expiration: Date
close: f64 (mid price)
option_type: String ("call" / "put")
```

### Output Schema (Parquet)

```
symbol: String
date: Date
spot: f64
atm_iv_30d: f64 (nullable)
atm_iv_60d: f64 (nullable)
atm_iv_90d: f64 (nullable)
term_spread_30_60: f64 (nullable)
term_spread_30_90: f64 (nullable)
```

---

## Configuration Options

```rust
pub struct AtmIvConfig {
    // Maturity selection
    pub maturity_targets: Vec<u32>,     // Default: [30, 60, 90]
    pub maturity_tolerance: u32,        // Default: 7 days

    // ATM definition
    pub atm_strike_method: AtmMethod,   // Closest, BelowSpot, AboveSpot

    // IV calculation
    pub iv_min_bound: f64,              // Default: 0.01
    pub iv_max_bound: f64,              // Default: 5.0

    // Detection thresholds
    pub spike_threshold: f64,           // Default: 0.20 (20%)
    pub spike_lookback_days: usize,     // Default: 5
    pub crush_threshold: f64,           // Default: 0.15 (15%)
    pub backwardation_threshold: f64,   // Default: 0.05 (5%)

    // Output
    pub output_format: OutputFormat,    // Parquet, CSV, Both
    pub generate_plots: bool,           // Default: true
}
```

---

## Implementation Steps

### Step 1: Domain Layer Extensions

1. Create `AtmIvObservation` value object in `cs-domain`
2. Create `AtmIvConfig` configuration struct
3. Define `AtmIvRepository` trait for persistence

### Step 2: Analytics Layer

1. Create `atm_iv_computer.rs` in `cs-analytics`
2. Implement ATM strike selection logic
3. Implement IV averaging (call + put)
4. Implement maturity bucketing

### Step 3: Use Case Implementation

1. Create `cs-atm-iv` crate (or add to `cs-backtest`)
2. Implement `GenerateIvTimeSeriesUseCase`
3. Implement batch processing with progress reporting

### Step 4: CLI Integration

1. Add `atm-iv` subcommand to `cs-cli`
2. Options:
   - `--symbols`: Comma-separated or "all"
   - `--start-date`, `--end-date`: Date range
   - `--maturities`: Target DTEs (default: 30,60,90)
   - `--output-dir`: Output location
   - `--plot`: Generate plots (flag)

### Step 5: Visualization

1. Use `plotters` crate for Rust-native plotting
2. Alternative: Output CSV and use Python matplotlib
3. Create comparison plots with known earnings overlay

---

## Validation Strategy

### Ground Truth Comparison

1. Run on stocks with known earnings dates
2. Compare detected candidates vs actual earnings
3. Metrics:
   - Precision: % of detected candidates that are actual earnings
   - Recall: % of actual earnings detected
   - Lead time: How many days before earnings is spike detected?

### Test Cases

| Stock | Known Earnings | Expected IV Pattern |
|-------|---------------|---------------------|
| AAPL | Quarterly | Strong spike, reliable crush |
| NVDA | Quarterly | High volatility, large crush |
| TSLA | Quarterly | Very high IV, dramatic moves |
| KO | Quarterly | Low volatility, subtle patterns |

### Edge Cases to Handle

1. **Missing data**: Some days may have no options data
2. **Illiquid options**: Wide bid-ask spreads distort IV
3. **Corporate actions**: Splits, dividends affect strike structure
4. **Holiday effects**: Reduced trading around holidays
5. **Multiple events**: M&A, FDA approvals also spike IV

---

## Alternative Approaches (Future Consideration)

### Machine Learning Detection

- Train classifier on labeled earnings dates
- Features: IV level, IV change, term spread, volume
- Could improve detection accuracy

### VIX-Relative IV

- Normalize stock IV by market VIX
- Reduces false positives from market-wide volatility events

### Earnings Jump Extraction

- ORATS-style "earnings effect" calculation
- Decompose total IV into:
  - Base volatility
  - Earnings jump component
  - Calendar effects

---

## References

- [ORATS: Volatility Around Earnings](https://orats.com/university/volatility-around-earnings)
- [MenthorQ: IV Crush Guide](https://menthorq.com/guide/iv-crush-understanding-the-earnings-driven-volatility-spike-and-how-to-capitalize-on-it/)
- [PyQuant: Volatility Term Structure](https://www.pyquantnews.com/the-pyquant-newsletter/understanding-volatility-term-structure-and-skew)
- [ORATS: Term Structure Parameters](https://orats.com/blog/implied-volatility-term-structures-three-parameters)

---

## Questions for User

1. **Scope**: Start with a few test symbols or all available data?
2. **Date Range**: What time period should we analyze?
3. **Output**: Rust-native plots (`plotters`) or Python visualization?
4. **Crate Structure**: New crate `cs-atm-iv` or extend `cs-backtest`?
5. **Priority**: Focus on detection accuracy or quick visualization first?
