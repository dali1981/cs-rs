# Feature: Historical Volatility with IV Comparison

## Overview

Add historical (realized) volatility computation to the ATM IV pipeline and enable plotting HV alongside IV for volatility analysis.

## Current State

### Existing Infrastructure
1. **`realized_volatility()` function** exists in `cs-analytics/src/historical_iv.rs:28-56`
   - Takes: `prices: &[f64]`, `window: usize`, `annualization_factor: f64`
   - Returns: `Option<f64>` (annualized volatility as decimal)
   - Uses log returns and sample std deviation
   - **Note**: File is misnamed - contains realized vol, not historical IV

2. **Spot prices** are available via `FinqEquityRepository.get_bars(symbol, date)`
   - Returns DataFrame with columns: `timestamp`, `open`, `high`, `low`, `close`, `volume`
   - Minute-level granularity available

3. **AtmIvObservation** already stores `spot: Decimal` for each trading day

4. **Plotting scripts** use Polars -> Pandas -> Matplotlib pattern

### Naming Issue to Fix

The file `historical_iv.rs` is misnamed. It contains:
- `realized_volatility()` - computes HV from price returns
- `iv_percentile()` / `iv_rank()` - computes percentile/rank of current IV vs past IVs

These are **different concepts**:
| Term | Meaning |
|------|---------|
| **Historical IV** | What was the implied volatility in the past (time series of IV) |
| **Realized/Historical Volatility** | Volatility computed from actual price movements |

---

## Implementation Plan

### Phase 0: Refactor Existing Code (File Split)

Split `cs-analytics/src/historical_iv.rs` into properly named modules:

#### 0.1 Create `cs-analytics/src/realized_volatility.rs`

Move from `historical_iv.rs`:
```rust
/// Calculate realized volatility from price returns
///
/// Uses log returns and sample standard deviation, annualized.
///
/// # Arguments
/// * `prices` - Daily close prices (chronological order)
/// * `window` - Rolling window size (e.g., 20, 30, 60 days)
/// * `annualization_factor` - Trading days per year (typically 252)
///
/// # Returns
/// Annualized realized volatility as decimal (0.20 = 20%)
pub fn realized_volatility(
    prices: &[f64],
    window: usize,
    annualization_factor: f64,
) -> Option<f64>;
```

#### 0.2 Create `cs-analytics/src/iv_statistics.rs`

Move from `historical_iv.rs`:
```rust
/// Calculate IV percentile over lookback period
///
/// Returns percentage of historical IVs that are below current IV.
/// Example: 80th percentile means current IV is higher than 80% of history.
pub fn iv_percentile(current_iv: f64, historical_ivs: &[f64]) -> f64;

/// Calculate IV rank (position in range)
///
/// Returns (current - min) / (max - min) as percentage.
/// Example: 50% rank means current IV is at midpoint of historical range.
pub fn iv_rank(current_iv: f64, historical_ivs: &[f64]) -> f64;
```

#### 0.3 Update `cs-analytics/src/lib.rs`

```rust
// Remove
// pub mod historical_iv;
// pub use historical_iv::{iv_percentile, iv_rank, realized_volatility};

// Add
pub mod realized_volatility;
pub mod iv_statistics;

pub use realized_volatility::realized_volatility;
pub use iv_statistics::{iv_percentile, iv_rank};
```

#### 0.4 Delete `cs-analytics/src/historical_iv.rs`

After migration is complete and tests pass.

---

### Phase 1: Extend Rust Data Model

#### 1.1 Add HV fields to `AtmIvObservation` (cs-domain/src/value_objects.rs)

```rust
pub struct AtmIvObservation {
    // ... existing fields ...

    // NEW: Historical Volatility fields
    pub hv_10d: Option<f64>,   // 10-day realized vol
    pub hv_20d: Option<f64>,   // 20-day realized vol
    pub hv_30d: Option<f64>,   // 30-day realized vol (default)
    pub hv_60d: Option<f64>,   // 60-day realized vol

    // NEW: IV vs HV spreads (IV Premium)
    pub iv_hv_spread_30d: Option<f64>,  // cm_iv_30d - hv_30d
}
```

