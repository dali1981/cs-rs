# Trading Costs Integration Plan

**Date:** 2026-01-09
**Status:** Plan for Validation

---

## Overview

Complete the integration of trading costs into the backtest execution flow so that costs are calculated and subtracted from P&L.

## Current State

✅ **Infrastructure Built**
- `cs-domain/src/trading_costs/` - All cost models implemented
- `ExecutionConfig.trading_costs` - Config field added
- `CompositePricing.to_trading_context()` - Bridge method added

❌ **Not Yet Integrated**
- Config parsing (TOML → BacktestConfig)
- Cost calculation in execution
- Cost subtraction from P&L
- Cost fields in result structs
- Cost display in output

---

## Integration Steps

### Step 1: Config Parsing (TOML → BacktestConfig)

**File:** `cs-backtest/src/config/mod.rs`

```rust
// Add to BacktestConfig struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    // ... existing fields ...

    /// Trading costs configuration (optional, defaults to no costs)
    #[serde(default)]
    pub trading_costs: TradingCostConfig,
}
```

**Why:** Parse `[trading_costs]` section from TOML files.

---

### Step 2: Pass Costs to Execution Layer

**File:** `cs-backtest/src/backtest_use_case.rs`

In `build_execution_config()`:

```rust
fn build_execution_config(&self) -> ExecutionConfig {
    ExecutionConfig::for_strategy(self.config.spread.to_option_strategy(), None)
        .with_trading_costs(self.config.trading_costs.clone())  // ADD THIS
}
```

**Why:** Thread costs config from BacktestConfig → ExecutionConfig → Executors.

---

### Step 3: Add Cost Fields to Result Structs

**Files:** `cs-domain/src/entities.rs`

Add to ALL trade result types:

```rust
pub struct CalendarSpreadResult {
    // ... existing fields ...

    /// Trading costs breakdown (entry + exit)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trading_cost: Option<TradingCost>,

    /// Gross P&L (before costs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gross_pnl: Option<Decimal>,

    /// Net P&L (after costs) - this becomes the main pnl field
    /// NOTE: pnl field should represent NET P&L
}

// Apply to:
// - CalendarSpreadResult
// - StraddleResult
// - IronButterflyResult
// - CalendarStraddleResult
// - PostEarningsStraddleResult
```

**Design Decision:**
- `pnl` = net P&L (after costs) - PRIMARY METRIC
- `gross_pnl` = optional field showing P&L before costs
- `trading_cost` = optional breakdown

**Why:** Preserve backward compatibility (pnl field remains), but add cost transparency.

---

### Step 4: Calculate Costs in Execution Layer

**Files:** `cs-backtest/src/execution/*.rs`

For each `to_result()` method in execution implementations:

```rust
// Example: cs-backtest/src/execution/calendar_spread_impl.rs

fn to_result(
    entry: &TradeEntry<CalendarSpread>,
    exit: &TradeExit,
    entry_pricing: CompositePricing,
    exit_pricing: CompositePricing,
    output: &SimulationOutput,
) -> CalendarSpreadResult {
    // Step 1: Calculate gross P&L (existing logic, unchanged)
    let pnl_per_share = exit_pricing.net_cost - entry_pricing.net_cost;
    let gross_pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);

    // Step 2: Build trading contexts
    let entry_ctx = entry_pricing.to_trading_context(
        &entry.trade.symbol(),
        output.entry_spot,
        output.entry_time,
        TradeType::CalendarSpread,
    );

    let exit_ctx = exit_pricing.to_trading_context(
        &entry.trade.symbol(),
        output.exit_spot,
        output.exit_time,
        TradeType::CalendarSpread,
    );

    // Step 3: Calculate costs
    let cost_calc = entry.config.cost_calculator();
    let entry_cost = cost_calc.entry_cost(&entry_ctx);
    let exit_cost = cost_calc.exit_cost(&exit_ctx);
    let total_cost = entry_cost + exit_cost;

    // Step 4: Calculate net P&L
    let net_pnl = gross_pnl - total_cost.total;
    let net_pnl_pct = if entry_pricing.net_cost.abs() > Decimal::ZERO {
        (net_pnl / entry_pricing.net_cost.abs()) * Decimal::from(100)
    } else {
        Decimal::ZERO
    };

    CalendarSpreadResult {
        // ... all existing fields ...
        pnl: net_pnl,                          // NET P&L (main metric)
        pnl_pct: net_pnl_pct,                 // NET return %
        gross_pnl: Some(gross_pnl),           // NEW: gross before costs
        trading_cost: Some(total_cost),       // NEW: cost breakdown
        // ... rest of fields ...
    }
}
```

