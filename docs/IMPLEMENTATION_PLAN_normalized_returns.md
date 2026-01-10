# Implementation Plan: Vega-Weighted and Notional-Weighted Returns

## Overview

Add exposure-normalized return metrics to the backtest system:
1. **Vega-Weighted Return** — Primary metric for volatility strategies
2. **Notional-Weighted Return** — Cross-symbol comparison metric

## Files to Modify

| File | Changes |
|------|---------|
| `cs-domain/src/accounting/mod.rs` | Export new trait |
| `cs-domain/src/accounting/has_vega_exposure.rs` | **NEW** - Trait for vega/notional extraction |
| `cs-domain/src/accounting/statistics.rs` | Add new fields and calculations |
| `cs-domain/src/trade_result/straddle.rs` | Implement HasVegaExposure |
| `cs-domain/src/trade_result/calendar.rs` | Implement HasVegaExposure |
| `cs-domain/src/trade_result/iron_butterfly.rs` | Implement HasVegaExposure |
| `cs-cli/src/output/backtest.rs` | Add display section |
| `cs-backtest/src/backtest_use_case.rs` | Add aggregate methods |

## Step-by-Step Implementation

### Step 1: Create HasVegaExposure Trait

**File:** `cs-domain/src/accounting/has_vega_exposure.rs`

```rust
use rust_decimal::Decimal;

/// Trait for extracting exposure metrics from trade results.
///
/// This enables vega-weighted and notional-weighted return calculations
/// without coupling the accounting system to specific trade result types.
pub trait HasVegaExposure {
    /// Net vega exposure at entry (dollar vega = vega × 100)
    /// Returns None if vega data is not available
    fn dollar_vega(&self) -> Option<f64>;

    /// Notional value of the position (strike × 100 for options)
    /// Used for cross-symbol normalization
    fn notional_value(&self) -> Decimal;

    /// Net delta exposure at entry (dollar delta = delta × spot × 100)
    /// Returns None if delta data is not available
    fn dollar_delta(&self) -> Option<f64> {
        None // Default: not all strategies track delta
    }
}
```

**Rationale:** Following the same pattern as `HasAccounting` trait — non-invasive extraction of exposure data.

### Step 2: Implement Trait for Trade Results

**File:** `cs-domain/src/trade_result/straddle.rs`

```rust
impl HasVegaExposure for StraddleResult {
    fn dollar_vega(&self) -> Option<f64> {
        // net_vega is already the combined vega of call + put
        // Dollar vega = vega × 100 (contract multiplier)
        self.net_vega.map(|v| v * 100.0)
    }

    fn notional_value(&self) -> Decimal {
        // For ATM straddle, notional = strike × 100
        self.strike * Decimal::from(100)
    }

    fn dollar_delta(&self) -> Option<f64> {
        self.net_delta.map(|d| d * self.spot.to_f64().unwrap_or(0.0) * 100.0)
    }
}
```

**File:** `cs-domain/src/trade_result/calendar.rs`

```rust
impl HasVegaExposure for CalendarSpreadResult {
    fn dollar_vega(&self) -> Option<f64> {
        // Net vega = long_vega - short_vega (calendar is long vega)
        match (self.long_vega, self.short_vega) {
            (Some(lv), Some(sv)) => Some((lv - sv) * 100.0),
            _ => None,
        }
    }

    fn notional_value(&self) -> Decimal {
        self.strike * Decimal::from(100)
    }
}
```

**File:** `cs-domain/src/trade_result/iron_butterfly.rs`

```rust
impl HasVegaExposure for IronButterflyResult {
    fn dollar_vega(&self) -> Option<f64> {
        self.net_vega.map(|v| v * 100.0)
    }

    fn notional_value(&self) -> Decimal {
        // Use center strike for iron butterfly
        self.center_strike * Decimal::from(100)
    }
}
```

### Step 3: Add Fields to TradeStatistics

**File:** `cs-domain/src/accounting/statistics.rs`

Add new fields to the struct:

```rust
pub struct TradeStatistics {
    // ... existing fields ...

    // === NEW: Exposure-Normalized Returns ===

    /// Return weighted by dollar vega exposure
    /// Formula: Σ(dollar_vega_i × return_i) / Σ(dollar_vega_i)
    pub vega_weighted_return: f64,

    /// Return weighted by notional value
    /// Formula: Σ(notional_i × return_i) / Σ(notional_i)
    pub notional_weighted_return: f64,

    /// Total dollar vega exposure across all trades
    pub total_dollar_vega: f64,

    /// Total notional value across all trades
    pub total_notional: Decimal,

    /// Average dollar vega per trade
    pub avg_dollar_vega: f64,

    /// Number of trades with valid vega data
    pub trades_with_vega: usize,

    /// Sharpe ratio using vega-weighted returns
    pub vega_weighted_sharpe: f64,

    /// Sharpe ratio using notional-weighted returns
    pub notional_weighted_sharpe: f64,
}
```

