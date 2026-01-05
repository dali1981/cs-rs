# Integrated Position Model with Daily P&L Attribution

**Date**: 2026-01-05
**Status**: Planning
**Scope**: Delta-hedged straddle strategies

## Problem Statement

Current P&L attribution is confusing and incorrect for delta-hedged strategies:

1. **Option-only attribution**: Delta P&L shows `-$130.48` (options only), while hedge P&L shows `+$13.13` separately
2. **Entry Greeks × total move**: Uses static entry Greeks × total spot move, ignoring that Greeks change daily
3. **No integrated view**: Cannot understand actual delta exposure of the hedged position

### Example from PENG Roll #1:
```
Current Output (Confusing):
  Option Delta P&L: -$130.48
  Hedge P&L:        +$13.13   (shown separately)
  Net Delta Loss:   ???       (user must do mental math)

Desired Output (Clear):
  Gross Delta P&L:  -$130.48  (unhedged)
  Net Delta P&L:    -$117.35  (hedged: options + shares)
  Hedge Efficiency: 10.1%     (13.13 / 130.48)
```

## Design Goals

1. **Position-level deltas** (not per-share):
   - Long 1 ATM Call: delta ≈ +50 (0.5 × 100 shares)
   - Long 1 ATM Put: delta ≈ -50 (-0.5 × 100 shares)
   - Short N shares: delta = -N (direct share count)

2. **Daily attribution loop**:
   ```
   For each day in holding period:
     net_delta = option_delta + hedge_shares
     daily_pnl = net_delta × daily_spot_move
     total_delta_pnl += daily_pnl
   ```

3. **Integrated position model**:
   - `HedgedPosition` aggregate tracks options + hedge as one position
   - Net Greeks computed at any point = option_greeks + hedge_contribution

---

## Architecture

### Domain Model

```
cs-domain/src/
├── hedging.rs           # Existing (HedgeState, HedgePosition, HedgeAction)
├── position/            # NEW module
│   ├── mod.rs
│   ├── hedged_position.rs       # Integrated position aggregate
│   ├── daily_snapshot.rs        # Daily position state
│   └── position_attribution.rs  # Daily P&L attribution
```

### Key Types

```rust
/// Position-level Greeks (already scaled by multiplier)
/// These represent real P&L exposure, not per-share values
pub struct PositionGreeks {
    pub delta: f64,    // e.g., +50 for long ATM call
    pub gamma: f64,    // e.g., +5 for long straddle
    pub theta: f64,    // e.g., -20 per day
    pub vega: f64,     // e.g., +30 per 1% IV
}

/// A snapshot of position state at a point in time
/// Greeks are recomputed daily from the IV surface (not carried forward from entry)
pub struct PositionSnapshot {
    pub timestamp: DateTime<Utc>,
    pub spot: f64,
    pub iv: f64,                         // IV at snapshot time (for vega attribution)
    pub option_greeks: PositionGreeks,   // Recomputed from current spot/IV/DTE
    pub hedge_shares: i32,               // Negative = short
    pub net_delta: f64,                  // option_delta + hedge_shares
}

/// Daily P&L attribution breakdown
/// All values computed from daily moves (not cumulative from entry)
pub struct DailyAttribution {
    pub date: NaiveDate,

    // Daily market data
    pub spot_open: f64,
    pub spot_close: f64,
    pub spot_change: f64,       // Daily spot move
    pub iv_open: f64,
    pub iv_close: f64,
    pub iv_change: f64,         // Daily IV move

    // Position state at start of day (Greeks recomputed daily)
    pub option_delta: f64,      // Position-level (×100)
    pub option_gamma: f64,      // Position-level (×100)
    pub hedge_shares: i32,
    pub net_delta: f64,         // option_delta + hedge_shares

    // P&L components (position-level, in dollars)
    pub gross_delta_pnl: f64,   // option_delta × daily_spot_change
    pub hedge_delta_pnl: f64,   // hedge_shares × daily_spot_change
    pub net_delta_pnl: f64,     // net_delta × daily_spot_change
    pub gamma_pnl: f64,         // 0.5 × gamma × daily_spot_change²
    pub theta_pnl: f64,         // theta (per day, recomputed daily)
    pub vega_pnl: f64,          // vega × daily_iv_change × 100
}

/// Aggregated attribution over entire holding period
pub struct PositionAttribution {
    pub daily: Vec<DailyAttribution>,

    // Totals (sum of daily)
    pub total_gross_delta_pnl: Decimal,
    pub total_hedge_delta_pnl: Decimal,
    pub total_net_delta_pnl: Decimal,
    pub total_gamma_pnl: Decimal,
    pub total_theta_pnl: Decimal,
    pub total_vega_pnl: Decimal,
    pub total_unexplained: Decimal,

    // Hedge effectiveness
    pub hedge_efficiency: f64,  // |hedge_delta_pnl| / |gross_delta_pnl|
}
```

