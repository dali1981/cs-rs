# Feature: Expected Move from Straddle & Earnings Analysis

## Overview

Compute expected move from ATM straddle prices, compare to actual moves on earnings, and analyze whether gamma outweighs vega (i.e., whether realized moves exceed market expectations).

---

## Methodology Reference

### Expected Move Formulas

#### Method 1: 85% Rule (Simple, DTE-Dependent)
```
Expected Move ($) = ATM Straddle Price × 0.85
Expected Move (%) = (ATM Straddle × 0.85) / Spot × 100
```

**When it works:**
- Using **nearest weekly expiration** (1-7 DTE)
- For **single earnings events**
- Straddle premium is mostly event-driven

**When it fails:**
- Longer-dated options (30+ DTE) - time value dominates
- Multiple events before expiration
- High baseline volatility environments

**Why 0.85:** Empirical decomposition:
```
Straddle Price = Earnings Premium + Residual Time Value
Earnings Premium ≈ 0.85 × Straddle (observed for short-dated options)
```

#### Method 2: IV-Based (Recommended, More Accurate)

**For move to expiration:**
```
Expected Move (%) = IV × sqrt(DTE / 365) × 100
```

**For 1-day move (earnings):**
```
Expected 1-Day Move (%) = IV / sqrt(252) × 100
                        ≈ IV / 15.87
```

Example: If IV = 45% annualized:
- 1-Day Move = 45% / 15.87 = **2.84%**
- 7-Day Move = 45% × sqrt(7/365) = **6.23%**
- 30-Day Move = 45% × sqrt(30/365) = **12.90%**

#### Method 3: Straddle Approximation (Black-Scholes Derived)
```
Straddle ≈ 0.8 × Spot × σ × sqrt(T)

Solving for IV:
σ ≈ Straddle / (0.8 × Spot × sqrt(T))
```

Where T = time to expiration in years (DTE/365).

### Isolating Earnings Premium (ORATS Method)

More sophisticated approach to separate earnings-specific IV:
```
Total IV = Base IV + Earnings Effect IV
Earnings IV = Total_IV_30d - Ex_Earnings_IV_30d
1-Day Earnings Move = Earnings_IV / sqrt(252)
```

With constant-maturity IV, the term spread approximates this:
```
Earnings Premium ≈ cm_iv_7d - cm_iv_30d
```
When spread > 5pp, earnings event is being priced.

### Gamma vs Vega Analysis

**P&L Decomposition for delta-hedged straddle:**
```
P&L = Gamma P&L + Vega P&L + Theta P&L

Gamma P&L = 0.5 × Γ × (ΔSpot)²      # Profits from realized movement
Vega P&L  = ν × ΔIV                  # Usually negative post-earnings (IV crush)
Theta P&L = θ × Δt                   # Time decay (negative for long straddle)
```

**Net outcome simplified:**
```
Straddle P&L ∝ Realized Volatility - Implied Volatility
```

**Gamma Dominates When:**
```
|Actual Move| > Expected Move
```
This means realized volatility exceeded implied volatility.

### Academic Research Findings

From SSRN and Journal of Banking & Finance research:
- Options market generally prices earnings moves accurately on average
- Distribution of earnings moves is **fat-tailed** (more extreme moves than normal)
- IV overstates realized vol ~85% of the time in non-event periods
- Earnings events are approximately symmetric (equal up/down probability)
- The **S-jump measure** (jump risk component of straddle) increases substantially before earnings

---

## Data Requirements

### Inputs Needed
1. **ATM Call + Put prices** at earnings entry (pre-earnings close)
2. **Spot price** at entry and exit
3. **Earnings date and time** (BMO/AMC)
4. **Pre-earnings IV** (from existing ATM IV pipeline)
5. **Post-earnings IV** (for IV crush calculation)

### Currently Available
- ✅ `EarningsEvent` entity with `earnings_date`, `earnings_time`
- ✅ ATM IV time series from `MinuteAlignedIvUseCase`
- ✅ Spot prices via `FinqEquityRepository`
- ✅ Option prices via `FinqOptionsRepository`
- ❌ Pre/post earnings straddle prices (need to compute)
- ❌ Earnings outcome tracking (actual move)

---

## Implementation Plan

### Phase 1: Data Model for Expected Move

