# Delta-Space Opportunity Detection Strategy

## Overview

This document outlines the implementation of a **delta-space trading strategy** that:
1. Analyzes the IV surface in delta-space to identify calendar spread opportunities
2. Selects the closest executable contracts to the theoretical optimum
3. Uses sticky-delta interpolation for more realistic earnings vol modeling

## Milestones

| Milestone | Description | Complexity | Status |
|-----------|-------------|------------|--------|
| **M1** | Simple linear interpolation in delta-space | Low | ✅ Complete |
| **M2** | SVI fitting, variance-space, arbitrage detection | High | Planned |

**Note:** M2 is **additive** - M1 linear interpolation remains the default. SVI fitting is opt-in via `--vol-model svi`.

---

## Clarification: IV Model vs Vol Model

These are **two orthogonal concepts**:

### IV Model (`--iv-model`): Spot Dynamics
**Question:** When spot price moves, how does the IV surface shift?

| Model | IV stays anchored to | Example |
|-------|---------------------|---------|
| `sticky-strike` | Absolute strike K=$100 | If spot moves $100→$105, the K=100 strike keeps its IV |
| `sticky-moneyness` | Moneyness ratio K/S | If spot moves, the 5% OTM option keeps its IV |
| `sticky-delta` | Delta level Δ=0.25 | If spot moves, the 25Δ put keeps its IV |

**For earnings:** Use `sticky-delta` because IV crush is uniform across deltas (the 25Δ put IV drops the same % as the 50Δ call IV).

### Vol Model (`--vol-model`): Smile Interpolation
**Question:** Given market quotes at deltas 0.25, 0.50, 0.75, what's the IV at delta 0.40?

```
IV
 |    *                 *     <- Market quotes
 |      \             /
 |        *---------*         <- Linear (connect dots)
 |         \_______/          <- SVI (smooth curve)
 |
 +-----------------------------> Delta
      0.25  0.40  0.50  0.75
```

| Model | Method | Interpolation across expiries |
|-------|--------|-------------------------------|
| `linear` | Connect dots | Linear in **total variance** (σ²T) |
| `svi` | Fit parametric curve | Linear in **total variance** (σ²T) |

**Important:** Both models interpolate across expiries the same way (linear in variance). The difference is only **within a single expiry's smile**.

### What SVI Actually Is

SVI (Stochastic Volatility Inspired) is a **5-parameter smile model**:

```
w(k) = a + b × (ρ(k - m) + √((k - m)² + σ²))

where:
- w = total variance (σ²T)
- k = log-moneyness = ln(K/F)
- a, b, ρ, m, σ = 5 fitted parameters
```

It's parameterized by **log-moneyness**, not delta. The curve is **non-linear** (contains a square root). The 5 parameters control:
- `a`: minimum variance level
- `b`: wing slope
- `ρ`: skew (left wing steeper if ρ < 0)
- `m`: horizontal shift
- `σ`: ATM curvature

### Summary

| Concept | What it controls | Options |
|---------|-----------------|---------|
| **IV Model** | Spot dynamics (how surface shifts when spot moves) | sticky-strike, sticky-moneyness, sticky-delta |
| **Vol Model** | Smile interpolation (how to fill gaps between quotes) | linear, svi |

Both are independent - you can combine any IV model with any vol model.

---

## Why Delta-Space for Earnings Calendars

### Sticky-Strike vs Sticky-Delta

| Aspect | Sticky-Strike | Sticky-Delta |
|--------|---------------|--------------|
| IV indexed by | Absolute strike K | Black-Scholes delta Δ |
| Spot moves | Smile stays anchored to strikes | Smile "floats" with spot |
| Delta changes | Purely from spot/strike relationship | Delta stays stable |
| Post-earnings | IV crush varies by strike | IV crush uniform in delta-space |

### Why Sticky-Delta is Better for Earnings

1. **Modeling a vol event, not a spot event**
   - Earnings cause IV crush across the curve
   - The crush is roughly uniform in delta-space (25Δ put keeps its vol)
   - Strike-space would incorrectly model different IV changes at different strikes

2. **Surface reshapes in delta-space post-earnings**
   - Front-month IV collapses uniformly by delta
   - Back-month IV shifts less, preserving term structure shape
   - Delta-space captures this better than strike-space

3. **Cleaner gamma/vega hedging**
   - Delta-neutral hedging is straightforward in delta-space
   - Greeks computed at target delta, not strike
   - Rebalancing logic simpler when thinking in delta terms

### Why Variance-Space for Time Interpolation

Variance (σ²τ, or total variance) is **additive in time** and better behaved for interpolation than volatility directly:

```
Var(T) = σ² × T

# Interpolate total variance linearly:
Var(T) = Var(T1) + (Var(T2) - Var(T1)) * (T - T1) / (T2 - T1)

# Back out IV:
IV(T) = sqrt(Var(T) / T)
```

**Benefits:**
- Interpolating in vol-space can produce arbitrage (negative forward variance)
- Variance-space makes arbitrage easy to detect and avoid
- Forward variance = Var(T2) - Var(T1) must be ≥ 0

---

## Architecture

### Current Flow (ATM Strategy)
```
EarningsEvent → ATMStrategy → CalendarSpread (single strike = ATM)
                    ↓
              spot.closest(strikes)
```