---

## Implementation Plan

### Phase 1: Domain Types (cs-domain)

#### 1.1 Create `position` module

**File**: `cs-domain/src/position/mod.rs`
```rust
mod hedged_position;
mod daily_snapshot;
mod position_attribution;

pub use hedged_position::HedgedPosition;
pub use daily_snapshot::{PositionSnapshot, PositionGreeks};
pub use position_attribution::{DailyAttribution, PositionAttribution};
```

#### 1.2 Implement `PositionGreeks`

**File**: `cs-domain/src/position/daily_snapshot.rs`

Key responsibilities:
- Hold position-level Greeks (already multiplied by contract size)
- Conversion from per-share Greeks: `from_per_share(greeks: Greeks, multiplier: i32)`
- Addition/subtraction operators for combining legs

```rust
impl PositionGreeks {
    /// Convert from per-share Greeks to position-level
    pub fn from_per_share(greeks: &Greeks, multiplier: i32) -> Self {
        Self {
            delta: greeks.delta * multiplier as f64,
            gamma: greeks.gamma * multiplier as f64,
            theta: greeks.theta * multiplier as f64,
            vega: greeks.vega * multiplier as f64,
        }
    }

    /// Combine call + put for straddle
    pub fn straddle(call: &Greeks, put: &Greeks, multiplier: i32) -> Self {
        let combined = *call + *put;
        Self::from_per_share(&combined, multiplier)
    }
}
```

#### 1.3 Implement `PositionSnapshot`

**File**: `cs-domain/src/position/daily_snapshot.rs`

```rust
impl PositionSnapshot {
    /// Create snapshot with freshly computed Greeks
    /// Greeks should be recomputed from the IV surface at this timestamp
    pub fn new(
        timestamp: DateTime<Utc>,
        spot: f64,
        iv: f64,
        option_greeks: PositionGreeks,
        hedge_shares: i32,
    ) -> Self {
        let net_delta = option_greeks.delta + hedge_shares as f64;
        Self {
            timestamp,
            spot,
            iv,
            option_greeks,
            hedge_shares,
            net_delta,
        }
    }

    /// Intraday delta approximation using gamma (between full recomputations)
    /// Only used for hedge trigger checks, NOT for P&L attribution
    pub fn with_gamma_adjusted_delta(&self, new_spot: f64) -> Self {
        let spot_change = new_spot - self.spot;
        let new_option_delta = self.option_greeks.delta
            + self.option_greeks.gamma * spot_change;

        Self {
            option_greeks: PositionGreeks {
                delta: new_option_delta,
                ..self.option_greeks
            },
            net_delta: new_option_delta + self.hedge_shares as f64,
            spot: new_spot,
            ..self.clone()
        }
    }
}
```

#### 1.4 Implement `DailyAttribution`

**File**: `cs-domain/src/position/position_attribution.rs`

