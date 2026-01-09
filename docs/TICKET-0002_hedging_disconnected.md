# TICKET-0002: Hedging Infrastructure Disconnected from Trade Execution

**Date**: 2026-01-09
**Status**: 🔴 Open (Blocking Feature)
**Priority**: High
**Component**: Delta Hedging, Trade Execution
**Related**: Phases 1-3 (Trade-centric execution refactoring)

---

## Problem

Delta hedging functionality exists but is **not being called** during trade execution. Users can enable hedging via CLI/config, but it has no effect:

```bash
target/debug/cs backtest -c straddle_hedged.toml --hedge --delta-threshold 0.10
# Result: No hedging applied, hedge_pnl = None, hedge_position = None
```

**User Evidence**: Ran command with `hedge=true`, but output shows all hedging fields as `None`.

---

## Root Cause

The refactoring to trade-centric execution (Phases 1-3) disconnected the hedging layer from trade execution. The infrastructure exists but orchestration is missing.

### What Exists ✅

**Delta Provider Strategies** (implemented, tested, working):
- `cs-backtest/src/delta_providers/mod.rs`
- `GammaApproximationProvider` - Fast gamma approximation
- `EntryVolatilityProvider` - Fixed vol at entry (HV or IV)
- `CurrentHVProvider` - Current historical vol
- `CurrentMarketIVProvider` - Rebuild IV surface at each rehedge
- `HistoricalAverageIVProvider` - Average IV over lookback
- All 7 files present and accessible

**Result Types Support Hedging**:
- `StraddleResult` has `hedge_position`, `hedge_pnl`, `total_pnl_with_hedge` fields
- `HedgeConfig` exists in domain (enabled, strategy, interval, threshold, etc.)
- `HedgePosition`, `HedgeAction` domain types exist

**Analytics Code**:
- `cs-backtest/src/hedging_analytics.rs` - Compares hedged vs unhedged
- Methods for calculating hedge efficiency, rehedge counts

### What's Missing ❌

**No Orchestration**:
- `TradeSimulator::run()` only prices entry/exit
- `StraddleStrategy::execute_trade()` **never calls hedging logic after simulation**
- No invocation of delta providers or rehedge scheduling

**Execution Flow Gap**:
```rust
// Current (no hedging)
let result = simulator.run(&trade, &pricer).await?;
trade.to_result(raw.entry_pricing, raw.exit_pricing, &raw.output, Some(event))
                                                      ↑
                                     Hardcoded None for hedge fields

// What should happen
let result = simulator.run(&trade, &pricer).await?;
let hedge_result = apply_hedging(
    &result,
    event,
    delta_provider,
    hedge_config,
    rehedge_schedule
)?;
trade.to_result_with_hedge(...)
```

**Missing Connection Points**:
1. `BacktestUseCase::execute_tradable_batch()` doesn't pass `HedgeConfig` to strategy
2. `TradeStrategy::execute_trade()` signature doesn't include hedging parameters
3. `StraddleStrategy` doesn't instantiate or use delta providers
4. No rehedge scheduling logic
5. Result construction never populates hedge fields

---

## Git History

Hedging was working in earlier commits:
- `f4a5c07` (Jan 6): "Refactor hedging with DeltaProvider strategy pattern" - organized providers
- `5320b85` (Jan ?): "Implement P&L attribution for hedged positions"
- `855d867` (Jan ?): "Implement trade-agnostic hedging and complete ExecutableTrade framework"

But was **not migrated** to the new trade-centric execution model during Phases 1-3.

---

## Solution Approach

### Phase 1: Update TradeStrategy Trait
Pass hedging context to `execute_trade()`:

```rust
pub trait TradeStrategy<R: TradeResultMethods + Send>: Send + Sync {
    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        exec_config: &'a ExecutionConfig,
        hedge_config: &'a Option<HedgeConfig>,  // ← NEW
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<R>> + Send + 'a>>;
}
```

### Phase 2: Implement Hedging in StraddleStrategy

After `simulator.run()` completes, apply hedging:

```rust
impl TradeStrategy<StraddleResult> for StraddleStrategy {
    fn execute_trade<'a>(
        ...
        hedge_config: &'a Option<HedgeConfig>,
        ...
    ) -> ... {
        Box::pin(async move {
            // ... existing code: simulator.run() ...
            let mut result = trade.to_result(...);

            // NEW: Apply hedging if configured
            if let Some(hedge_cfg) = hedge_config {
                let delta_provider = create_delta_provider(&hedge_cfg.strategy);
                result = apply_hedging(
                    result,
                    &data,
                    delta_provider,
                    hedge_cfg,
                    event,
                ).await?;
            }

            Some(result)
        })
    }
}
```

### Phase 3: Create Hedging Application Logic

New module: `cs-backtest/src/hedging_executor.rs`