### Step 4: Implement Calculation in from_trades_with_exposure

**File:** `cs-domain/src/accounting/statistics.rs`

Add a new constructor that accepts exposure data:

```rust
/// Extended trade data with exposure metrics
pub struct TradeWithExposure {
    pub accounting: TradeAccounting,
    pub dollar_vega: Option<f64>,
    pub notional: Decimal,
}

impl TradeStatistics {
    /// Build statistics with exposure-normalized metrics
    pub fn from_trades_with_exposure(trades: &[TradeWithExposure]) -> Self {
        // First, compute base statistics using existing from_trades
        let accountings: Vec<TradeAccounting> = trades
            .iter()
            .map(|t| t.accounting.clone())
            .collect();
        let mut stats = Self::from_trades(&accountings);

        // === Vega-Weighted Return ===
        let (vega_weighted_sum, total_vega, vega_count) = trades.iter().fold(
            (0.0, 0.0, 0usize),
            |(sum, total, count), trade| {
                if let Some(vega) = trade.dollar_vega {
                    let return_pct = trade.accounting.return_on_capital;
                    (sum + vega.abs() * return_pct, total + vega.abs(), count + 1)
                } else {
                    (sum, total, count)
                }
            },
        );

        stats.vega_weighted_return = if total_vega > 0.0 {
            vega_weighted_sum / total_vega
        } else {
            0.0
        };
        stats.total_dollar_vega = total_vega;
        stats.trades_with_vega = vega_count;
        stats.avg_dollar_vega = if vega_count > 0 {
            total_vega / vega_count as f64
        } else {
            0.0
        };

        // === Notional-Weighted Return ===
        let (notional_weighted_sum, total_notional) = trades.iter().fold(
            (Decimal::ZERO, Decimal::ZERO),
            |(sum, total), trade| {
                let notional = trade.notional;
                let return_pct = Decimal::from_f64_retain(trade.accounting.return_on_capital)
                    .unwrap_or(Decimal::ZERO);
                (sum + notional * return_pct, total + notional)
            },
        );

        stats.notional_weighted_return = if !total_notional.is_zero() {
            (notional_weighted_sum / total_notional).to_f64().unwrap_or(0.0)
        } else {
            0.0
        };
        stats.total_notional = total_notional;

        // === Vega-Weighted Sharpe ===
        // Calculate variance of vega-weighted returns
        if total_vega > 0.0 && vega_count > 1 {
            let mean = stats.vega_weighted_return;
            let variance: f64 = trades.iter()
                .filter_map(|t| t.dollar_vega.map(|v| (v.abs(), t.accounting.return_on_capital)))
                .map(|(vega, ret)| {
                    let weight = vega / total_vega;
                    weight * (ret - mean).powi(2)
                })
                .sum();

            let std_dev = variance.sqrt();
            stats.vega_weighted_sharpe = if std_dev > 0.0 {
                (mean / std_dev) * (252.0_f64).sqrt() // Annualized
            } else {
                0.0
            };
        }

        // === Notional-Weighted Sharpe ===
        if !total_notional.is_zero() && trades.len() > 1 {
            let mean = stats.notional_weighted_return;
            let variance: f64 = trades.iter()
                .map(|t| {
                    let weight = (t.notional / total_notional).to_f64().unwrap_or(0.0);
                    let ret = t.accounting.return_on_capital;
                    weight * (ret - mean).powi(2)
                })
                .sum();

            let std_dev = variance.sqrt();
            stats.notional_weighted_sharpe = if std_dev > 0.0 {
                (mean / std_dev) * (252.0_f64).sqrt()
            } else {
                0.0
            };
        }

        stats
    }
}
```

### Step 5: Update BacktestResult

**File:** `cs-backtest/src/backtest_use_case.rs`

Add methods to extract exposure data and compute statistics:

```rust
impl<R> BacktestResult<R>
where
    R: TradeResultTrait + HasAccounting + HasVegaExposure,
{
    /// Get vega-weighted return across all trades
    pub fn vega_weighted_return(&self) -> f64 {
        let (weighted_sum, total_vega) = self.results.iter().fold(
            (0.0, 0.0),
            |(sum, total), result| {
                if let Some(vega) = result.dollar_vega() {
                    let return_pct = result.return_on_capital();
                    (sum + vega.abs() * return_pct, total + vega.abs())
                } else {
                    (sum, total)
                }
            },
        );

        if total_vega > 0.0 {
            weighted_sum / total_vega
        } else {
            0.0
        }
    }

    /// Get notional-weighted return across all trades
    pub fn notional_weighted_return(&self) -> f64 {
        let (weighted_sum, total_notional) = self.results.iter().fold(
            (Decimal::ZERO, Decimal::ZERO),
            |(sum, total), result| {
                let notional = result.notional_value();
                let return_pct = Decimal::from_f64_retain(result.return_on_capital())
                    .unwrap_or(Decimal::ZERO);
                (sum + notional * return_pct, total + notional)
            },
        );

        if !total_notional.is_zero() {
            (weighted_sum / total_notional).to_f64().unwrap_or(0.0)
        } else {
            0.0
        }
    }

    /// Get total dollar vega exposure
    pub fn total_dollar_vega(&self) -> f64 {
        self.results.iter()
            .filter_map(|r| r.dollar_vega())
            .map(|v| v.abs())
            .sum()
    }

    /// Get total notional value
    pub fn total_notional(&self) -> Decimal {
        self.results.iter()
            .map(|r| r.notional_value())
            .sum()
    }
}
```