```rust
impl DailyAttribution {
    /// Compute daily P&L attribution from start-of-day and end-of-day snapshots
    ///
    /// IMPORTANT: Greeks in start_snapshot must be freshly computed for that day
    /// (not carried forward from entry). This ensures accurate attribution as
    /// delta/gamma/theta/vega evolve with spot, IV, and time-to-expiry.
    pub fn compute(
        start_snapshot: &PositionSnapshot,
        end_snapshot: &PositionSnapshot,
    ) -> Self {
        // Daily spot move (NOT total move from trade entry)
        let spot_change = end_snapshot.spot - start_snapshot.spot;

        // Delta P&L components (using start-of-day Greeks)
        let gross_delta_pnl = start_snapshot.option_greeks.delta * spot_change;
        let hedge_delta_pnl = start_snapshot.hedge_shares as f64 * spot_change;
        let net_delta_pnl = start_snapshot.net_delta * spot_change;

        // Gamma P&L: 0.5 × gamma × (daily_spot_change)²
        // This is the KEY fix: uses daily move squared, not total move squared
        let gamma_pnl = 0.5 * start_snapshot.option_greeks.gamma * spot_change.powi(2);

        // Theta P&L: already expressed per day
        let theta_pnl = start_snapshot.option_greeks.theta;

        // Vega P&L: vega × daily_iv_change × 100
        let iv_change = end_snapshot.iv - start_snapshot.iv;
        let vega_pnl = start_snapshot.option_greeks.vega * iv_change * 100.0;

        Self {
            date: start_snapshot.timestamp.date_naive(),
            spot_open: start_snapshot.spot,
            spot_close: end_snapshot.spot,
            spot_change,
            iv_open: start_snapshot.iv,
            iv_close: end_snapshot.iv,
            iv_change,
            option_delta: start_snapshot.option_greeks.delta,
            option_gamma: start_snapshot.option_greeks.gamma,
            hedge_shares: start_snapshot.hedge_shares,
            net_delta: start_snapshot.net_delta,
            gross_delta_pnl,
            hedge_delta_pnl,
            net_delta_pnl,
            gamma_pnl,
            theta_pnl,
            vega_pnl,
        }
    }
}
```

#### 1.5 Implement `PositionAttribution`

```rust
impl PositionAttribution {
    pub fn from_daily(daily: Vec<DailyAttribution>, actual_pnl: Decimal) -> Self {
        let total_gross_delta: f64 = daily.iter().map(|d| d.gross_delta_pnl).sum();
        let total_hedge_delta: f64 = daily.iter().map(|d| d.hedge_delta_pnl).sum();
        let total_net_delta: f64 = daily.iter().map(|d| d.net_delta_pnl).sum();
        let total_gamma: f64 = daily.iter().map(|d| d.gamma_pnl).sum();
        let total_theta: f64 = daily.iter().map(|d| d.theta_pnl).sum();
        let total_vega: f64 = daily.iter().map(|d| d.vega_pnl).sum();

        let explained = total_net_delta + total_gamma + total_theta + total_vega;
        let unexplained = actual_pnl.to_f64().unwrap_or(0.0) - explained;

        let hedge_efficiency = if total_gross_delta.abs() > 0.01 {
            (total_hedge_delta.abs() / total_gross_delta.abs()) * 100.0
        } else {
            0.0
        };

        Self {
            daily,
            total_gross_delta_pnl: Decimal::try_from(total_gross_delta).unwrap_or_default(),
            total_hedge_delta_pnl: Decimal::try_from(total_hedge_delta).unwrap_or_default(),
            total_net_delta_pnl: Decimal::try_from(total_net_delta).unwrap_or_default(),
            total_gamma_pnl: Decimal::try_from(total_gamma).unwrap_or_default(),
            total_theta_pnl: Decimal::try_from(total_theta).unwrap_or_default(),
            total_vega_pnl: Decimal::try_from(total_vega).unwrap_or_default(),
            total_unexplained: Decimal::try_from(unexplained).unwrap_or_default(),
            hedge_efficiency,
        }
    }
}
```

---

### Phase 2: Enhanced HedgeState (cs-domain)

#### 2.1 Add snapshot tracking to HedgeState

**File**: `cs-domain/src/hedging.rs`

Add field to `HedgeState`:
```rust
pub struct HedgeState {
    // ... existing fields ...

    /// Daily snapshots for attribution (new)
    snapshots: Vec<PositionSnapshot>,
}
```

#### 2.2 Capture snapshots on hedge events

Modify `HedgeState::update()` to capture snapshot:
```rust
pub fn update(&mut self, timestamp: DateTime<Utc>, new_spot: f64) -> Option<HedgeAction> {
    // ... existing delta update logic ...

    // Capture snapshot BEFORE hedge (for attribution)
    let snapshot = PositionSnapshot::new(
        timestamp,
        new_spot,
        PositionGreeks::from_per_share(&self.current_greeks(), self.config.contract_multiplier),
        self.stock_shares,
    );
    self.snapshots.push(snapshot);

    // ... rest of method ...
}
```