**Apply to ALL execution implementations:**
- `cs-backtest/src/execution/calendar_spread_impl.rs`
- `cs-backtest/src/execution/straddle_impl.rs`
- `cs-backtest/src/execution/iron_butterfly_impl.rs`
- `cs-backtest/src/execution/calendar_straddle_impl.rs`
- `cs-backtest/src/execution/post_earnings_straddle_impl.rs`

**Critical:** Each executor's `to_result()` needs access to `ExecutionConfig` to get the cost calculator.

---

### Step 5: Thread ExecutionConfig Through Executors

**Problem:** Current `to_result()` signatures don't have access to `ExecutionConfig`.

**Solution:** Add config parameter to `to_result()` in the trait:

**File:** `cs-backtest/src/execution/mod.rs`

```rust
pub trait ExecutableTrade: Sized {
    // ... existing methods ...

    // UPDATE SIGNATURE to include config
    fn to_result(
        entry: &TradeEntry<Self>,
        exit: &TradeExit,
        entry_pricing: Self::Pricing,
        exit_pricing: Self::Pricing,
        output: &SimulationOutput,
        config: &ExecutionConfig,  // ADD THIS
    ) -> Self::Result;
}
```

**Impact:** This is a **breaking change** to the trait. All implementations must be updated.

---

### Step 6: Update Display Output

**File:** `cs-cli/src/output/backtest.rs`

Add costs to display:

```rust
fn display_summary<R>(result: &BacktestResult<R>)
where
    R: TradeResultTrait + TradeResultMethods + HasTradingCost,  // NEW TRAIT
{
    // ... existing metrics ...

    // Add cost breakdown
    if result.has_trading_costs() {
        let total_costs = result.total_trading_costs();
        let gross_pnl = result.gross_pnl();
        let net_pnl = result.total_pnl();  // Already net

        rows.extend(vec![
            ResultRow { metric: "".into(), value: "".into() },
            ResultRow { metric: "Gross P&L (before costs)".into(), value: format!("${:.2}", gross_pnl) },
            ResultRow { metric: "Trading Costs".into(), value: format!("$-{:.2}", total_costs.total) },
            ResultRow { metric: "  Slippage".into(), value: format!("$-{:.2}", total_costs.breakdown.slippage) },
            ResultRow { metric: "  Commission".into(), value: format!("$-{:.2}", total_costs.breakdown.commission) },
            ResultRow { metric: "Net P&L (after costs)".into(), value: format!("${:.2}", net_pnl) },
        ]);
    }
}
```

---

## New Trait: HasTradingCost

**File:** `cs-domain/src/trading_costs/has_cost.rs` (NEW)

```rust
/// Trait for trade results that include trading costs
pub trait HasTradingCost {
    /// Get trading cost if available
    fn trading_cost(&self) -> Option<&TradingCost>;

    /// Get gross P&L (before costs) if available
    fn gross_pnl(&self) -> Option<Decimal>;

    /// Check if costs were calculated
    fn has_costs(&self) -> bool {
        self.trading_cost().is_some()
    }
}

// Implement for all result types
impl HasTradingCost for CalendarSpreadResult {
    fn trading_cost(&self) -> Option<&TradingCost> {
        self.trading_cost.as_ref()
    }

    fn gross_pnl(&self) -> Option<Decimal> {
        self.gross_pnl
    }
}

// ... implement for StraddleResult, IronButterflyResult, etc.
```

---

## Backward Compatibility

**Existing behavior preserved:**
1. **No config**: `TradingCostConfig::default()` → Uses `NoCost` → Zero costs
2. **JSON output**: New fields are `Option<T>` with `skip_serializing_if` → Old parsers won't break
3. **pnl field**: Still the primary metric, just now represents NET instead of GROSS

**Migration:**
- Old configs without `[trading_costs]` → Zero costs (backward compatible)
- Old result parsers → Ignore new fields (backward compatible)
- Analytics expecting gross P&L → Use new `gross_pnl` field

---

## Testing Strategy

### Unit Tests

1. **Cost Calculation**
   ```rust
   #[test]
   fn test_calendar_spread_with_costs() {
       // Setup: $60 entry, $80 P&L
       // Expected costs: ~$11 entry + $11 exit = $22
       // Net P&L: $80 - $22 = $58
   }
   ```