### New Flow (Delta Strategy)
```
EarningsEvent → DeltaStrategy → CalendarSpread (strike from delta target)
                    ↓
              1. Build delta-parameterized IV surface
              2. Analyze term structure for opportunity
              3. Find optimal delta (e.g., 50Δ, 25Δ put, etc.)
              4. Map delta → strike for execution
```

---

# MILESTONE 1: Simple Linear Interpolation

**Goal:** Get delta-space strategy working with minimal complexity.

## M1 Phase 1: Delta-Space IV Surface (cs-analytics)

### M1.1 Create `VolSlice` Type (Single Expiry)

```rust
// cs-analytics/src/vol_slice.rs

use std::collections::BTreeMap;

/// A single expiry's volatility smile, parameterized by delta
#[derive(Debug, Clone)]
pub struct VolSlice {
    /// Expiration date
    expiration: NaiveDate,
    /// Time to expiry in years
    tte: f64,
    /// Delta → IV mapping (sorted by delta)
    smile: BTreeMap<OrderedFloat<f64>, f64>,
    /// Reference spot price
    spot: f64,
    /// Risk-free rate used for delta calculations
    risk_free_rate: f64,
}

impl VolSlice {
    /// Build from market data points
    pub fn from_points(
        points: &[(f64, f64)],  // (strike, iv) pairs
        spot: f64,
        tte: f64,
        risk_free_rate: f64,
        expiration: NaiveDate,
    ) -> Self {
        let mut smile = BTreeMap::new();

        for &(strike, iv) in points {
            // Compute delta for this strike/iv
            let delta = bs_delta(spot, strike, tte, iv, true, risk_free_rate);
            smile.insert(OrderedFloat(delta), iv);
        }

        Self {
            expiration,
            tte,
            smile,
            spot,
            risk_free_rate,
        }
    }

    /// Interpolate IV at a target delta using linear interpolation
    pub fn get_iv(&self, target_delta: f64) -> Option<f64> {
        if self.smile.is_empty() {
            return None;
        }

        let target = OrderedFloat(target_delta);

        // Find bracketing points
        let mut lower: Option<(f64, f64)> = None;
        let mut upper: Option<(f64, f64)> = None;

        for (&delta, &iv) in &self.smile {
            if delta <= target {
                lower = Some((delta.0, iv));
            } else if upper.is_none() {
                upper = Some((delta.0, iv));
                break;
            }
        }

        match (lower, upper) {
            (Some((d1, iv1)), Some((d2, iv2))) => {
                // Linear interpolation
                let weight = (target_delta - d1) / (d2 - d1);
                Some(iv1 + weight * (iv2 - iv1))
            }
            (Some((_, iv)), None) => Some(iv),  // Extrapolate flat
            (None, Some((_, iv))) => Some(iv),  // Extrapolate flat
            (None, None) => None,
        }
    }

    /// Get total variance at a delta (for time interpolation)
    pub fn get_total_variance(&self, delta: f64) -> Option<f64> {
        self.get_iv(delta).map(|iv| iv * iv * self.tte)
    }

    /// Map delta back to strike
    pub fn delta_to_strike(&self, delta: f64, is_call: bool) -> Option<f64> {
        let iv = self.get_iv(delta)?;

        // Invert Black-Scholes delta formula
        // For calls: Δ = N(d1), so d1 = N⁻¹(Δ)
        // d1 = [ln(S/K) + (r + σ²/2)T] / (σ√T)
        // Solve for K:
        // ln(S/K) = d1 * σ√T - (r + σ²/2)T
        // K = S * exp(-(d1 * σ√T - (r + σ²/2)T))

        let d1 = if is_call {
            inv_norm_cdf(delta)
        } else {
            inv_norm_cdf(delta + 1.0)  // Put delta is negative
        };

        let sqrt_t = self.tte.sqrt();
        let exponent = -(d1 * iv * sqrt_t - (self.risk_free_rate + 0.5 * iv * iv) * self.tte);

        Some(self.spot * exponent.exp())
    }
}
```

### M1.2 Create `DeltaVolSurface` Type (Multi-Expiry)

