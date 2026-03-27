# Refactoring Plan: Trading Costs & Attribution in Strategy Execute

**Date:** 2026-01-11
**Branch:** fix/trading-costs-in-strategy

## Problem Statement

Trading costs are configured in `create_execution_config()` but never applied in the backtest path:
- `TradeExecutor::execute()` applies costs (line 176-189) ✅
- `TradeStrategy::execute_trade()` does NOT apply costs ❌

The backtest uses `TradeStrategy::execute_trade()` directly, bypassing `TradeExecutor`.

## Solution: Default Implementation Pattern

Move common execution logic to a default `execute_trade` implementation in `TradeStrategy` trait.
This ensures costs and attribution are applied in ONE place for all strategies.

---

## Phase 1: Extract Attribution as Standalone Function

**File:** `cs-backtest/src/attribution/mod.rs`

### 1.1 Create `compute_position_attribution()` function

Extract the logic from `TradeExecutor::compute_attribution()` (lines 516-557) into a standalone async function:

```rust
pub async fn compute_position_attribution<T: CompositeTrade + Clone>(
    trade: T,
    hedge_position: &HedgePosition,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    actual_pnl: Decimal,
    attribution_config: &AttributionConfig,
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    contract_multiplier: i32,
) -> Result<PositionAttribution, String>
```

### 1.2 Update TradeExecutor to use extracted function

Modify `TradeExecutor::compute_attribution()` to delegate to the new standalone function.

---

## Phase 2: Refactor TradeStrategy Trait

**File:** `cs-backtest/src/trade_strategy.rs`

### 2.1 Add Associated Types

```rust
pub trait TradeStrategy<R: TradeResultMethods + Send>: Send + Sync {
    type Trade: CompositeTrade + ExecutableTrade<Result = R>;
    type SelectionParams: Send + Sync;

    // ... existing methods
}
```

### 2.2 Add Required Methods for Strategy-Specific Logic

```rust
/// Get strategy-specific selection parameters
fn selection_params(&self, criteria: &ExpirationCriteria) -> Self::SelectionParams;

/// Select trade using strategy-specific logic
fn select_trade(
    &self,
    selector: &dyn StrikeSelector,
    data: &PreparedData,
    params: &Self::SelectionParams,
) -> Result<Self::Trade, SelectionError>;

/// Optional: Post-selection validation (e.g., DTE checks for straddles)
fn validate_selection(
    &self,
    _trade: &Self::Trade,
    _event: &EarningsEvent,
    _criteria: &ExpirationCriteria,
) -> Result<(), TradeGenerationError> {
    Ok(())  // Default: no extra validation
}
```

### 2.3 Move Common Code to Default `execute_trade` Implementation

The default implementation will:
1. Prepare market data via TradeSimulator
2. Check market-level rules
3. Call `self.select_trade()` (strategy-specific)
4. Call `self.validate_selection()` (strategy-specific, optional)
5. Price using generic CompositePricer
6. Check trade-level rules
7. Simulate with hedging
8. Build result
9. **Apply trading costs** ← NEW
10. Attach hedge data
11. **Compute attribution if configured** ← NEW
12. Return outcome

---

## Phase 3: Simplify Strategy Implementations

Each of the 7 strategies becomes minimal:

### 3.1 CalendarSpreadStrategy
- `SelectionParams = (OptionType, ExpirationCriteria)`
- `select_trade()`: calls `selector.select_calendar_spread()`

### 3.2 IronButterflyStrategy
- `SelectionParams = (Decimal, i32, i32)` (wing_width, min_dte, max_dte)
- `select_trade()`: calls `selector.select_iron_butterfly()`

### 3.3 LongIronButterflyStrategy
- `SelectionParams = (Decimal, i32, i32)`
- `select_trade()`: calls `selector.select_long_iron_butterfly()`

### 3.4 LongStraddleStrategy
- `SelectionParams = NaiveDate` (min_expiration)
- `select_trade()`: calls `selector.select_long_straddle()`
- `validate_selection()`: calls `ensure_straddle_max_dte()`

### 3.5 ShortStraddleStrategy
- `SelectionParams = NaiveDate`
- `select_trade()`: calls `selector.select_short_straddle()`
- `validate_selection()`: calls `ensure_straddle_max_dte()`

### 3.6 PostEarningsStraddleStrategy
- `SelectionParams = NaiveDate`
- `select_trade()`: calls `selector.select_long_straddle()`
- `validate_selection()`: calls `ensure_straddle_max_dte()`

### 3.7 CalendarStraddleStrategy
- `SelectionParams = ExpirationCriteria`
- `select_trade()`: calls `selector.select_calendar_straddle()`

---

## Phase 4: Update Cost Application

**File:** `cs-backtest/src/execution/cost_helpers.rs`

Ensure `apply_costs_to_result()` works with all result types:
- Already implements `ApplyCosts` for all result types ✅
- Need to ensure `ToTradingContext` is implemented for pricing types ✅

---

## Implementation Order

1. **Phase 1.1**: Create standalone `compute_position_attribution()` function
2. **Phase 1.2**: Update TradeExecutor to use it (verify no regression)
3. **Phase 2.1-2.3**: Refactor TradeStrategy trait with associated types and default impl
4. **Phase 3.1**: Migrate CalendarSpreadStrategy (simplest, has tests)
5. **Phase 3.2-3.7**: Migrate remaining strategies one by one
6. **Phase 4**: Verify cost application works for all strategies
7. **Testing**: Run full test suite, verify costs appear in results

---

## Files to Modify

| File | Changes |
|------|---------|
| `cs-backtest/src/attribution/mod.rs` | Add `compute_position_attribution()` |
| `cs-backtest/src/trade_executor.rs` | Delegate to extracted attribution fn |
| `cs-backtest/src/trade_strategy.rs` | Major refactor - trait + 7 impls |
| `cs-backtest/src/execution/cost_helpers.rs` | May need minor updates |

---

## Risk Mitigation

1. **Incremental migration**: Migrate one strategy at a time
2. **Keep old code**: Don't delete old implementations until all tests pass
3. **Type safety**: Associated types ensure compile-time correctness
4. **Test coverage**: Run existing tests after each strategy migration

---

## Success Criteria

- [ ] All 7 strategies use default `execute_trade` implementation
- [ ] Trading costs applied to all backtest results
- [ ] Attribution computed when configured
- [ ] All existing tests pass
- [ ] No performance regression
