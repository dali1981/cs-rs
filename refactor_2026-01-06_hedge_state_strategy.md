# Hedge State Refactoring: Strategy Pattern for Delta Computation

**Date**: 2026-01-06
**Status**: Implementation Plan
**Goal**: Fix 100x delta bug and eliminate ~800-1000 lines of duplication

---

## Problem Summary

### 1. Root Cause: Delta Unit Mismatch

All 5 new delta modes multiply by `contract_multiplier` (100) when they shouldn't:

```rust
// BUG: compute_position_delta() in trade_executor.rs
fn compute_position_delta(..., contract_multiplier: i32) -> f64 {
    trade.legs().iter().map(|(leg, position)| {
        let leg_delta = bs_delta(...);
        leg_delta * position.sign() * contract_multiplier as f64  // ❌ WRONG
    }).sum()
}
```

**Convention**: Delta should be **per-share** (e.g., 0.5 for ATM call)

**Evidence from `HedgeState`** (`cs-domain/src/hedging.rs:202`):
```rust
initial_delta: f64,    // Option delta at entry (per-share)  <-- COMMENT CONFIRMS
```

**Evidence from `shares_to_hedge()`** (`cs-domain/src/hedging.rs:155-157`):
```rust
pub fn shares_to_hedge(&self, position_delta: f64) -> i32 {
    // Multiplies by 100 internally, so input MUST be per-share
    let raw_shares = (-position_delta * self.contract_multiplier as f64).round() as i32;
    ...
}
```

### 2. Massive Code Duplication

Each of the 6 modes duplicates the entire hedging loop:

| Component | Lines | Duplicated 6× |
|-----------|-------|---------------|
| Hedging loop structure | ~80-120 | ✓ |
| Stock delta calculation | 3 | ✓ |
| Net delta computation | 2 | ✓ |
| Rehedge decision | 3 | ✓ |
| Shares calculation | 3 | ✓ |
| HedgeAction creation | 7 | ✓ |
| RV tracking | ~15 | ✓ |
| Snapshot collection | ~20 | 3× |
| Exit handling | ~15 | ✓ |

**Total duplication**: ~800-1000 lines across 6 implementations

---

## Solution: Strategy Pattern for Delta Computation

### Core Insight

`HedgeState` already encapsulates hedging logic correctly. The problem is that new modes **bypass** `HedgeState` entirely instead of extending it.

**Current Architecture** (Wrong):
```
TradeExecutor::apply_hedging()
├── if GammaApproximation → use HedgeState (correct)
├── if EntryHV → duplicate 100+ lines (wrong)
├── if EntryIV → duplicate 100+ lines (wrong)
├── if CurrentHV → duplicate 100+ lines (wrong)
├── if CurrentMarketIV → duplicate 100+ lines (wrong)
└── if HistoricalAverageIV → duplicate 100+ lines (wrong)
```

**Target Architecture** (Correct):
```
TradeExecutor::apply_hedging()
└── HedgeState::new_with_delta_provider(config, provider)
    └── All modes use the same hedging loop
        └── DeltaProvider::compute_delta() called at each rehedge
```

---

## Detailed Design

### Phase 1: DeltaProvider Trait (`cs-domain/src/hedging.rs`)

```rust
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Strategy for computing position delta at a given point in time
///
/// Implementations provide different methods:
/// - GammaApproximation: δ' = δ + γ × ΔS (fast, incremental)
/// - EntryVolatility: Recompute from Black-Scholes with fixed volatility
/// - CurrentMarketIV: Build IV surface and compute fresh delta
#[async_trait]
pub trait DeltaProvider: Send + Sync {
    /// Compute the current per-share position delta
    ///
    /// # Arguments
    /// * `spot` - Current spot price
    /// * `timestamp` - Current time (for DTE calculation)
    ///
    /// # Returns
    /// Per-share position delta (e.g., 0.5 for ATM call, NOT 50)
    async fn compute_delta(&mut self, spot: f64, timestamp: DateTime<Utc>) -> Result<f64, String>;

    /// Optional: Compute position gamma (for reporting)
    fn compute_gamma(&self, spot: f64, timestamp: DateTime<Utc>) -> Option<f64> {
        None
    }

    /// Human-readable name for logging
    fn name(&self) -> &'static str;
}
```

### Phase 2: Provider Implementations (`cs-backtest/src/delta_providers/`)

