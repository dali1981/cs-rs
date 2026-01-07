# Results Enhancement: Volatility Summary & Capital Metrics

**Date**: 2026-01-07
**Status**: Implementation Plan
**Goal**: Add missing volatility summary, capital requirements, and return on capital to results display

---

## Current State Analysis

### What's Currently Displayed

```
Results:

  Symbol:               PENG
  Period:               2025-03-01 to 2025-04-15
  Number of rolls:      7

  Total Option P&L:     $-222.73
  Total Hedge P&L:      $474.14
  Transaction Cost:     $15.70
  Total P&L:            $235.70

  Win Rate:             28.6%
  Avg P&L per Roll:     $33.67
  Max Drawdown:         $199.44
```

### What's Missing

1. **Volatility Summary** - IV entry, HV entry, realized vol
2. **Capital Metrics** - Capital required, return on capital
3. **P&L Attribution Summary** - Aggregated totals (not just per-roll table)

---

## Issue 1: Volatility Summary Missing

### Current Data Available

`RealizedVolatilityMetrics` (in `HedgePosition`) has:
- `entry_hv: Option<f64>` - Historical volatility at entry
- `entry_iv: Option<f64>` - Implied volatility at entry
- `exit_iv: Option<f64>` - Implied volatility at exit
- `realized_vol: f64` - Actual volatility during holding
- `iv_premium_at_entry: Option<f64>` - (IV - HV) / HV as %
- `realized_vs_implied: Option<f64>` - (RV - IV) / IV as %

### Problem

1. `RealizedVolatilityMetrics` is stored in `HedgePosition`
2. `HedgePosition` is NOT propagated to `RollPeriod`
3. CLI only accesses `RollPeriod` fields for display

### Solution

#### Phase 1a: Add Volatility Metrics to RollPeriod

**File**: `cs-domain/src/entities/rolling_result.rs`

```rust
/// A single roll period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollPeriod {
    // ... existing fields ...

    // NEW: Volatility metrics (when hedging with track_realized_vol=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realized_vol_metrics: Option<RealizedVolatilityMetrics>,
}
```

#### Phase 1b: Add Aggregated Volatility to RollingResult

**File**: `cs-domain/src/entities/rolling_result.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingResult {
    // ... existing fields ...

    // NEW: Aggregated volatility summary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volatility_summary: Option<VolatilitySummary>,
}

/// Aggregated volatility metrics across all rolls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilitySummary {
    /// Average entry IV across all rolls (annualized)
    pub avg_entry_iv: Option<f64>,
    /// Average entry HV across all rolls (annualized)
    pub avg_entry_hv: Option<f64>,
    /// Average realized vol across all rolls (annualized)
    pub avg_realized_vol: Option<f64>,
    /// Average IV premium at entry: (IV - HV) / HV
    pub avg_iv_premium: Option<f64>,
    /// Average realized vs implied: (RV - IV) / IV
    pub avg_realized_vs_implied: Option<f64>,
    /// Number of rolls with valid volatility data
    pub rolls_with_vol_data: usize,
}
```

#### Phase 1c: Propagate Metrics from HedgePosition to RollPeriod

**File**: `cs-backtest/src/trade_executor.rs` (in `to_roll_period()`)

```rust
fn to_roll_period(
    &self,
    trade: &T,
    result: <T as ExecutableTrade>::Result,
    roll_reason: RollReason,
) -> RollPeriod {
    // ... existing code ...

    // NEW: Extract realized vol metrics from hedge position
    let realized_vol_metrics = result.hedge_position()
        .and_then(|hp| hp.realized_vol_metrics.clone());

    RollPeriod {
        // ... existing fields ...
        realized_vol_metrics,
    }
}
```

#### Phase 1d: Compute Aggregated Summary in RollingResult::from_rolls()

**File**: `cs-domain/src/entities/rolling_result.rs`

