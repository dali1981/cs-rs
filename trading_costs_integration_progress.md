# Trading Costs Integration - Progress Note

**Date:** 2026-01-09
**Session:** Continuation from context compaction

---

## ✅ COMPLETED

### 1. Infrastructure Setup (100% Complete)
- ✅ Added `trading_costs: TradingCostConfig` field to `BacktestConfig`
- ✅ Changed `TradingCostConfig::default()` to `None` (zero costs, explicit opt-in)
- ✅ Threaded config through `BacktestUseCase.create_execution_config()`
- ✅ Config properly passed to execution layer via `ExecutionConfig`

### 2. Domain Model Updates (100% Complete)
- ✅ Created `CostSummary` struct in `cs-domain/src/entities.rs` (lines 8-27)
  - Encapsulates `TradingCost` + `gross_pnl` in single object
  - Provides `net_pnl()` convenience method
- ✅ Added `cost_summary: Option<CostSummary>` field to ALL 8 result structs:
  - `CalendarSpreadResult`
  - `IronButterflyResult`
  - `StraddleResult`
  - `CalendarStraddleResult`
  - `StrangleResult`
  - `ButterflyResult`
  - `CondorResult`
  - `IronCondorResult`

### 3. Trait Infrastructure (100% Complete)
- ✅ Created `HasTradingCost` trait in `cs-domain/src/trading_costs/has_cost.rs`
  - Methods: `cost_summary()`, `has_costs()`, `gross_pnl()`, `total_costs()`
- ✅ Implemented trait for all 8 result types in `cs-domain/src/entities/cost_impls.rs`
- ✅ Exported from `cs-domain/src/trading_costs/mod.rs`

### 4. Execution Trait Updates (100% Complete)
- ✅ Updated `ExecutableTrade::to_result()` signature in `cs-backtest/src/execution/traits.rs`
  - Added `config: &ExecutionConfig` parameter (6th parameter)
- ✅ Updated all call sites (6 locations):
  - `cs-backtest/src/trade_executor.rs` (1 call site)
  - `cs-backtest/src/trade_strategy.rs` (5 call sites - used replace_all)

### 5. Implementation Updates (62.5% Complete - 5 of 8)

#### ✅ Fully Updated Implementations:
1. **CalendarSpread** (`calendar_spread_impl.rs`)
   - Uses `CompositePricing.to_trading_context()`
   - Calculates costs, subtracts from gross P&L
   - Sets `cost_summary` in success result
   - Sets `cost_summary: None` in failed result

2. **Straddle** (`straddle_impl.rs`)
   - Uses `CompositePricing.to_trading_context()`
   - Calculates costs, subtracts from gross P&L
   - Sets `cost_summary` in success result
   - Sets `cost_summary: None` in failed result

3. **CalendarStraddle** (`calendar_straddle_impl.rs`)
   - Uses `CompositePricing.to_trading_context()`
   - Calculates costs, subtracts from gross P&L
   - Sets `cost_summary` in success result
   - Sets `cost_summary: None` in failed result

4. **IronButterfly** (`iron_butterfly_impl.rs`)
   - Uses `CompositePricing.to_trading_context()`
   - Calculates costs, subtracts from gross P&L
   - Renamed `exit_cost` variable to `exit_cost_trading` to avoid conflict
   - Sets `cost_summary` in success result
   - Sets `cost_summary: None` in failed result

5. **Strangle** (`strangle_impl.rs`)
   - Uses `StranglePricing` (NOT CompositePricing)
   - Manually constructs `TradingContext` from leg data
   - Pattern: `vec![LegContext::long(...), LegContext::long(...)]`
   - Sets `cost_summary` in success and failed results

---

## 🚧 IN PROGRESS

### 6. Remaining Implementations (37.5% - 3 of 8 remaining)