**Module Structure**:
```
cs-backtest/src/delta_providers/
├── mod.rs                     # Exports
├── gamma_approximation.rs     # Current behavior (stateful)
├── entry_volatility.rs        # EntryHV and EntryIV (use fixed vol)
├── current_hv.rs              # Recompute HV at each step
├── current_market_iv.rs       # Build IV surface at each step
└── historical_average_iv.rs   # Average IV over lookback
```

#### 2.1 GammaApproximationProvider

```rust
/// Incremental delta using gamma approximation (current behavior)
pub struct GammaApproximationProvider {
    option_delta: f64,      // Per-share delta
    option_gamma: f64,      // Per-share gamma
    last_spot: f64,
}

impl GammaApproximationProvider {
    pub fn new(initial_delta: f64, initial_gamma: f64, initial_spot: f64) -> Self {
        Self {
            option_delta: initial_delta,
            option_gamma: initial_gamma,
            last_spot: initial_spot,
        }
    }
}

#[async_trait]
impl DeltaProvider for GammaApproximationProvider {
    async fn compute_delta(&mut self, spot: f64, _timestamp: DateTime<Utc>) -> Result<f64, String> {
        // Incremental update: δ' = δ + γ × (S' - S)
        let spot_change = spot - self.last_spot;
        self.option_delta += self.option_gamma * spot_change;
        self.last_spot = spot;

        Ok(self.option_delta)  // Per-share, NO multiplier
    }

    fn compute_gamma(&self, _spot: f64, _timestamp: DateTime<Utc>) -> Option<f64> {
        Some(self.option_gamma)
    }

    fn name(&self) -> &'static str {
        "gamma_approximation"
    }
}
```

#### 2.2 EntryVolatilityProvider (Shared by EntryHV and EntryIV)

```rust
use cs_domain::trade::CompositeTrade;
use cs_analytics::bs_delta;
use finq_core::OptionType;

/// Recompute delta from Black-Scholes using fixed volatility
///
/// Used for both EntryHV and EntryIV modes - they differ only
/// in where the volatility value comes from.
pub struct EntryVolatilityProvider<T: CompositeTrade> {
    trade: T,
    entry_volatility: f64,      // Fixed vol (HV or IV at entry)
    risk_free_rate: f64,
    vol_source_name: &'static str,  // "entry_hv" or "entry_iv"
}

impl<T: CompositeTrade> EntryVolatilityProvider<T> {
    pub fn new_entry_hv(trade: T, entry_hv: f64, risk_free_rate: f64) -> Self {
        Self {
            trade,
            entry_volatility: entry_hv,
            risk_free_rate,
            vol_source_name: "entry_hv",
        }
    }

    pub fn new_entry_iv(trade: T, entry_iv: f64, risk_free_rate: f64) -> Self {
        Self {
            trade,
            entry_volatility: entry_iv,
            risk_free_rate,
            vol_source_name: "entry_iv",
        }
    }
}

#[async_trait]
impl<T: CompositeTrade + Send + Sync> DeltaProvider for EntryVolatilityProvider<T> {
    async fn compute_delta(&mut self, spot: f64, timestamp: DateTime<Utc>) -> Result<f64, String> {
        // Sum delta across all legs (per-share, NO multiplier)
        let position_delta: f64 = self.trade.legs().iter().map(|(leg, position)| {
            let tte = (leg.expiration - timestamp.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 {
                return 0.0;  // Expired
            }

            let is_call = leg.option_type == OptionType::Call;
            let strike = leg.strike.value().to_f64().unwrap_or(0.0);

            // Per-share delta from Black-Scholes
            let leg_delta = bs_delta(
                spot,
                strike,
                tte,
                self.entry_volatility,
                is_call,
                self.risk_free_rate,
            );

            // Apply position sign (long = +1, short = -1)
            // NO multiplier here - we return per-share delta
            leg_delta * position.sign()
        }).sum();

        Ok(position_delta)
    }

    fn name(&self) -> &'static str {
        self.vol_source_name
    }
}
```

#### 2.3 CurrentHVProvider