```rust
pub async fn apply_hedging<T: ExecutableTrade>(
    mut result: T::Result,
    market_data: &PreparedData,
    delta_provider: Box<dyn DeltaProvider>,
    config: &HedgeConfig,
    event: &EarningsEvent,
) -> Result<T::Result> {
    // 1. Generate rehedge schedule (time-based or delta-based)
    let rehedges = generate_rehedge_schedule(
        result.entry_time,
        result.exit_time,
        config.strategy,
        config.interval_hours,
        config.delta_threshold,
    );

    // 2. For each rehedge opportunity
    for rehedge in rehedges {
        let spot = market_data.spot_at(rehedge.time)?;
        let current_delta = delta_provider.update(spot, rehedge.time)?;

        // 3. Check if rehedge triggered
        if should_rehedge(current_delta, result.entry_delta, config.delta_threshold) {
            let hedge_action = compute_hedge_action(current_delta);
            let hedge_cost = hedge_action.cost(config.cost_per_share);

            result.hedge_position.push(hedge_action);
            result.hedge_pnl += compute_hedge_pnl(...)
        }
    }

    result.total_pnl_with_hedge = result.pnl + result.hedge_pnl;
    Ok(result)
}
```

### Phase 4: Update Call Sites

Update `BacktestUseCase::execute_tradable_batch()`:

```rust
async fn execute_tradable_batch<S, R>(
    &self,
    tradable_events: &[&TradableEvent],
    strategy: &S,
    selector: &dyn StrikeSelector,
    criteria: &ExpirationCriteria,
    exec_config: &ExecutionConfig,
    hedge_config: &Option<HedgeConfig>,  // ← NEW
) -> Vec<Option<R>> {
    // ... existing code ...
    crate::execution::run_batch(&events, self.config.parallel, |event| {
        strategy.execute_trade(
            self.options_repo.as_ref(),
            self.equity_repo.as_ref(),
            selector,
            criteria,
            exec_config,
            hedge_config,  // ← PASS HERE
            event,
            tradable.entry_datetime(),
            tradable.exit_datetime(),
        )
    }).await
}
```

And extract hedge_config in backtest():

```rust
pub async fn backtest_straddle(&self) -> Result<BacktestResult<StraddleResult>> {
    // ... existing code ...

    let hedge_config = self.config.hedge_config.clone();  // ← Extract config

    let batch_results = self.execute_tradable_batch(
        &tradable_refs,
        strategy,
        &*selector,
        &criteria,
        &exec_config,
        &hedge_config,  // ← PASS TO BATCH
    ).await;
}
```

---

## Testing

After implementation:

```bash
# Without hedging (baseline)
target/debug/cs backtest -c straddle_1m_before_15d_cap1b.toml --output unhedged.json

# With gamma approximation hedging
target/debug/cs backtest -c straddle_1m_before_15d_cap1b_hedged.toml --output hedged_gamma.json

# Compare:
cat unhedged.json | jq '.[] | {symbol, unhedged_pnl: .pnl}'
cat hedged_gamma.json | jq '.[] | {symbol, hedged_pnl: .total_pnl_with_hedge}'

# Verify hedge_position is populated
cat hedged_gamma.json | jq '.[] | select(.hedge_position != null) | {symbol, hedge_position, num_rehedges}'
```

---

## Dependencies

- Delta providers already implemented ✅
- HedgeConfig domain type exists ✅
- Result types support hedging ✅
- Analytics code exists ✅

Just need to **wire it all together** in the execution layer.

---

## Effort Estimate

- Phase 1 (trait update): 30 min
- Phase 2 (straddle integration): 1-2 hours
- Phase 3 (hedging logic): 2-3 hours
- Phase 4 (call sites): 1 hour
- Testing & debug: 1-2 hours

**Total**: ~6-9 hours

---

## Files to Modify

1. `cs-backtest/src/trade_strategy.rs` - Update trait signature
2. `cs-backtest/src/trade_strategy.rs` - Implement in StraddleStrategy, PostEarningsStraddleStrategy, others
3. `cs-backtest/src/backtest_use_case.rs` - Update execute_tradable_batch(), backtest_straddle()
4. `cs-backtest/src/backtest_use_case_helpers.rs` - Add hedging_executor module usage
5. NEW: `cs-backtest/src/hedging_executor.rs` - Core hedging application logic

## Files to Review

- `cs-backtest/src/delta_providers/*` - Already implemented
- `cs-domain/src/entities.rs` - HedgePosition, HedgeConfig types
- `cs-backtest/src/hedging_analytics.rs` - Analytics (reference)

---

## Notes

This is a **reconnection task**, not greenfield implementation. All pieces exist; just need to wire them together in the new execution model.

Highest priority after fixing it: **test with real data** to ensure hedging actually reduces variance as expected.

---

*End of TICKET-0002*
