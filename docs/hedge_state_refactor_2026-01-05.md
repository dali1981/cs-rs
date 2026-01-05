# Hedge State Refactoring - 2026-01-05

## Problem Statement

The current hedging implementation in `cs-backtest/src/hedging_executor.rs` has a critical bug that causes cumulative over-hedging:

**Location**: Lines 84-92
```rust
let gamma = base_result.net_gamma.unwrap_or(0.0);
let spot_change = spot - base_result.spot_at_entry;  // ← BUG: always from entry
let new_delta = base_result.net_delta.unwrap_or(0.0) + gamma * spot_change;  // ← BUG: ignores existing hedge
```

**Issue**: Each rehedge calculates option delta from entry point, then hedges the FULL delta without accounting for existing stock position's delta contribution. This causes exponential over-hedging.

**Impact**: Trade #4 example:
- Expected hedge: ~100 shares, P&L ≈ -$200
- Actual hedge: 769 shares, P&L = -$1,667 (8.3x over-hedged)

## Root Cause

The algorithm recalculates option delta from entry on each iteration:
```
new_delta = entry_delta + gamma × (current_spot - entry_spot)
```

But it should track the **net position delta** (options + stock):
```
net_delta = option_delta + (stock_shares / CONTRACT_MULTIPLIER)
```

Then only hedge the **incremental** delta change, not the full amount.

## Solution: Stateful Hedge Tracking

Introduce `HedgeState` struct that encapsulates:
1. Option delta (updated incrementally via gamma)
2. Stock position (accumulated hedge shares)
3. Net position delta calculation
4. Hedge decision logic

### Design Principles

1. **Encapsulation**: All hedge state in one struct
2. **Correctness**: Track net position delta (options + stock)
3. **Incremental Updates**: Each update only needs current spot
4. **Real-time Ready**: Same interface works for live trading
5. **Testability**: State transitions can be unit tested
6. **Minimal Changes**: Preserve existing types

## Detailed Design

### 1. New Struct: `HedgeState`

**File**: `cs-domain/src/hedging.rs`

```rust
/// Stateful delta hedge manager
///
/// Tracks both option greeks and stock position to compute net exposure.
/// Call `update()` with each new spot observation; it returns a HedgeAction
/// if rebalancing is needed.
#[derive(Debug, Clone)]
pub struct HedgeState {
    // Configuration (immutable after creation)
    config: HedgeConfig,

    // Option position greeks (per-share, updated incrementally)
    option_delta: f64,
    option_gamma: f64,

    // Stock hedge position
    stock_shares: i32,

    // Reference point for incremental delta updates
    last_spot: f64,

    // Transaction history
    position: HedgePosition,
}
```

### 2. Interface

#### Constructor
```rust
pub fn new(
    config: HedgeConfig,
    initial_delta: f64,    // Option delta at entry (per-share)
    initial_gamma: f64,    // Option gamma at entry (per-share)
    initial_spot: f64,     // Spot price at entry
) -> Self
```

#### Query Methods
```rust
/// Net position delta (options + stock)
pub fn net_delta(&self) -> f64

/// Current stock position
pub fn stock_shares(&self) -> i32

/// Number of rehedges executed
pub fn rehedge_count(&self) -> usize

/// Check if max rehedges reached
pub fn at_max_rehedges(&self) -> bool
```

#### State Transition
```rust
/// Process a new spot price observation
///
/// Returns Some(HedgeAction) if a rebalance was executed, None otherwise.
pub fn update(
    &mut self,
    timestamp: DateTime<Utc>,
    new_spot: f64,
) -> Option<HedgeAction>
```

**Algorithm**:
1. `spot_change = new_spot - last_spot`
2. `option_delta += gamma × spot_change` (incremental update)
3. `last_spot = new_spot`
4. `net_delta = option_delta + stock_shares / multiplier`
5. If `should_rehedge(net_delta)`:
   - `shares_to_trade = -round(net_delta × multiplier)`
   - If `|shares_to_trade| >= min_size`:
     - `stock_shares += shares_to_trade`
     - Record `HedgeAction`
     - Return `Some(action)`
6. Return `None`

#### Finalization
```rust
/// Finalize position and compute P&L at exit
pub fn finalize(self, exit_spot: f64) -> HedgePosition
```

### 3. State Invariants

At any point:
- `net_delta = option_delta + (stock_shares / contract_multiplier)`
- After hedge: `|net_delta| < threshold` (for delta-threshold strategy)
- `position.cumulative_shares == stock_shares`

