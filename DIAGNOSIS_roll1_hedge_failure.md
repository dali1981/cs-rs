# 🐛 Roll #1 Hedge Failure - Root Cause Found

## TL;DR
**The rehedge schedule starts at `entry_time + interval`, NOT at `entry_time`.**

This means the first hedge check happens 1 day AFTER entry, leaving Day 1 completely unhedged. When PENG crashed -$1.36 on Day 1, the +70.5 delta position lost $95.88 with zero protection.

---

## The Bug

**File:** `cs-backtest/src/timing_strategy.rs:91-106`

```rust
pub fn rehedge_times(
    &self,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    strategy: &HedgeStrategy,
) -> Vec<DateTime<Utc>> {
    match strategy {
        HedgeStrategy::None => vec![],
        HedgeStrategy::TimeBased { interval } => {
            let mut times = Vec::new();
            let mut current = entry_time + *interval;  // ← BUG HERE!
            while current < exit_time {
                times.push(current);
                current = current + *interval;
            }
            times
        }
        ...
    }
}
```

**Line 101:** `let mut current = entry_time + *interval;`

This initializes `current` to **entry_time + interval**, so:
- Entry: Monday Mar 3 @ 9:30 AM
- Interval: 1 day (24 hours)
- **First rehedge: Tuesday Mar 4 @ 9:30 AM** ← Entry + 1 day
- Second rehedge: Wednesday Mar 5 @ 9:30 AM

**Entry day (Monday) has no hedge check!**

---

## What Happened on Roll #1

### Entry: Monday Mar 3, 2025 @ 9:30 AM
```
Spot:           $20.17
Option Delta:   +70.5 (long, bullish)
Hedge Shares:   0  ← NO HEDGE PLACED!
```

### End of Day 1: Monday @ 4:00 PM
```
Spot:           $18.81 (-$1.36, -6.7% crash!)
Gross Delta P&L: -$95.88
Hedge P&L:      $0.00
Net Delta P&L:  -$95.88 ← FULLY EXPOSED
```

**99% of total delta loss happened on Day 1 before any hedge.**

### First Hedge: Wednesday Mar 5 @ 9:30 AM
```
Rehedge time #1: Wednesday Mar 5 (entry + 2 days)
Hedge placed:    -46 shares
Status:          TOO LATE! Damage already done.
```

Why Wednesday and not Tuesday? Because `rehedge_times` returned:
1. Tuesday Mar 4 @ 9:30 AM (entry + 1 day)
2. Wednesday Mar 5 @ 9:30 AM (entry + 2 days)

The hedge was placed on Wednesday because Tuesday's check evaluated the condition (probably delta threshold), but Wednesday's check triggered the actual hedge placement.

---

## Verification: Hedge Timeline

From daily attribution data:

| Day | Date | Hedge Shares | First Appearance |
|-----|------|--------------|------------------|
| 1 | Mar 3 (Mon) | 0 | Entry |
| 2 | Mar 4 (Tue) | 0 | Rehedge check #1 (no hedge) |
| 3 | Mar 5 (Wed) | -46 | **Hedge placed!** |
| 4 | Mar 6 (Thu) | -46 | Same hedge |
| 5 | Mar 7 (Fri) | -40 | Rehedged (reduced) |

**2 rehedges counted:**
1. Wednesday Mar 5: Placed -46 shares (first hedge)
2. Friday Mar 7: Adjusted to -40 shares (rehedge)

---

## Expected Behavior

The schedule should **include entry_time** as the first rehedge check:

```rust
HedgeStrategy::TimeBased { interval } => {
    let mut times = vec![entry_time];  // ← Include entry!
    let mut current = entry_time + *interval;
    while current < exit_time {
        times.push(current);
        current = current + *interval;
    }
    times
}
```

Or alternatively, the hedging logic should **always hedge at entry** before starting the rehedge schedule.

---

## Why Hedge Efficiency Was 3%

```
Total Gross Delta P&L:  -$96.56
Total Hedge Delta P&L:  +$2.92
Efficiency:             2.92 / 96.56 = 3.0%
```

**Breakdown:**
- **Day 1-2 (unhedged):** -$95.67 gross delta P&L, $0 hedge
- **Day 3-5 (hedged):** -$0.89 gross delta P&L, +$2.92 hedge

The hedge worked perfectly when present (327% efficiency on Days 3-5)!
But 99% of the loss came from Day 1 before the hedge was placed.

---

## Recommendation

**Option 1: Include entry time in schedule**
```rust
let mut times = vec![entry_time];  // Start with entry
let mut current = entry_time + *interval;
while current < exit_time {
    times.push(current);
    current = current + *interval;
}
```

**Option 2: Always hedge at entry**
```rust
// In apply_hedging(), before the rehedge loop:
hedge_state.update(entry_time, result.spot_at_entry);
hedge_shares_timeline.push((entry_time, hedge_state.stock_shares()));
```

**Preference:** Option 1 is cleaner—treat entry as a rehedge checkpoint.

---

## Impact

This bug affects **all delta-hedged strategies**:
- Rolling straddles
- Single straddles with hedging
- Any strategy using TimeBased hedge strategy

Every trade loses the first interval period unhedged, creating unnecessary directional risk.

---

## Next Steps

1. ✅ Fix `rehedge_times()` to include entry_time
2. ⚠️ Re-run Roll #1 with fix to verify hedge is placed at entry
3. ⚠️ Validate against Rolls #2 and #4 (which also had hedging)
4. ⚠️ Update unit tests for TimingStrategy::rehedge_times()