```rust
// cs-analytics/src/delta_surface.rs

/// Multi-expiry volatility surface in delta-space
#[derive(Debug, Clone)]
pub struct DeltaVolSurface {
    /// Slices indexed by expiration
    slices: BTreeMap<NaiveDate, VolSlice>,
    /// Reference spot
    spot: f64,
    /// Surface timestamp
    as_of: DateTime<Utc>,
    /// Symbol
    symbol: String,
}

impl DeltaVolSurface {
    /// Build from IVSurface
    pub fn from_iv_surface(
        surface: &IVSurface,
        risk_free_rate: f64,
    ) -> Self {
        let spot: f64 = surface.spot_price().try_into().unwrap_or(0.0);
        let as_of = surface.as_of_time();

        // Group points by expiration
        let mut by_expiry: BTreeMap<NaiveDate, Vec<(f64, f64)>> = BTreeMap::new();

        for point in surface.points() {
            let strike: f64 = point.strike.try_into().unwrap_or(0.0);
            by_expiry
                .entry(point.expiration)
                .or_default()
                .push((strike, point.iv));
        }

        // Build slices
        let mut slices = BTreeMap::new();
        for (exp, points) in by_expiry {
            let tte = (exp - as_of.date_naive()).num_days() as f64 / 365.0;
            if tte > 0.0 {
                let slice = VolSlice::from_points(&points, spot, tte, risk_free_rate, exp);
                slices.insert(exp, slice);
            }
        }

        Self {
            slices,
            spot,
            as_of,
            symbol: surface.symbol().to_string(),
        }
    }

    /// Get IV at target delta and expiration
    /// Uses linear interpolation in variance-time across expiries
    pub fn get_iv(&self, delta: f64, expiration: NaiveDate) -> Option<f64> {
        // Exact match
        if let Some(slice) = self.slices.get(&expiration) {
            return slice.get_iv(delta);
        }

        // Find bracketing expiries
        let mut lower: Option<(&NaiveDate, &VolSlice)> = None;
        let mut upper: Option<(&NaiveDate, &VolSlice)> = None;

        for (exp, slice) in &self.slices {
            if *exp <= expiration {
                lower = Some((exp, slice));
            } else if upper.is_none() {
                upper = Some((exp, slice));
                break;
            }
        }

        match (lower, upper) {
            (Some((_, s1)), Some((_, s2))) => {
                // Interpolate in variance-time space
                let var1 = s1.get_total_variance(delta)?;
                let var2 = s2.get_total_variance(delta)?;

                let t1 = s1.tte;
                let t2 = s2.tte;
                let t_target = (expiration - self.as_of.date_naive()).num_days() as f64 / 365.0;

                // Linear interpolation in variance
                let var_target = var1 + (var2 - var1) * (t_target - t1) / (t2 - t1);

                // Back out IV
                if t_target > 0.0 && var_target >= 0.0 {
                    Some((var_target / t_target).sqrt())
                } else {
                    None
                }
            }
            (Some((_, s)), None) => s.get_iv(delta),
            (None, Some((_, s))) => s.get_iv(delta),
            (None, None) => None,
        }
    }

    /// Get term structure at fixed delta
    pub fn term_structure(&self, delta: f64) -> Vec<(NaiveDate, f64)> {
        self.slices
            .iter()
            .filter_map(|(exp, slice)| {
                slice.get_iv(delta).map(|iv| (*exp, iv))
            })
            .collect()
    }

    /// Get smile at fixed expiration
    pub fn smile(&self, expiration: NaiveDate) -> Option<Vec<(f64, f64)>> {
        let slice = self.slices.get(&expiration)?;
        Some(
            slice.smile
                .iter()
                .map(|(d, iv)| (d.0, *iv))
                .collect()
        )
    }

    /// Map delta to strike at given expiration
    pub fn delta_to_strike(
        &self,
        delta: f64,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        // Get IV at this delta
        let iv = self.get_iv(delta, expiration)?;
        let tte = (expiration - self.as_of.date_naive()).num_days() as f64 / 365.0;

        if tte <= 0.0 {
            return None;
        }

        // Use any slice's risk_free_rate (they should all be the same)
        let rfr = self.slices.values().next()?.risk_free_rate;

        // Invert delta formula
        let d1 = if is_call {
            inv_norm_cdf(delta)
        } else {
            inv_norm_cdf(delta + 1.0)
        };

        let sqrt_t = tte.sqrt();
        let exponent = -(d1 * iv * sqrt_t - (rfr + 0.5 * iv * iv) * tte);

        Some(self.spot * exponent.exp())
    }
}
```

### M1.3 Helper: Inverse Normal CDF

```rust
// cs-analytics/src/math_utils.rs

/// Inverse of standard normal CDF (quantile function)
/// Uses Abramowitz & Stegun approximation
pub fn inv_norm_cdf(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }

    // Rational approximation for central region
    let a = [
        -3.969683028665376e+01,
         2.209460984245205e+02,
        -2.759285104469687e+02,
         1.383577518672690e+02,
        -3.066479806614716e+01,
         2.506628277459239e+00,
    ];
    let b = [
        -5.447609879822406e+01,
         1.615858368580409e+02,
        -1.556989798598866e+02,
         6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    let c = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
         4.374664141464968e+00,
         2.938163982698783e+00,
    ];
    let d = [
         7.784695709041462e-03,
         3.224671290700398e-01,
         2.445134137142996e+00,
         3.754408661907416e+00,
    ];

    let p_low = 0.02425;
    let p_high = 1.0 - p_low;

    if p < p_low {
        let q = (-2.0 * p.ln()).sqrt();
        (((((c[0]*q + c[1])*q + c[2])*q + c[3])*q + c[4])*q + c[5]) /
        ((((d[0]*q + d[1])*q + d[2])*q + d[3])*q + 1.0)
    } else if p <= p_high {
        let q = p - 0.5;
        let r = q * q;
        (((((a[0]*r + a[1])*r + a[2])*r + a[3])*r + a[4])*r + a[5]) * q /
        (((((b[0]*r + b[1])*r + b[2])*r + b[3])*r + b[4])*r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((c[0]*q + c[1])*q + c[2])*q + c[3])*q + c[4])*q + c[5]) /
        ((((d[0]*q + d[1])*q + d[2])*q + d[3])*q + 1.0)
    }
}
```