#### 1.2 Add HV computation service (cs-analytics/src/historical_volatility.rs)

```rust
/// Historical Volatility Computer
///
/// Computes rolling realized volatility from a price series
pub struct HistoricalVolatilityComputer;

impl HistoricalVolatilityComputer {
    /// Compute realized volatility for multiple windows
    ///
    /// # Arguments
    /// * `close_prices` - Vector of (date, close_price) sorted by date
    /// * `windows` - Rolling windows to compute (e.g., [10, 20, 30, 60])
    /// * `annualization` - Trading days per year (typically 252)
    ///
    /// # Returns
    /// HashMap<window_size, Vec<(date, Option<f64>)>>
    pub fn compute_rolling_hv(
        close_prices: &[(NaiveDate, f64)],
        windows: &[usize],
        annualization: f64,
    ) -> HashMap<usize, Vec<(NaiveDate, Option<f64>)>>;

    /// Compute HV at a single date given historical prices
    pub fn compute_at_date(
        prices_up_to_date: &[f64],
        window: usize,
        annualization: f64,
    ) -> Option<f64>;
}
```

### Phase 2: Integrate into IV Use Case

#### 2.1 Modify `MinuteAlignedIvUseCase` (cs-backtest/src/minute_aligned_iv_use_case.rs)

Add method to collect daily close prices and compute HV:

```rust
impl MinuteAlignedIvUseCase {
    /// Collect daily close prices for HV computation
    async fn collect_daily_closes(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Vec<(NaiveDate, f64)>, RepositoryError>;

    /// Compute HV and enrich observations
    fn enrich_with_hv(
        &self,
        observations: &mut [AtmIvObservation],
        daily_closes: &[(NaiveDate, f64)],
        config: &HvConfig,
    );
}
```

#### 2.2 Add HV configuration (cs-domain/src/value_objects.rs)

```rust
/// Configuration for Historical Volatility computation
#[derive(Debug, Clone)]
pub struct HvConfig {
    pub windows: Vec<usize>,           // Default: [10, 20, 30, 60]
    pub annualization_factor: f64,     // Default: 252.0
    pub min_data_points: usize,        // Default: 20 (require at least 20 days of data)
}

impl Default for HvConfig {
    fn default() -> Self {
        Self {
            windows: vec![10, 20, 30, 60],
            annualization_factor: 252.0,
            min_data_points: 20,
        }
    }
}
```

### Phase 3: CLI Integration

#### 3.1 Add `--with-hv` flag to `cs atm-iv` command

```rust
// In cs-cli/src/main.rs, AtmIvArgs struct
#[arg(long, help = "Include historical volatility computation")]
with_hv: bool,

#[arg(long, default_value = "10,20,30,60", help = "HV windows in days")]
hv_windows: String,
```

### Phase 4: Extend Parquet Output

#### 4.1 Add HV columns to parquet schema

```rust
// In save_to_parquet method
let hv_10d = Series::new("hv_10d".into(), observations.iter().map(|o| o.hv_10d).collect::<Vec<_>>());
let hv_20d = Series::new("hv_20d".into(), observations.iter().map(|o| o.hv_20d).collect::<Vec<_>>());
let hv_30d = Series::new("hv_30d".into(), observations.iter().map(|o| o.hv_30d).collect::<Vec<_>>());
let hv_60d = Series::new("hv_60d".into(), observations.iter().map(|o| o.hv_60d).collect::<Vec<_>>());
let iv_hv_spread = Series::new("iv_hv_spread_30d".into(), observations.iter().map(|o| o.iv_hv_spread_30d).collect::<Vec<_>>());
```

New parquet schema (26 columns total):
- Existing 21 columns
- `hv_10d`, `hv_20d`, `hv_30d`, `hv_60d` (Float64)
- `iv_hv_spread_30d` (Float64)

### Phase 5: Python Visualization

#### 5.1 Create `plot_iv_vs_hv.py`

