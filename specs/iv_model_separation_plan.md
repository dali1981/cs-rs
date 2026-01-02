# IV Model Separation Plan

## Problem Statement

**Critical Bug**: OpportunityAnalyzer compares IVs at the same **delta** for short and long expirations, but calendar spreads trade at the same **strike**. These are not equivalent due to forward drift.

```
Delta 0.50 @ 7 DTE  → Strike $100
Delta 0.50 @ 30 DTE → Strike $102 (different due to forward drift)

Current scoring:  short_iv @ $100  vs  long_iv @ $102  ← Wrong comparison
Actual trade:     short_iv @ $100  vs  long_iv @ $100  ← What we actually trade
```

## Solution: Separate Selection and Pricing Models

### Rationale

| Concern | Best Model | Why |
|---------|------------|-----|
| **Trade Selection** | Sticky-Strike | Calendar spreads trade at same strike; compare IVs at same K |
| **Pricing/Interpolation** | Sticky-Delta | Handles spot movement better; smile floats with underlying |

### Architecture

```
                    ┌─────────────────────────────────────┐
                    │         SpreadPricer                │
                    │  - selection_provider: Box<dyn SelectionIVProvider>
                    │  - pricing_provider: Box<dyn PricingIVProvider>
                    └─────────────────────────────────────┘
                                    │
                    ┌───────────────┴───────────────┐
                    ▼                               ▼
         ┌──────────────────┐           ┌──────────────────┐
         │ SelectionIVProvider │         │ PricingIVProvider │
         │ (for scoring)       │         │ (for interpolation)│
         └──────────────────┘           └──────────────────┘
                    │                               │
         ┌──────────┴──────────┐         ┌─────────┴──────────┐
         ▼                     ▼         ▼         ▼          ▼
   StrikeSpace          DeltaSpace    Sticky    Sticky     Sticky
   Selection            Selection     Strike    Moneyness  Delta
   (correct)            (current)
```

---

## Phase 1: Define Selection Provider Trait

### New Types in `cs-analytics/src/selection_model.rs`

```rust
use chrono::NaiveDate;
use crate::delta_surface::DeltaVolSurface;

/// Model for trade selection IV comparison
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionModel {
    /// Compare IVs at same strike (correct for calendar spreads)
    #[default]
    StrikeSpace,
    /// Compare IVs at same delta (current behavior, incorrect)
    DeltaSpace,
}

/// IV pair result for selection scoring
#[derive(Debug, Clone, Copy)]
pub struct SelectionIVPair {
    pub short_iv: f64,
    pub long_iv: f64,
    pub strike: f64,  // The strike where comparison is made
}

/// Provider for IV comparison during trade selection
pub trait SelectionIVProvider: Send + Sync {
    /// Get IV pair at a target delta for two expirations
    ///
    /// For StrikeSpace: maps delta to strike once, gets both IVs at that strike
    /// For DeltaSpace: gets IVs at delta (current behavior)
    fn get_iv_pair(
        &self,
        surface: &DeltaVolSurface,
        delta: f64,
        short_exp: NaiveDate,
        long_exp: NaiveDate,
        is_call: bool,
    ) -> Option<SelectionIVPair>;
}
```

### Implementation: `StrikeSpaceSelection`

```rust
/// Correct selection model for calendar spreads
///
/// Maps delta to strike using the SHORT expiration, then compares
/// IVs at that same strike for both expirations.
pub struct StrikeSpaceSelection;

impl SelectionIVProvider for StrikeSpaceSelection {
    fn get_iv_pair(
        &self,
        surface: &DeltaVolSurface,
        delta: f64,
        short_exp: NaiveDate,
        long_exp: NaiveDate,
        is_call: bool,
    ) -> Option<SelectionIVPair> {
        // 1. Map delta to strike using SHORT expiration
        let strike = surface.delta_to_strike(delta, short_exp, is_call)?;

        // 2. Get IVs at that SAME strike for both expirations
        //    Need to convert strike back to delta for each expiration
        //    OR use strike-space surface directly
        let short_slice = surface.slice(short_exp)?;
        let long_slice = surface.slice(long_exp)?;

        // Get IV at strike (not delta) - need to add this method
        let short_iv = short_slice.get_iv_at_strike(strike)?;
        let long_iv = long_slice.get_iv_at_strike(strike)?;

        Some(SelectionIVPair {
            short_iv,
            long_iv,
            strike,
        })
    }
}
```

### Implementation: `DeltaSpaceSelection` (Current Behavior)