## M1 Phase 2: Opportunity Detection

### M1.2.1 Simple Opportunity Analyzer

```rust
// cs-analytics/src/opportunity.rs

/// Calendar spread opportunity identified in delta-space
#[derive(Debug, Clone)]
pub struct CalendarOpportunity {
    pub target_delta: f64,
    pub short_expiry: NaiveDate,
    pub long_expiry: NaiveDate,
    pub short_iv: f64,
    pub long_iv: f64,
    pub iv_ratio: f64,
    pub score: f64,
}

/// Simple opportunity analyzer (M1)
pub struct OpportunityAnalyzer {
    pub min_iv_ratio: f64,
    pub delta_targets: Vec<f64>,
}

impl Default for OpportunityAnalyzer {
    fn default() -> Self {
        Self {
            min_iv_ratio: 1.05,
            delta_targets: vec![0.25, 0.40, 0.50, 0.60, 0.75],
        }
    }
}

impl OpportunityAnalyzer {
    /// Find calendar opportunities across delta targets
    pub fn find_opportunities(
        &self,
        surface: &DeltaVolSurface,
        short_expiry: NaiveDate,
        long_expiry: NaiveDate,
    ) -> Vec<CalendarOpportunity> {
        let mut opportunities = Vec::new();

        for &delta in &self.delta_targets {
            let short_iv = match surface.get_iv(delta, short_expiry) {
                Some(iv) => iv,
                None => continue,
            };
            let long_iv = match surface.get_iv(delta, long_expiry) {
                Some(iv) => iv,
                None => continue,
            };

            let ratio = short_iv / long_iv;

            if ratio >= self.min_iv_ratio {
                let score = self.score_opportunity(delta, ratio, short_iv);
                opportunities.push(CalendarOpportunity {
                    target_delta: delta,
                    short_expiry,
                    long_expiry,
                    short_iv,
                    long_iv,
                    iv_ratio: ratio,
                    score,
                });
            }
        }

        opportunities.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        opportunities
    }

    fn score_opportunity(&self, delta: f64, ratio: f64, short_iv: f64) -> f64 {
        // Simple scoring:
        // - Higher IV ratio = more edge
        // - Higher absolute IV = more theta
        // - Prefer deltas closer to ATM (more liquid)
        let ratio_score = (ratio - 1.0) * 10.0;
        let iv_score = short_iv * 2.0;
        let liquidity_score = 1.0 - (delta - 0.5).abs() * 2.0;

        ratio_score + iv_score + liquidity_score
    }
}
```

## M1 Phase 3: Delta Strategy

```rust
// cs-domain/src/strategies/delta.rs

pub struct DeltaStrategy {
    pub criteria: TradeSelectionCriteria,
    pub target_delta: f64,
    pub risk_free_rate: f64,
    pub scan_mode: DeltaScanMode,
}

pub enum DeltaScanMode {
    Fixed,
    Scan { delta_range: (f64, f64), steps: usize },
}

impl TradingStrategy for DeltaStrategy {
    fn select(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
        option_type: OptionType,
    ) -> Result<CalendarSpread, StrategyError> {
        let iv_surface = chain_data.iv_surface
            .as_ref()
            .ok_or(StrategyError::NoIVData)?;

        let delta_surface = DeltaVolSurface::from_iv_surface(
            iv_surface,
            self.risk_free_rate,
        );

        // Select expirations
        let (short_exp, long_exp) = select_expirations(
            &chain_data.expirations,
            event.earnings_date,
            &self.criteria,
        )?;

        // Find target delta (fixed or scan)
        let target_delta = match self.scan_mode {
            DeltaScanMode::Fixed => self.target_delta,
            DeltaScanMode::Scan { delta_range, steps } => {
                let analyzer = OpportunityAnalyzer {
                    min_iv_ratio: self.criteria.min_iv_ratio.unwrap_or(1.0),
                    delta_targets: linspace(delta_range.0, delta_range.1, steps),
                };
                let opps = analyzer.find_opportunities(&delta_surface, short_exp, long_exp);
                opps.first()
                    .map(|o| o.target_delta)
                    .unwrap_or(self.target_delta)
            }
        };

        // Map delta to strike
        let theoretical_strike = delta_surface
            .delta_to_strike(target_delta, short_exp, option_type == OptionType::Call)
            .ok_or(StrategyError::NoStrikes)?;

        // Find closest tradable strike
        let closest_strike = find_closest_strike(&chain_data.strikes, theoretical_strike)?;

        // Build spread
        let short_leg = OptionLeg::new(
            event.symbol.clone(),
            closest_strike,
            short_exp,
            option_type,
        );
        let long_leg = OptionLeg::new(
            event.symbol.clone(),
            closest_strike,
            long_exp,
            option_type,
        );

        CalendarSpread::new(short_leg, long_leg).map_err(Into::into)
    }
}

fn linspace(start: f64, end: f64, n: usize) -> Vec<f64> {
    if n <= 1 {
        return vec![start];
    }
    let step = (end - start) / (n - 1) as f64;
    (0..n).map(|i| start + i as f64 * step).collect()
}

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

## M1 Phase 4: Integration

### Config Changes

```rust
// cs-backtest/src/config.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StrategyType {
    #[default]
    Atm,
    Delta,
    DeltaScan,
}