```rust
impl RollingResult {
    pub fn from_rolls(...) -> Self {
        // ... existing code ...

        // NEW: Compute volatility summary
        let volatility_summary = Self::compute_volatility_summary(&rolls);

        Self {
            // ... existing fields ...
            volatility_summary,
        }
    }

    fn compute_volatility_summary(rolls: &[RollPeriod]) -> Option<VolatilitySummary> {
        let rolls_with_vol: Vec<_> = rolls.iter()
            .filter_map(|r| r.realized_vol_metrics.as_ref())
            .collect();

        if rolls_with_vol.is_empty() {
            return None;
        }

        let count = rolls_with_vol.len() as f64;

        let avg_entry_iv = {
            let ivs: Vec<f64> = rolls_with_vol.iter()
                .filter_map(|m| m.entry_iv)
                .collect();
            if ivs.is_empty() { None } else { Some(ivs.iter().sum::<f64>() / ivs.len() as f64) }
        };

        let avg_entry_hv = {
            let hvs: Vec<f64> = rolls_with_vol.iter()
                .filter_map(|m| m.entry_hv)
                .collect();
            if hvs.is_empty() { None } else { Some(hvs.iter().sum::<f64>() / hvs.len() as f64) }
        };

        let avg_realized_vol = {
            let rvs: Vec<f64> = rolls_with_vol.iter()
                .map(|m| m.realized_vol)
                .collect();
            if rvs.is_empty() { None } else { Some(rvs.iter().sum::<f64>() / rvs.len() as f64) }
        };

        let avg_iv_premium = {
            let prems: Vec<f64> = rolls_with_vol.iter()
                .filter_map(|m| m.iv_premium_at_entry)
                .collect();
            if prems.is_empty() { None } else { Some(prems.iter().sum::<f64>() / prems.len() as f64) }
        };

        let avg_realized_vs_implied = {
            let diffs: Vec<f64> = rolls_with_vol.iter()
                .filter_map(|m| m.realized_vs_implied)
                .collect();
            if diffs.is_empty() { None } else { Some(diffs.iter().sum::<f64>() / diffs.len() as f64) }
        };

        Some(VolatilitySummary {
            avg_entry_iv,
            avg_entry_hv,
            avg_realized_vol,
            avg_iv_premium,
            avg_realized_vs_implied,
            rolls_with_vol_data: rolls_with_vol.len(),
        })
    }
}
```

#### Phase 1e: Display in CLI

**File**: `cs-cli/src/main.rs` (in `display_rolling_results()`)

```rust
fn display_rolling_results(result: &cs_domain::RollingResult) {
    // ... existing summary display ...

    // NEW: Volatility Summary
    if let Some(ref vol) = result.volatility_summary {
        println!();
        println!("{}", style("Volatility Summary:").bold());
        if let Some(iv) = vol.avg_entry_iv {
            println!("  Avg Entry IV:       {:.1}%", iv * 100.0);
        }
        if let Some(hv) = vol.avg_entry_hv {
            println!("  Avg Entry HV:       {:.1}%", hv * 100.0);
        }
        if let Some(rv) = vol.avg_realized_vol {
            println!("  Avg Realized Vol:   {:.1}%", rv * 100.0);
        }
        if let Some(prem) = vol.avg_iv_premium {
            let sign = if prem >= 0.0 { "+" } else { "" };
            println!("  IV Premium (avg):   {}{}% (IV vs HV)", sign, prem as i64);
        }
        if let Some(diff) = vol.avg_realized_vs_implied {
            let sign = if diff >= 0.0 { "+" } else { "" };
            println!("  RV vs IV (avg):     {}{}%", sign, diff as i64);
        }
        println!("  Rolls with vol data: {}/{}", vol.rolls_with_vol_data, result.num_rolls);
    }
}
```

---

## Issue 2: Capital Requirements Missing

### Problem Statement

When hedging options with stock:
1. Buying stock requires capital
2. Short selling stock requires margin
3. Total capital deployed is NOT visible in results
4. Return on capital cannot be computed

### What to Track

#### Capital Components

| Component | Description | Computation |
|-----------|-------------|-------------|
| **Option Premium** | Initial debit paid for options | `entry_debit` |
| **Peak Long Stock Capital** | Max capital for long hedge shares | `max(cumulative_shares, 0) × avg_price` |
| **Peak Short Stock Margin** | Margin for short hedge shares | `abs(min(cumulative_shares, 0)) × avg_price × margin_req` |
| **Total Capital Required** | Maximum capital at any point | `option_premium + max(peak_long, peak_short_margin)` |

