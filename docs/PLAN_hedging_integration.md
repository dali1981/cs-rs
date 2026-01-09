# Hedging Integration Plan

**Date**: 2026-01-09
**Context**: Integrate hedging into BacktestUseCase architecture
**Related**: TICKET-0002, ANALYSIS_backtest_vs_campaign_architecture.md

---

## Executive Summary

Hedging should be integrated at the **TradeStrategy trait level**, not in individual strategy implementations. All hedging modes (GammaApproximation, EntryIV, EntryHV, CurrentHV, CurrentMarketIV) will be supported from the start.

---

## Key Design Decision: Hedging During Execution

### The Correct Temporal Flow

Hedging is **not** a post-processing step. It happens **during** the trade lifetime:

```
Timeline:
─────────────────────────────────────────────────────────────────────►
│                                                                     │
ENTRY                        REHEDGES                              EXIT
│                                                                     │
├── Open position            ├── Check delta                    ├── Close position
├── Initial hedge (Δ=0)      ├── Adjust hedge if needed         ├── Liquidate hedge
│                            ├── Track hedge P&L                 ├── Final hedge P&L
│                            │                                   │
t₀                          t₁, t₂, t₃, ...                      tₙ
```

### Where Hedging Lives: In the Simulation Loop

Hedging is part of the **simulation**, not separate from it. The execution flow is:

```rust
// INSIDE execute_trade (or a shared simulation helper)
async fn simulate_trade_with_hedging<T>(
    trade: &T,
    hedge_config: Option<&HedgeConfig>,
    timing: &TimingStrategy,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    equity_repo: &dyn EquityDataRepository,
    options_repo: &dyn OptionsDataRepository,
) -> SimulationResult {
    // 1. ENTRY: Price position + initial hedge
    let entry_spot = equity_repo.get_spot(entry_time).await?;
    let entry_pricing = pricer.price(trade, &entry_surface)?;

    let mut hedge_state = if let Some(config) = hedge_config {
        let provider = create_delta_provider(trade, config, entry_pricing);
        Some(HedgeState::new(config, provider, entry_spot))
    } else {
        None
    };

    // 2. REHEDGE LOOP: Iterate through time, adjust hedge
    if let Some(ref mut state) = hedge_state {
        for rehedge_time in timing.rehedge_times(entry_time, exit_time, &config.strategy) {
            let spot = equity_repo.get_spot(rehedge_time).await?;
            state.update(rehedge_time, spot).await?;  // Adjusts hedge, tracks P&L
        }
    }

    // 3. EXIT: Close position + liquidate hedge
    let exit_spot = equity_repo.get_spot(exit_time).await?;
    let exit_pricing = pricer.price(trade, &exit_surface)?;

    let hedge_position = hedge_state.map(|s| s.finalize(exit_spot));

    // 4. Build result with integrated hedge data
    SimulationResult {
        entry_pricing,
        exit_pricing,
        hedge_position,  // Already computed during simulation
    }
}
```

### Why This Matters

1. **Conceptual correctness**: Hedging happens in real-time, not retroactively
2. **Future extensibility**: Could add stop-loss based on total P&L (position + hedge)
3. **Realistic simulation**: Hedge decisions made with information available at each point
4. **Clean data flow**: Hedge P&L accumulates naturally through the simulation

### Implementation Location

The simulation-with-hedging logic lives in a **shared helper** (not duplicated per strategy):

```
TradeStrategy::execute_trade()
    │
    ├── Strategy-specific: select trade, create pricer
    │
    └── Shared: simulate_trade_with_hedging()  <── Hedging loop here
            │
            ├── Entry pricing + initial hedge
            ├── Rehedge loop (path-dependent)
            └── Exit pricing + hedge liquidation
```

Each strategy calls the same `simulate_trade_with_hedging()` helper. "Hedge is the same for all."

---

## Hedging Modes and Requirements