```python
#!/usr/bin/env python3
"""
Plot Implied Volatility vs Historical (Realized) Volatility.

Visualization panels:
1. IV Term Structure (7d, 14d, 30d CM IVs)
2. Historical Volatility (10d, 20d, 30d HV)
3. IV-HV Spread (Volatility Risk Premium)
4. IV/HV Ratio (how expensive options are vs realized)

Usage:
    python plot_iv_vs_hv.py <parquet_file> [--maturities 30,60] [--hv-window 30]
"""

import polars as pl
import matplotlib.pyplot as plt
from datetime import datetime, timedelta
import sys
from pathlib import Path
import argparse

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('parquet_file', help='Input parquet file')
    parser.add_argument('--output', help='Output PNG file')
    parser.add_argument('--iv-tenors', default='7,14,30', help='IV tenors to plot')
    parser.add_argument('--hv-windows', default='20,30', help='HV windows to plot')
    args = parser.parse_args()

    df = pl.read_parquet(args.parquet_file)

    # Convert date
    epoch = datetime(1970, 1, 1)
    df = df.with_columns([
        pl.col('date').map_elements(
            lambda d: epoch + timedelta(days=d),
            return_dtype=pl.Datetime
        ).alias('datetime')
    ])

    df_pd = df.to_pandas()
    symbol = df["symbol"].unique()[0]

    # Create 4-panel figure
    fig, axes = plt.subplots(4, 1, figsize=(16, 16), sharex=True)

    # Panel 1: Implied Volatility
    ax1 = axes[0]
    for tenor in args.iv_tenors.split(','):
        col = f'cm_iv_{tenor}d'
        if col in df_pd.columns:
            ax1.plot(df_pd['datetime'], df_pd[col] * 100,
                     label=f'{tenor}d IV', linewidth=2)
    ax1.set_ylabel('Implied Vol (%)')
    ax1.set_title(f'{symbol} - Implied vs Realized Volatility')
    ax1.legend(loc='upper left')
    ax1.grid(True, alpha=0.3)

    # Panel 2: Historical Volatility
    ax2 = axes[1]
    colors = ['#e74c3c', '#3498db', '#2ecc71', '#9b59b6']
    for i, window in enumerate(args.hv_windows.split(',')):
        col = f'hv_{window}d'
        if col in df_pd.columns:
            ax2.plot(df_pd['datetime'], df_pd[col] * 100,
                     label=f'{window}d HV', linewidth=2, color=colors[i % len(colors)])
    ax2.set_ylabel('Realized Vol (%)')
    ax2.set_title('Historical (Realized) Volatility')
    ax2.legend(loc='upper left')
    ax2.grid(True, alpha=0.3)

    # Panel 3: IV-HV Spread (Volatility Risk Premium)
    ax3 = axes[2]
    if 'iv_hv_spread_30d' in df_pd.columns:
        spread = df_pd['iv_hv_spread_30d'] * 100
        ax3.fill_between(df_pd['datetime'], 0, spread,
                         where=spread > 0, color='red', alpha=0.3, label='IV Premium')
        ax3.fill_between(df_pd['datetime'], 0, spread,
                         where=spread <= 0, color='green', alpha=0.3, label='HV Premium')
        ax3.plot(df_pd['datetime'], spread, color='black', linewidth=1.5)
        ax3.axhline(y=0, color='gray', linestyle='--', linewidth=1)
    ax3.set_ylabel('IV - HV (pp)')
    ax3.set_title('Volatility Risk Premium (30d IV - 30d HV)')
    ax3.legend(loc='upper left')
    ax3.grid(True, alpha=0.3)

    # Panel 4: IV/HV Ratio
    ax4 = axes[3]
    if 'cm_iv_30d' in df_pd.columns and 'hv_30d' in df_pd.columns:
        ratio = df_pd['cm_iv_30d'] / df_pd['hv_30d']
        ax4.plot(df_pd['datetime'], ratio, color='purple', linewidth=2)
        ax4.axhline(y=1.0, color='gray', linestyle='--', linewidth=1, label='Fair Value')
        ax4.fill_between(df_pd['datetime'], 1.0, ratio,
                         where=ratio > 1.0, color='red', alpha=0.2)
        ax4.fill_between(df_pd['datetime'], 1.0, ratio,
                         where=ratio <= 1.0, color='green', alpha=0.2)
    ax4.set_ylabel('IV / HV Ratio')
    ax4.set_xlabel('Date')
    ax4.set_title('Relative IV (>1 = Options Expensive)')
    ax4.legend(loc='upper left')
    ax4.grid(True, alpha=0.3)

    plt.tight_layout()

    # Save or show
    output_file = args.output or f"{Path(args.parquet_file).stem}_iv_vs_hv.png"
    plt.savefig(output_file, dpi=300, bbox_inches='tight')
    print(f"Saved: {output_file}")

    # Print statistics
    print("\n=== IV vs HV Statistics ===")
    if 'iv_hv_spread_30d' in df_pd.columns:
        spread = df_pd['iv_hv_spread_30d'].dropna() * 100
        print(f"\n30d IV-HV Spread:")
        print(f"  Mean:   {spread.mean():+.2f}pp")
        print(f"  Median: {spread.median():+.2f}pp")
        print(f"  Days IV > HV: {(spread > 0).sum()} / {len(spread)} ({100*(spread > 0).mean():.1f}%)")

if __name__ == '__main__':
    main()
```

