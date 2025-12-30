# Multiple IV Models Design

## Overview

This document describes how to enable multiple IV interpolation models in cs-rs, specifically adding **sticky delta** support while preserving the current **sticky strike** behavior.

---

## Current Implementation: Sticky Strike

### Location
- `cs-analytics/src/iv_surface.rs` - Core IV surface implementation
- `cs-backtest/src/spread_pricer.rs` - Uses IV surface for pricing

### How It Works

The current `IVSurface` uses **sticky strike** interpolation:

```rust
// cs-analytics/src/iv_surface.rs:121-159
fn interpolate_strike(&self, points: &[&IVPoint], target_strike: Decimal) -> Option<f64> {
    // Linear interpolation between absolute strike values
    let weight: f64 = ((target_strike - l.strike) / (u.strike - l.strike))
        .try_into().unwrap_or(0.5);
    Some(l.iv + weight * (u.iv - l.iv))
}
```

**Key Characteristics:**
1. IV is indexed by **absolute strike (K)**
2. Interpolation is linear in K-space
3. Time interpolation uses sqrt(T) weighting
4. When spot moves, same K → same IV (smile doesn't move)

---

## IV Model Comparison

### Literature References

The canonical reference is **Derman (1999) "Regimes of Volatility"** which identifies three rules:
- Sticky Strike Rule ("Greed")
- Sticky Delta Rule ("Moderation")
- Implied Tree Model ("Fear")

Additional references:
- [Daglish, Hull, Suo - Volatility Surfaces](https://www-2.rotman.utoronto.ca/~hull/downloadablepublications/DaglishHullSuoRevised.pdf)
- [Derman - Patterns of Volatility Change](https://emanuelderman.com/wp-content/uploads/2013/09/smile-lecture9.pdf)
- [CFM - Smile Dynamics](https://www.cfm.com/wp-content/uploads/2022/12/249-2008-smile-dynamics-a-theory-of-the-implied-leverage-effect.pdf)

### Three Common Models

| Model | X-Axis | Formula | Literature Name |
|-------|--------|---------|-----------------|
| **Sticky Strike** | K (absolute) | σ(K, T) | "Sticky Strike Rule" |
| **Sticky Moneyness** | K/S | σ(K/S, T) | "Sticky Moneyness" (less common) |
| **Sticky Delta** | Δ | σ(Δ, T) | "Sticky Delta Rule" |

**Note**: "Sticky moneyness" appears in some papers (e.g., discussing hedging delta under different regimes) but is less standard than sticky strike/delta. Some authors treat it as a simplified version of sticky delta.

---

## Sticky Delta Model

### Concept

In **sticky delta**:
- IV is indexed by **Black-Scholes delta (Δ)**, not moneyness
- Call delta: Δ = N(d₁)
- Put delta: Δ = -N(-d₁)
- Options are quoted by delta: 25Δ put, ATM (50Δ), 25Δ call

### Key Insight: Circular Dependency

Delta depends on IV, but in sticky delta, IV depends on delta:

```
d₁ = [ln(S/K) + (r + σ²/2)T] / (σ√T)
Δ_call = N(d₁)
```

To find IV for a given strike K:
1. Need to know what delta K corresponds to
2. But delta depends on σ
3. And σ depends on delta (in sticky delta model)

**Solution**: Iterative approach or parameterized smile (SABR, SVI).

### Why Sticky Delta?

| Scenario | Sticky Strike | Sticky Delta |
|----------|---------------|--------------|
| Spot moves up | K=100 call keeps same IV | K=100 call's delta changed, gets new IV |
| 25Δ put | Different K at different spots | Always same IV regardless of spot |
| Market convention | Less common | FX and some equity options |

For **calendar spreads around earnings**, sticky delta can be more appropriate because:
- Spot jumps significantly post-earnings
- The smile re-centers based on delta, not absolute strikes
- More consistent with how market makers reprice

### Practical Implementation

Since delta is circular, two approaches:

**Approach A: Iterative (accurate)**
```
1. Start with initial guess σ₀
2. Compute Δ(K, σ₀)
3. Look up σ₁ = smile(Δ)
4. Repeat until σ converges
```

**Approach B: Delta-parameterized smile (common)**
```
1. Store smile as σ(Δ) directly from market quotes
2. For new strike K, compute Δ using ATM vol as proxy
3. Look up σ(Δ) from smile
```

Most practitioners use Approach B with standard delta pillars:
- 10Δ put, 25Δ put, ATM, 25Δ call, 10Δ call

---

## Proposed Design: Strategy Pattern

### 1. Create IV Model Trait

```rust
// cs-analytics/src/iv_model.rs

/// Interpolation model for IV surfaces
pub trait IVInterpolator: Send + Sync {
    /// Get IV for a specific option
    fn get_iv(
        &self,
        surface: &IVSurface,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64>;

    /// Model name for logging/debugging
    fn name(&self) -> &'static str;
}
```

### 2. Implement Sticky Strike (Current Behavior)

```rust
// cs-analytics/src/iv_model.rs

pub struct StickyStrikeInterpolator;

impl IVInterpolator for StickyStrikeInterpolator {
    fn get_iv(
        &self,
        surface: &IVSurface,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        // Current implementation - linear in K-space
        surface.interpolate_strike_internal(strike, expiration, is_call)
    }

    fn name(&self) -> &'static str { "sticky_strike" }
}
```

### 3. Implement Sticky Delta

```rust
// cs-analytics/src/iv_model.rs

pub struct StickyDeltaInterpolator;

impl IVInterpolator for StickyDeltaInterpolator {
    fn get_iv(
        &self,
        surface: &IVSurface,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        let spot = surface.spot_price();
        let moneyness = strike / spot;

        // Convert observed strikes to moneyness
        // Interpolate in moneyness space
        // Same logic but using K/S instead of K
        surface.interpolate_by_moneyness_internal(moneyness, expiration, is_call)
    }

    fn name(&self) -> &'static str { "sticky_delta" }
}
```

### 4. Add Model Enum for Configuration

```rust
// cs-domain/src/value_objects.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IVModel {
    #[default]
    StickyStrike,
    StickyMoneyness,
    StickyDelta,
}

impl IVModel {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().replace("-", "_").as_str() {
            "sticky_moneyness" | "moneyness" => IVModel::StickyMoneyness,
            "sticky_delta" | "delta" => IVModel::StickyDelta,
            _ => IVModel::StickyStrike,
        }
    }

    pub fn to_interpolator(&self) -> Box<dyn IVInterpolator> {
        match self {
            IVModel::StickyStrike => Box::new(StickyStrikeInterpolator),
            IVModel::StickyMoneyness => Box::new(StickyMoneynessInterpolator),
            IVModel::StickyDelta => Box::new(StickyDeltaInterpolator::default()),
        }
    }
}
```

### 5. Update IVSurface

```rust
// cs-analytics/src/iv_surface.rs

impl IVSurface {
    /// Get IV using specified interpolation model
    pub fn get_iv_with_model(
        &self,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
        model: &dyn IVInterpolator,
    ) -> Option<f64> {
        model.get_iv(self, strike, expiration, is_call)
    }

    /// Current method - remains as sticky strike for backwards compatibility
    pub fn get_iv(
        &self,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        // Unchanged - preserves current behavior
        self.get_iv_with_model(strike, expiration, is_call, &StickyStrikeInterpolator)
    }

    // Make internal methods public(crate) for interpolators
    pub(crate) fn interpolate_strike_internal(...) -> Option<f64> { ... }
    pub(crate) fn interpolate_by_moneyness_internal(...) -> Option<f64> { ... }
}
```

---

## Configuration Integration

### Add to BSConfig or Create IVConfig

```rust
// cs-analytics/src/black_scholes.rs

pub struct IVConfig {
    pub model: IVModel,
    pub min_iv: f64,
    pub max_iv: f64,
    // Future: SABR parameters, SVI parameters, etc.
}

impl Default for IVConfig {
    fn default() -> Self {
        Self {
            model: IVModel::StickyStrike,  // Preserve current behavior
            min_iv: 0.0001,
            max_iv: 5.0,
        }
    }
}
```

### Update SpreadPricer

```rust
// cs-backtest/src/spread_pricer.rs

pub struct SpreadPricer {
    bs_config: BSConfig,
    iv_config: IVConfig,  // NEW
    market_close: MarketTime,
}

impl SpreadPricer {
    pub fn with_iv_model(mut self, model: IVModel) -> Self {
        self.iv_config.model = model;
        self
    }

    fn price_leg(...) -> Result<LegPricing, PricingError> {
        // Use configured model
        let interpolator = self.iv_config.model.to_interpolator();
        let estimated_iv = iv_surface
            .and_then(|surface| {
                surface.get_iv_with_model(
                    strike.value(),
                    expiration,
                    option_type == OptionType::Call,
                    &interpolator,
                )
            })
            .unwrap_or(0.30);
        // ...
    }
}
```

---

## CLI Integration

```rust
// cs-cli/src/main.rs

#[derive(clap::ValueEnum, Clone, Debug, Default)]
pub enum IVModelArg {
    #[default]
    StickyStrike,
    StickyDelta,
}

#[derive(Parser)]
struct BacktestArgs {
    /// IV interpolation model
    #[arg(long, default_value = "sticky-strike")]
    iv_model: IVModelArg,
}
```

---

## Implementation Steps

### Phase 1: Refactor (Non-Breaking)
1. Add `IVInterpolator` trait to `cs-analytics`
2. Move current interpolation logic to `StickyStrikeInterpolator`
3. Add `get_iv_with_model()` method to `IVSurface`
4. Keep existing `get_iv()` unchanged (calls sticky strike internally)

### Phase 2: Add Sticky Delta
1. Implement `StickyDeltaInterpolator`
2. Add `IVModel` enum to `cs-domain`
3. Update `SpreadPricer` to accept model config
4. Add CLI flag

### Phase 3: Future Models
- **SABR**: Stochastic Alpha Beta Rho model
- **SVI**: Stochastic Volatility Inspired parameterization
- **Local Vol**: Dupire-style local volatility

---

## Sticky Delta Implementation Details

### Data Structure: Delta-Parameterized Smile

```rust
// cs-analytics/src/iv_model.rs

/// A point on the delta-parameterized smile
#[derive(Debug, Clone)]
pub struct DeltaIVPoint {
    pub delta: f64,        // e.g., -0.25 for 25Δ put, 0.25 for 25Δ call
    pub iv: f64,
    pub expiration: NaiveDate,
}

/// Smile parameterized by delta (for sticky delta model)
pub struct DeltaSmile {
    points: Vec<DeltaIVPoint>,  // Sorted by delta
    underlying: String,
    as_of_time: DateTime<Utc>,
    spot_price: Decimal,
}
```

### Building Delta Smile from Market Data

```rust
impl DeltaSmile {
    /// Build from IVSurface by computing delta for each point
    pub fn from_iv_surface(surface: &IVSurface, risk_free_rate: f64) -> Self {
        let spot: f64 = surface.spot_price().try_into().unwrap_or(0.0);
        let as_of = surface.as_of_time();

        let points: Vec<DeltaIVPoint> = surface.points().iter()
            .filter_map(|p| {
                let strike: f64 = p.strike.try_into().ok()?;
                let ttm = (p.expiration - as_of.date_naive()).num_days() as f64 / 365.0;
                if ttm <= 0.0 { return None; }

                // Compute delta using the point's own IV
                let delta = bs_delta(spot, strike, ttm, p.iv, p.is_call, risk_free_rate);

                Some(DeltaIVPoint {
                    delta,
                    iv: p.iv,
                    expiration: p.expiration,
                })
            })
            .collect();

        Self {
            points,
            underlying: surface.underlying().to_string(),
            as_of_time: as_of,
            spot_price: surface.spot_price(),
        }
    }
}
```

### Sticky Delta Interpolator

```rust
pub struct StickyDeltaInterpolator {
    risk_free_rate: f64,
    max_iterations: usize,
    tolerance: f64,
}

impl IVInterpolator for StickyDeltaInterpolator {
    fn get_iv(
        &self,
        surface: &IVSurface,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        // Build delta smile from surface
        let delta_smile = DeltaSmile::from_iv_surface(surface, self.risk_free_rate);

        // Iterative solve: find σ such that σ = smile(Δ(K, σ))
        let spot: f64 = surface.spot_price().try_into().ok()?;
        let strike_f64: f64 = strike.try_into().ok()?;
        let ttm = (expiration - surface.as_of_time().date_naive()).num_days() as f64 / 365.0;

        if ttm <= 0.0 { return None; }

        // Initial guess: ATM vol or 30%
        let mut sigma = delta_smile.get_atm_iv(expiration).unwrap_or(0.30);

        for _ in 0..self.max_iterations {
            // Compute delta at current sigma
            let delta = bs_delta(spot, strike_f64, ttm, sigma, is_call, self.risk_free_rate);

            // Look up IV for this delta
            let new_sigma = delta_smile.interpolate_by_delta(delta, expiration)?;

            // Check convergence
            if (new_sigma - sigma).abs() < self.tolerance {
                return Some(new_sigma);
            }

            sigma = new_sigma;
        }

        Some(sigma)  // Return best estimate even if not fully converged
    }

    fn name(&self) -> &'static str { "sticky_delta" }
}
```

### Delta Interpolation

```rust
impl DeltaSmile {
    /// Interpolate IV for a given delta value
    pub fn interpolate_by_delta(&self, target_delta: f64, expiration: NaiveDate) -> Option<f64> {
        // Filter to matching expiration
        let mut matching: Vec<_> = self.points.iter()
            .filter(|p| p.expiration == expiration)
            .collect();

        if matching.is_empty() {
            // Fall back to nearest expiration or cross-expiry interpolation
            return self.interpolate_cross_expiry(target_delta, expiration);
        }

        // Sort by delta
        matching.sort_by(|a, b| a.delta.partial_cmp(&b.delta).unwrap());

        // Find bracketing deltas
        let mut lower: Option<&DeltaIVPoint> = None;
        let mut upper: Option<&DeltaIVPoint> = None;

        for p in &matching {
            if p.delta < target_delta {
                lower = Some(p);
            } else if p.delta > target_delta && upper.is_none() {
                upper = Some(p);
                break;
            } else if (p.delta - target_delta).abs() < 1e-6 {
                return Some(p.iv);
            }
        }

        match (lower, upper) {
            (Some(l), Some(u)) => {
                let weight = (target_delta - l.delta) / (u.delta - l.delta);
                Some(l.iv + weight * (u.iv - l.iv))
            }
            (Some(l), None) => Some(l.iv),
            (None, Some(u)) => Some(u.iv),
            (None, None) => None,
        }
    }

    /// Get ATM IV (delta ≈ 0.5 for calls, -0.5 for puts)
    pub fn get_atm_iv(&self, expiration: NaiveDate) -> Option<f64> {
        // ATM is typically defined as 50-delta
        // Try both call and put side
        self.interpolate_by_delta(0.5, expiration)
            .or_else(|| self.interpolate_by_delta(-0.5, expiration))
    }
}
```

### Helper: Black-Scholes Delta

```rust
// cs-analytics/src/black_scholes.rs

/// Calculate Black-Scholes delta
pub fn bs_delta(
    spot: f64,
    strike: f64,
    ttm: f64,
    volatility: f64,
    is_call: bool,
    risk_free_rate: f64,
) -> f64 {
    use statrs::distribution::{ContinuousCDF, Normal};

    let normal = Normal::new(0.0, 1.0).unwrap();
    let sqrt_t = ttm.sqrt();
    let d1 = ((spot / strike).ln() + (risk_free_rate + 0.5 * volatility.powi(2)) * ttm)
        / (volatility * sqrt_t);

    if is_call {
        normal.cdf(d1)
    } else {
        normal.cdf(d1) - 1.0  // Put delta is negative
    }
}
```

---

## Sticky Moneyness (Simpler Alternative)

If you want a simpler "floating smile" without the delta circularity:

```rust
pub struct StickyMoneynessInterpolator;

impl IVInterpolator for StickyMoneynessInterpolator {
    fn get_iv(
        &self,
        surface: &IVSurface,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        let spot = surface.spot_price();
        let target_moneyness: f64 = (strike / spot).try_into().ok()?;

        // Interpolate in moneyness space (K/S)
        surface.interpolate_by_moneyness(target_moneyness, expiration, is_call)
    }

    fn name(&self) -> &'static str { "sticky_moneyness" }
}
```

This is simpler than sticky delta but still "floats" with spot.

---

## Testing Strategy

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_surface(spot: f64) -> IVSurface {
        // Create surface with smile: OTM puts have higher IV
        // 25Δ put (K≈95): IV=0.35
        // ATM (K=100): IV=0.30
        // 25Δ call (K≈105): IV=0.28
        let points = vec![
            IVPoint { strike: dec(95), iv: 0.35, ... },   // OTM put
            IVPoint { strike: dec(100), iv: 0.30, ... },  // ATM
            IVPoint { strike: dec(105), iv: 0.28, ... },  // OTM call
        ];
        IVSurface::new(points, "TEST", now, Decimal::from(spot))
    }

    #[test]
    fn test_sticky_strike_unchanged() {
        // Verify sticky strike gives same results as before refactor
        let surface = create_test_surface(100.0);
        let old_iv = surface.get_iv(dec(100), exp, true);
        let new_iv = surface.get_iv_with_model(dec(100), exp, true, &StickyStrikeInterpolator);
        assert_eq!(old_iv, new_iv);
    }

    #[test]
    fn test_sticky_strike_fixed_on_spot_move() {
        // Sticky strike: same K gives same IV regardless of spot
        let surface1 = create_test_surface(100.0);
        let surface2 = create_test_surface(110.0);  // Spot moved up

        let iv1 = surface1.get_iv_with_model(dec(100), exp, true, &StickyStrikeInterpolator);
        let iv2 = surface2.get_iv_with_model(dec(100), exp, true, &StickyStrikeInterpolator);

        // Same strike K=100 → same IV (0.30) in both
        assert_eq!(iv1, iv2);
    }

    #[test]
    fn test_sticky_moneyness_follows_spot() {
        // Sticky moneyness: same K/S gives same IV
        let surface1 = create_test_surface(100.0);
        let surface2 = create_test_surface(110.0);

        // K/S = 0.95 in both cases
        let iv1 = surface1.get_iv_with_model(
            dec(95),   // K=95, S=100, K/S=0.95
            exp, false, &StickyMoneynessInterpolator
        );
        let iv2 = surface2.get_iv_with_model(
            dec(1045) / dec(10),  // K=104.5, S=110, K/S=0.95
            exp, false, &StickyMoneynessInterpolator
        );

        // Same moneyness → same IV
        assert!((iv1.unwrap() - iv2.unwrap()).abs() < 0.001);
    }

    #[test]
    fn test_sticky_delta_follows_spot() {
        // Sticky delta: same Δ gives same IV
        let surface1 = create_test_surface(100.0);
        let surface2 = create_test_surface(110.0);

        let interpolator = StickyDeltaInterpolator::default();

        // Find strikes that give same delta in both surfaces
        // 25Δ put in surface1: K≈95 → Δ=-0.25
        // 25Δ put in surface2: K≈104.5 → Δ=-0.25 (different K, same Δ)
        let iv1 = surface1.get_iv_with_model(dec(95), exp, false, &interpolator);
        let iv2 = surface2.get_iv_with_model(dec(1045) / dec(10), exp, false, &interpolator);

        // Same delta → same IV (approximately, due to iteration)
        assert!((iv1.unwrap() - iv2.unwrap()).abs() < 0.005);
    }

    #[test]
    fn test_sticky_delta_convergence() {
        // Verify the iterative solver converges
        let surface = create_test_surface(100.0);
        let interpolator = StickyDeltaInterpolator {
            risk_free_rate: 0.05,
            max_iterations: 50,
            tolerance: 1e-6,
        };

        let iv = surface.get_iv_with_model(dec(97), exp, false, &interpolator);
        assert!(iv.is_some());
        assert!(iv.unwrap() > 0.0 && iv.unwrap() < 1.0);
    }
}
```

---

## Summary

### Available Models After Refactor

| Model | X-Axis | Use Case | Complexity |
|-------|--------|----------|------------|
| **Sticky Strike** | K | Default, backtesting with historical data | Simple |
| **Sticky Moneyness** | K/S | Earnings, spot jumps, simpler floating smile | Simple |
| **Sticky Delta** | Δ | FX-style, accurate floating smile | Iterative |

### Before vs After

| Aspect | Current | After Refactor |
|--------|---------|----------------|
| Default behavior | Sticky strike | Sticky strike (unchanged) |
| Model selection | Hardcoded | Configurable via trait |
| Breaking changes | N/A | None |
| CLI integration | None | `--iv-model [sticky-strike|sticky-moneyness|sticky-delta]` |
| Future extensibility | Limited | Add new `IVInterpolator` impls |

### When to Use Each

```
Sticky Strike (default):
  - Backtesting with exact historical option prices
  - Intraday where spot hasn't moved much
  - When you have dense strike grid

Sticky Moneyness:
  - Post-earnings repricing (spot jumped)
  - Quick approximation of floating smile
  - When you want simplicity

Sticky Delta:
  - FX options (market standard)
  - Accurate floating smile behavior
  - When delta-hedging Greeks matter
```

### Important Note: Intraday Pricing with Minute Data

**In our backtest setup, sticky strike and sticky moneyness give identical results.**

This is because we have minute-level option data and rebuild the IV surface at each
pricing time (entry and exit) with the current spot price. When `surface.spot_price()`
equals the spot used for all `IVPoint.underlying_price` values, interpolation in K-space
and K/S-space produces the same weights:

```
K₁ < K₂ < K₃  ⟺  K₁/S < K₂/S < K₃/S  (when S is constant)
```

**The models would differ if:**
- Surface is built at one spot, then reused when spot has moved
- Stress testing: shock spot while keeping the same smile
- EOD pricing with stale surface from market open

**For our use case:** Only `sticky_delta` produces different results because it uses
the Black-Scholes delta formula with iterative solving, which captures the non-linear
relationship between strike and delta.

### Key Insight

The **Strategy Pattern** allows:
1. Preserving current behavior as the default
2. Adding new models without touching existing code
3. Easy testing of each model in isolation
4. Runtime model selection via CLI or config
5. Future models (SABR, SVI) slot in cleanly
