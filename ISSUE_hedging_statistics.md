# Issue: Statistics and Sample Trades Display Ignore Hedging

## Status
Open - 2026-01-05

## Summary
When `--hedge` is enabled, the backtest summary correctly shows separate "Option P&L", "Hedge P&L", and "Total P&L (with hedge)". However, all other statistics and the sample trades display continue to use **option-only P&L** instead of **hedged P&L**.

## Observed Behavior

Example output with hedging enabled:
```
+------------------------+------------------+
| Option P&L             | $1.73            |  ✓ Correct
| Hedge P&L              | $-2266.86        |  ✓ Correct
| Total P&L (with hedge) | $-2295.23        |  ✓ Correct
| Avg P&L per Trade      | $-573.80         |  ✓ Correct (uses hedged P&L)
+------------------------+------------------+
| Mean Return            | 10.71%           |  ✗ WRONG (uses option-only)
| Std Dev                | 82.45%           |  ✗ WRONG (uses option-only)
| Sharpe Ratio           | 2.08             |  ✗ WRONG (uses option-only)
+------------------------+------------------+
| Avg Winner             | $1.56 (75.66%)   |  ✗ WRONG (uses option-only)
| Avg Loser              | $-0.69 (-54.24%) |  ✗ WRONG (uses option-only)
+------------------------+------------------+

Sample Trades:
  1. PENG Straddle @ 20 | P&L: $-1.15 (-88.47%)  ✗ WRONG (should show hedged P&L)
  2. PENG Straddle @ 20 | P&L: $0.94 (51.38%)    ✗ WRONG
  3. PENG Straddle @ 20 | P&L: $-0.23 (-20.00%)  ✗ WRONG
  4. PENG Straddle @ 25 | P&L: $2.18 (99.93%)    ✗ WRONG
```

### Expected Behavior
When hedging is enabled, sample trades should show:
```
  1. PENG Straddle @ 20 | P&L: $429.36 (option: -$1.15, hedge: +$434.90)
  2. PENG Straddle @ 20 | P&L: $-405.91 (option: +$0.94, hedge: -$396.97)
  3. PENG Straddle @ 20 | P&L: $122.99 (option: -$0.23, hedge: +$127.46)
  4. PENG Straddle @ 25 | P&L: $-2441.67 (option: +$2.18, hedge: -$2432.26)
```

And statistics should be calculated from hedged P&L.

## Affected Code

### 1. `cs-backtest/src/backtest_use_case.rs`

**`pnl_pcts()` method (line ~51)**
- Currently uses `r.pnl_pct()` which returns option-only percentage
- Should use hedged P&L percentage when hedging is enabled
- Used by: `mean_return()`, `std_return()`, `sharpe_ratio()`

**`avg_winner()` method (line ~96)**
- Currently uses `r.pnl()` which returns option-only dollars
- Should use `r.total_pnl_with_hedge()` when hedging is enabled

**`avg_winner_pct()` method (line ~109)**
- Currently uses `r.pnl_pct()` which returns option-only percentage
- Should calculate percentage from hedged P&L when hedging is enabled

**`avg_loser()` method (line ~127)**
- Currently uses `r.pnl()` which returns option-only dollars
- Should use `r.total_pnl_with_hedge()` when hedging is enabled

**`avg_loser_pct()` method (line ~140)**
- Currently uses `r.pnl_pct()` which returns option-only percentage
- Should calculate percentage from hedged P&L when hedging is enabled

### 2. `cs-backtest/src/unified_executor.rs`

**`is_winner()` method (line ~59)**
- Currently checks if option-only `pnl() > 0`
- Should check hedged P&L when hedging is enabled
- This affects win rate calculation

### 3. `cs-cli/src/main.rs`

**Sample trades display (line ~1026)**
```rust
println!("  {}. {} {} @ {} | P&L: ${:.2} ({:.2}%)",
    i + 1,
    trade.symbol(),
    option_type_str,
    strike_str,
    trade.pnl(),      // <-- uses option-only
    trade.pnl_pct(),  // <-- uses option-only
);
```
Should display hedged P&L when hedging is enabled, with optional breakdown.

## Root Cause

The methods in `BacktestResult` and `TradeResult` don't have awareness of whether hedging is enabled. They always return option-only metrics.

## Proposed Solution

### Approach 1: Conditional Methods (Simpler)
Modify `BacktestResult` methods to check `has_hedging()` and use hedged values when available:

```rust
fn pnl_pcts(&self) -> Vec<f64> {
    self.results.iter()
        .filter(|r| r.success())
        .map(|r| {
            let pnl = if r.has_hedge_data() {
                r.total_pnl_with_hedge().unwrap()
            } else {
                r.pnl()
            };
            // Calculate percentage from pnl
            let pnl_f64: f64 = pnl.try_into().unwrap_or(0.0);
            pnl_f64 / entry_cost  // Need entry cost for proper percentage
        })
        .collect()
}
```

**Problem**: Need to recalculate percentage from hedged dollar P&L, which requires entry cost.

### Approach 2: Add Hedged Percentage to Domain (Better)
Add `hedged_pnl_pct` field to `StraddleResult`:

```rust
pub struct StraddleResult {
    // ... existing fields
    pub hedge_pnl: Option<Decimal>,
    pub total_pnl_with_hedge: Option<Decimal>,
    pub hedged_pnl_pct: Option<Decimal>,  // NEW: percentage return including hedge
}
```

Calculate this in `unified_executor.rs` when applying hedging:
```rust
let hedged_pnl_pct = (total_pnl / result.entry_debit) * Decimal::from(100);
result.hedged_pnl_pct = Some(hedged_pnl_pct);
```

Then update `TradeResult` to return hedged percentage when available:
```rust
pub fn pnl_pct_effective(&self) -> Decimal {
    match self {
        TradeResult::Straddle(r) => r.hedged_pnl_pct.unwrap_or(r.pnl_pct),
        // ... other variants use regular pnl_pct
    }
}
```

### Approach 3: Separate Hedged Statistics Methods (Most Explicit)
Add parallel methods for hedged statistics:
- `pnl_pcts_hedged()`
- `mean_return_hedged()`
- `avg_winner_hedged()`
- etc.

Use these in CLI when `has_hedging()` is true.

## Recommendation

**Approach 2** is cleanest:
1. Add `hedged_pnl_pct` to `StraddleResult`
2. Calculate it when applying hedging
3. Add `pnl_pct_effective()` and `pnl_effective()` methods to `TradeResult`
4. Update statistics methods to use effective values
5. Update CLI display to use effective values

This keeps the change localized and maintains backward compatibility for non-hedged trades.

## Testing

After fix, verify with:
```bash
./target/debug/cs backtest \
  --earnings-file ./custom_earnings/PENG_2025.parquet \
  --symbols PENG \
  --start 2024-12-01 \
  --end 2025-12-31 \
  --spread straddle \
  --entry-time 15:45 \
  --straddle-entry-days 20 \
  --straddle-exit-days 1 \
  --hedge \
  --hedge-strategy time \
  --hedge-interval-hours 24 \
  --output test.json
```

Expected results should use hedged P&L for all statistics and sample trade display.

## Additional Notes

- The issue affects **straddle trades only** (other trade types don't support hedging yet)
- Win rate calculation is also affected - a trade profitable with options but unprofitable with hedge should count as a loser
- Consider whether "Mean Return" should be shown at all when entry capital varies due to hedging (different capital deployed per trade)