impl StrategyType {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().replace('-', "_").as_str() {
            "delta" => StrategyType::Delta,
            "delta_scan" => StrategyType::DeltaScan,
            _ => StrategyType::Atm,
        }
    }
}
```

### CLI Changes

```rust
#[arg(long, default_value = "atm")]
strategy: String,

#[arg(long, default_value = "0.50")]
target_delta: f64,

#[arg(long)]
delta_range: Option<String>,  // "0.25,0.75"
```

## M1 Deliverables

| File | Description |
|------|-------------|
| `cs-analytics/src/vol_slice.rs` | Single-expiry delta-parameterized smile |
| `cs-analytics/src/delta_surface.rs` | Multi-expiry surface with variance interpolation |
| `cs-analytics/src/math_utils.rs` | `inv_norm_cdf` helper |
| `cs-analytics/src/opportunity.rs` | Simple opportunity analyzer |
| `cs-domain/src/strategies/delta.rs` | Delta strategy implementation |

---

# MILESTONE 2: SVI Fitting & Arbitrage Detection

**Goal:** Production-quality surface with arbitrage-free guarantees and cleaner Greeks.

**Important:** M2 is **additive** to M1. The existing linear interpolation remains available and is the default. SVI fitting is an optional interpolation mode selected via configuration.

```rust
// InterpolationMode enum (configurable)
pub enum InterpolationMode {
    Linear,      // M1 - default, always available
    CubicSpline, // M2 - optional
    SVI,         // M2 - optional, requires fitting
}
```

```bash
# Use M1 (default)
./cs backtest --strategy delta-scan --vol-model linear

# Use M2 SVI
./cs backtest --strategy delta-scan --vol-model svi
```

## Why SVI?

SVI (Stochastic Volatility Inspired) is the industry workhorse for equity vol surfaces:

```
w(k) = a + b × (ρ(k - m) + √((k - m)² + σ²))
```

Where:
- `w` = total variance (σ²τ)
- `k` = log-moneyness = ln(K/F)
- `a, b, ρ, m, σ` = 5 parameters per slice

**Advantages:**
- Parsimonious (5 params vs many points)
- Guarantees no butterfly arbitrage if params satisfy constraints
- Fits equity skew naturally
- Extrapolates well to wings

## M2 Phase 1: SVI Slice Implementation

### M2.1.1 SVI Parameters & Constraints

```rust
// cs-analytics/src/svi.rs

/// SVI parameterization for a single expiry
#[derive(Debug, Clone, Copy)]
pub struct SVIParams {
    /// Minimum variance level
    pub a: f64,
    /// Slope of the wings
    pub b: f64,
    /// Correlation/skew (-1 to 1)
    pub rho: f64,
    /// Horizontal shift
    pub m: f64,
    /// Smoothness/ATM curvature
    pub sigma: f64,
}

impl SVIParams {
    /// Check no-arbitrage constraints
    pub fn is_valid(&self) -> bool {
        // Gatheral & Jacquier constraints:
        // 1. b >= 0 (positive slope)
        // 2. |rho| < 1 (valid correlation)
        // 3. sigma > 0 (positive curvature)
        // 4. a + b*sigma*sqrt(1 - rho^2) >= 0 (non-negative variance at wings)

        self.b >= 0.0
            && self.rho.abs() < 1.0
            && self.sigma > 0.0
            && self.a + self.b * self.sigma * (1.0 - self.rho.powi(2)).sqrt() >= 0.0
    }

    /// Compute total variance at log-moneyness k
    pub fn total_variance(&self, k: f64) -> f64 {
        let x = k - self.m;
        self.a + self.b * (self.rho * x + (x * x + self.sigma * self.sigma).sqrt())
    }

    /// Compute IV at log-moneyness k, given time to expiry
    pub fn iv(&self, k: f64, tte: f64) -> f64 {
        let w = self.total_variance(k);
        if w > 0.0 && tte > 0.0 {
            (w / tte).sqrt()
        } else {
            0.0
        }
    }
}
```

### M2.1.2 SVI Fitting via Quasi-Newton

```rust
// cs-analytics/src/svi_fitter.rs

/// Fit SVI to market data using L-BFGS-B
pub struct SVIFitter {
    /// Maximum iterations
    pub max_iter: usize,
    /// Convergence tolerance
    pub tolerance: f64,
}

impl SVIFitter {
    /// Fit SVI params to (log_moneyness, total_variance) pairs
    pub fn fit(&self, data: &[(f64, f64)]) -> Result<SVIParams, FitError> {
        if data.len() < 5 {
            return Err(FitError::InsufficientData);
        }

        // Initial guess from data
        let initial = self.initial_guess(data);

        // Optimize using Levenberg-Marquardt or L-BFGS-B
        let result = self.optimize(data, initial)?;

        // Verify constraints
        if !result.is_valid() {
            return Err(FitError::ConstraintViolation);
        }

        Ok(result)
    }