#### 2.3 Add daily snapshot generation

New method for EOD snapshot capture:
```rust
impl HedgeState {
    /// Capture end-of-day snapshot for attribution
    pub fn capture_daily_snapshot(&mut self, timestamp: DateTime<Utc>, spot: f64) {
        let snapshot = PositionSnapshot::new(
            timestamp,
            spot,
            PositionGreeks {
                delta: self.option_delta * self.config.contract_multiplier as f64,
                gamma: self.option_gamma * self.config.contract_multiplier as f64,
                theta: 0.0,  // Would need to track
                vega: 0.0,   // Would need to track
            },
            self.stock_shares,
        );
        self.snapshots.push(snapshot);
    }

    /// Get all snapshots for attribution
    pub fn snapshots(&self) -> &[PositionSnapshot] {
        &self.snapshots
    }
}
```

---

### Phase 3: Attribution Calculator (cs-analytics)

#### 3.1 Create integrated attribution module

**File**: `cs-analytics/src/position_attribution.rs`

```rust
use cs_domain::position::{PositionSnapshot, DailyAttribution, PositionAttribution};

/// Calculate daily P&L attribution from paired snapshots
///
/// Snapshots must be provided in pairs: (start_of_day, end_of_day) for each trading day.
/// Greeks in each snapshot must be freshly recomputed from the IV surface at that time.
///
/// # Arguments
/// * `daily_snapshots` - Vec of (start_snapshot, end_snapshot) pairs for each day
/// * `actual_pnl` - Actual realized P&L for unexplained calculation
pub fn calculate_daily_attribution(
    daily_snapshots: Vec<(PositionSnapshot, PositionSnapshot)>,
    actual_pnl: Decimal,
) -> PositionAttribution {
    let attributions: Vec<DailyAttribution> = daily_snapshots
        .iter()
        .map(|(start, end)| DailyAttribution::compute(start, end))
        .collect();

    PositionAttribution::from_daily(attributions, actual_pnl)
}
```

#### 3.2 Snapshot collection helper

```rust
/// Collect daily snapshot pairs for attribution
///
/// For each trading day between entry and exit:
/// 1. Get spot and IV at market open (or entry time for first day)
/// 2. Recompute option Greeks using Black-Scholes with current spot/IV/DTE
/// 3. Get hedge_shares from HedgeState
/// 4. Create PositionSnapshot
/// 5. Repeat at market close
pub async fn collect_daily_snapshots<O, E>(
    straddle: &Straddle,
    hedge_state: &HedgeState,
    options_repo: &O,
    equity_repo: &E,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
) -> Vec<(PositionSnapshot, PositionSnapshot)>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    let trading_days = TradingCalendar::trading_days_between(
        entry_time.date_naive(),
        exit_time.date_naive(),
    );

    let mut snapshots = Vec::new();

    for day in trading_days {
        let open_time = market_open_utc(day);
        let close_time = market_close_utc(day);

        // Get market data at open
        let (spot_open, iv_open) = get_spot_and_iv(straddle, options_repo, equity_repo, open_time).await;

        // Recompute Greeks at open
        let greeks_open = compute_greeks(straddle, spot_open, iv_open, day);
        let hedge_shares = hedge_state.shares_at(open_time);

        let start_snapshot = PositionSnapshot::new(
            open_time,
            spot_open,
            iv_open,
            PositionGreeks::straddle(&greeks_open.call, &greeks_open.put, CONTRACT_MULTIPLIER),
            hedge_shares,
        );

        // Get market data at close
        let (spot_close, iv_close) = get_spot_and_iv(straddle, options_repo, equity_repo, close_time).await;

        // Recompute Greeks at close (for next day's start, and to capture day's IV)
        let greeks_close = compute_greeks(straddle, spot_close, iv_close, day);

        let end_snapshot = PositionSnapshot::new(
            close_time,
            spot_close,
            iv_close,
            PositionGreeks::straddle(&greeks_close.call, &greeks_close.put, CONTRACT_MULTIPLIER),
            hedge_shares,  // Hedge shares same unless rehedge occurred during day
        );

        snapshots.push((start_snapshot, end_snapshot));
    }

    snapshots
}
```