#### Return Metrics

| Metric | Formula |
|--------|---------|
| **Return on Capital** | `total_pnl / total_capital_required × 100` |
| **Annualized Return** | `return_on_capital × (365 / holding_days)` |

### Solution

#### Phase 2a: Add Capital Metrics to HedgePosition

**File**: `cs-domain/src/hedging.rs`

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HedgePosition {
    // ... existing fields ...

    // NEW: Capital metrics
    /// Peak long shares held during hedging
    pub peak_long_shares: i32,
    /// Peak short shares held during hedging (absolute value)
    pub peak_short_shares: i32,
    /// Average hedge price (for capital computation)
    pub avg_hedge_price: f64,
}

impl HedgePosition {
    /// Record a new hedge action
    pub fn add_hedge(&mut self, action: HedgeAction) {
        self.cumulative_shares += action.shares;
        self.total_cost += action.cost;
        self.hedges.push(action);

        // NEW: Track peak shares
        if self.cumulative_shares > self.peak_long_shares {
            self.peak_long_shares = self.cumulative_shares;
        }
        if self.cumulative_shares < 0 && self.cumulative_shares.abs() > self.peak_short_shares {
            self.peak_short_shares = self.cumulative_shares.abs();
        }
    }

    /// Compute average hedge price for capital calculation
    pub fn compute_avg_hedge_price(&mut self) {
        if let Some(avg) = self.average_hedge_price() {
            self.avg_hedge_price = avg;
        }
    }

    /// Compute capital required for long hedge position
    pub fn long_hedge_capital(&self) -> Decimal {
        Decimal::from(self.peak_long_shares) * Decimal::try_from(self.avg_hedge_price).unwrap_or_default()
    }

    /// Compute margin required for short hedge position (assuming 50% margin)
    pub fn short_hedge_margin(&self, margin_rate: f64) -> Decimal {
        Decimal::from(self.peak_short_shares)
            * Decimal::try_from(self.avg_hedge_price).unwrap_or_default()
            * Decimal::try_from(margin_rate).unwrap_or(Decimal::from_str("0.5").unwrap())
    }
}
```

#### Phase 2b: Add Capital Metrics to RollPeriod

**File**: `cs-domain/src/entities/rolling_result.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollPeriod {
    // ... existing fields ...

    // NEW: Capital metrics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hedge_capital: Option<HedgeCapitalMetrics>,
}

/// Capital metrics for a single roll period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HedgeCapitalMetrics {
    /// Peak long shares held
    pub peak_long_shares: i32,
    /// Peak short shares held (absolute)
    pub peak_short_shares: i32,
    /// Average hedge price
    pub avg_hedge_price: f64,
    /// Capital required for long hedge
    pub long_capital: Decimal,
    /// Margin required for short hedge
    pub short_margin: Decimal,
}
```

#### Phase 2c: Add Aggregated Capital to RollingResult

**File**: `cs-domain/src/entities/rolling_result.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingResult {
    // ... existing fields ...

    // NEW: Capital metrics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capital_summary: Option<CapitalSummary>,
}