    fn initial_guess(&self, data: &[(f64, f64)]) -> SVIParams {
        // Heuristic initial values:
        // a = ATM variance
        // b = wing slope estimate
        // rho = skew direction
        // m = ATM location
        // sigma = curvature

        let atm_var = data.iter()
            .min_by(|(k1, _), (k2, _)| k1.abs().partial_cmp(&k2.abs()).unwrap())
            .map(|(_, v)| *v)
            .unwrap_or(0.04);

        SVIParams {
            a: atm_var * 0.5,
            b: 0.1,
            rho: -0.3,  // Typical equity skew
            m: 0.0,
            sigma: 0.1,
        }
    }

    fn optimize(&self, data: &[(f64, f64)], initial: SVIParams) -> Result<SVIParams, FitError> {
        // Levenberg-Marquardt on residuals:
        // min Σ (w_market(k_i) - w_svi(k_i; params))²

        let mut params = initial;

        for _ in 0..self.max_iter {
            let (residuals, jacobian) = self.compute_residuals_jacobian(data, &params);

            let delta = self.solve_normal_equations(&residuals, &jacobian)?;

            // Update with line search
            params = self.apply_update(params, delta);

            // Check convergence
            let norm: f64 = residuals.iter().map(|r| r * r).sum();
            if norm.sqrt() < self.tolerance {
                break;
            }
        }

        Ok(params)
    }

    // ... implementation details
}
```

### M2.1.3 Enhanced VolSlice with SVI

```rust
// cs-analytics/src/vol_slice.rs (M2 version)

/// Volatility slice with optional SVI fit
pub struct VolSlice {
    expiration: NaiveDate,
    tte: f64,
    spot: f64,
    forward: f64,  // Forward price for log-moneyness
    risk_free_rate: f64,

    /// Raw market points (delta → iv)
    raw_smile: BTreeMap<OrderedFloat<f64>, f64>,

    /// SVI fit (if available)
    svi_params: Option<SVIParams>,

    /// Interpolation mode
    mode: InterpolationMode,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum InterpolationMode {
    #[default]
    Linear,
    CubicSpline,
    SVI,
}

impl VolSlice {
    /// Get IV at target delta
    pub fn get_iv(&self, delta: f64) -> Option<f64> {
        match self.mode {
            InterpolationMode::Linear => self.linear_interp(delta),
            InterpolationMode::CubicSpline => self.spline_interp(delta),
            InterpolationMode::SVI => self.svi_interp(delta),
        }
    }

    fn svi_interp(&self, delta: f64) -> Option<f64> {
        let params = self.svi_params.as_ref()?;

        // Convert delta to log-moneyness
        // This requires inverting delta = N(d1) to get strike, then k = ln(K/F)
        let strike = self.delta_to_strike_internal(delta)?;
        let k = (strike / self.forward).ln();

        Some(params.iv(k, self.tte))
    }

    /// Fit SVI to the slice data
    pub fn fit_svi(&mut self) -> Result<(), FitError> {
        // Convert (delta, iv) to (log_moneyness, total_variance)
        let data: Vec<(f64, f64)> = self.raw_smile
            .iter()
            .filter_map(|(delta, iv)| {
                let strike = self.delta_to_strike_internal(delta.0)?;
                let k = (strike / self.forward).ln();
                let w = iv * iv * self.tte;
                Some((k, w))
            })
            .collect();

        let fitter = SVIFitter::default();
        self.svi_params = Some(fitter.fit(&data)?);
        self.mode = InterpolationMode::SVI;

        Ok(())
    }
}
```

## M2 Phase 2: SSVI for Surface Consistency

### M2.2.1 SSVI Parameterization

```rust
// cs-analytics/src/ssvi.rs

/// Surface SVI - consistent across all expiries
/// w(k, θ) = (θ/2) × (1 + ρφ(θ)k + √((φ(θ)k + ρ)² + 1 - ρ²))
pub struct SSVIParams {
    /// Correlation (skew)
    pub rho: f64,
    /// ATM total variance function: θ(t)
    pub theta: ATMVarianceCurve,
    /// Phi function: controls smile shape vs ATM level
    pub phi: PhiFunction,
}

/// ATM total variance as function of time
pub enum ATMVarianceCurve {
    /// Linear in time (simplest)
    Linear { slope: f64 },
    /// Power law: θ(t) = α × t^β
    PowerLaw { alpha: f64, beta: f64 },
    /// Interpolated from market
    Interpolated(Vec<(f64, f64)>),
}

/// Phi function controlling smile shape
pub enum PhiFunction {
    /// Heston-like: φ(θ) = η / θ^γ
    Heston { eta: f64, gamma: f64 },
    /// Power law: φ(θ) = η × θ^(-λ)
    PowerLaw { eta: f64, lambda: f64 },
}

impl SSVIParams {
    /// Total variance at (log_moneyness, time)
    pub fn total_variance(&self, k: f64, t: f64) -> f64 {
        let theta = self.theta.eval(t);
        let phi = self.phi.eval(theta);

        let x = phi * k;
        (theta / 2.0) * (1.0 + self.rho * x + ((x + self.rho).powi(2) + 1.0 - self.rho.powi(2)).sqrt())
    }