### Step 6: Add Console Output Section

**File:** `cs-cli/src/output/backtest.rs`

Add new display function:

```rust
/// Display exposure-normalized metrics (vega-weighted, notional-weighted returns)
pub fn display_exposure_normalized<R>(result: &BacktestResult<R>)
where
    R: TradeResultTrait + HasAccounting + HasVegaExposure,
{
    use comfy_table::{Table, Row, Cell};

    let vega_return = result.vega_weighted_return();
    let notional_return = result.notional_weighted_return();
    let total_vega = result.total_dollar_vega();
    let total_notional = result.total_notional();

    // Count trades with vega data
    let trades_with_vega = result.results.iter()
        .filter(|r| r.dollar_vega().is_some())
        .count();

    println!("\nExposure-Normalized Metrics:");

    let mut table = Table::new();
    table.load_preset(comfy_table::presets::ASCII_BORDERS_ONLY_CONDENSED);

    // Vega section
    table.add_row(Row::from(vec![
        Cell::new("Vega-Weighted Return"),
        Cell::new(format!("{:.2}%", vega_return * 100.0)),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Total Dollar Vega"),
        Cell::new(format!("${:.2}", total_vega)),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Avg Dollar Vega/Trade"),
        Cell::new(format!("${:.2}", if trades_with_vega > 0 {
            total_vega / trades_with_vega as f64
        } else {
            0.0
        })),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Trades with Vega Data"),
        Cell::new(format!("{} / {}", trades_with_vega, result.results.len())),
    ]));

    // Separator
    table.add_row(Row::from(vec![Cell::new(""), Cell::new("")]));

    // Notional section
    table.add_row(Row::from(vec![
        Cell::new("Notional-Weighted Return"),
        Cell::new(format!("{:.2}%", notional_return * 100.0)),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Total Notional"),
        Cell::new(format!("${:.2}", total_notional)),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Capital/Notional Ratio"),
        Cell::new(format!("{:.2}%",
            if !total_notional.is_zero() {
                (result.total_capital() / total_notional).to_f64().unwrap_or(0.0) * 100.0
            } else {
                0.0
            }
        )),
    ]));

    println!("{table}");
}
```

### Step 7: Update JSON Output

**File:** `cs-cli/src/output/backtest.rs` or serialization code

Add fields to JSON output:

```rust
#[derive(Serialize)]
pub struct BacktestOutput {
    // ... existing fields ...

    // Exposure-normalized metrics
    pub vega_weighted_return: f64,
    pub notional_weighted_return: f64,
    pub total_dollar_vega: f64,
    pub total_notional: String, // Decimal as string for precision
    pub trades_with_vega: usize,
}
```

### Step 8: Update mod.rs Exports

**File:** `cs-domain/src/accounting/mod.rs`

```rust
mod has_vega_exposure;

pub use has_vega_exposure::HasVegaExposure;
```

## Testing Plan

### Unit Tests

1. **Test vega-weighted return calculation:**
   - Three trades with different vega exposures
   - Verify weighted average is correct

2. **Test notional-weighted return calculation:**
   - Trades with different strike prices
   - Verify cross-symbol normalization works

3. **Test edge cases:**
   - All trades missing vega data → returns 0.0
   - Single trade → weighted return equals simple return
   - Zero total vega/notional → handle divide by zero

### Integration Tests

1. Run backtest with `--output` and verify JSON contains new fields
2. Verify console output displays new section
3. Compare vega-weighted vs capital-weighted for known scenarios

## Migration Notes

1. **Backward Compatibility:** Existing code calling `TradeStatistics::from_trades()` continues to work; new fields default to 0.0
2. **Feature Flag:** Consider adding `--show-exposure-metrics` flag initially
3. **Performance:** Minimal impact — single pass over trades for calculations

## Summary

| Component | Change Type | Complexity |
|-----------|-------------|------------|
| HasVegaExposure trait | New file | Low |
| Trait implementations | Additions | Low |
| TradeStatistics fields | Additions | Low |
| Calculation logic | New methods | Medium |
| Console output | New section | Low |
| JSON output | Field additions | Low |

**Estimated LOC:** ~250-300 new lines of code

**Dependencies:** None (uses existing rust_decimal, serde)
