# Implementation Plan: Trait-Based Straddle Design

## Overview

Replace the single `Straddle` struct with a trait + two concrete types (`LongStraddle`, `ShortStraddle`). This provides compile-time type safety for trade direction.

## Design Principles

- **No direction enum on strategy** - direction is encoded in the type itself
- **Two selector methods** - `select_long_straddle()` and `select_short_straddle()`
- **Shared validation** - extract common logic to avoid duplication
- **Backward compatible** - deprecate old `Straddle` as alias to `LongStraddle`

---

## Phase 1: Domain Layer (`cs-domain`)

### File: `cs-domain/src/entities.rs`

1. **Create `Straddle` trait** (new)
```rust
pub trait Straddle: CompositeTrade + Send + Sync {
    fn call_leg(&self) -> &OptionLeg;
    fn put_leg(&self) -> &OptionLeg;
    fn symbol(&self) -> &str;
    fn strike(&self) -> Strike;
    fn expiration(&self) -> NaiveDate;
    fn dte(&self, from: NaiveDate) -> i32;
}
```

2. **Create `LongStraddle` struct** (replaces current `Straddle`)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LongStraddle {
    pub call_leg: OptionLeg,
    pub put_leg: OptionLeg,
}
```
- `CompositeTrade::legs()` → both `LegPosition::Long`
- `LongStraddle::new(call_leg, put_leg)` with validation

3. **Create `ShortStraddle` struct** (new)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortStraddle {
    pub call_leg: OptionLeg,
    pub put_leg: OptionLeg,
}
```
- `CompositeTrade::legs()` → both `LegPosition::Short`
- `ShortStraddle::new(call_leg, put_leg)` with validation

4. **Shared validation** - Extract to private function:
```rust
fn validate_straddle_legs(call: &OptionLeg, put: &OptionLeg) -> Result<(), ValidationError>
```

5. **Backward compatibility** - Deprecate old `Straddle`:
```rust
#[deprecated(since = "0.3.0", note = "Use LongStraddle or ShortStraddle")]
pub type Straddle = LongStraddle;
```

### File: `cs-domain/src/lib.rs`
- Export `Straddle` trait, `LongStraddle`, `ShortStraddle`

---

## Phase 2: Strike Selection (`cs-domain/src/strike_selection/`)

### File: `mod.rs`

Update `StrikeSelector` trait:
```rust
fn select_long_straddle(
    &self, spot: &SpotPrice, surface: &IVSurface, min_expiration: NaiveDate,
) -> Result<LongStraddle, SelectionError> {
    Err(SelectionError::UnsupportedStrategy("...".into()))
}

fn select_short_straddle(
    &self, spot: &SpotPrice, surface: &IVSurface, min_expiration: NaiveDate,
) -> Result<ShortStraddle, SelectionError> {
    Err(SelectionError::UnsupportedStrategy("...".into()))
}

// Deprecate old method
#[deprecated]
fn select_straddle(...) -> Result<LongStraddle, SelectionError> {
    self.select_long_straddle(...)
}
```

Update `SelectionStrategy` trait similarly.

### File: `atm.rs`
- Implement `select_long_straddle` and `select_short_straddle`
- Share leg construction logic, differ only in final type construction

### File: `delta.rs`
- Delegate to ATM (straddles are always ATM)

### File: `straddle.rs`
- Update `StraddleStrategy` to implement both methods

---

## Phase 3: Backtest Config (`cs-backtest/src/config/`)

### File: `mod.rs`

Keep existing variants (no change needed):
```rust
pub enum SpreadType {
    // ...
    Straddle,           // → LongStraddle
    #[serde(rename = "short-straddle")]
    ShortStraddle,      // → ShortStraddle
    // ...
}
```

The `SpreadType` determines which selector method to call.

---

## Phase 4: Backtest Strategy (`cs-backtest/src/trade_strategy.rs`)

Two concrete strategies:
```rust
pub struct LongStraddleStrategy { timing: TimingStrategy }
pub struct ShortStraddleStrategy { timing: TimingStrategy }
```

Update `StrategyDispatch`:
```rust
match self.config.spread {
    SpreadType::Straddle => {
        // Use LongStraddleStrategy, call select_long_straddle
    }
    SpreadType::ShortStraddle => {
        // Use ShortStraddleStrategy, call select_short_straddle
    }
}
```

---

## Phase 5: Execution (`cs-backtest/src/execution/`)

### File: `straddle_impl.rs`

Implement `ExecutableTrade` for both types:
```rust
impl ExecutableTrade for LongStraddle {
    fn validate_entry(pricing: &CompositePricing, config: &ExecutionConfig) -> Result<(), ExecutionError> {
        // net_cost > 0 for long (debit)
        if pricing.net_cost < config.min_entry_cost { ... }
    }
}

impl ExecutableTrade for ShortStraddle {
    fn validate_entry(pricing: &CompositePricing, config: &ExecutionConfig) -> Result<(), ExecutionError> {
        // net_cost < 0 for short (credit)
        if pricing.net_cost.abs() < config.min_entry_cost { ... }
    }
}
```

---

## Phase 6: Results (`cs-domain/src/entities.rs`)

Add direction field to `StraddleResult`:
```rust
pub struct StraddleResult {
    // ... existing fields
    pub direction: StraddleDirection,  // For output serialization
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StraddleDirection { Long, Short }
```

---

## Files Changed Summary

| Crate | File | Changes |
|-------|------|---------|
| cs-domain | `entities.rs` | Add trait, two structs, deprecate old |
| cs-domain | `lib.rs` | Update exports |
| cs-domain | `strike_selection/mod.rs` | Two new trait methods |
| cs-domain | `strike_selection/atm.rs` | Implement both methods |
| cs-domain | `strike_selection/delta.rs` | Delegate both methods |
| cs-domain | `strike_selection/straddle.rs` | Implement both methods |
| cs-backtest | `config/mod.rs` | Add ShortStraddle variant (if not present) |
| cs-backtest | `trade_strategy.rs` | Update strategy dispatch |
| cs-backtest | `execution/straddle_impl.rs` | Impl for both types |

---

## Migration Path

1. Add new types alongside old `Straddle`
2. Deprecate `Straddle` (type alias to `LongStraddle`)
3. Update call sites incrementally
4. Remove deprecated items in future release