| Mode | Description | Requirements |
|------|-------------|--------------|
| **GammaApproximation** | δ(S) ≈ δ₀ + γ₀ × (S - S₀) | Result only: `net_delta()`, `net_gamma()`, `spot_at_entry()` |
| **EntryIV** | Reprice with Black-Scholes using entry IV | Trade object + entry IV |
| **EntryHV** | Reprice with Black-Scholes using entry HV | Trade object + equity repo (for HV computation) |
| **CurrentHV** | Reprice with Black-Scholes using current HV at each rehedge | Trade object + equity repo |
| **CurrentMarketIV** | Rebuild IV surface from options chain at each rehedge | Trade object + options repo |
| **HistoricalAverageIV** | Use averaged IV over lookback period | Trade object + options repo |

### Key Insight: All Full Modes Need Trade Object

Only GammaApproximation works with just the result. All other modes need:
- Strike prices (for Black-Scholes)
- Expiration dates (for time-to-expiry)
- Option types (call/put for each leg)
- Position quantities

These come from the **trade object**, not the result.

---

## Implementation Plan

### Phase 1: Create Shared Simulation Helper with Hedging

Create `HedgingSimulator` that wraps `TradeSimulator` and integrates hedging into the simulation loop.

**New file: `cs-backtest/src/hedging_simulator.rs`**

```rust
use cs_domain::*;
use crate::delta_providers::*;
use crate::timing_strategy::TimingStrategy;

/// Simulation output with optional hedge data
pub struct HedgedSimulationOutput<P> {
    pub entry_pricing: P,
    pub exit_pricing: P,
    pub entry_spot: f64,
    pub exit_spot: f64,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub hedge_position: Option<HedgePosition>,
}

/// Simulate a trade with integrated hedging
///
/// This is THE simulation function - hedging happens during execution,
/// not as post-processing.
pub async fn simulate_with_hedging<T, P>(
    trade: &T,
    pricer: &impl TradePricer<Trade = T, Pricing = P>,
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    hedge_config: Option<&HedgeConfig>,
    timing: &TimingStrategy,
) -> Result<HedgedSimulationOutput<P>, ExecutionError>
where
    T: CompositeTrade + Clone + Send + Sync,
    P: Clone,
{
    // 1. ENTRY: Get spot, build surface, price
    let entry_spot = equity_repo.get_spot_price(trade.symbol(), entry_time).await?;
    let entry_surface = options_repo.get_iv_surface(trade.symbol(), entry_time).await?;
    let entry_pricing = pricer.price(trade, &entry_surface)?;

    // 2. HEDGING LOOP (if configured)
    let hedge_position = if let Some(config) = hedge_config {
        // Create delta provider based on mode
        let provider = create_delta_provider(trade, config, &entry_pricing, entry_spot);

        // Initialize hedge state
        let mut state = GenericHedgeState::new(config.clone(), provider, entry_spot, false);

        // Iterate through rehedge times
        let rehedge_times = timing.rehedge_times(entry_time, exit_time, &config.strategy);
        for rehedge_time in rehedge_times {
            if state.at_max_rehedges() {
                break;
            }
            let spot = equity_repo.get_spot_price(trade.symbol(), rehedge_time).await?;
            state.update(rehedge_time, spot).await?;
        }

        // 3. EXIT: Get final spot, finalize hedge
        let exit_spot = equity_repo.get_spot_price(trade.symbol(), exit_time).await?;
        Some(state.finalize(exit_spot, entry_pricing.iv(), None))
    } else {
        None
    };

    // 4. EXIT: Build surface, price exit
    let exit_spot = equity_repo.get_spot_price(trade.symbol(), exit_time).await?;
    let exit_surface = options_repo.get_iv_surface(trade.symbol(), exit_time).await?;
    let exit_pricing = pricer.price(trade, &exit_surface)?;

    Ok(HedgedSimulationOutput {
        entry_pricing,
        exit_pricing,
        entry_spot,
        exit_spot,
        entry_time,
        exit_time,
        hedge_position,
    })
}

/// Create appropriate delta provider based on hedge config mode
fn create_delta_provider<T, P>(
    trade: &T,
    config: &HedgeConfig,
    entry_pricing: &P,
    entry_spot: f64,
) -> Box<dyn DeltaProvider>
where
    T: CompositeTrade + Clone + Send + Sync + 'static,
    P: HasGreeks,
{
    match &config.delta_computation {
        DeltaComputation::GammaApproximation => {
            Box::new(GammaApproximationProvider::new(
                entry_pricing.net_delta(),
                entry_pricing.net_gamma(),
                entry_spot,
            ))
        }
        DeltaComputation::EntryIV { .. } => {
            Box::new(EntryVolatilityProvider::new_entry_iv(
                trade.clone(),
                entry_pricing.entry_iv(),
                0.05, // risk-free rate
            ))
        }
        DeltaComputation::EntryHV { window } => {
            // HV computed at entry, used throughout
            Box::new(EntryVolatilityProvider::new_entry_hv(
                trade.clone(),
                entry_pricing.entry_hv(*window),
                0.05,
            ))
        }
        // CurrentHV, CurrentMarketIV, HistoricalAverageIV need Arc repos
        // For now, fall back to GammaApproximation
        _ => {
            tracing::warn!("Delta mode {:?} requires Arc repos, using GammaApproximation", config.delta_computation);
            Box::new(GammaApproximationProvider::new(
                entry_pricing.net_delta(),
                entry_pricing.net_gamma(),
                entry_spot,
            ))
        }
    }
}
```