```rust
use std::sync::Arc;
use cs_domain::EquityDataRepository;
use cs_analytics::realized_volatility;

/// Recompute HV at each rehedge from recent underlying prices
pub struct CurrentHVProvider<T: CompositeTrade> {
    trade: T,
    equity_repo: Arc<dyn EquityDataRepository>,
    symbol: String,
    window: u32,
    risk_free_rate: f64,
}

#[async_trait]
impl<T: CompositeTrade + Send + Sync> DeltaProvider for CurrentHVProvider<T> {
    async fn compute_delta(&mut self, spot: f64, timestamp: DateTime<Utc>) -> Result<f64, String> {
        // 1. Compute current HV from recent price history
        let end_date = timestamp.date_naive();
        let start_date = end_date - chrono::Duration::days(self.window as i64 + 10);

        let bars = self.equity_repo
            .get_bars(&self.symbol, start_date, end_date)
            .await
            .map_err(|e| e.to_string())?;

        let closes: Vec<f64> = bars.column("close")
            .map_err(|_| "No close column")?
            .f64()
            .map_err(|_| "Invalid type")?
            .into_no_null_iter()
            .collect();

        let current_hv = realized_volatility(&closes, self.window as usize, 252.0)
            .ok_or("Insufficient data for HV")?;

        // 2. Compute delta using current HV (per-share, NO multiplier)
        let position_delta: f64 = self.trade.legs().iter().map(|(leg, position)| {
            let tte = (leg.expiration - timestamp.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 { return 0.0; }

            let is_call = leg.option_type == OptionType::Call;
            let strike = leg.strike.value().to_f64().unwrap_or(0.0);

            bs_delta(spot, strike, tte, current_hv, is_call, self.risk_free_rate)
                * position.sign()  // NO multiplier
        }).sum();

        Ok(position_delta)
    }

    fn name(&self) -> &'static str {
        "current_hv"
    }
}
```

### Phase 3: Refactored HedgeState (`cs-domain/src/hedging.rs`)

```rust
/// Stateful delta hedge manager with pluggable delta computation
///
/// # Key Changes from Original
/// - Delta computation is delegated to DeltaProvider
/// - All hedging logic remains centralized (no duplication)
/// - Same interface for all 6 modes
pub struct HedgeState<P: DeltaProvider> {
    config: HedgeConfig,
    delta_provider: P,

    // Stock hedge position
    stock_shares: i32,

    // Last known values
    last_delta: f64,
    last_gamma: Option<f64>,

    // Transaction history
    position: HedgePosition,

    // RV tracking (optional)
    spot_history: Vec<(DateTime<Utc>, f64)>,
    track_rv: bool,
}

impl<P: DeltaProvider> HedgeState<P> {
    pub fn new(config: HedgeConfig, delta_provider: P, initial_spot: f64) -> Self {
        let track_rv = config.track_realized_vol;
        Self {
            config,
            delta_provider,
            stock_shares: 0,
            last_delta: 0.0,
            last_gamma: None,
            position: HedgePosition::new(),
            spot_history: if track_rv { vec![(Utc::now(), initial_spot)] } else { vec![] },
            track_rv,
        }
    }

    /// Net position delta (options + stock) - ALWAYS per-share
    pub fn net_delta(&self) -> f64 {
        // stock_shares is actual shares, convert to per-share delta
        let stock_delta = self.stock_shares as f64 / self.config.contract_multiplier as f64;
        self.last_delta + stock_delta
    }

    /// Process a new spot price observation
    ///
    /// Returns Some(HedgeAction) if a rebalance was executed.
    pub async fn update(&mut self, timestamp: DateTime<Utc>, spot: f64) -> Result<Option<HedgeAction>, String> {
        // Track spot for RV computation
        if self.track_rv {
            self.spot_history.push((timestamp, spot));
        }

        // 1. Get fresh delta from provider (per-share)
        let option_delta = self.delta_provider.compute_delta(spot, timestamp).await?;
        self.last_delta = option_delta;
        self.last_gamma = self.delta_provider.compute_gamma(spot, timestamp);

        // 2. Compute net delta (options + stock hedge)
        let net_delta = self.net_delta();

        // 3. Check if rehedge needed
        let gamma = self.last_gamma.unwrap_or(0.0);
        if !self.config.should_rehedge(net_delta, spot, gamma) {
            return Ok(None);
        }

        // 4. Calculate shares to trade (multiplier applied INSIDE shares_to_hedge)
        let shares = self.config.shares_to_hedge(net_delta);
        if shares == 0 {
            return Ok(None);
        }

        // 5. Execute hedge
        let delta_before = net_delta;
        self.stock_shares += shares;
        let delta_after = self.net_delta();

        let action = HedgeAction {
            timestamp,
            shares,
            spot_price: spot,
            delta_before,
            delta_after,
            cost: self.config.transaction_cost_per_share * Decimal::from(shares.abs()),
        };

        self.position.add_hedge(action.clone());

        Ok(Some(action))
    }

    /// Finalize and compute P&L
    pub fn finalize(mut self, exit_spot: f64, entry_iv: Option<f64>, exit_iv: Option<f64>) -> HedgePosition {
        self.position.unrealized_pnl = self.position.calculate_pnl(exit_spot);

        // Compute RV metrics if tracking was enabled
        if self.track_rv && !self.spot_history.is_empty() {
            self.position.realized_vol_metrics = Some(
                RealizedVolatilityMetrics::from_spot_history(
                    &self.spot_history,
                    None,  // entry_hv - could be passed if available
                    entry_iv,
                    exit_iv,
                )
            );
            self.position.spot_history = self.spot_history;
        }

        self.position
    }
}
```