#### 1.1 Add `ExpectedMoveObservation` (cs-domain/src/value_objects.rs)

```rust
/// Expected move observation for a trading day
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedMoveObservation {
    pub symbol: String,
    pub date: NaiveDate,
    pub spot: Decimal,

    // ATM Straddle components
    pub atm_strike: Decimal,
    pub atm_call_price: Option<Decimal>,
    pub atm_put_price: Option<Decimal>,
    pub straddle_price: Option<Decimal>,

    // Expected move calculations
    pub expected_move_pct: Option<f64>,      // Straddle / Spot (as %)
    pub expected_move_85_pct: Option<f64>,   // × 0.85 rule

    // For specific DTE targets
    pub dte: Option<i64>,
    pub iv_derived_move: Option<f64>,        // IV × sqrt(DTE/365)

    // Earnings context
    pub is_earnings_day: bool,
    pub days_to_earnings: Option<i64>,
}
```

#### 1.2 Add `EarningsOutcome` (cs-domain/src/value_objects.rs)

```rust
/// Actual earnings outcome for comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarningsOutcome {
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,

    // Pre-earnings state (close before)
    pub pre_spot: Decimal,
    pub pre_straddle: Decimal,
    pub expected_move_pct: f64,
    pub pre_iv_30d: f64,

    // Post-earnings state (open/close after)
    pub post_spot: Decimal,
    pub post_iv_30d: Option<f64>,

    // Actual move
    pub actual_move: Decimal,        // Absolute move in $
    pub actual_move_pct: f64,        // Move as % of pre_spot
    pub actual_direction: MoveDirection,

    // Comparison metrics
    pub move_ratio: f64,             // actual_move_pct / expected_move_pct
    pub iv_crush_pct: Option<f64>,   // (pre_iv - post_iv) / pre_iv
    pub gamma_dominated: bool,        // actual > expected
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MoveDirection {
    Up,
    Down,
    Flat,  // < 0.5% move
}
```

### Phase 2: Straddle Price Computation

#### 2.1 Add `StraddlePriceComputer` (cs-analytics/src/straddle.rs)

```rust
/// Compute ATM straddle price from option chain
pub struct StraddlePriceComputer;

impl StraddlePriceComputer {
    /// Find ATM strike and compute straddle price
    ///
    /// # Arguments
    /// * `options` - Option chain with strikes, prices, types
    /// * `spot` - Current spot price
    /// * `target_dte` - Optional specific DTE (None = nearest)
    /// * `atm_method` - Strike selection method (Closest, Below, Above)
    ///
    /// # Returns
    /// (atm_strike, call_price, put_price, straddle_price)
    pub fn compute_straddle(
        options: &[OptionPoint],
        spot: f64,
        target_dte: Option<i64>,
        atm_method: AtmMethod,
    ) -> Option<StraddlePrice>;

    /// Compute expected move from straddle
    pub fn expected_move(straddle_price: f64, spot: f64) -> f64 {
        (straddle_price / spot) * 100.0  // As percentage
    }

    /// Compute expected move with 85% rule
    pub fn expected_move_85(straddle_price: f64, spot: f64) -> f64 {
        (straddle_price * 0.85 / spot) * 100.0
    }
}

#[derive(Debug, Clone)]
pub struct StraddlePrice {
    pub strike: f64,
    pub call_price: f64,
    pub put_price: f64,
    pub straddle_price: f64,
    pub expiration: NaiveDate,
    pub dte: i64,
}
```

### Phase 3: Earnings Analysis Use Case

#### 3.1 Add `EarningsAnalysisUseCase` (cs-backtest/src/earnings_analysis_use_case.rs)