2. **Zero Costs**
   ```rust
   #[test]
   fn test_calendar_spread_no_costs() {
       // Config with NoCost
       // P&L should equal gross_pnl
   }
   ```

### Integration Tests

1. **Run existing test_accounting.json backtest**
   - With costs: Expect ~$45 total costs on $68 P&L
   - Net P&L: ~$23
   - Net ROC: ~17.8%

2. **Compare with/without costs**
   ```bash
   # Without costs
   cargo run -- backtest --start 2025-10-01 --end 2025-10-03 -c no_costs.toml

   # With costs
   cargo run -- backtest --start 2025-10-01 --end 2025-10-03 -c with_costs.toml
   ```

---

## Implementation Order

1. ✅ Add `trading_costs` to `BacktestConfig` (simple serde field)
2. ✅ Add cost fields to result structs (backward compatible)
3. ✅ Create `HasTradingCost` trait + implementations
4. ⚠️ **BREAKING:** Update `ExecutableTrade::to_result()` signature
5. ✅ Update all `to_result()` implementations with cost calculations
6. ✅ Add cost aggregation methods to `BacktestResult`
7. ✅ Update display output
8. ✅ Test with real backtest

---

## Example Output (After Integration)

```
Results:
+-----------------------+-------------------+
| Win Rate              | 50.00%            |
+-----------------------+-------------------+
| Gross P&L (before)    | $68.00            |
+-----------------------+-------------------+
| Trading Costs         | $-45.20           |
|   Slippage            | $-40.64           |
|   Commission          | $-4.56            |
+-----------------------+-------------------+
| Net P&L (after costs) | $22.80            |
+-----------------------+-------------------+

Capital-Weighted Metrics:
+-------------------------+---------+
| Gross ROC               | 52.71%  |
| Net ROC (after costs)   | 17.67%  |
| Profit Factor (net)     | 2.90    |
+-------------------------+---------+
```

---

## Risk Assessment

### Breaking Changes
- `ExecutableTrade::to_result()` signature change
- **Mitigation:** All implementations in same codebase, update together

### Data Impact
- `pnl` field changes meaning (gross → net)
- **Mitigation:** Add `gross_pnl` field, document change
- **Alternative:** Keep `pnl` as gross, add `net_pnl` field (less breaking)

### Performance
- Additional cost calculation per trade
- **Impact:** Negligible (simple arithmetic)

---

## Alternative: Non-Breaking Design

**Option:** Keep `pnl` as gross, add `net_pnl`:

```rust
pub struct CalendarSpreadResult {
    pub pnl: Decimal,              // UNCHANGED: gross P&L
    pub net_pnl: Option<Decimal>,  // NEW: P&L after costs
    pub trading_cost: Option<TradingCost>,
}
```

**Pros:**
- No breaking change to existing analytics
- Clear separation of gross vs net

**Cons:**
- `pnl` name is ambiguous (should be `gross_pnl`)
- More fields

**Recommendation:** Go with breaking change (`pnl = net`) for cleaner API. Analytics should use net P&L as primary metric.

---

## Questions for Validation

1. **Breaking Change OK?** Update `pnl` to mean NET P&L (after costs)?
   - Alternative: Keep `pnl` as gross, add `net_pnl` field

2. **Cost Calculation Location?** In `to_result()` methods?
   - Alternative: Calculate costs in BacktestUseCase after execution

3. **Default Behavior?** With no `[trading_costs]` config:
   - Current: Uses `TradingCostConfig::default()` → Normal spread (4%)
   - Alternative: Default to `NoCost` (zero costs)

4. **Display Priority?** Show costs in main summary or separate section?
   - Proposed: Main summary with breakdown

5. **JSON Field Names?**
   - `trading_cost` (object) or `trading_costs` (plural)?
   - `gross_pnl` or `pnl_before_costs`?

---

## Decision Log

| Decision | Choice | Rationale |
|----------|--------|-----------|
| pnl field meaning | NET P&L | Primary metric should be actionable (net) |
| New fields | `gross_pnl`, `trading_cost` | Transparency + backward compat |
| Cost calculation location | `to_result()` | Close to P&L calculation, clean |
| Default config | `NoCost` | Explicit opt-in, no surprises |
| Trait signature | Add `config` param | Needed for cost calculator access |

---

## Approval Checklist

- [ ] Approve breaking change to `ExecutableTrade::to_result()` signature
- [ ] Approve `pnl` field meaning change (gross → net)
- [ ] Approve default cost model (none vs normal spread)
- [ ] Approve display format (cost breakdown in main summary)
- [ ] Approve implementation order