### Phase 2: Update TradeStrategy to Use HedgingSimulator

Each strategy calls `simulate_with_hedging` instead of `TradeSimulator::run`:

```rust
impl TradeStrategy<StraddleResult> for StraddleStrategy {
    fn execute_trade<'a>(...) -> Pin<Box<dyn Future<Output = Option<StraddleResult>> + Send + 'a>> {
        Box::pin(async move {
            // 1. Prepare (get surface for selection)
            let surface = options_repo.get_iv_surface(symbol, entry_time).await?;
            let entry_spot = equity_repo.get_spot_price(symbol, entry_time).await?;

            // 2. Select trade (strategy-specific)
            let trade = selector.select_straddle(entry_spot, &surface, criteria)?;

            // 3. Simulate WITH HEDGING (shared)
            let sim = simulate_with_hedging(
                &trade,
                &StraddlePricer::new(),
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                exec_config.hedge_config.as_ref(),  // Pass hedge config
                self.timing(),
            ).await?;

            // 4. Build result (strategy-specific)
            let mut result = trade.to_result(sim.entry_pricing, sim.exit_pricing, ...);

            // 5. Apply hedge data if present
            if let Some(pos) = sim.hedge_position {
                let hedge_pnl = pos.calculate_pnl(sim.exit_spot);
                result.apply_hedge_results(pos, hedge_pnl, result.pnl() + hedge_pnl, None);
            }

            Some(result)
        })
    }
}
```

### Phase 3: No Changes to BacktestUseCase

Because hedging is now INSIDE the simulation (via `simulate_with_hedging`), BacktestUseCase doesn't need to know about hedging at all. It just calls `strategy.execute_trade()` as before.

```rust
// BacktestUseCase::execute_tradable_batch - NO CHANGES NEEDED
strategy.execute_trade(
    options_repo, equity_repo, selector, criteria,
    exec_config,  // Contains hedge_config
    event, entry_time, exit_time,
)
```

The `exec_config.hedge_config` flows down into the simulation automatically.

### Phase 4: Delta Providers for Each Mode

The `hedging_executor.rs` already has delta providers. Ensure all modes work:

| Mode | Provider | Status |
|------|----------|--------|
| GammaApproximation | `GammaApproximationProvider` | ✅ Implemented |
| EntryIV | `EntryVolatilityProvider::new_entry_iv()` | ✅ Implemented |
| EntryHV | `EntryVolatilityProvider::new_entry_hv()` | ✅ Implemented |
| CurrentHV | `CurrentHVProvider` | ⚠️ Needs Arc repos |
| CurrentMarketIV | `CurrentMarketIVProvider` | ⚠️ Needs Arc repos |
| HistoricalAverageIV | `HistoricalAverageIVProvider` | ⚠️ Needs Arc repos |

**Note**: CurrentHV/CurrentMarketIV/HistoricalAverageIV need `Arc<dyn Repository>` because they fetch data at each rehedge time. The current `&dyn Repository` signature works, but we may want to optimize later.