```rust
/// Use case for analyzing expected vs actual moves on earnings
pub struct EarningsAnalysisUseCase {
    equity_repo: Arc<dyn EquityDataRepository>,
    options_repo: Arc<dyn OptionsDataRepository>,
    earnings_repo: Arc<dyn EarningsRepository>,
}

impl EarningsAnalysisUseCase {
    /// Analyze all earnings events for a symbol
    ///
    /// For each earnings event:
    /// 1. Get pre-earnings straddle price (close before announcement)
    /// 2. Compute expected move
    /// 3. Get post-earnings spot price
    /// 4. Compare actual vs expected
    /// 5. Determine if gamma dominated
    pub async fn analyze_earnings(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<EarningsAnalysisResult>;

    /// Compute pre-earnings metrics
    async fn get_pre_earnings_state(
        &self,
        symbol: &str,
        earnings_event: &EarningsEvent,
    ) -> Result<PreEarningsState>;

    /// Compute post-earnings metrics
    async fn get_post_earnings_state(
        &self,
        symbol: &str,
        earnings_event: &EarningsEvent,
    ) -> Result<PostEarningsState>;
}

#[derive(Debug)]
pub struct EarningsAnalysisResult {
    pub symbol: String,
    pub outcomes: Vec<EarningsOutcome>,
    pub summary: EarningsSummaryStats,
}

#[derive(Debug)]
pub struct EarningsSummaryStats {
    pub total_events: usize,
    pub gamma_dominated_count: usize,  // Actual > Expected
    pub vega_dominated_count: usize,   // Actual < Expected
    pub avg_move_ratio: f64,           // Average of actual/expected
    pub avg_iv_crush_pct: f64,
    pub avg_expected_move_pct: f64,
    pub avg_actual_move_pct: f64,
    pub up_moves: usize,
    pub down_moves: usize,
}
```

### Phase 4: Expected Move Time Series

#### 4.1 Extend existing ATM IV pipeline

Add straddle/expected move computation to `MinuteAlignedIvUseCase`:

```rust
// In MinuteAlignedIvUseCase

/// Compute expected move alongside IV
fn compute_expected_move(
    &self,
    options: &[TimestampedOption],
    spot: f64,
    target_dte: i64,
) -> Option<ExpectedMoveData> {
    let straddle = StraddlePriceComputer::compute_straddle(
        options.as_option_points(),
        spot,
        Some(target_dte),
        AtmMethod::Closest,
    )?;

    Some(ExpectedMoveData {
        straddle_price: straddle.straddle_price,
        expected_move_pct: StraddlePriceComputer::expected_move(straddle.straddle_price, spot),
        expected_move_85_pct: StraddlePriceComputer::expected_move_85(straddle.straddle_price, spot),
        dte: straddle.dte,
    })
}
```

#### 4.2 Add to `AtmIvObservation`

```rust
pub struct AtmIvObservation {
    // ... existing fields ...

    // NEW: Expected Move fields
    pub straddle_price_nearest: Option<f64>,
    pub expected_move_pct: Option<f64>,      // Straddle/Spot %
    pub expected_move_85_pct: Option<f64>,   // × 0.85 rule

    // For 30-day options specifically
    pub straddle_price_30d: Option<f64>,
    pub expected_move_30d_pct: Option<f64>,
}
```

### Phase 5: CLI Commands

#### 5.1 New command: `cs earnings-analysis`

```rust
#[derive(Args)]
struct EarningsAnalysisArgs {
    #[arg(long, required = true)]
    symbols: String,

    #[arg(long)]
    start: String,

    #[arg(long)]
    end: String,

    #[arg(long, default_value = "csv")]
    format: String,  // csv, json, parquet

    #[arg(long)]
    output: Option<PathBuf>,
}
```

#### 5.2 Add `--with-expected-move` to `cs atm-iv`

```rust
#[arg(long, help = "Include expected move from straddle prices")]
with_expected_move: bool,
```

### Phase 6: Parquet Output Extension

New columns (31 total):
- Existing 21 + 5 HV = 26
- `straddle_price_nearest`, `straddle_price_30d` (f64)
- `expected_move_pct`, `expected_move_85_pct` (f64)
- `expected_move_30d_pct` (f64)

### Phase 7: Python Visualization

#### 7.1 Create `plot_expected_move.py`

