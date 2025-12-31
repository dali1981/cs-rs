# Trade Selection & Opportunity Detection

This document explains how cs-rs selects calendar spread trades around earnings events.

## Overview

Trade selection has two main phases:

1. **Expiration Selection**: Find suitable short/long expiry pairs based on DTE criteria
2. **Strike Selection**: Find the optimal strike via delta-space opportunity analysis

```
EarningsEvent
     │
     ▼
┌─────────────────┐     ┌──────────────────┐
│  Select Expiry  │────▶│  Build IV Surface │
│  (DTE filters)  │     │  (delta-space)    │
└─────────────────┘     └──────────────────┘
                               │
                               ▼
                        ┌──────────────────┐
                        │  Scan for Best   │
                        │  IV Ratio/Delta  │
                        └──────────────────┘
                               │
                               ▼
                        ┌──────────────────┐
                        │  Map Delta to    │
                        │  Tradable Strike │
                        └──────────────────┘
                               │
                               ▼
                        CalendarSpread
```

---

## Trade Selection Criteria

Defined in `cs-domain/src/strategies/mod.rs`:

```rust
pub struct TradeSelectionCriteria {
    pub min_short_dte: i32,      // Default: 3  (avoid gamma/pin risk)
    pub max_short_dte: i32,      // Default: 45 (reasonable front month)
    pub min_long_dte: i32,       // Default: 14 (ensure time value)
    pub max_long_dte: i32,       // Default: 90 (reasonable back month)
    pub target_delta: Option<f64>,        // Fixed delta (e.g., 0.50)
    pub min_iv_ratio: Option<f64>,        // Min short/long IV ratio
    pub max_bid_ask_spread_pct: Option<f64>,  // Liquidity filter
}
```

### DTE Constraints

| Leg | Min | Max | Rationale |
|-----|-----|-----|-----------|
| **Short** | 3 | 45 | Avoid gamma risk near expiry; reasonable front month |
| **Long** | 14 | 90 | Ensure time value; reasonable back month |

### Why These Defaults?

- **min_short_dte = 3**: Options within 3 DTE have high gamma risk and pin risk around strikes
- **max_short_dte = 45**: Beyond 45 days, the "earnings premium" in the short leg diminishes
- **min_long_dte = 14**: Long leg needs enough time value to not decay too fast
- **max_long_dte = 90**: Very long-dated options have low theta and liquidity concerns

---

## Opportunity Detection

The `OpportunityAnalyzer` scans delta-space for favorable IV ratios.

### Location

`cs-analytics/src/opportunity.rs`

### Key Structures

```rust
/// Calendar spread opportunity identified in delta-space
pub struct CalendarOpportunity {
    pub target_delta: f64,      // Delta level (e.g., 0.50)
    pub short_expiry: NaiveDate,
    pub long_expiry: NaiveDate,
    pub short_iv: f64,          // IV of short leg
    pub long_iv: f64,           // IV of long leg
    pub iv_ratio: f64,          // short_iv / long_iv
    pub score: f64,             // Composite score (higher = better)
}
```

### Opportunity Scoring

The analyzer scores opportunities based on three factors:

```rust
fn score_opportunity(&self, delta: f64, ratio: f64, short_iv: f64) -> f64 {
    // 1. Higher IV ratio = more edge (weight: 10x)
    let ratio_score = (ratio - 1.0) * 10.0;

    // 2. Higher absolute IV = more theta (weight: 2x)
    let iv_score = short_iv * 2.0;

    // 3. Prefer deltas closer to ATM (more liquid)
    let liquidity_score = 1.0 - (delta - 0.5).abs() * 2.0;

    ratio_score + iv_score + liquidity_score
}
```

**Example scores:**

| Delta | IV Ratio | Short IV | Score |
|-------|----------|----------|-------|
| 0.50  | 1.40     | 0.50     | 5.0   |
| 0.25  | 1.40     | 0.55     | 4.6   |
| 0.50  | 1.20     | 0.50     | 3.0   |

The opportunity with the highest score is selected.

---

## Strategy Types

### ATM Strategy (`--strategy atm`)

Simple strategy that selects the strike closest to spot price.

```rust
// cs-domain/src/strategies/atm.rs
impl TradingStrategy for ATMStrategy {
    fn select(...) -> Result<CalendarSpread, StrategyError> {
        let atm_strike = find_closest_strike(&chain_data.strikes, spot.value());
        // Use same strike for both legs
    }
}
```

### Delta Strategy (`--strategy delta`)

Fixed delta strategy that:
1. Builds delta-space IV surface
2. Converts target delta (e.g., 0.50) to strike price
3. Finds closest tradable strike

```bash
./cs backtest --strategy delta --target-delta 0.50
```

### Delta Scan Strategy (`--strategy delta-scan`)

Scans a range of deltas to find the best opportunity:

```bash
./cs backtest --strategy delta-scan --delta-range "0.25,0.75" --delta-scan-steps 5
```