### Phase 4: Simplified TradeExecutor (`cs-backtest/src/trade_executor.rs`)

```rust
impl<T> TradeExecutor<T>
where
    T: RollableTrade + ExecutableTrade + CompositeTrade + Clone,
{
    /// Apply hedging - NOW UNIFIED FOR ALL MODES
    async fn apply_hedging(
        &self,
        trade: &T,
        result: &mut <T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
    ) -> Result<(), String> {
        let hedge_config = self.hedge_config.as_ref()
            .ok_or("Hedge config not set")?;

        let symbol = result.symbol();
        let entry_spot = result.spot_at_entry();

        // Create delta provider based on mode
        let delta_provider: Box<dyn DeltaProvider> = match &hedge_config.delta_computation {
            DeltaComputation::GammaApproximation => {
                let delta = result.net_delta().unwrap_or(0.0);
                let gamma = result.net_gamma().unwrap_or(0.0);
                Box::new(GammaApproximationProvider::new(delta, gamma, entry_spot))
            }
            DeltaComputation::EntryHV { window } => {
                let entry_hv = self.compute_hv(symbol, entry_time, *window).await?;
                Box::new(EntryVolatilityProvider::new_entry_hv(trade.clone(), entry_hv, 0.05))
            }
            DeltaComputation::EntryIV { .. } => {
                let entry_iv = result.entry_iv()
                    .map(|iv| iv.primary)
                    .ok_or("No entry IV available")?;
                Box::new(EntryVolatilityProvider::new_entry_iv(trade.clone(), entry_iv, 0.05))
            }
            DeltaComputation::CurrentHV { window } => {
                Box::new(CurrentHVProvider::new(
                    trade.clone(),
                    self.equity_repo.clone(),
                    symbol.to_string(),
                    *window,
                    0.05,
                ))
            }
            DeltaComputation::CurrentMarketIV { .. } => {
                Box::new(CurrentMarketIVProvider::new(
                    trade.clone(),
                    self.options_repo.clone(),
                    self.equity_repo.clone(),
                    symbol.to_string(),
                    0.05,
                ))
            }
            DeltaComputation::HistoricalAverageIV { lookback_days, .. } => {
                Box::new(HistoricalAverageIVProvider::new(
                    trade.clone(),
                    self.options_repo.clone(),
                    self.equity_repo.clone(),
                    symbol.to_string(),
                    *lookback_days,
                    0.05,
                ))
            }
        };

        // Create hedge state with the provider
        let mut hedge_state = HedgeState::new(
            hedge_config.clone(),
            delta_provider,
            entry_spot,
        );

        // === UNIFIED HEDGING LOOP (no more duplication!) ===
        for rehedge_time in rehedge_times {
            if hedge_state.at_max_rehedges() {
                break;
            }

            let spot = self.equity_repo
                .get_spot_price(symbol, rehedge_time)
                .await
                .map_err(|e| e.to_string())?
                .to_f64();

            hedge_state.update(rehedge_time, spot).await?;
        }

        // Finalize
        let exit_spot = result.spot_at_exit();
        let entry_iv = result.entry_iv().map(|iv| iv.primary);
        let exit_iv = result.exit_iv().map(|iv| iv.primary);
        let hedge_position = hedge_state.finalize(exit_spot, entry_iv, exit_iv);

        // Apply results
        if hedge_position.rehedge_count() > 0 {
            let hedge_pnl = hedge_position.calculate_pnl(exit_spot);
            let total_pnl = result.pnl() + hedge_pnl - hedge_position.total_cost;
            result.apply_hedge_results(hedge_position, hedge_pnl, total_pnl, None);
        }

        Ok(())
    }
}
```

---

## Summary of Changes

### Files Modified