    /// Check calendar arbitrage: ∂w/∂t ≥ 0 for all k
    pub fn check_calendar_arbitrage(&self, t1: f64, t2: f64, k_range: (f64, f64)) -> bool {
        let n_points = 20;
        let dk = (k_range.1 - k_range.0) / n_points as f64;

        for i in 0..=n_points {
            let k = k_range.0 + i as f64 * dk;
            let w1 = self.total_variance(k, t1);
            let w2 = self.total_variance(k, t2);

            if w2 < w1 - 1e-10 {
                return false;  // Calendar arbitrage!
            }
        }
        true
    }
}
```

## M2 Phase 3: Arbitrage Detection

### M2.3.1 Butterfly Arbitrage Check

```rust
// cs-analytics/src/arbitrage.rs

/// Check for butterfly arbitrage in a smile
/// d²w/dk² ≥ 0 everywhere (convexity)
pub fn check_butterfly_arbitrage(slice: &VolSlice) -> Vec<ArbitrageViolation> {
    let mut violations = Vec::new();

    let deltas: Vec<f64> = (10..=90).map(|d| d as f64 / 100.0).collect();

    for window in deltas.windows(3) {
        let d1 = window[0];
        let d2 = window[1];
        let d3 = window[2];

        if let (Some(w1), Some(w2), Some(w3)) = (
            slice.get_total_variance(d1),
            slice.get_total_variance(d2),
            slice.get_total_variance(d3),
        ) {
            // Discrete second derivative
            let d2w = w1 - 2.0 * w2 + w3;

            if d2w < -1e-10 {
                violations.push(ArbitrageViolation::Butterfly {
                    delta: d2,
                    severity: -d2w,
                });
            }
        }
    }

    violations
}

/// Check for calendar arbitrage between slices
/// Forward variance must be non-negative
pub fn check_calendar_arbitrage(
    slice1: &VolSlice,
    slice2: &VolSlice,
) -> Vec<ArbitrageViolation> {
    let mut violations = Vec::new();

    if slice1.tte >= slice2.tte {
        return violations;  // slice1 should be nearer
    }

    let deltas: Vec<f64> = (10..=90).map(|d| d as f64 / 100.0).collect();

    for delta in deltas {
        if let (Some(w1), Some(w2)) = (
            slice1.get_total_variance(delta),
            slice2.get_total_variance(delta),
        ) {
            let forward_var = w2 - w1;

            if forward_var < -1e-10 {
                violations.push(ArbitrageViolation::Calendar {
                    delta,
                    expiry1: slice1.expiration,
                    expiry2: slice2.expiration,
                    forward_variance: forward_var,
                });
            }
        }
    }

    violations
}

#[derive(Debug, Clone)]
pub enum ArbitrageViolation {
    Butterfly { delta: f64, severity: f64 },
    Calendar { delta: f64, expiry1: NaiveDate, expiry2: NaiveDate, forward_variance: f64 },
}
```

## M2 Phase 4: Enhanced Opportunity Analyzer

### M2.4.1 Earnings-Aware Analysis

```rust
// cs-analytics/src/opportunity.rs (M2 version)

/// Enhanced analyzer with earnings-specific logic
pub struct EarningsOpportunityAnalyzer {
    pub min_iv_ratio: f64,
    pub delta_targets: Vec<f64>,
    pub crush_model: IVCrushModel,
}

/// Model for expected IV crush
pub enum IVCrushModel {
    /// Fixed percentage crush
    Fixed { crush_pct: f64 },
    /// Decays with DTE: crush(dte) = base × exp(-decay × dte)
    ExponentialDecay { base_crush: f64, decay_rate: f64 },
    /// Variance attribution: separate earnings vol from base vol
    VarianceAttribution,
}

impl EarningsOpportunityAnalyzer {
    /// Estimate implied earnings move from term structure
    pub fn implied_earnings_move(
        &self,
        surface: &DeltaVolSurface,
        pre_earnings_exp: NaiveDate,
        post_earnings_exp: NaiveDate,
        delta: f64,
    ) -> Option<f64> {
        let pre_iv = surface.get_iv(delta, pre_earnings_exp)?;
        let post_iv = surface.get_iv(delta, post_earnings_exp)?;

        let pre_tte = surface.tte(pre_earnings_exp)?;
        let post_tte = surface.tte(post_earnings_exp)?;

        // Variance attribution:
        // pre_var = base_var × pre_tte + earnings_var
        // post_var = base_var × post_tte
        //
        // Assuming base_var ≈ post_var / post_tte:
        // earnings_var = pre_var - (post_var / post_tte) × pre_tte

        let pre_var = pre_iv * pre_iv * pre_tte;
        let post_var = post_iv * post_iv * post_tte;
        let base_var_per_year = post_var / post_tte;

        let earnings_var = pre_var - base_var_per_year * pre_tte;

        if earnings_var > 0.0 {
            // Implied move ≈ sqrt(earnings_var) × sqrt(252) for daily
            // Or just sqrt(earnings_var) as a percentage
            Some(earnings_var.sqrt())
        } else {
            None
        }
    }