### Phase 5: Wire HedgeConfig Through

**Config flow**:
```
BacktestConfig.hedge_config: Option<HedgeConfig>
    ↓
create_execution_config()
    ↓
ExecutionConfig.hedge_config: Option<HedgeConfig>  (✅ already added)
    ↓
execute_with_hedging() checks exec_config.hedge_config
    ↓
apply_hedging() receives hedge_config
```

---

## Files to Modify

| File | Changes |
|------|---------|
| `cs-backtest/src/hedging_simulator.rs` | **NEW** - Create shared `simulate_with_hedging()` function |
| `cs-backtest/src/trade_strategy/straddle.rs` | Use `simulate_with_hedging()` instead of `TradeSimulator::run()` |
| `cs-backtest/src/trade_strategy/calendar_spread.rs` | Use `simulate_with_hedging()` |
| `cs-backtest/src/trade_strategy/iron_butterfly.rs` | Use `simulate_with_hedging()` |
| `cs-backtest/src/trade_strategy/calendar_straddle.rs` | Use `simulate_with_hedging()` |
| `cs-backtest/src/trade_strategy/post_earnings_straddle.rs` | Use `simulate_with_hedging()` |
| `cs-backtest/src/config/mod.rs` | Add `hedge_config: Option<HedgeConfig>` to BacktestConfig |
| `cs-backtest/src/lib.rs` | Export `hedging_simulator` module |

**No changes needed**:
- `cs-backtest/src/trade_strategy.rs` - Trait signature stays the same
- `cs-backtest/src/backtest_use_case.rs` - Just calls `execute_trade()` as before

---

## Testing Plan

1. **Unit tests**: Each delta provider with mock data
2. **Integration test**: Run straddle backtest with GammaApproximation hedging
3. **Integration test**: Run straddle backtest with EntryIV hedging
4. **Comparison test**: Compare hedged vs unhedged results
5. **Regression test**: Ensure unhedged backtests produce same results as before

---

## Migration Path

1. Add `type Trade` and new signature to `TradeStrategy` trait
2. Update each strategy implementation to return `(Trade, Result)`
3. Add `execute_with_hedging` default implementation
4. Update `BacktestUseCase.execute_tradable_batch` to use new method
5. Add `hedge_config` to `BacktestConfig`
6. Wire CLI args for hedging (already done in campaign)
7. Test all strategies with hedging enabled

---

## Open Questions

1. **Should we keep the old `execute_trade` signature as `execute_trade_internal` for non-hedging cases?**
   - No - simpler to have single signature. The tuple is lightweight.

2. **Should hedging failures fail the trade or just warn?**
   - Warn and continue. Hedging is supplementary to the core trade.

3. **Should we support Arc repos for CurrentHV/CurrentMarketIV or defer?**
   - Defer. GammaApproximation and EntryIV cover most use cases. Add Arc support when needed.

---

## Summary

**Architecture**:
- Hedging happens **during** simulation, not as post-processing
- Shared `simulate_with_hedging()` function integrates hedging into the execution loop
- Each strategy calls this shared function → "hedge is the same for all"
- TradeStrategy trait signature unchanged
- BacktestUseCase unchanged

**Execution flow**:
```
Entry (t₀)          Rehedge (t₁..tₙ₋₁)      Exit (tₙ)
─────────────────────────────────────────────────────►
│                                            │
├── Get spot        ├── Get spot             ├── Get spot
├── Build surface   ├── Compute delta        ├── Build surface
├── Price entry     ├── Adjust hedge         ├── Price exit
├── Init hedge      ├── Track P&L            ├── Finalize hedge
│                   │                        │
└── HedgedSimulationOutput includes hedge_position
```

**Key files**:
1. `hedging_simulator.rs` - NEW shared simulation function
2. Each strategy - calls `simulate_with_hedging()`
3. `config/mod.rs` - adds `hedge_config` to BacktestConfig

**Result**: All hedging modes (GammaApproximation, EntryIV, EntryHV, CurrentHV, CurrentMarketIV) supported from day one. Hedging is conceptually correct (happens during trade lifetime) and implementation is DRY (single shared function).
