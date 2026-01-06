# Plan: Derive IV from CompositeTrade Structure

**Date**: 2026-01-05
**Status**: Draft
**Depends on**: CompositeTrade trait implementation

## Problem

Different trade types represent IV differently:

| Trade Type | Current IV Fields | Key Metric |
|------------|-------------------|------------|
| Straddle | `iv_entry`, `iv_exit` | avg of call/put |
| IronButterfly | `iv_entry`, `iv_exit` | avg of 4 legs |
| CalendarSpread | `iv_short_entry`, `iv_long_entry`, ... | `iv_ratio` |
| CalendarStraddle | `short_iv_entry`, `long_iv_entry`, ... | `iv_ratio` |

This leads to:
- Hardcoded `None` in generic code (like `to_roll_period()`)
- Each trade type needs custom IV extraction
- Adding new strategies requires updating trait implementations

## Solution: Derive IV from Leg Structure

Since `CompositeTrade` knows the legs and their positions, we can:
1. Store IV per leg in `CompositePricing`
2. Compute aggregate IV metrics generically
3. Handle calendar vs non-calendar automatically based on expiration structure

## Design

### 1. Enhanced LegPricing

```rust
// cs-backtest/src/spread_pricer.rs (existing, enhance)
pub struct LegPricing {
    pub price: Decimal,
    pub iv: Option<f64>,
    pub greeks: Option<Greeks>,
    pub expiration: NaiveDate,  // ADD: needed for calendar detection
}
```

### 2. CompositePricing IV Methods

```rust
// cs-backtest/src/composite_pricer.rs

impl CompositePricing {
    /// Average IV across all legs
    pub fn avg_iv(&self) -> Option<f64> {
        let ivs: Vec<f64> = self.legs.iter()
            .filter_map(|(p, _)| p.iv)
            .collect();

        if ivs.is_empty() {
            None
        } else {
            Some(ivs.iter().sum::<f64>() / ivs.len() as f64)
        }
    }

    /// IV grouped by expiration (for calendars)
    pub fn iv_by_expiration(&self) -> BTreeMap<NaiveDate, f64> {
        use std::collections::BTreeMap;

        let mut by_expiry: BTreeMap<NaiveDate, Vec<f64>> = BTreeMap::new();

        for (pricing, _) in &self.legs {
            if let Some(iv) = pricing.iv {
                by_expiry.entry(pricing.expiration)
                    .or_default()
                    .push(iv);
            }
        }

        by_expiry.into_iter()
            .map(|(exp, ivs)| (exp, ivs.iter().sum::<f64>() / ivs.len() as f64))
            .collect()
    }

    /// Detect if this is a calendar structure (multiple expirations)
    pub fn is_calendar(&self) -> bool {
        let expirations: HashSet<NaiveDate> = self.legs.iter()
            .map(|(p, _)| p.expiration)
            .collect();
        expirations.len() > 1
    }

    /// For calendars: short IV / long IV ratio
    pub fn iv_ratio(&self) -> Option<f64> {
        if !self.is_calendar() {
            return None;
        }

        let by_exp = self.iv_by_expiration();
        let expirations: Vec<_> = by_exp.keys().collect();

        if expirations.len() != 2 {
            return None;  // Not a simple calendar
        }

        let short_exp = expirations[0];  // Earlier = short
        let long_exp = expirations[1];   // Later = long

        let short_iv = by_exp.get(short_exp)?;
        let long_iv = by_exp.get(long_exp)?;

        Some(short_iv / long_iv)
    }

    /// Primary IV metric (for display)
    /// - Non-calendar: average IV
    /// - Calendar: short leg IV (earnings-affected)
    pub fn primary_iv(&self) -> Option<f64> {
        if self.is_calendar() {
            let by_exp = self.iv_by_expiration();
            by_exp.values().next().copied()  // Earliest expiration
        } else {
            self.avg_iv()
        }
    }
}
```

### 3. CompositeIV Struct (for results)

```rust
// cs-domain/src/trade/composite.rs

/// IV information extracted from composite pricing
#[derive(Debug, Clone, Copy)]
pub struct CompositeIV {
    /// Primary IV (avg for non-calendars, short for calendars)
    pub primary: f64,
    /// IV ratio for calendars (short/long), None for non-calendars
    pub ratio: Option<f64>,
    /// Full breakdown by expiration
    pub by_expiration: Option<(f64, f64)>,  // (short_iv, long_iv)
}

impl CompositeIV {
    /// Create from non-calendar (single IV)
    pub fn single(iv: f64) -> Self {
        Self {
            primary: iv,
            ratio: None,
            by_expiration: None,
        }
    }

    /// Create from calendar (short/long IV)
    pub fn calendar(short_iv: f64, long_iv: f64) -> Self {
        Self {
            primary: short_iv,  // Short = earnings-affected
            ratio: Some(short_iv / long_iv),
            by_expiration: Some((short_iv, long_iv)),
        }
    }

    /// Change between entry and exit
    pub fn change(&self, exit: &CompositeIV) -> CompositeIVChange {
        CompositeIVChange {
            primary_change: (exit.primary - self.primary) / self.primary * 100.0,
            ratio_change: match (self.ratio, exit.ratio) {
                (Some(entry), Some(exit)) => Some((exit - entry) / entry * 100.0),
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CompositeIVChange {
    pub primary_change: f64,
    pub ratio_change: Option<f64>,
}
```

