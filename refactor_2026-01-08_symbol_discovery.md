# Refactoring Plan: Unified Symbol Discovery for Rolling Strategies

**Date**: 2026-01-08
**Status**: Planning
**Issue**: `--symbols is required for rolling strategy`

## Problem Statement

When running rolling strategies without `--symbols`, the CLI fails:
```
Error: --symbols is required for rolling strategy
```

The user wants to:
1. Run rolling strategy for all symbols with earnings in the date range
2. Keep original usage where symbols can be loaded from sample file
3. Support "between earnings" or "last month before earnings" timing

## Current Architecture Analysis

### Existing Components

| Component | Location | Capabilities |
|-----------|----------|-------------|
| `TradingCampaign` | `cs-domain/src/campaign/campaign.rs` | Per-symbol trading definition |
| `SessionSchedule` | `cs-domain/src/campaign/schedule.rs` | Multi-symbol session coordination |
| `PeriodPolicy` | `cs-domain/src/campaign/period_policy.rs` | Timing policies (earnings, inter-earnings, fixed) |
| `TradingPeriodSpec` | `cs-domain/src/trading_period/spec.rs` | Timing templates |
| `EarningsRepository` | `cs-domain/src/repositories.rs` | Loads earnings with optional symbol filter |

### Execution Paths

```
CLI Commands
├── backtest (no --roll-strategy)
│   └── BacktestUseCase → processes earnings-based trades
│
├── backtest --roll-strategy weekly
│   └── run_rolling_straddle() → single symbol, has hedging ✓
│
└── campaign --period-policy inter-earnings
    └── run_campaign_command() → multi-symbol, NO hedging ✗
```

### The Gap

`run_rolling_straddle`:
- ✓ Has hedging support
- ✓ Has attribution support
- ✗ Single symbol only
- ✗ Requires explicit `--symbols`

`run_campaign_command`:
- ✓ Multi-symbol support
- ✓ Uses Campaign/SessionSchedule architecture
- ✓ Can auto-discover symbols from earnings
- ✗ No hedging (TODO at line 2355)
- ✗ Uses SessionExecutor, not TradeExecutor

## Proposed Solutions

### Option A: Add Hedging to Campaign Command (Recommended)

**Scope**: Modify `SessionExecutor` to support hedging, unifying the execution path.

**Changes**:
1. `SessionExecutor::with_hedging()` - Add hedging configuration
2. `SessionExecutor::execute_session_with_hedging()` - Integrate hedge logic
3. `run_campaign_command()` - Wire up hedging when `--hedge` is passed
4. `run_rolling_straddle()` - Deprecate in favor of campaign command

**Benefits**:
- All option types benefit (straddles, calendars, iron butterflies)
- Single code path for rolling + hedging
- Proper multi-symbol support
- Uses existing architecture

**Effort**: Medium (2-3 days)

### Option B: Enhance run_rolling_straddle with Multi-Symbol Loop

**Scope**: Minimal change to existing function.

**Changes**:
1. Auto-discover symbols when `--symbols` not provided
2. Loop over each symbol, collect `Vec<RollingResult>`
3. Save aggregated results

**Benefits**:
- Quick fix
- Preserves existing hedging logic

**Drawbacks**:
- Band-aid solution
- Doesn't benefit other option types
- Duplicates Campaign logic

**Effort**: Low (1 day)

### Option C: Bridge - Route Rolling to Campaign with Hedging Shim

**Scope**: Create adapter that uses Campaign scheduling but TradeExecutor execution.

**Changes**:
1. Generate sessions via `SessionSchedule::from_campaigns()`
2. Execute via `TradeExecutor` with hedging per symbol
3. Aggregate results

**Benefits**:
- Uses Campaign for scheduling
- Uses TradeExecutor for hedging
- Moderate refactoring

**Effort**: Medium (2 days)

## Recommendation

**Option A** is the cleanest long-term solution but requires more work.

For immediate needs, consider **Option C** as a bridge:
- Use Campaign/SessionSchedule for symbol discovery and timing
- Use existing TradeExecutor for hedged execution
- Later migrate to unified SessionExecutor with hedging

## Implementation Steps (Option C)

### Phase 1: Symbol Discovery via Campaigns
1. When `--symbols` not provided and `--roll-strategy` is set:
   - Load all earnings for date range (no symbol filter)
   - Extract unique symbols
   - Create `TradingCampaign` per symbol with `PeriodPolicy::FixedPeriod`

### Phase 2: Session Scheduling
2. Generate sessions via `SessionSchedule::from_campaigns()`
3. Group by symbol for per-symbol execution

### Phase 3: Hedged Execution
4. For each symbol:
   - Extract sessions for that symbol
   - Execute via `TradeExecutor` with hedging
   - Collect `RollingResult`

### Phase 4: Aggregation
5. Combine all `RollingResult` into output
6. Display summary across all symbols
7. Save to output file

## Code Locations to Modify