```python
#!/usr/bin/env python3
"""
Plot Expected Move Time Series.

Panels:
1. Expected Move (%) from straddle with earnings markers
2. IV Term Structure with expected move overlay
3. Historical comparison: Expected vs Actual on earnings

Usage:
    python plot_expected_move.py <parquet_file> [--earnings-file <file>]
"""

import polars as pl
import matplotlib.pyplot as plt
from datetime import datetime, timedelta
import sys
import argparse

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('parquet_file')
    parser.add_argument('--earnings-file', help='Parquet with earnings outcomes')
    parser.add_argument('--output', help='Output PNG')
    args = parser.parse_args()

    df = pl.read_parquet(args.parquet_file)

    # Convert date
    epoch = datetime(1970, 1, 1)
    df = df.with_columns([
        pl.col('date').map_elements(lambda d: epoch + timedelta(days=d), return_dtype=pl.Datetime).alias('datetime')
    ])
    df_pd = df.to_pandas()
    symbol = df["symbol"].unique()[0]

    fig, axes = plt.subplots(3, 1, figsize=(16, 14), sharex=True)

    # Panel 1: Expected Move over time
    ax1 = axes[0]
    if 'expected_move_pct' in df_pd.columns:
        ax1.plot(df_pd['datetime'], df_pd['expected_move_pct'],
                 label='Expected Move (%)', color='#e74c3c', linewidth=2)
        ax1.plot(df_pd['datetime'], df_pd['expected_move_85_pct'],
                 label='Expected Move × 0.85', color='#3498db', linewidth=2, linestyle='--')
    ax1.set_ylabel('Expected Move (%)')
    ax1.set_title(f'{symbol} - Expected Move from ATM Straddle')
    ax1.legend()
    ax1.grid(True, alpha=0.3)

    # Panel 2: IV with straddle overlay
    ax2 = axes[1]
    ax2.plot(df_pd['datetime'], df_pd['cm_iv_7d'] * 100, label='7d IV', color='red', linewidth=1.5)
    ax2.plot(df_pd['datetime'], df_pd['cm_iv_30d'] * 100, label='30d IV', color='blue', linewidth=1.5)

    ax2_twin = ax2.twinx()
    if 'straddle_price_nearest' in df_pd.columns:
        ax2_twin.plot(df_pd['datetime'], df_pd['straddle_price_nearest'],
                      label='Straddle $', color='green', linewidth=1.5, alpha=0.7)
        ax2_twin.set_ylabel('Straddle Price ($)', color='green')

    ax2.set_ylabel('Implied Volatility (%)')
    ax2.set_title('IV and Straddle Price')
    ax2.legend(loc='upper left')
    ax2.grid(True, alpha=0.3)

    # Panel 3: If earnings outcomes available
    ax3 = axes[2]
    if args.earnings_file:
        earnings_df = pl.read_parquet(args.earnings_file).to_pandas()

        ax3.scatter(earnings_df['earnings_date'], earnings_df['expected_move_pct'],
                    label='Expected', color='blue', s=100, marker='o')
        ax3.scatter(earnings_df['earnings_date'], earnings_df['actual_move_pct'],
                    label='Actual', color='red', s=100, marker='x')

        # Draw lines connecting expected to actual
        for _, row in earnings_df.iterrows():
            color = 'green' if row['gamma_dominated'] else 'red'
            ax3.plot([row['earnings_date'], row['earnings_date']],
                     [row['expected_move_pct'], row['actual_move_pct']],
                     color=color, linewidth=2, alpha=0.5)

        ax3.axhline(y=0, color='gray', linestyle='--')
        ax3.legend()
    else:
        ax3.text(0.5, 0.5, 'Earnings outcomes not provided\n(use --earnings-file)',
                 ha='center', va='center', transform=ax3.transAxes, fontsize=14)

    ax3.set_ylabel('Move (%)')
    ax3.set_xlabel('Date')
    ax3.set_title('Expected vs Actual Moves on Earnings')
    ax3.grid(True, alpha=0.3)

    plt.tight_layout()

    output = args.output or f"{args.parquet_file.replace('.parquet', '')}_expected_move.png"
    plt.savefig(output, dpi=300, bbox_inches='tight')
    print(f"Saved: {output}")

if __name__ == '__main__':
    main()
```

#### 7.2 Create `earnings_analysis_report.py`