### 4. Updated TradeResult Trait

```rust
// cs-domain/src/trade/rollable.rs

pub trait TradeResult: Send + Sync {
    // ... existing methods ...

    /// IV at entry (derived from composite structure)
    fn entry_iv(&self) -> Option<CompositeIV> { None }

    /// IV at exit
    fn exit_iv(&self) -> Option<CompositeIV> { None }

    /// IV change (computed from entry/exit)
    fn iv_change(&self) -> Option<CompositeIVChange> {
        match (self.entry_iv(), self.exit_iv()) {
            (Some(entry), Some(exit)) => Some(entry.change(&exit)),
            _ => None,
        }
    }
}
```

### 5. Generic Result with IV

```rust
// cs-backtest/src/execution/composite_impl.rs

pub struct CompositeResult {
    // ... existing fields ...

    /// IV at entry (automatically computed from pricing)
    pub entry_iv: Option<CompositeIV>,
    /// IV at exit
    pub exit_iv: Option<CompositeIV>,
}

impl CompositeResult {
    pub fn from_pricing(
        entry_pricing: &CompositePricing,
        exit_pricing: &CompositePricing,
        // ... other args ...
    ) -> Self {
        let entry_iv = Self::extract_iv(entry_pricing);
        let exit_iv = Self::extract_iv(exit_pricing);

        Self {
            entry_iv,
            exit_iv,
            // ...
        }
    }

    fn extract_iv(pricing: &CompositePricing) -> Option<CompositeIV> {
        if pricing.is_calendar() {
            let by_exp = pricing.iv_by_expiration();
            let exps: Vec<_> = by_exp.iter().collect();
            if exps.len() == 2 {
                Some(CompositeIV::calendar(*exps[0].1, *exps[1].1))
            } else {
                None
            }
        } else {
            pricing.avg_iv().map(CompositeIV::single)
        }
    }
}

impl TradeResult for CompositeResult {
    fn entry_iv(&self) -> Option<CompositeIV> { self.entry_iv }
    fn exit_iv(&self) -> Option<CompositeIV> { self.exit_iv }
}
```

### 6. Updated to_roll_period()

```rust
// cs-backtest/src/trade_executor.rs

fn to_roll_period(&self, trade: &T, result: T::Result, roll_reason: RollReason) -> RollPeriod {
    let iv_change = result.iv_change();

    RollPeriod {
        // ... existing fields ...

        // IV now derived automatically!
        iv_entry: result.entry_iv().map(|iv| iv.primary),
        iv_exit: result.exit_iv().map(|iv| iv.primary),
        iv_change: iv_change.map(|c| c.primary_change),

        // Calendar-specific (optional display)
        iv_ratio_entry: result.entry_iv().and_then(|iv| iv.ratio),
        iv_ratio_exit: result.exit_iv().and_then(|iv| iv.ratio),

        // ...
    }
}
```

## Benefits

1. **Automatic detection**: Calendar vs non-calendar determined by expiration structure
2. **No per-type code**: IV extraction is generic across all composite trades
3. **Adding new strategies**: Just implement `CompositeTrade::legs()`, IV works automatically
4. **Rich data**: Both primary IV and ratio available for analytics
5. **Type-safe**: `CompositeIV` captures the structure, not just `Option<f64>`

## Migration Path

### Phase 1: Add to CompositePricing (non-breaking)

1. Add `expiration` field to `LegPricing`
2. Add `avg_iv()`, `iv_by_expiration()`, `is_calendar()`, `iv_ratio()` to `CompositePricing`
3. Test with existing pricers

### Phase 2: Add CompositeIV (non-breaking)

1. Create `CompositeIV` and `CompositeIVChange` structs
2. Add `entry_iv()`, `exit_iv()` to `TradeResult` trait with default impl
3. Implement for existing result types

### Phase 3: Use in Generic Code

1. Update `to_roll_period()` to use trait methods
2. Update CLI output to show IV data
3. Remove hardcoded `None` values

### Phase 4: Migrate to CompositeResult (optional)

1. Create generic `CompositeResult` struct
2. Migrate existing result types or keep both
3. Use `CompositeResult` for new strategies

## Files Changed

| File | Change |
|------|--------|
| `cs-backtest/src/spread_pricer.rs` | Add `expiration` to `LegPricing` |
| `cs-backtest/src/composite_pricer.rs` | Add IV computation methods |
| `cs-domain/src/trade/composite.rs` | Add `CompositeIV`, `CompositeIVChange` |
| `cs-domain/src/trade/rollable.rs` | Add `entry_iv()`, `exit_iv()` to trait |
| `cs-backtest/src/trade_executor.rs` | Use trait methods in `to_roll_period()` |
| `cs-domain/src/entities.rs` | Implement trait for existing result types |

## Example: Adding Iron Condor

With this design, adding Iron Condor requires only:

```rust
impl CompositeTrade for IronCondor {
    fn legs(&self) -> Vec<(&OptionLeg, LegPosition)> {
        vec![
            (&self.long_put_wing, Long),
            (&self.short_put, Short),
            (&self.short_call, Short),
            (&self.long_call_wing, Long),
        ]
    }
}

// IV extraction is automatic:
// - is_calendar() returns false (single expiration)
// - avg_iv() returns average of 4 legs
// - iv_ratio() returns None
```

No custom IV code needed!
