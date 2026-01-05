# BUG: Straddle Expiration Selected BEFORE Earnings Date

## Status
**CRITICAL** - 2026-01-05

## Summary
The `StraddleStrategy` selects expirations based on DTE from entry date, but **does not verify the expiration is after the earnings date**. This results in all trades having expirations BEFORE earnings, which is fundamentally wrong for an earnings straddle strategy.

## Observed Behavior

All 4 PENG trades have expirations BEFORE earnings:

```
Trade 1:
  Entry:      2024-12-11
  Expiration: 2024-12-20  ⚠️
  Earnings:   2025-01-08
  Exit:       2025-01-07

Trade 2:
  Entry:      2025-03-05
  Expiration: 2025-03-21  ⚠️
  Earnings:   2025-04-02
  Exit:       2025-04-01

Trade 3:
  Entry:      2025-06-10
  Expiration: 2025-06-20  ⚠️
  Earnings:   2025-07-08
  Exit:       2025-07-07

Trade 4:
  Entry:      2025-09-09
  Expiration: 2025-09-19  ⚠️
  Earnings:   2025-10-07
  Exit:       2025-10-06
```

## Root Cause

**File**: `cs-domain/src/strike_selection/straddle.rs`
**Method**: `select_expiration()` (lines 35-47)

```rust
fn select_expiration(
    &self,
    expirations: &[NaiveDate],
) -> Option<NaiveDate> {
    expirations
        .iter()
        .filter(|&&exp| {
            let dte = (exp - self.entry_date).num_days() as i32;
            dte >= self.min_dte_from_entry
        })
        .min()  // Selects EARLIEST expiration with sufficient DTE from entry
        .copied()
}
```

**The Problem**:
1. Filters expirations by `DTE from entry >= min_dte_from_entry`
2. Returns the **earliest** (`.min()`) expiration that meets this criteria
3. **Never checks if expiration is after earnings date!**

## Example Failure

For October trade:
- Entry: 2025-09-09
- Earnings: 2025-10-07
- min_dte_from_entry: 7 (default)

Available expirations (hypothetical):
- 2025-09-19 (10 days from entry) ✓ meets min_dte
- 2025-10-17 (38 days from entry) ✓ meets min_dte

Current logic selects: **2025-09-19** (earliest with DTE >= 7)
But 2025-09-19 is BEFORE earnings (2025-10-07)!

Should select: **2025-10-17** (first expiration AFTER earnings)

## Expected Behavior

The `select_expiration` method should:
1. Filter expirations that are **AFTER** earnings date
2. Among those, select the first one with sufficient DTE from entry
3. If no expiration meets both criteria, return None

```rust
fn select_expiration(
    &self,
    expirations: &[NaiveDate],
    earnings_date: NaiveDate,  // NEW PARAMETER
) -> Option<NaiveDate> {
    expirations
        .iter()
        .filter(|&&exp| {
            // Must be AFTER earnings
            exp > earnings_date && {
                // AND have sufficient DTE from entry
                let dte = (exp - self.entry_date).num_days() as i32;
                dte >= self.min_dte_from_entry
            }
        })
        .min()  // Earliest expiration that meets both criteria
        .copied()
}
```

## Implications

This bug has **massive implications** for backtest results:

1. **Options expire worthless BEFORE earnings** - the entire point of the strategy is to capture earnings volatility, but the options expire before earnings happens!

2. **Exit timing is wrong** - Current exit is "1 day before earnings" but the options have already expired weeks earlier

3. **Hedge analysis is invalid** - The October trade analysis showing $2,432 hedge loss is based on a trade that doesn't make strategic sense

4. **All PENG results are invalid** - Every single trade in the current backtest is fundamentally flawed

## Fix Required

### Files to Modify:

**1. `cs-domain/src/strike_selection/straddle.rs`**

Update `select_expiration()` signature:
```rust
fn select_expiration(
    &self,
    expirations: &[NaiveDate],
    earnings_date: NaiveDate,  // ADD THIS
) -> Option<NaiveDate>
```

Update filter logic:
```rust
.filter(|&&exp| {
    exp > earnings_date && {
        let dte = (exp - self.entry_date).num_days() as i32;
        dte >= self.min_dte_from_entry
    }
})
```

Update `select_straddle()` call site:
```rust
let expiration = self.select_expiration(&chain_data.expirations, event.date)
    .ok_or(StrategyError::NoExpirations)?;
```

**2. Update Tests**

The test at line 114 `test_select_expiration_after_earnings` should be updated to pass earnings_date.

## Testing

After fix, run:
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
  --output peng_fixed.json
```

Verify in output that ALL expirations are AFTER earnings dates.

## Additional Notes

- This explains why the October "hedge loss" looked so bad - we're hedging a position that expired 3 weeks before we think we're exiting it!
- The current results showing option P&L of +$218 on October trade is meaningless because the options expired before earnings
- This bug affects **earnings straddle strategy only** - calendar spreads and iron butterflies are not affected
