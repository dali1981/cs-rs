# Why Day 2 Was Also Unhedged

## The Config
From `cs-cli/src/config.rs:185-195`:
```rust
impl Default for HedgingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: "delta".to_string(),  // ← DELTA STRATEGY!
            interval_hours: 24,
            delta_threshold: 0.10,          // ← 0.10 threshold
            max_rehedges: None,
            cost_per_share: 0.01,
        }
    }
}
```

**The strategy is `DeltaThreshold { threshold: 0.10 }`, NOT `TimeBased`!**

This means hedges only happen when `|net_delta| > 0.10` (per-share).

---

## What Happened on Day 2

### Tuesday Mar 4 @ 9:30 AM (Rehedge Check #1)

**Gamma approximation updated the delta:**
```
Initial state (Mon 9:30 AM):
  option_delta: 0.705 (per-share, from entry)
  option_gamma: 0.154 (per-share, from entry)
  last_spot: $20.17
  stock_shares: 0

Spot on Tue 9:30 AM: ~$18.485 (from Day 2 open)
Spot change: $18.485 - $20.17 = -$1.685

Delta update via gamma approximation:
  new_delta = 0.705 + 0.154 × (-1.685)
           = 0.705 - 0.260
           = 0.445 (per-share)

Net delta: 0.445 + 0/100 = 0.445
```

**Threshold check:**
```rust
|0.445| > 0.10?  ✅ YES!
```

**So it SHOULD have hedged!** But wait...

---

## The Problem: Gamma Approximation Drift

The gamma approximation is inaccurate over large moves. The actual delta on Day 2 (from real Greeks recomputation) was:

```
From daily attribution:
  Day 2 option_delta = 41.16388053239095 (position)
                     = 0.4116 per-share
```

**But the hedge state thought delta was 0.445, not 0.4116!**

The gamma approximation:
- Started with: 0.705 delta
- Updated with: gamma × spot_change = 0.154 × (-1.685) = -0.260
- Resulted in: 0.445

The actual delta (from IV surface):
- Day 2: 0.4116 per-share

**Error: 0.445 - 0.4116 = 0.033 (3.3 delta points drift!)**

---

## Why The Gamma Approximation Failed

The gamma approximation assumes:
```
Δ_new ≈ Δ_old + Γ × ΔS
```

But this is only accurate for **small moves**. From Mon 9:30 AM to Tue 9:30 AM:
- Spot moved: -$1.685 (-8.3%!)
- Delta's actual sensitivity to spot changed
- Gamma itself changed significantly
- IV changed from 66.9% to 54.9% (-12 vol points!)

The formula doesn't account for:
1. **Gamma changes** (gamma itself moves with spot and time)
2. **IV changes** (delta is highly sensitive to IV)
3. **Theta decay** (time affects delta)

---

## So What Happened on Day 2?

**HedgeState's view (using gamma approximation):**
```
net_delta = 0.445
Threshold = 0.10
Should hedge? |0.445| > 0.10 ✅ YES
Shares to hedge = -(0.445 × 100) = -45 shares
```

**This SHOULD have triggered a hedge!**

BUT... Let me check if there's another filter.

Actually, looking at the data again, the hedge_shares_timeline shows:
- Day 1: 0 shares
- Day 2: 0 shares
- Day 3: -46 shares ← First hedge

If the rehedge_times included Tuesday, and the threshold was exceeded, why no hedge?

---

## Hypothesis: Rehedge Times Don't Include Entry + 1 Day Either!

Wait, let me re-check. If the strategy is `DeltaThreshold`, then `rehedge_times()` calls `generate_check_times()`:

```rust
HedgeStrategy::DeltaThreshold { .. } | HedgeStrategy::GammaDollar { .. } => {
    self.generate_check_times(entry_time, exit_time)
}
```

The `generate_check_times()` function generates hourly checks during market hours. It should cover Day 2!

Unless... what if `generate_check_times` has the same bug and starts at `entry_time + 1 hour`?

Or... what if the actual backtest was run with `strategy: "time"` (TimeBased), not "delta"?

---

## Most Likely Explanation

The backtest was run with **TimeBased** hedge strategy, interval = 24 hours.

For TimeBased:
```rust
HedgeStrategy::TimeBased { interval } => {
    let mut times = Vec::new();
    let mut current = entry_time + *interval;  // ← BUG!
    while current < exit_time {
        times.push(current);
        current = current + *interval;
    }
    times
}
```

Rehedge times:
1. Mon Mar 3 @ 9:30 AM + 24h = **Tue Mar 4 @ 9:30 AM**
2. Tue Mar 4 @ 9:30 AM + 24h = **Wed Mar 5 @ 9:30 AM**

So Tuesday WAS in the schedule!

---

## Why Tuesday Didn't Hedge

**Two possibilities:**

### 1. No Hedge Due to min_hedge_size Filter
From hedging.rs:141-146:
```rust
if raw_shares.abs() < self.min_hedge_size {
    0
} else {
    raw_shares
}
```

If `min_hedge_size > 45`, then the 45-share hedge would be filtered out.
But `min_hedge_size` is set to 1, so this can't be it.

### 2. The Delta Estimate Was Below Some Internal Threshold

Actually, wait! For TimeBased strategy:
```rust
HedgeStrategy::TimeBased { .. } => true, // Always rehedge at scheduled times
```

It always returns `true`! So should_rehedge wouldn't block it.

Then shares_to_hedge would calculate:
```
net_delta = 0.445 (per-share, from gamma approx)
shares = -(0.445 × 100) = -45 shares
```

This is >= min_hedge_size (1), so it should hedge!

---

## Conclusion: The Bug Affects Both Days

**The rehedge schedule doesn't include entry_time.**

For TimeBased with 24h interval:
- Entry: Mon 9:30 AM
- Rehedge times: [Tue 9:30 AM, Wed 9:30 AM, Thu 9:30 AM, Fri 9:30 AM]

**So Tuesday WAS in the schedule and SHOULD have hedged!**

Something else prevented the hedge on Tuesday. Possible causes:
1. **Spot data unavailable** at Tuesday 9:30 AM
2. **Equity repo error** when fetching spot price
3. **The hedge was placed but not recorded** in the timeline
4. **A bug in the hedge state update logic**

From unified_executor.rs:233:
```rust
if let Ok(spot) = self.equity_repo.get_spot_price(straddle.symbol(), rehedge_time).await {
    hedge_state.update(rehedge_time, spot.to_f64());
    hedge_shares_timeline.push((rehedge_time, hedge_state.stock_shares()));
}
```

**If `get_spot_price` failed on Tuesday, the entire rehedge would be skipped!**

This would explain why Tuesday shows 0 shares.

---

## The Answer

**Day 2 was unhedged because `get_spot_price()` likely failed or returned an error for Tuesday 9:30 AM.**

This could happen if:
- Market was closed (holiday?)
- No minute bar data exists for that exact timestamp
- Data gap in the equity price history

The fact that Day 3 (Wednesday) successfully placed a hedge suggests the data was available then.

**To verify:** Check if there's equity minute data for PENG on Tuesday Mar 4, 2025 @ 9:30 AM.