```rust
/// Current (incorrect) selection model - preserved for comparison
///
/// Compares IVs at the same delta, which means different strikes
/// for different expirations. This is INCORRECT for calendar spreads.
pub struct DeltaSpaceSelection;

impl SelectionIVProvider for DeltaSpaceSelection {
    fn get_iv_pair(
        &self,
        surface: &DeltaVolSurface,
        delta: f64,
        short_exp: NaiveDate,
        long_exp: NaiveDate,
        _is_call: bool,
    ) -> Option<SelectionIVPair> {
        let short_iv = surface.get_iv(delta, short_exp)?;
        let long_iv = surface.get_iv(delta, long_exp)?;

        // Strike is approximate - different for each expiration
        let strike = surface.delta_to_strike(delta, short_exp, true)?;

        Some(SelectionIVPair {
            short_iv,
            long_iv,
            strike,
        })
    }
}
```

---

## Phase 2: Add Strike-Space IV Lookup to VolSlice

### Required Addition to `cs-analytics/src/vol_slice.rs`

The `VolSlice` currently stores delta-IV pairs. We need to add reverse lookup:

```rust
impl VolSlice {
    /// Get IV at a specific strike (reverse lookup from delta-space)
    ///
    /// This converts strike to delta, then interpolates in delta-space.
    pub fn get_iv_at_strike(&self, strike: f64) -> Option<f64> {
        // Convert strike to delta using current slice parameters
        let delta = strike_to_delta(
            strike,
            self.spot,
            self.tte,
            self.atm_iv()?,  // Use ATM IV as initial guess
            self.risk_free_rate,
            true,  // Assume call for now
        )?;

        // Now interpolate in delta-space
        self.get_iv(delta)
    }
}

/// Convert strike to Black-Scholes delta
fn strike_to_delta(
    strike: f64,
    spot: f64,
    tte: f64,
    iv: f64,
    rfr: f64,
    is_call: bool,
) -> Option<f64> {
    if tte <= 0.0 || iv <= 0.0 {
        return None;
    }

    let d1 = ((spot / strike).ln() + (rfr + iv * iv / 2.0) * tte) / (iv * tte.sqrt());

    // Use standard normal CDF
    let delta = if is_call {
        normal_cdf(d1)
    } else {
        normal_cdf(d1) - 1.0
    };

    Some(delta)
}
```

**Note**: This requires iterative refinement since we're using ATM IV as an approximation. A more accurate implementation would iterate until IV and delta are consistent.

---

## Phase 3: Rename Existing Pricing Types

### Current Names → New Names

| Current | New | Location |
|---------|-----|----------|
| `IVModel` | `PricingModel` | `cs-analytics/src/iv_model.rs` |
| `IVInterpolator` | `PricingIVProvider` | `cs-analytics/src/iv_model.rs` |
| `StickyStrikeInterpolator` | `StickyStrikePricing` | `cs-analytics/src/iv_model.rs` |
| `StickyMoneynessInterpolator` | `StickyMoneynessPricing` | `cs-analytics/src/iv_model.rs` |
| `StickyDeltaInterpolator` | `StickyDeltaPricing` | `cs-analytics/src/iv_model.rs` |

### Updated Trait

```rust
/// Model for pricing IV interpolation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PricingModel {
    #[default]
    StickyStrike,
    StickyMoneyness,
    StickyDelta,
}

/// Provider for IV interpolation during pricing
pub trait PricingIVProvider: Send + Sync {
    fn get_iv(
        &self,
        surface: &IVSurface,
        strike: f64,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64>;
}
```

---

## Phase 4: Update OpportunityAnalyzer

### Current Code (cs-analytics/src/opportunity.rs:71-111)

```rust
// CURRENT - uses delta-space directly
for &delta in &self.config.delta_targets {
    let short_iv = match surface.get_iv(delta, short_expiry) {
        Some(iv) => iv,
        None => continue,
    };
    let long_iv = match surface.get_iv(delta, long_expiry) {
        Some(iv) if iv > 0.0 => iv,
        _ => continue,
    };
    let ratio = short_iv / long_iv;
    // ...
}
```

### New Code - Delegate to SelectionIVProvider