#### ⏳ Butterfly (`butterfly_impl.rs`)
- **Status:** Signature needs updating
- **Pricing Type:** `ButterflyPricing` (custom struct, similar to Strangle)
- **Action Needed:**
  1. Add `config: &ExecutionConfig` parameter to `to_result()`
  2. Check `ButterflyPricing` structure in `multi_leg_pricer.rs`
  3. Build `TradingContext` manually (similar to Strangle pattern)
  4. Calculate costs, subtract from gross P&L
  5. Add `cost_summary` to success result
  6. Add `cost_summary: None` to failed result

#### ⏳ Condor (`condor_impl.rs`)
- **Status:** Signature needs updating
- **Pricing Type:** `CondorPricing` (custom struct)
- **Action Needed:** Same as Butterfly

#### ⏳ IronCondor (`iron_condor_impl.rs`)
- **Status:** Signature needs updating
- **Pricing Type:** `IronCondorPricing` (custom struct)
- **Action Needed:** Same as Butterfly

**Current Compilation Errors:** 3 signature mismatches (E0050)

---

## 📋 TODO (Not Yet Started)

### 7. BacktestResult Cost Aggregation
**File:** `cs-backtest/src/backtest_result.rs` (likely location)

Add methods to aggregate costs across all trades:
```rust
impl<R: TradeResult + HasTradingCost> BacktestResult<R> {
    /// Total trading costs across all trades
    pub fn total_trading_costs(&self) -> Decimal {
        self.results.iter()
            .filter_map(|r| r.cost_summary())
            .map(|cs| cs.costs.total)
            .sum()
    }

    /// Total gross P&L (before costs)
    pub fn total_gross_pnl(&self) -> Decimal {
        self.results.iter()
            .filter_map(|r| r.cost_summary())
            .map(|cs| cs.gross_pnl)
            .sum()
    }

    /// Check if any trades have costs
    pub fn has_trading_costs(&self) -> bool {
        self.results.iter().any(|r| r.has_costs())
    }
}
```

### 8. Display Output Updates
**File:** `cs-cli/src/output/backtest.rs`

Add cost breakdown to summary display:
```rust
// Add to results display
if result.has_trading_costs() {
    let total_costs = result.total_trading_costs();
    let gross_pnl = result.total_gross_pnl();
    let net_pnl = result.total_pnl();

    rows.extend(vec![
        ResultRow { metric: "".into(), value: "".into() },
        ResultRow { metric: "Gross P&L (before costs)".into(), value: format!("${:.2}", gross_pnl) },
        ResultRow { metric: "Trading Costs".into(), value: format!("$-{:.2}", total_costs.total) },
        ResultRow { metric: "  Slippage".into(), value: format!("$-{:.2}", total_costs.breakdown.slippage) },
        ResultRow { metric: "  Commission".into(), value: format!("$-{:.2}", total_costs.breakdown.commission) },
        ResultRow { metric: "Net P&L (after costs)".into(), value: format!("${:.2}", net_pnl) },
    ]);
}
```

### 9. Integration Testing
- Run backtest with `straddle_with_costs.toml` config
- Verify costs are calculated and applied
- Compare results with/without costs
- Expected: ~$45 costs on $68 gross P&L → ~$23 net P&L

---

## 📝 KEY DESIGN DECISIONS

1. **CostSummary Encapsulation**
   - Single `cost_summary: Option<CostSummary>` field instead of separate `trading_cost` and `gross_pnl` fields
   - Cleaner API, better encapsulation

2. **pnl Field Meaning**
   - `pnl` field represents NET P&L (after costs)
   - `cost_summary.gross_pnl` contains gross P&L (before costs)
   - **BREAKING CHANGE:** Old code assuming `pnl` is gross will be incorrect

3. **Default Behavior**
   - `TradingCostConfig::default()` = `None` (zero costs)
   - Explicit opt-in required
   - Backward compatible: old configs without `[trading_costs]` = zero costs