| File | Change |
|------|--------|
| `cs-cli/src/main.rs` | New function `run_rolling_via_campaigns()` |
| `cs-cli/src/main.rs:1520` | Route to new function when no symbols |
| `cs-domain/src/campaign/period_policy.rs` | Ensure `FixedPeriod` works with roll policies |

## Testing

1. `cs backtest --start 2025-03-01 --end 2025-03-31 --spread straddle --roll-strategy weekly --roll-day friday`
   - Should auto-discover symbols from earnings
   - Should run rolling for each symbol
   - Should output aggregated results

2. `cs backtest --symbols AAPL --start 2025-03-01 --end 2025-03-31 --spread straddle --roll-strategy weekly`
   - Should work as before (explicit symbol)

## Selected Approach

**Option A: Full Unification** - Add hedging to SessionExecutor
**Timing**: PreEarnings (last N trading days before each earnings)

---

## Detailed Implementation Plan

### Phase 1: Current State Analysis

**SessionExecutor already has:**
- `with_hedging(config, timing_strategy)` method (lines 212-220)
- `hedge_config: Option<HedgeConfig>` field
- `timing_strategy: Option<TimingStrategy>` field
- Hedging integration for **Straddle only** (lines 402-405)

**Missing in SessionExecutor:**
- Hedging for CalendarSpread (`execute_calendar_spread`)
- Hedging for IronButterfly (`execute_iron_butterfly`)

**Missing in CLI:**
- Wiring hedging to SessionExecutor in `run_campaign_command()` (line 2355: TODO)

### Phase 2: Add Hedging to Other Strategies in SessionExecutor

**Goal**: Apply hedging consistently across all strategy types.

**Changes to `execute_calendar_spread()`** (starting line 314):
```rust
// Add after line 346 (executor creation):
if let (Some(ref hedge_config), Some(ref timing)) = (&self.hedge_config, &self.timing_strategy) {
    executor = executor.with_hedging(hedge_config.clone(), timing.clone());
}
```

**Changes to `execute_iron_butterfly()`** (starting line 436):
Same pattern as above.

### Phase 3: Wire Hedging in Campaign Command

**Goal**: Connect CLI `--hedge` flags to SessionExecutor.

**Changes to `run_campaign_command()`** (around line 2348):
```rust
// Build executor with optional hedging
let mut executor = SessionExecutor::new(
    options_repo_arc,
    equity_repo_arc,
    trade_factory,
    config,
);

if hedge {
    let hedge_config = HedgeConfig {
        strategy: parse_hedge_strategy(hedge_strategy)?,
        ..Default::default()
    };
    let timing_strategy = TimingStrategy::default(); // Or derive from period_policy
    executor = executor.with_hedging(hedge_config, timing_strategy);
}
```

Remove the TODO warning and replace with actual implementation.

### Phase 4: Add Symbol Auto-Discovery

**Goal**: When `--symbols` not provided, discover from earnings.

**Logic**:
```rust
let symbols = if symbols.is_empty() {
    // Load all earnings in date range
    let earnings = earnings_repo.load_earnings(start, end, None).await?;
    earnings.iter().map(|e| e.symbol.clone()).collect::<HashSet>()
} else {
    symbols.into_iter().collect()
};
```

### Phase 5: Support PreEarnings Period Policy

**Goal**: Default to PreEarnings timing for auto-discovered symbols.

**Config**:
```rust
PeriodPolicy::EarningsOnly {
    timing: TradingPeriodSpec::PreEarnings {
        entry_days_before: 14,  // Or from --entry-days-before
        exit_days_before: 1,
        entry_time,
        exit_time,
    }
}
```

### Phase 6: Deprecate run_rolling_straddle

**Goal**: Route all rolling strategies through Campaign system.

**Migration**:
1. `--roll-strategy` flag → maps to `PeriodPolicy::FixedPeriod` or similar
2. Remove `run_rolling_straddle()` function
3. Update CLI help text

---

## Files to Modify

| File | Changes |
|------|---------|
| `cs-backtest/src/session_executor.rs` | Add `with_hedging()`, hedge execution |
| `cs-backtest/src/lib.rs` | Export new types if needed |
| `cs-cli/src/main.rs` | Update campaign command, add auto-discovery |
| `cs-domain/src/campaign/period_policy.rs` | Ensure PreEarnings works correctly |

## Testing Plan

1. **Unit tests**: SessionExecutor with hedging
2. **Integration test**: Campaign command with `--hedge`
3. **CLI test**: Auto-discovery mode
   ```bash
   cs campaign --strategy straddle --period-policy pre-earnings \
     --start 2025-03-01 --end 2025-03-31 --hedge
   ```

## Risks

1. **SessionExecutor vs TradeExecutor**: Different execution models
   - SessionExecutor: session-based (entry/exit events)
   - TradeExecutor: rolling-based (continuous rebalancing)
   - May need to reconcile or support both

2. **Hedging state across sessions**: For multi-session campaigns,
   hedge state may need to persist or reset between sessions

3. **Result aggregation**: Need to decide how to combine results
   from multiple symbols/sessions