```python
#!/usr/bin/env python3
"""
Generate comprehensive earnings analysis report.

Outputs:
1. Summary statistics table
2. Scatter plot: Expected vs Actual
3. Move ratio histogram
4. IV crush distribution
5. Per-symbol breakdown

Usage:
    python earnings_analysis_report.py <earnings_outcomes.parquet>
"""

import polars as pl
import matplotlib.pyplot as plt
import numpy as np
import sys

def main():
    file = sys.argv[1]
    df = pl.read_parquet(file).to_pandas()

    print("=" * 60)
    print("EARNINGS ANALYSIS REPORT")
    print("=" * 60)

    # Summary stats
    n = len(df)
    gamma_wins = df['gamma_dominated'].sum()
    vega_wins = n - gamma_wins

    print(f"\nTotal Earnings Events: {n}")
    print(f"Gamma Dominated (Actual > Expected): {gamma_wins} ({100*gamma_wins/n:.1f}%)")
    print(f"Vega Dominated  (Actual < Expected): {vega_wins} ({100*vega_wins/n:.1f}%)")

    print(f"\nAverage Expected Move: {df['expected_move_pct'].mean():.2f}%")
    print(f"Average Actual Move:   {df['actual_move_pct'].mean():.2f}%")
    print(f"Average Move Ratio:    {df['move_ratio'].mean():.2f}x")

    if 'iv_crush_pct' in df.columns:
        print(f"Average IV Crush:      {df['iv_crush_pct'].mean()*100:.1f}%")

    # Create visualization
    fig, axes = plt.subplots(2, 2, figsize=(14, 12))

    # 1. Scatter: Expected vs Actual
    ax1 = axes[0, 0]
    colors = ['green' if g else 'red' for g in df['gamma_dominated']]
    ax1.scatter(df['expected_move_pct'], df['actual_move_pct'], c=colors, alpha=0.6, s=50)
    max_val = max(df['expected_move_pct'].max(), df['actual_move_pct'].max())
    ax1.plot([0, max_val], [0, max_val], 'k--', label='Expected = Actual')
    ax1.set_xlabel('Expected Move (%)')
    ax1.set_ylabel('Actual Move (%)')
    ax1.set_title('Expected vs Actual Earnings Moves')
    ax1.legend()
    ax1.grid(True, alpha=0.3)

    # 2. Move ratio histogram
    ax2 = axes[0, 1]
    ax2.hist(df['move_ratio'], bins=20, color='steelblue', edgecolor='black', alpha=0.7)
    ax2.axvline(x=1.0, color='red', linestyle='--', linewidth=2, label='Break-even')
    ax2.axvline(x=df['move_ratio'].mean(), color='green', linewidth=2, label=f"Mean: {df['move_ratio'].mean():.2f}")
    ax2.set_xlabel('Move Ratio (Actual / Expected)')
    ax2.set_ylabel('Frequency')
    ax2.set_title('Move Ratio Distribution')
    ax2.legend()
    ax2.grid(True, alpha=0.3)

    # 3. IV Crush distribution
    ax3 = axes[1, 0]
    if 'iv_crush_pct' in df.columns:
        crush = df['iv_crush_pct'].dropna() * 100
        ax3.hist(crush, bins=20, color='coral', edgecolor='black', alpha=0.7)
        ax3.axvline(x=crush.mean(), color='blue', linewidth=2, label=f"Mean: {crush.mean():.1f}%")
        ax3.set_xlabel('IV Crush (%)')
        ax3.set_ylabel('Frequency')
    ax3.set_title('IV Crush Distribution')
    ax3.legend()
    ax3.grid(True, alpha=0.3)

    # 4. Win rate by expected move size
    ax4 = axes[1, 1]
    df['expected_bucket'] = pd.cut(df['expected_move_pct'], bins=[0, 3, 5, 7, 10, 100])
    win_rates = df.groupby('expected_bucket', observed=True)['gamma_dominated'].mean() * 100
    win_rates.plot(kind='bar', ax=ax4, color='teal', edgecolor='black')
    ax4.axhline(y=50, color='red', linestyle='--')
    ax4.set_xlabel('Expected Move Range (%)')
    ax4.set_ylabel('Gamma Win Rate (%)')
    ax4.set_title('Gamma Win Rate by Expected Move Size')
    ax4.set_xticklabels(ax4.get_xticklabels(), rotation=45)
    ax4.grid(True, alpha=0.3)

    plt.tight_layout()
    output = file.replace('.parquet', '_report.png')
    plt.savefig(output, dpi=300, bbox_inches='tight')
    print(f"\nSaved visualization: {output}")

if __name__ == '__main__':
    import pandas as pd
    main()
```

---

## File Changes Summary