---

### Phase 4: Update Executor (cs-backtest)

#### 4.1 Modify `UnifiedExecutor::apply_hedging()`

**File**: `cs-backtest/src/unified_executor.rs`

```rust
async fn apply_hedging(
    &self,
    result: &mut StraddleResult,
    straddle: &cs_domain::Straddle,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    rehedge_times: Vec<DateTime<Utc>>,
) {
    // ... existing hedge execution ...

    // NEW: Collect daily snapshots for attribution
    let trading_days = self.get_trading_days(entry_time, exit_time);
    for day in &trading_days {
        if let Ok(spot) = self.equity_repo.get_spot_at_close(straddle.symbol(), *day).await {
            hedge_state.capture_daily_snapshot(*day, spot);
        }
    }

    // Finalize and compute attribution
    let hedge_position = hedge_state.finalize(result.spot_at_exit);

    // NEW: Calculate integrated attribution
    let daily_spots = self.collect_daily_spots(straddle.symbol(), &trading_days).await;
    let daily_ivs = self.collect_daily_ivs(straddle, &trading_days).await;

    let attribution = calculate_daily_attribution(
        hedge_state.snapshots(),
        &daily_spots,
        &daily_ivs,
        result.pnl,
    );

    // Store attribution in result
    result.position_attribution = Some(attribution);
    result.hedge_position = Some(hedge_position);

    // ... rest of method ...
}
```

---

### Phase 5: Update Result Types (cs-domain)

#### 5.1 Add attribution to StraddleResult

**File**: `cs-domain/src/entities.rs`

```rust
pub struct StraddleResult {
    // ... existing fields ...

    // NEW: Integrated P&L Attribution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_attribution: Option<PositionAttribution>,
}
```

#### 5.2 Update RollPeriod

**File**: `cs-domain/src/entities/rolling_result.rs`

```rust
pub struct RollPeriod {
    // ... existing fields ...

    // NEW: Integrated attribution (replaces separate delta_pnl, etc.)
    pub attribution: Option<PositionAttribution>,
}
```

---

### Phase 6: Update CLI Display (cs-cli)

#### 6.1 New attribution table format

**File**: `cs-cli/src/main.rs`

```
P&L Attribution (Integrated Position):

+---+-------------+-------------+-------------+----------+------------+-----------+---------+
| # | Gross Delta | Hedge Delta | Net Delta   | Gamma    | Theta      | Vega      | Unexpl. |
+---+-------------+-------------+-------------+----------+------------+-----------+---------+
| 1 | $-130.48    | $+13.13     | $-117.35    | $26.10   | $-16.90    | $-10.51   | $-1.99  |
+---+-------------+-------------+-------------+----------+------------+-----------+---------+
| 2 | ...         |             |             |          |            |           |         |
+---+-------------+-------------+-------------+----------+------------+-----------+---------+

Hedge Effectiveness: 10.1% (of gross delta offset by hedges)
```

---

## Data Flow Summary

```
Entry Time
    │
    ▼
┌─────────────────────────────────────────────────────────────────┐
│  StraddleExecutor::execute_trade()                              │
│  - Compute entry Greeks (per-share)                             │
│  - Scale to position-level: PositionGreeks::straddle()          │
│  - Create initial PositionSnapshot                              │
└─────────────────────────────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────────────────────────────┐
│  UnifiedExecutor::apply_hedging()                               │
│  For each rehedge_time:                                         │
│    - Get spot price                                             │
│    - Update HedgeState (may execute hedge)                      │
│    - Capture PositionSnapshot                                   │
│                                                                 │
│  For each trading_day:                                          │
│    - Get EOD spot                                               │
│    - Capture daily PositionSnapshot                             │
└─────────────────────────────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────────────────────────────┐
│  calculate_daily_attribution()                                  │
│  For each day:                                                  │
│    - Find PositionSnapshot for day start                        │
│    - Compute: net_delta × spot_change                           │
│    - Compute: gamma, theta, vega contributions                  │
│    - Create DailyAttribution                                    │
│                                                                 │
│  Aggregate into PositionAttribution                             │
└─────────────────────────────────────────────────────────────────┘
    │
    ▼
Exit Time
```