/// Aggregated capital metrics across all rolls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapitalSummary {
    /// Total option premium (sum of entry_debit)
    pub total_option_premium: Decimal,
    /// Peak hedge capital (max across rolls)
    pub peak_hedge_capital: Decimal,
    /// Peak hedge margin (max across rolls)
    pub peak_hedge_margin: Decimal,
    /// Total capital required = option_premium + hedge_capital
    pub total_capital_required: Decimal,
    /// Return on capital: total_pnl / total_capital_required
    pub return_on_capital: f64,
    /// Annualized return
    pub annualized_return: f64,
    /// Holding period in days
    pub holding_days: i64,
}
```

#### Phase 2d: Compute Capital Metrics

**File**: `cs-domain/src/entities/rolling_result.rs`

```rust
impl RollingResult {
    fn compute_capital_summary(rolls: &[RollPeriod], total_pnl: Decimal, start_date: NaiveDate, end_date: NaiveDate) -> Option<CapitalSummary> {
        if rolls.is_empty() {
            return None;
        }

        let total_option_premium: Decimal = rolls.iter()
            .map(|r| r.entry_debit)
            .sum();

        // Peak hedge capital across all rolls
        let peak_hedge_capital: Decimal = rolls.iter()
            .filter_map(|r| r.hedge_capital.as_ref())
            .map(|c| c.long_capital)
            .max()
            .unwrap_or(Decimal::ZERO);

        let peak_hedge_margin: Decimal = rolls.iter()
            .filter_map(|r| r.hedge_capital.as_ref())
            .map(|c| c.short_margin)
            .max()
            .unwrap_or(Decimal::ZERO);

        // For simplicity: total capital = option premium + max(peak_long, peak_short_margin)
        let hedge_capital = peak_hedge_capital.max(peak_hedge_margin);
        let total_capital_required = total_option_premium + hedge_capital;

        let holding_days = (end_date - start_date).num_days();

        let return_on_capital = if total_capital_required > Decimal::ZERO {
            (total_pnl / total_capital_required).to_f64().unwrap_or(0.0) * 100.0
        } else {
            0.0
        };

        let annualized_return = if holding_days > 0 {
            return_on_capital * 365.0 / holding_days as f64
        } else {
            0.0
        };

        Some(CapitalSummary {
            total_option_premium,
            peak_hedge_capital,
            peak_hedge_margin,
            total_capital_required,
            return_on_capital,
            annualized_return,
            holding_days,
        })
    }
}
```

#### Phase 2e: Display in CLI

**File**: `cs-cli/src/main.rs`

```rust
fn display_rolling_results(result: &cs_domain::RollingResult) {
    // ... existing summary ...

    // NEW: Capital Metrics
    if let Some(ref cap) = result.capital_summary {
        println!();
        println!("{}", style("Capital Metrics:").bold());
        println!("  Option Premium:     ${:.2}", cap.total_option_premium);
        if cap.peak_hedge_capital > Decimal::ZERO {
            println!("  Peak Hedge Capital: ${:.2}", cap.peak_hedge_capital);
        }
        if cap.peak_hedge_margin > Decimal::ZERO {
            println!("  Peak Hedge Margin:  ${:.2}", cap.peak_hedge_margin);
        }
        println!("  Total Capital Req:  ${:.2}", cap.total_capital_required);
        println!();
        println!("  Return on Capital:  {:.1}%", cap.return_on_capital);
        println!("  Annualized Return:  {:.1}%", cap.annualized_return);
        println!("  Holding Period:     {} days", cap.holding_days);
    }
}
```

---

## Issue 3: P&L Attribution Summary Missing

### Problem

Individual roll attribution is shown in table, but aggregated totals are missing.

### Solution

#### Phase 3a: Add Attribution Summary to RollingResult

Already exists in plan - need to ensure it's computed and displayed.

**File**: `cs-domain/src/entities/rolling_result.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingResult {
    // ... existing fields ...

    // NEW: Aggregated P&L attribution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution_summary: Option<AttributionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionSummary {
    pub total_gross_delta_pnl: Decimal,
    pub total_hedge_delta_pnl: Decimal,
    pub total_net_delta_pnl: Decimal,
    pub total_gamma_pnl: Decimal,
    pub total_theta_pnl: Decimal,
    pub total_vega_pnl: Decimal,
    pub total_unexplained: Decimal,
    pub avg_hedge_efficiency: f64,
    pub rolls_with_attribution: usize,
}
```

#### Phase 3b: Display Attribution Summary

**File**: `cs-cli/src/main.rs`

```rust
// After the per-roll attribution table
if let Some(ref attr) = result.attribution_summary {
    println!();
    println!("{}", style("Attribution Summary:").bold());
    println!("  Gross Delta P&L:  ${:.2}", attr.total_gross_delta_pnl);
    println!("  Hedge Delta P&L:  ${:.2}", attr.total_hedge_delta_pnl);
    println!("  Net Delta P&L:    ${:.2}", attr.total_net_delta_pnl);
    println!("  Gamma P&L:        ${:.2}", attr.total_gamma_pnl);
    println!("  Theta P&L:        ${:.2}", attr.total_theta_pnl);
    println!("  Vega P&L:         ${:.2}", attr.total_vega_pnl);
    println!("  Unexplained:      ${:.2}", attr.total_unexplained);
    println!();
    println!("  Avg Hedge Eff:    {:.1}%", attr.avg_hedge_efficiency);
}
```

---

## Expected Output After Implementation

```
Results:

  Symbol:               PENG
  Period:               2025-03-01 to 2025-04-15
  Number of rolls:      7

  Total Option P&L:     $-222.73
  Total Hedge P&L:      $474.14
  Transaction Cost:     $15.70
  Total P&L:            $235.70

  Win Rate:             28.6%
  Avg P&L per Roll:     $33.67
  Max Drawdown:         $199.44