This scans deltas: 0.25, 0.375, 0.50, 0.625, 0.75 and selects the one with highest score.

---

## Delta-to-Strike Mapping

The key challenge is converting a target delta (e.g., 0.50) to an executable strike price.

### Formula

For call options:
```
Δ = N(d₁)
d₁ = [ln(S/K) + (r + σ²/2)T] / (σ√T)

Solving for K:
K = S × exp(-(d₁ × σ√T - (r + σ²/2)T))

where d₁ = N⁻¹(Δ)
```

### Implementation

```rust
// cs-analytics/src/vol_slice.rs
pub fn delta_to_strike(&self, delta: f64, is_call: bool) -> Option<f64> {
    let iv = self.get_iv(delta)?;

    let d1 = if is_call {
        inv_norm_cdf(delta)
    } else {
        inv_norm_cdf(delta + 1.0)  // Put delta is negative
    };

    let sqrt_t = self.tte.sqrt();
    let exponent = -(d1 * iv * sqrt_t - (self.risk_free_rate + 0.5 * iv * iv) * self.tte);

    Some(self.spot * exponent.exp())
}
```

### Closest Strike Selection

After computing the theoretical strike, we find the closest tradable strike:

```rust
fn find_closest_strike(strikes: &[Strike], target: f64) -> Result<Strike, StrategyError> {
    strikes
        .iter()
        .min_by(|a, b| {
            let a_diff = (f64::from(**a) - target).abs();
            let b_diff = (f64::from(**b) - target).abs();
            a_diff.partial_cmp(&b_diff).unwrap()
        })
        .copied()
        .ok_or(StrategyError::NoStrikes)
}
```

---

## Expiration Selection

### Algorithm

```rust
fn select_expirations(
    expirations: &[NaiveDate],
    reference_date: NaiveDate,  // Earnings date
    min_short_dte, max_short_dte,
    min_long_dte, max_long_dte,
) -> Result<(NaiveDate, NaiveDate), StrategyError> {
    // 1. Sort expirations chronologically
    // 2. Find first expiry in short DTE range
    // 3. Find first expiry after short that's in long DTE range
    // 4. Return (short_exp, long_exp)
}
```

### Example

Given:
- Earnings date: 2025-06-20
- Available expirations: 2025-06-27, 2025-07-04, 2025-07-18, 2025-08-15
- Criteria: short 3-45 DTE, long 14-90 DTE

Result:
- Short: 2025-06-27 (7 DTE) ✓
- Long: 2025-07-18 (28 DTE) ✓

---

## IV Ratio Analysis

### What is IV Ratio?

```
IV Ratio = Short IV / Long IV
```

For earnings plays:
- **Short leg** (pre-earnings): High IV due to earnings uncertainty
- **Long leg** (post-earnings): Normal IV after uncertainty resolves

### Minimum IV Ratio

The `min_iv_ratio` filter ensures we only take trades with sufficient "edge":

```bash
./cs backtest --min-iv-ratio 1.10  # Require at least 10% IV premium
```

### Typical Values

| IV Ratio | Interpretation |
|----------|----------------|
| < 1.0    | Inverted term structure (unusual) |
| 1.0-1.1  | Minimal edge |
| 1.1-1.3  | Normal earnings premium |
| 1.3-1.5  | Strong opportunity |
| > 1.5    | Very high premium (verify liquidity) |

---

## CLI Examples

### Basic Backtest (ATM)
```bash
./cs backtest --start 2025-01-01 --end 2025-12-31 --strategy atm
```

### Fixed Delta
```bash
./cs backtest --start 2025-01-01 --end 2025-12-31 \
  --strategy delta --target-delta 0.40
```

### Delta Scan with IV Filter
```bash
./cs backtest --start 2025-01-01 --end 2025-12-31 \
  --strategy delta-scan \
  --delta-range "0.30,0.70" \
  --delta-scan-steps 9 \
  --min-iv-ratio 1.15
```

### Custom DTE Range
```bash
./cs backtest --start 2025-01-01 --end 2025-12-31 \
  --strategy delta-scan \
  --min-short-dte 5 \
  --max-short-dte 30 \
  --min-long-dte 21 \
  --max-long-dte 60
```

---

## Related Files

| File | Description |
|------|-------------|
| `cs-analytics/src/opportunity.rs` | Opportunity scoring & detection |
| `cs-analytics/src/vol_slice.rs` | Delta-to-strike conversion |
| `cs-analytics/src/delta_surface.rs` | Multi-expiry IV surface |
| `cs-domain/src/strategies/mod.rs` | Trade selection criteria |
| `cs-domain/src/strategies/delta.rs` | Delta strategy implementation |
| `cs-domain/src/strategies/atm.rs` | ATM strategy implementation |

---

## See Also

- [Delta Strategy Plan](delta_strategy_plan.md) - Full design document for delta-space strategies
- [IV Models Design](iv_models_design.md) - IV interpolation models (sticky-strike vs sticky-delta)