---

## Testing Strategy

### Unit Tests

1. **PositionGreeks::from_per_share()**
   - Input: `Greeks { delta: 0.5, gamma: 0.03, ... }`, multiplier: 100
   - Expected: `PositionGreeks { delta: 50.0, gamma: 3.0, ... }`

2. **DailyAttribution::compute()**
   - Snapshot: delta=50, gamma=5, hedge_shares=-30, spot=100
   - End spot: 102 (+2)
   - Expected:
     - gross_delta_pnl = 50 × 2 = $100
     - hedge_delta_pnl = -30 × 2 = -$60
     - net_delta_pnl = 20 × 2 = $40
     - gamma_pnl = 0.5 × 5 × 4 = $10

3. **Hedge efficiency calculation**
   - gross_delta_pnl = -$130
   - hedge_delta_pnl = +$13
   - efficiency = 13/130 = 10%

### Integration Tests

1. **PENG Roll #1 Verification**
   - Entry: 2025-03-03, Exit: 2025-03-07
   - Verify daily snapshots captured
   - Verify net_delta_pnl ≈ gross_delta_pnl + hedge_delta_pnl

2. **Perfect hedge scenario**
   - Set hedge_shares = -option_delta at all times
   - Verify net_delta_pnl ≈ 0
   - Verify hedge_efficiency ≈ 100%

---

## Migration Notes

### Backward Compatibility

- Keep existing `delta_pnl`, `gamma_pnl`, etc. fields for non-hedged strategies
- Only populate `position_attribution` when hedging is enabled
- CLI can show old format for non-hedged, new format for hedged

### Data Requirements

For daily attribution to work, we need:
1. **Daily spot prices** - Already available via `EquityDataRepository`
2. **Daily IV** - May need to add IV snapshot collection
3. **Position snapshots** - New collection in `HedgeState`

---

## Design Decisions (Resolved)

### Greeks Recomputed Daily

All Greeks are **recomputed daily** using the IV surface at each day's close. This ensures accurate attribution as the position evolves:

1. **Delta**: Recomputed daily from Black-Scholes (or gamma approximation between recomputations)
2. **Gamma**: Recomputed daily - critical for accurate gamma P&L
3. **Theta**: Recomputed daily (theta accelerates as expiry approaches)
4. **Vega**: Recomputed daily with actual IV observations

### Daily Attribution Formulas

```
For each day d:
  spot_change_d = spot_close_d - spot_open_d
  iv_change_d   = iv_close_d - iv_open_d

  delta_pnl_d = net_delta_d × spot_change_d
  gamma_pnl_d = 0.5 × gamma_d × spot_change_d²    # Daily spot move squared
  theta_pnl_d = theta_d                            # Already per-day
  vega_pnl_d  = vega_d × iv_change_d × 100

Total = Σ(daily attributions)
```

**Key insight**: Gamma P&L uses **daily** spot move squared, not total move squared. This is critical because:
- `gamma × (total_move)²` overstates gamma P&L when moves are spread across days
- `Σ(gamma_d × daily_move_d²)` is the correct path-dependent calculation

---

## File Changes Summary

| File | Change |
|------|--------|
| `cs-domain/src/position/mod.rs` | NEW |
| `cs-domain/src/position/hedged_position.rs` | NEW |
| `cs-domain/src/position/daily_snapshot.rs` | NEW |
| `cs-domain/src/position/position_attribution.rs` | NEW |
| `cs-domain/src/hedging.rs` | Add snapshot tracking |
| `cs-domain/src/entities.rs` | Add `position_attribution` to `StraddleResult` |
| `cs-domain/src/entities/rolling_result.rs` | Add `attribution` to `RollPeriod` |
| `cs-domain/src/lib.rs` | Export new types |
| `cs-analytics/src/position_attribution.rs` | NEW |
| `cs-analytics/src/lib.rs` | Export new module |
| `cs-backtest/src/unified_executor.rs` | Collect snapshots, compute attribution |
| `cs-backtest/src/straddle_executor.rs` | Return position-level Greeks |
| `cs-cli/src/main.rs` | New attribution display format |