4. **Cost Calculation Pattern**
   ```rust
   // 1. Calculate gross P&L
   let gross_pnl = pnl_per_share * CONTRACT_MULTIPLIER;

   // 2. Build trading contexts
   let entry_ctx = entry_pricing.to_trading_context(...);
   let exit_ctx = exit_pricing.to_trading_context(...);

   // 3. Calculate costs
   let cost_calc = config.cost_calculator();
   let total_cost = cost_calc.entry_cost(&entry_ctx) + cost_calc.exit_cost(&exit_ctx);

   // 4. Calculate net P&L
   let pnl = gross_pnl - total_cost.total;

   // 5. Create summary (only if costs > 0)
   let cost_summary = if total_cost.total > Decimal::ZERO {
       Some(CostSummary::new(total_cost, gross_pnl))
   } else {
       None
   };
   ```

5. **ExecutionConfig Has Behavior**
   - `ExecutionConfig.cost_calculator()` method returns `Box<dyn TradingCostCalculator>`
   - Infrastructure concern (acceptable to have behavior)
   - Alternative: Pass calculator directly (more parameters, duplicated logic)

---

## 🔍 NEXT STEPS

1. **Immediate:** Update last 3 implementations (Butterfly, Condor, IronCondor)
   - Check pricing struct fields in `multi_leg_pricer.rs`
   - Apply Strangle pattern (manual TradingContext construction)
   - Test compilation

2. **Then:** Add cost aggregation methods to BacktestResult

3. **Then:** Update display output with cost breakdown

4. **Finally:** Run integration test with costs enabled

---

## 📂 FILES MODIFIED

### cs-domain (8 files)
- `src/entities.rs` - Added CostSummary struct, cost_summary fields
- `src/entities/cost_impls.rs` - NEW: HasTradingCost implementations
- `src/trading_costs/mod.rs` - Export HasTradingCost
- `src/trading_costs/has_cost.rs` - NEW: HasTradingCost trait
- `src/trading_costs/config.rs` - Changed default to None

### cs-backtest (9 files)
- `src/config/mod.rs` - Added trading_costs field
- `src/backtest_use_case.rs` - Thread costs through create_execution_config()
- `src/execution/traits.rs` - Updated to_result() signature
- `src/trade_executor.rs` - Updated call site
- `src/trade_strategy.rs` - Updated 5 call sites
- `src/execution/calendar_spread_impl.rs` - ✅ Full cost integration
- `src/execution/straddle_impl.rs` - ✅ Full cost integration
- `src/execution/calendar_straddle_impl.rs` - ✅ Full cost integration
- `src/execution/iron_butterfly_impl.rs` - ✅ Full cost integration
- `src/execution/strangle_impl.rs` - ✅ Full cost integration (manual context)

### Not Yet Modified
- `src/execution/butterfly_impl.rs` - ⏳ TODO
- `src/execution/condor_impl.rs` - ⏳ TODO
- `src/execution/iron_condor_impl.rs` - ⏳ TODO
- `cs-cli/src/output/backtest.rs` - ⏳ TODO (display)

---

## 📊 PROGRESS SUMMARY

- **Overall:** ~70% complete
- **Infrastructure:** 100% ✅
- **Domain Model:** 100% ✅
- **Trait System:** 100% ✅
- **Execution Impls:** 62.5% (5/8) ✅
- **Aggregation:** 0% ⏳
- **Display:** 0% ⏳
- **Testing:** 0% ⏳

**Estimated Remaining Work:**
- 3 implementations: ~20 minutes
- Aggregation methods: ~10 minutes
- Display output: ~15 minutes
- Testing: ~10 minutes
**Total:** ~55 minutes to completion

---

## 🐛 KNOWN ISSUES

None - compilation errors are expected until remaining 3 implementations are updated.

---

## 📌 REFERENCE

- Original plan: `specs/trading_costs_integration_plan.md`
- Test config: `straddle_with_costs.toml`
- Trading costs module: `cs-domain/src/trading_costs/`