| File | Type | Description |
|------|------|-------------|
| `cs-domain/src/value_objects.rs` | Modify | Add `ExpectedMoveObservation`, `EarningsOutcome`, extend `AtmIvObservation` |
| `cs-analytics/src/lib.rs` | Modify | Re-export straddle module |
| `cs-analytics/src/straddle.rs` | **New** | `StraddlePriceComputer` |
| `cs-backtest/src/lib.rs` | Modify | Export earnings analysis |
| `cs-backtest/src/earnings_analysis_use_case.rs` | **New** | `EarningsAnalysisUseCase` |
| `cs-backtest/src/minute_aligned_iv_use_case.rs` | Modify | Add expected move computation |
| `cs-cli/src/main.rs` | Modify | Add `earnings-analysis` command, `--with-expected-move` flag |
| `plot_expected_move.py` | **New** | Expected move visualization |
| `earnings_analysis_report.py` | **New** | Earnings analysis report |

---

## Usage Examples

### Generate Expected Move Time Series

```bash
cs atm-iv --symbols AAPL,TSLA \
          --start 2025-01-01 \
          --end 2025-12-31 \
          --minute-aligned \
          --constant-maturity \
          --with-expected-move \
          --output ./output
```

### Analyze Earnings

```bash
cs earnings-analysis --symbols AAPL \
                     --start 2024-01-01 \
                     --end 2024-12-31 \
                     --output ./earnings/aapl_outcomes.parquet
```

### Visualize

```bash
# Plot expected move time series
uv run python3 plot_expected_move.py ./output/atm_iv_AAPL.parquet

# Generate earnings report with actual vs expected comparison
uv run python3 earnings_analysis_report.py ./earnings/aapl_outcomes.parquet
```

---

## Expected Output Example

```
============================================================
EARNINGS ANALYSIS REPORT
============================================================

Total Earnings Events: 4
Gamma Dominated (Actual > Expected): 2 (50.0%)
Vega Dominated  (Actual < Expected): 2 (50.0%)

Average Expected Move: 4.85%
Average Actual Move:   5.23%
Average Move Ratio:    1.08x
Average IV Crush:      35.2%
```

---

## Key Insights This Feature Enables

1. **Is the options market pricing earnings correctly?**
   - If `move_ratio > 1.0` on average → Market underestimates moves
   - If `move_ratio < 1.0` on average → Market overestimates moves

2. **When does gamma dominate vega?**
   - Track `gamma_dominated` flag per event
   - Analyze by sector, market cap, IV level

3. **IV crush severity**
   - Measure `(pre_iv - post_iv) / pre_iv`
   - Higher crush = more vega risk for long straddles

4. **Trading strategy implications**
   - If gamma consistently > vega: Long straddles profitable
   - If vega consistently > gamma: Short straddles profitable
   - Reality: Usually mixed, requiring event selection

---

## Formulas Reference

| Metric | Formula | Notes |
|--------|---------|-------|
| Expected Move (%) | `Straddle / Spot × 100` | Move to expiration |
| Expected Move 85% | `Straddle × 0.85 / Spot × 100` | Convenience rule for short DTE (1-7d) |
| Expected 1-Day Move | `IV / sqrt(252) × 100` | **PRIMARY for earnings** |
| Expected Move to Expiry | `IV × sqrt(DTE/365) × 100` | IV-based, DTE-adjusted |
| IV from Straddle | `Straddle / (0.8 × Spot × sqrt(T))` | Inverse calculation |
| Actual Move (%) | `\|Post_Spot - Pre_Spot\| / Pre_Spot × 100` | Absolute percentage move |
| Move Ratio | `Actual_Move / Expected_Move` | >1 = gamma wins |
| IV Crush | `(Pre_IV - Post_IV) / Pre_IV` | Vega loss measure |
| Gamma P&L | `0.5 × Γ × (ΔS)²` | Profits from movement |
| Vega P&L | `ν × ΔIV` | Usually negative post-earnings |
| Gamma Dominated | `Move_Ratio > 1.0` | Actual > Expected |

---

## Caveats & Limitations

1. **Timing matters**: AMC earnings use next-day open, BMO use same-day prices
2. **Gap risk**: Pre-market/after-hours moves not captured by daily data
3. **Straddle approximation**: Uses mid prices, ignores bid-ask spread
4. **Expiration selection**: Nearest expiration may have low liquidity
5. **Sample size**: Need multiple earnings events for statistical significance
6. **Market conditions**: Results may vary in high vs low vol regimes