    /// Find best opportunity with delta mismatch analysis
    pub fn find_opportunities_with_hedge(
        &self,
        surface: &DeltaVolSurface,
        short_exp: NaiveDate,
        long_exp: NaiveDate,
        available_strikes: &[f64],
    ) -> Vec<CalendarOpportunityWithHedge> {
        let mut opportunities = Vec::new();

        for &target_delta in &self.delta_targets {
            // Get theoretical strike
            let theoretical_strike = match surface.delta_to_strike(target_delta, short_exp, true) {
                Some(s) => s,
                None => continue,
            };

            // Find closest available strike
            let actual_strike = available_strikes
                .iter()
                .min_by(|a, b| {
                    ((*a - theoretical_strike).abs())
                        .partial_cmp(&((*b - theoretical_strike).abs()))
                        .unwrap()
                })
                .copied()
                .unwrap_or(theoretical_strike);

            // Compute delta mismatch
            let short_iv = surface.get_iv(target_delta, short_exp).unwrap_or(0.3);
            let actual_delta = bs_delta(
                surface.spot(),
                actual_strike,
                surface.tte(short_exp).unwrap_or(0.1),
                short_iv,
                true,
                surface.risk_free_rate(),
            );

            let delta_mismatch = actual_delta - target_delta;

            // Get IVs and compute opportunity
            let long_iv = surface.get_iv(target_delta, long_exp).unwrap_or(0.3);
            let ratio = short_iv / long_iv;

            if ratio >= self.min_iv_ratio {
                opportunities.push(CalendarOpportunityWithHedge {
                    target_delta,
                    actual_delta,
                    delta_mismatch,
                    theoretical_strike,
                    actual_strike,
                    short_expiry: short_exp,
                    long_expiry: long_exp,
                    short_iv,
                    long_iv,
                    iv_ratio: ratio,
                    implied_move: self.implied_earnings_move(surface, short_exp, long_exp, target_delta),
                    score: self.score_with_mismatch(ratio, short_iv, delta_mismatch),
                });
            }
        }

        opportunities.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        opportunities
    }

    fn score_with_mismatch(&self, ratio: f64, iv: f64, mismatch: f64) -> f64 {
        let base_score = (ratio - 1.0) * 10.0 + iv * 2.0;
        // Penalize delta mismatch
        let mismatch_penalty = mismatch.abs() * 5.0;
        base_score - mismatch_penalty
    }
}

#[derive(Debug, Clone)]
pub struct CalendarOpportunityWithHedge {
    pub target_delta: f64,
    pub actual_delta: f64,
    pub delta_mismatch: f64,
    pub theoretical_strike: f64,
    pub actual_strike: f64,
    pub short_expiry: NaiveDate,
    pub long_expiry: NaiveDate,
    pub short_iv: f64,
    pub long_iv: f64,
    pub iv_ratio: f64,
    pub implied_move: Option<f64>,
    pub score: f64,
}
```

## M2 Deliverables

| File | Description |
|------|-------------|
| `cs-analytics/src/svi.rs` | SVI parameters and evaluation |
| `cs-analytics/src/svi_fitter.rs` | SVI fitting via L-M or L-BFGS-B |
| `cs-analytics/src/ssvi.rs` | Surface SVI for cross-expiry consistency |
| `cs-analytics/src/arbitrage.rs` | Butterfly and calendar arbitrage detection |
| `cs-analytics/src/opportunity.rs` | Enhanced with earnings variance attribution |

---

## Testing Strategy

### M1 Tests

```rust
#[test]
fn test_vol_slice_linear_interp() {
    // Verify linear interpolation between known points
}

#[test]
fn test_delta_to_strike_roundtrip() {
    // delta → strike → delta should recover original
}

#[test]
fn test_variance_time_interpolation() {
    // Interpolate between expiries, verify no negative forward var
}

#[test]
fn test_opportunity_scoring() {
    // Higher IV ratio should score higher
}
```

### M2 Tests

```rust
#[test]
fn test_svi_fit_recovery() {
    // Generate synthetic SVI data, fit, verify params recovered
}

#[test]
fn test_svi_arbitrage_free_constraints() {
    // Verify fitted params satisfy no-arb constraints
}

#[test]
fn test_butterfly_arbitrage_detection() {
    // Create non-convex smile, verify detection
}

#[test]
fn test_calendar_arbitrage_detection() {
    // Create inverted term structure, verify detection
}

#[test]
fn test_implied_earnings_move() {
    // Known IV term structure → expected implied move
}
```

---

## CLI Usage Summary

```bash
# M1: Simple delta strategy (linear interpolation - default)
./cs backtest --strategy delta --target-delta 0.50

# M1: Scan for best delta (linear interpolation - default)
./cs backtest --strategy delta-scan --delta-range "0.25,0.75"

# M1: Explicit linear mode
./cs backtest --strategy delta-scan --vol-model linear

# M2: With SVI fitting (future, opt-in)
./cs backtest --strategy delta --target-delta 0.40 --vol-model svi

# M2: Show arbitrage warnings (future)
./cs backtest --strategy delta-scan --vol-model svi --check-arbitrage
```

---

## References

- Derman (1999) "Regimes of Volatility"
- Gatheral (2006) "The Volatility Surface"
- Gatheral & Jacquier (2014) "Arbitrage-free SVI volatility surfaces"
- Taleb (1997) "Dynamic Hedging"