---

## File Changes Summary

| File | Change Type | Description |
|------|-------------|-------------|
| `cs-analytics/src/historical_iv.rs` | **Delete** | Split into realized_volatility.rs and iv_statistics.rs |
| `cs-analytics/src/realized_volatility.rs` | **New** | `realized_volatility()` function (moved from historical_iv.rs) |
| `cs-analytics/src/iv_statistics.rs` | **New** | `iv_percentile()`, `iv_rank()` (moved from historical_iv.rs) |
| `cs-analytics/src/lib.rs` | Modify | Update re-exports for split modules |
| `cs-domain/src/value_objects.rs` | Modify | Add `hv_*` fields to `AtmIvObservation`, add `HvConfig` |
| `cs-backtest/src/minute_aligned_iv_use_case.rs` | Modify | Add HV computation integration |
| `cs-cli/src/main.rs` | Modify | Add `--with-hv` and `--hv-windows` flags |
| `plot_iv_vs_hv.py` | **New** | Python visualization script |

---

## Usage Examples

### CLI

```bash
# Generate ATM IV with Historical Volatility
cs atm-iv --symbols AAPL \
          --start 2025-01-01 \
          --end 2025-12-31 \
          --minute-aligned \
          --constant-maturity \
          --with-hv \
          --hv-windows 10,20,30,60 \
          --output ./output

# Plot IV vs HV
uv run python3 plot_iv_vs_hv.py ./output/atm_iv_AAPL.parquet
```

### Expected Output

```
=== IV vs HV Statistics ===

30d IV-HV Spread:
  Mean:   +3.24pp
  Median: +2.87pp
  Days IV > HV: 198 / 249 (79.5%)
```

---

## Key Formulas

### Realized Volatility
```
Returns[t] = ln(Close[t] / Close[t-1])
HV = std(Returns[t-window:t]) * sqrt(252)
```

### IV-HV Spread (Volatility Risk Premium)
```
VRP = IV_30d - HV_30d
```

- VRP > 0: Options expensive vs realized (typical 80% of time)
- VRP < 0: Options cheap vs realized (rare, often before events)

### IV/HV Ratio
```
Ratio = IV_30d / HV_30d
```

- Ratio > 1.0: Options overpriced
- Ratio < 1.0: Options underpriced
- Typical range: 1.05 - 1.25

---

## Testing Strategy

1. **Unit tests** for `HistoricalVolatilityComputer`:
   - Test with known price series
   - Test edge cases (insufficient data, constant prices)
   - Verify annualization

2. **Integration tests**:
   - Run full pipeline with `--with-hv`
   - Verify parquet output has HV columns
   - Compare HV values with known benchmark (e.g., Yahoo Finance historical vol)

3. **Visual validation**:
   - Plot against VIX for sanity check
   - Verify HV smoothness (no discontinuities)