Volatility Summary:
  Avg Entry IV:         45.2%
  Avg Entry HV:         32.1%
  Avg Realized Vol:     28.5%
  IV Premium (avg):     +41% (IV vs HV)
  RV vs IV (avg):       -37%
  Rolls with vol data:  7/7

Capital Metrics:
  Option Premium:       $1,850.00
  Peak Hedge Capital:   $4,200.00
  Total Capital Req:    $6,050.00

  Return on Capital:    3.9%
  Annualized Return:    31.6%
  Holding Period:       45 days

Attribution Summary:
  Gross Delta P&L:      $312.50
  Hedge Delta P&L:      $474.14
  Net Delta P&L:        $786.64
  Gamma P&L:            $-45.20
  Theta P&L:            $-890.00
  Vega P&L:             $-73.47
  Unexplained:          $-5.97

  Avg Hedge Eff:        88.2%
```

---

## Implementation Phases

| Phase | Deliverable | Files | Lines |
|-------|-------------|-------|-------|
| 1a | RollPeriod vol metrics field | `cs-domain/src/entities/rolling_result.rs` | +5 |
| 1b | VolatilitySummary struct | `cs-domain/src/entities/rolling_result.rs` | +30 |
| 1c | Propagate metrics to RollPeriod | `cs-backtest/src/trade_executor.rs` | +5 |
| 1d | Compute aggregated vol summary | `cs-domain/src/entities/rolling_result.rs` | +60 |
| 1e | CLI vol summary display | `cs-cli/src/main.rs` | +20 |
| 2a | HedgePosition capital tracking | `cs-domain/src/hedging.rs` | +40 |
| 2b | RollPeriod capital metrics | `cs-domain/src/entities/rolling_result.rs` | +15 |
| 2c | CapitalSummary struct | `cs-domain/src/entities/rolling_result.rs` | +25 |
| 2d | Compute capital summary | `cs-domain/src/entities/rolling_result.rs` | +50 |
| 2e | CLI capital display | `cs-cli/src/main.rs` | +20 |
| 3a | AttributionSummary struct | `cs-domain/src/entities/rolling_result.rs` | +20 |
| 3b | Compute attribution summary | `cs-domain/src/entities/rolling_result.rs` | +40 |
| 3c | CLI attribution summary display | `cs-cli/src/main.rs` | +15 |

**Total**: ~350 new lines

---

## Prerequisites

- `track_realized_vol` must be enabled in HedgeConfig
- Hedging must be enabled (otherwise no capital tracking needed)
- P&L attribution must be enabled for attribution summary

---

## Testing Strategy

### Unit Tests
1. `VolatilitySummary` computation from rolls with/without vol data
2. `CapitalSummary` computation - verify return calculation
3. `HedgePosition` peak share tracking

### Integration Tests
1. Run backtest with `--track-realized-vol` and verify output
2. Verify capital metrics match manual calculation
3. Compare annualized return with expected values

---

## References

- `cs-domain/src/hedging.rs` - HedgePosition, RealizedVolatilityMetrics
- `cs-domain/src/entities/rolling_result.rs` - RollingResult, RollPeriod
- `cs-cli/src/main.rs` - display_rolling_results()