## Implementation Plan

### Phase 1: Add HedgeState to Domain Layer

**File**: `cs-domain/src/hedging.rs`

1. Add `HedgeState` struct (~15 lines)
2. Implement `HedgeState::new()` (~15 lines)
3. Implement `HedgeState::net_delta()` (~5 lines)
4. Implement `HedgeState::stock_shares()` (~3 lines)
5. Implement `HedgeState::rehedge_count()` (~3 lines)
6. Implement `HedgeState::at_max_rehedges()` (~5 lines)
7. Implement `HedgeState::update()` (~40 lines)
8. Implement `HedgeState::finalize()` (~10 lines)

**Estimated**: +100 lines

### Phase 2: Export HedgeState

**File**: `cs-domain/src/lib.rs`

Add `HedgeState` to public exports in `hedging` re-export section.

**Estimated**: +1 line

### Phase 3: Refactor HedgingExecutor

**File**: `cs-backtest/src/hedging_executor.rs`

**Before** (lines 60-120):
```rust
let mut hedge_position = HedgePosition::new();
let mut current_delta = base_result.net_delta.unwrap_or(0.0);

for rehedge_time in rehedge_times {
    // Max rehedge check
    // Get spot price
    // Recalculate delta from entry (BUG)
    // Check should_rehedge
    // Calculate shares
    // Create HedgeAction
    // Add to position
}

let hedge_pnl = hedge_position.calculate_pnl(exit_spot);
```

**After**:
```rust
let mut hedge_state = HedgeState::new(
    self.hedge_config.clone(),
    base_result.net_delta.unwrap_or(0.0),
    base_result.net_gamma.unwrap_or(0.0),
    base_result.spot_at_entry,
);

for rehedge_time in rehedge_times {
    if hedge_state.at_max_rehedges() {
        break;
    }
    if let Ok(spot) = self.equity_repo.get_spot_price(symbol, rehedge_time).await {
        hedge_state.update(rehedge_time, spot.to_f64());
    }
}

let hedge_position = hedge_state.finalize(base_result.spot_at_exit);
```

**Estimated**: -20 lines (simpler code)

### Phase 4: Testing

#### Unit Tests

Add to `cs-domain/src/hedging.rs`:

| Test | Purpose |
|------|---------|
| `test_initial_state` | Verify net_delta equals initial option delta |
| `test_single_hedge` | One update triggers hedge correctly |
| `test_incremental_delta` | Multiple updates accumulate delta |
| `test_stock_delta_offset` | After hedge, net_delta near zero |
| `test_no_double_hedge` | Same spot doesn't retrigger |
| `test_reverse_hedge` | Spot reversal hedges opposite direction |
| `test_max_rehedges` | Stops at limit |
| `test_min_size_filter` | Small changes filtered |

#### Integration Test

```bash
./target/debug/cs backtest \
  --earnings-file ./custom_earnings/PENG_2025.parquet \
  --symbols PENG \
  --start 2024-12-01 --end 2025-12-31 \
  --spread straddle \
  --hedge --hedge-strategy time --hedge-interval-hours 24
```

**Expected Results for Trade #4**:

| Metric | Before | After |
|--------|--------|-------|
| Cumulative shares | -769 | ~-100 |
| Hedge P&L | -$1,667 | ~-$200 |
| Delta+Gamma P&L | +$202 | +$202 |
| Net directional | -$1,465 | ~$0 |

## Files Modified

1. `cs-domain/src/hedging.rs` - Add HedgeState (+100 lines)
2. `cs-domain/src/lib.rs` - Export HedgeState (+1 line)
3. `cs-backtest/src/hedging_executor.rs` - Use HedgeState (-20 lines)

## Edge Cases

1. **Gamma = 0**: Delta stays constant
2. **Spot gaps**: Large delta jump → large hedge
3. **Max rehedges**: Stop early, track P&L
4. **Transaction costs**: Already in HedgeAction
5. **Negative shares**: Short stock supported

## Future Enhancements (Out of Scope)

- Gamma updates (currently assumes constant)
- Portfolio-level hedging
- Real-time streaming integration
- Hedge P&L attribution by period

## Success Criteria

- ✅ Unit tests pass for all state transitions
- ✅ Integration test shows ~8x reduction in hedge shares
- ✅ Hedge P&L approximately offsets delta+gamma P&L
- ✅ Code is simpler and more maintainable
- ✅ Same interface works for real-time and historical
