# 🐛 Strike Selection Bug: Using Future Data

## The Problem

**The straddle entered with +70.5 delta because it used END-OF-DAY spot price to select the strike, then entered at BEGINNING-OF-DAY with a different spot.**

---

## The Bug

From `rolling_straddle_executor.rs:126-142`:

```rust
async fn find_atm_straddle(&self, symbol: &str, date: NaiveDate) -> Result<Straddle, String> {
    // Use 3:45pm ET as reference time for querying market data
    let dt = self.to_datetime(date, MarketTime { hour: 15, minute: 45 });  // ← 3:45 PM!

    // Require options to expire at least 1 day after entry
    let min_expiration = date + chrono::Duration::days(1);

    // Delegate to factory - uses REAL expirations from market data
    self.trade_factory
        .create_atm_straddle(symbol, dt, min_expiration)
        .await
}
```

Then later in `execute_rolling()`:

```rust
// Determine exit date for this roll
let (exit_date, roll_reason) = self.determine_exit_date(...);

// Execute this leg
let entry_dt = self.to_datetime(current_date, entry_time);  // ← Uses entry_time parameter!
let exit_dt = self.to_datetime(exit_date, exit_time);
```

---

## What Happened on Roll #1

### Selection Phase (3:45 PM ET)
```
Date: Monday March 3, 2025 @ 3:45 PM
Spot: Unknown (probably ~$18)
Strike selected: $17.50 (ATM at 3:45 PM)
```

### Entry Phase (9:30 AM ET)
```
Date: Monday March 3, 2025 @ 9:30 AM  ← SAME DAY, EARLIER TIME!
Spot: $20.17
Strike: $17.50 (selected using future data)
Delta: +70.5 (deeply ITM!)
```

**The trade is selected using data from 3:45 PM, but entered at 9:30 AM!**

This is **look-ahead bias** - using future information to make past decisions.

---

## Why This Causes Problems

At **9:30 AM** on March 3:
- Spot was $20.17
- ATM strike should have been ~$20
- But strike was $17.50 (selected from 3:45 PM data)

At **3:45 PM** on March 3:
- Spot probably dropped to ~$18 (based on the $18.81 close)
- $17.50 was ATM at that time

**Result:** The straddle entered $2.67 ITM instead of ATM, giving +70.5 delta instead of ~0.

---

## The Fix

**Option 1: Use entry_time for both selection AND entry**
```rust
async fn find_atm_straddle(&self, symbol: &str, date: NaiveDate) -> Result<Straddle, String> {
    // Use entry_time to select strike (not 3:45 PM)
    let dt = self.to_datetime(date, self.entry_time);  // Need to pass entry_time to this method

    let min_expiration = date + chrono::Duration::days(1);

    self.trade_factory
        .create_atm_straddle(symbol, dt, min_expiration)
        .await
}
```

**Option 2: Select strike on PREVIOUS day's close**
```rust
async fn find_atm_straddle(&self, symbol: &str, date: NaiveDate) -> Result<Straddle, String> {
    // Use previous trading day's close to avoid look-ahead
    let prev_day = TradingCalendar::prev_trading_day(date);
    let dt = self.to_datetime(prev_day, MarketTime { hour: 16, minute: 0 });  // 4:00 PM close

    let min_expiration = date + chrono::Duration::days(1);

    self.trade_factory
        .create_atm_straddle(symbol, dt, min_expiration)
        .await
}
```

---

## Recommendation

**Use Option 1** - select the strike at the **same time** you enter:
1. Pass `entry_time` to `find_atm_straddle()`
2. Use `entry_time` for strike selection (not 3:45 PM)
3. Enter the trade at `entry_time`

This ensures:
- No look-ahead bias
- Strike is truly ATM at entry
- Delta starts near 0 (neutral volatility trade)
- No need to hedge immediately at entry (you're already delta-neutral)

---

## User's Suggestion

> "better not hedge right at the market open. let s not open a position just at market open also. 10AM ET sounds fine."

✅ **Agreed!** Enter at 10:00 AM ET instead of 9:30 AM:
- Avoids opening auction volatility
- More stable spot price
- Better liquidity
- Strike selection and entry use same stable price

```rust
// Use 10:00 AM for both selection and entry
let entry_time = MarketTime { hour: 10, minute: 0 };
```

---

## Impact

This bug affects **every roll** in a rolling straddle strategy:
- All rolls enter with non-zero delta
- Directional bias instead of volatility-neutral
- Requires immediate hedging to fix what should have been neutral
- Defeats the purpose of a volatility strategy

This is actually **worse than the hedging bug** because it fundamentally breaks the strategy design.