```rust
impl OpportunityAnalyzer {
    pub fn new(config: OpportunityConfig) -> Self {
        Self {
            config,
            selection_provider: Box::new(StrikeSpaceSelection),  // Default to correct model
        }
    }

    pub fn with_selection_model(mut self, model: SelectionModel) -> Self {
        self.selection_provider = model.to_provider();
        self
    }

    pub fn find_opportunities(
        &self,
        surface: &DeltaVolSurface,
        short_expiry: NaiveDate,
        long_expiry: NaiveDate,
    ) -> Vec<Opportunity> {
        let mut opportunities = Vec::new();

        for &delta in &self.config.delta_targets {
            // DELEGATE to selection provider
            let iv_pair = match self.selection_provider.get_iv_pair(
                surface,
                delta,
                short_expiry,
                long_expiry,
                true,  // is_call
            ) {
                Some(pair) => pair,
                None => continue,
            };

            if iv_pair.long_iv <= 0.0 {
                continue;
            }

            let ratio = iv_pair.short_iv / iv_pair.long_iv;
            let score = self.calculate_score(ratio, iv_pair.short_iv, delta);

            opportunities.push(Opportunity {
                target_delta: delta,
                strike: iv_pair.strike,
                short_iv: iv_pair.short_iv,
                long_iv: iv_pair.long_iv,
                iv_ratio: ratio,
                score,
            });
        }

        opportunities.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        opportunities
    }
}
```

---

## Phase 5: Update SpreadPricer

### Add Configurable Pricing Provider

```rust
pub struct SpreadPricer {
    bs_config: BSConfig,
    market_close: MarketTime,
    pricing_model: PricingModel,           // Renamed from iv_model
    // selection_provider not needed here - pricing happens after selection
}

impl SpreadPricer {
    pub fn with_pricing_model(mut self, model: PricingModel) -> Self {
        self.pricing_model = model;
        self
    }

    pub fn pricing_model(&self) -> PricingModel {
        self.pricing_model
    }
}
```

---

## Phase 6: CLI Integration

### Update Backtest Command

```rust
/// IV model for trade selection (comparing short vs long IV)
#[arg(long, default_value = "strike-space")]
selection_model: String,

/// IV model for pricing interpolation (when market price unavailable)
#[arg(long, default_value = "sticky-delta")]
pricing_model: String,
```

**Recommended Defaults:**
- `--selection-model strike-space` (correct for calendars)
- `--pricing-model sticky-delta` (handles spot movement)

---

## Migration Path

### Step 1: Add New Types (Non-Breaking)
1. Create `selection_model.rs` with `SelectionModel`, `SelectionIVProvider` trait
2. Implement `StrikeSpaceSelection` and `DeltaSpaceSelection`
3. Add `get_iv_at_strike()` to `VolSlice`

### Step 2: Rename Existing Types (Breaking)
1. `IVModel` → `PricingModel`
2. `IVInterpolator` → `PricingIVProvider`
3. Update all usages across crates

### Step 3: Update OpportunityAnalyzer
1. Add `selection_provider` field
2. Delegate IV comparison to provider
3. Default to `StrikeSpaceSelection`

### Step 4: Update CLI
1. Add `--selection-model` flag
2. Rename `--iv-model` to `--pricing-model`

### Step 5: Tests
1. Add tests comparing `StrikeSpaceSelection` vs `DeltaSpaceSelection` results
2. Verify that IV ratios now match actual traded ratios
3. Regression tests for existing behavior

---

## Files to Modify

| File | Changes |
|------|---------|
| `cs-analytics/src/lib.rs` | Export new types |
| `cs-analytics/src/selection_model.rs` | **NEW** - SelectionIVProvider trait + impls |
| `cs-analytics/src/iv_model.rs` | Rename to PricingModel/PricingIVProvider |
| `cs-analytics/src/vol_slice.rs` | Add `get_iv_at_strike()`, `strike_to_delta()` |
| `cs-analytics/src/opportunity.rs` | Delegate to SelectionIVProvider |
| `cs-backtest/src/spread_pricer.rs` | Rename iv_model → pricing_model |
| `cs-backtest/src/backtest_runner.rs` | Pass selection model to analyzer |
| `cs-cli/src/commands/backtest.rs` | Add `--selection-model` flag |

---

## Expected Behavior After Fix

```
Before (DeltaSpaceSelection - WRONG):
  Delta 0.50 scan:
    short_iv @ $100 (delta=0.50) = 0.35
    long_iv  @ $102 (delta=0.50) = 0.28
    ratio = 1.25

  Actual trade @ $100:
    short_iv = 0.35 (delta=0.50)
    long_iv  = 0.30 (delta=0.48) ← Different IV than scored!
    actual_ratio = 1.17 ← Edge is LOWER than expected

After (StrikeSpaceSelection - CORRECT):
  Delta 0.50 scan → Strike $100:
    short_iv @ $100 = 0.35 (delta=0.50)
    long_iv  @ $100 = 0.30 (delta=0.48)
    ratio = 1.17

  Actual trade @ $100:
    short_iv = 0.35
    long_iv  = 0.30
    actual_ratio = 1.17 ← Matches scored ratio!
```

The fix ensures that **what we score is what we trade**.