| File | Change |
|------|--------|
| `cs-domain/src/hedging.rs` | Add `DeltaProvider` trait, refactor `HedgeState<P>` |
| `cs-backtest/src/delta_providers/mod.rs` | New module |
| `cs-backtest/src/delta_providers/gamma_approximation.rs` | New |
| `cs-backtest/src/delta_providers/entry_volatility.rs` | New (shared by EntryHV/EntryIV) |
| `cs-backtest/src/delta_providers/current_hv.rs` | New |
| `cs-backtest/src/delta_providers/current_market_iv.rs` | New |
| `cs-backtest/src/delta_providers/historical_average_iv.rs` | New |
| `cs-backtest/src/trade_executor.rs` | Delete 800+ lines, replace with ~50 line unified loop |
| `cs-backtest/src/lib.rs` | Export delta_providers module |

### Lines of Code

| Before | After | Reduction |
|--------|-------|-----------|
| ~1200 lines (6 modes × ~200 each) | ~350 lines (trait + 6 providers + unified loop) | **~70% reduction** |

### Bugs Fixed

| Bug | Fix |
|-----|-----|
| 100× delta | Providers return per-share delta, no multiplier |
| Magic number 100 | Use `CONTRACT_MULTIPLIER` constant everywhere |
| Duplication | Single hedging loop via strategy pattern |

---

## Implementation Plan

### Phase 1: Create DeltaProvider Trait (cs-domain)
**Files**: `cs-domain/src/hedging.rs`
1. Add `DeltaProvider` trait
2. Keep existing `HedgeState` temporarily (for backward compat)
3. Add `HedgeState<P>` with provider support
4. **Test**: Compile check

### Phase 2: Implement GammaApproximationProvider (cs-backtest)
**Files**: `cs-backtest/src/delta_providers/`
1. Create module structure
2. Implement `GammaApproximationProvider`
3. **Test**: Same results as current GammaApproximation mode

### Phase 3: Implement Entry Volatility Providers
**Files**: `cs-backtest/src/delta_providers/entry_volatility.rs`
1. Implement `EntryVolatilityProvider<T>` (shared by EntryHV and EntryIV)
2. Fix: NO multiplier in delta computation
3. **Test**: Verify delta values are per-share (0.5, not 50)

### Phase 4: Implement Current Volatility Providers
**Files**: `cs-backtest/src/delta_providers/`
1. Implement `CurrentHVProvider`
2. Implement `CurrentMarketIVProvider`
3. Implement `HistoricalAverageIVProvider`
4. **Test**: Each provider returns correct per-share delta

### Phase 5: Refactor TradeExecutor
**Files**: `cs-backtest/src/trade_executor.rs`
1. Create unified `apply_hedging()` with provider dispatch
2. Delete 6 individual mode methods (~800 lines)
3. **Test**: All modes produce same results as before (except 100× bug is fixed)

### Phase 6: Remove Legacy Code
**Files**: `cs-domain/src/hedging.rs`
1. Remove non-generic `HedgeState` (if no longer used)
2. Clean up any dead code
3. **Test**: Full test suite passes

### Phase 7: Add Tests
**Files**: `cs-backtest/tests/`
1. Unit tests for each `DeltaProvider`
2. Integration test: verify per-share delta convention
3. Regression test: ensure hedge P&L is reasonable

---

## Verification Criteria

### 1. Delta Convention Check
```rust
// After fix, all these should return per-share delta (~0.5 for ATM)
let delta = provider.compute_delta(100.0, timestamp).await?;
assert!(delta.abs() < 2.0, "Delta should be per-share, not multiplied by 100");
```

### 2. No Magic Numbers
```bash
# Should find 0 occurrences of hardcoded 100 for multiplier
grep -r "\\* 100" cs-backtest/src/delta_providers/
# Should find uses of CONTRACT_MULTIPLIER instead
grep -r "contract_multiplier" cs-domain/src/hedging.rs
```

### 3. Line Count Reduction
```bash
# Before: trade_executor.rs should have ~1200 lines
# After: trade_executor.rs should have ~400 lines
wc -l cs-backtest/src/trade_executor.rs
```

### 4. All Modes Work Identically
```rust
// Run same backtest with all 6 modes
// GammaApproximation should give same result as before
// Other modes should give reasonable (not 100× off) results
```

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Breaking existing gamma mode | Keep old HedgeState until new one verified |
| Async trait complexity | Use `async_trait` crate (already in use) |
| Generic type bounds | Require `T: CompositeTrade + Clone + Send + Sync` |
| Performance regression | Gamma mode stays incremental (no BS calls) |

---

## References

- `cs-domain/src/hedging.rs` - Current HedgeState implementation
- `cs-analytics/src/black_scholes.rs` - bs_delta function
- `cs-domain/src/trade/composite.rs` - CompositeTrade trait
- Strategy Pattern: Gang of Four, Design Patterns
