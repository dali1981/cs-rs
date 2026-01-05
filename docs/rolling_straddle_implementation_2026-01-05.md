# Rolling Straddle Implementation Plan - 2026-01-05

## Overview

Implement a rolling straddle strategy that maintains ATM positions by periodically closing and reopening straddles. This is how real traders maintain maximum vega exposure over extended periods.

## Motivation

**Problem with Static Straddles:**
- Entry: ATM straddle, maximum vega (e.g., 0.051 per share)
- After 27 days: Position drifts ITM/OTM, vega drops to ~0.017
- Result: 67% loss in vega sensitivity, theta decay dominates

**Solution with Rolling:**
- Week 1: $20 straddle @ $19.91 (ATM)
- Week 2: Roll to $20 straddle @ $20.54 (back to ATM)
- Week 3: Roll to $20 straddle @ $20.14 (back to ATM)
- Result: Maintains high vega throughout campaign

## Implementation Phases

### Phase 1: Domain Layer - Roll Policy Extensions

**File: `cs-domain/src/roll/policy.rs`**

Add trader-friendly roll policies:

```rust
pub enum RollPolicy {
    None,

    /// Roll every week on specified day
    Weekly {
        roll_day: Weekday,  // e.g., Friday
        exit_at_expiry: bool,  // Close early if option expires
    },

    /// Roll every N trading days
    TradingDays {
        interval: u16,
    },

    // Keep existing variants (OnExpiration, DteThreshold, TimeInterval)
}
```

**Helper functions:**
- `next_weekday(from: NaiveDate, target: Weekday) -> NaiveDate`
- `add_trading_days(from: NaiveDate, days: i64) -> NaiveDate`

---

### Phase 2: Result Structures

**File: `cs-domain/src/entities/rolling_result.rs` (NEW)**

```rust
/// Result of a rolling straddle strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingStraddleResult {
    pub symbol: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub roll_policy: String,

    // Individual roll periods
    pub rolls: Vec<RollPeriod>,

    // Aggregated metrics
    pub total_option_pnl: Decimal,
    pub total_hedge_pnl: Decimal,
    pub total_transaction_cost: Decimal,
    pub total_pnl: Decimal,

    // Statistics
    pub num_rolls: usize,
    pub win_rate: f64,
    pub avg_roll_pnl: Decimal,
    pub max_drawdown: Decimal,
}

/// A single roll period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollPeriod {
    pub entry_date: NaiveDate,
    pub exit_date: NaiveDate,
    pub strike: Decimal,
    pub expiration: NaiveDate,

    pub entry_debit: Decimal,
    pub exit_credit: Decimal,
    pub pnl: Decimal,

    pub spot_at_entry: f64,
    pub spot_at_exit: f64,
    pub spot_move_pct: f64,

    pub iv_entry: f64,
    pub iv_exit: f64,
    pub iv_change: f64,

    pub net_delta: Option<f64>,
    pub net_vega: Option<f64>,

    pub hedge_pnl: Option<Decimal>,
    pub hedge_count: usize,

    pub roll_reason: RollReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RollReason {
    Scheduled,      // Normal weekly/monthly roll
    Expiry,         // Option expired before roll date
    EndOfCampaign,  // Reached target end date
}
```

---

### Phase 3: Rolling Executor

**File: `cs-backtest/src/rolling_straddle_executor.rs` (NEW)**

```rust
/// Executes a rolling straddle strategy
pub struct RollingStraddleExecutor<O, E> {
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    straddle_executor: StraddleExecutor<O, E>,
    roll_policy: RollPolicy,
    hedge_config: HedgeConfig,
}

impl<O, E> RollingStraddleExecutor<O, E> {
    pub fn new(
        options_repo: Arc<O>,
        equity_repo: Arc<E>,
        roll_policy: RollPolicy,
        hedge_config: HedgeConfig,
    ) -> Self;

    /// Execute rolling strategy from start_date to end_date
    pub async fn execute_rolling(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        entry_time: MarketTime,
        exit_time: MarketTime,
    ) -> RollingStraddleResult;
}
```

**Algorithm:**

```
1. current_date = start_date
2. While current_date < end_date:
   a. Find next roll date based on policy
   b. If roll_date > end_date: roll_date = end_date
   c. Select ATM straddle at current_date
   d. Find straddle expiration
   e. exit_date = min(roll_date, expiration)
   f. Execute straddle trade (current_date → exit_date)
   g. Record result in rolls vector
   h. current_date = exit_date + 1 day
3. Aggregate all rolls into final result
4. Return RollingStraddleResult
```

---

### Phase 4: CLI Integration

**File: `cs-cli/src/commands/backtest.rs`**

Add new flags:

```rust
#[arg(long, value_name = "STRATEGY")]
/// Rolling strategy: "none", "weekly", "days:N"
roll_strategy: Option<String>,

#[arg(long, value_name = "DAY")]
/// Day to roll on for weekly: "monday", "tuesday", ..., "friday"
roll_day: Option<String>,

#[arg(long)]
/// Exit early if option expires before next roll date
exit_at_expiry: bool,

#[arg(long, value_name = "DATE")]
/// Roll until this date (format: YYYY-MM-DD)
/// If not specified, uses --end date
roll_until: Option<String>,
```

**Parse logic:**

```rust
let roll_policy = match roll_strategy.as_deref() {
    Some("weekly") => {
        let weekday = parse_weekday(roll_day.as_deref().unwrap_or("friday"))?;
        RollPolicy::Weekly {
            roll_day: weekday,
            exit_at_expiry,
        }
    }
    Some(s) if s.starts_with("days:") => {
        let days: u16 = s.strip_prefix("days:").unwrap().parse()?;
        RollPolicy::TradingDays { interval: days }
    }
    _ => RollPolicy::None,
};
```

---

### Phase 5: Output Formatting

**File: `cs-cli/src/commands/display.rs`**

Add rolling results display:

```rust
fn display_rolling_result(result: &RollingStraddleResult) {
    println!("Rolling Straddle Results");
    println!("Symbol: {}", result.symbol);
    println!("Period: {} to {}", result.start_date, result.end_date);
    println!("Roll Policy: {}", result.roll_policy);
    println!("Number of Rolls: {}", result.num_rolls);
    println!();

    println!("Individual Roll Periods:");
    println!("{:>4} {:>10} {:>10} {:>8} {:>10} {:>10} {:>12}",
             "#", "Entry", "Exit", "Strike", "P&L", "Hedge", "Reason");

    for (i, roll) in result.rolls.iter().enumerate() {
        println!("{:>4} {:>10} {:>10} ${:>7.2} ${:>9.2} ${:>9.2} {:>12}",
                 i+1,
                 roll.entry_date,
                 roll.exit_date,
                 roll.strike,
                 roll.pnl,
                 roll.hedge_pnl.unwrap_or_default(),
                 format!("{:?}", roll.roll_reason));
    }

    println!();
    println!("Summary:");
    println!("  Total Option P&L:  ${:.2}", result.total_option_pnl);
    println!("  Total Hedge P&L:   ${:.2}", result.total_hedge_pnl);
    println!("  Transaction Cost:  ${:.2}", result.total_transaction_cost);
    println!("  Net P&L:           ${:.2}", result.total_pnl);
    println!("  Win Rate:          {:.1}%", result.win_rate * 100.0);
    println!("  Avg Roll P&L:      ${:.2}", result.avg_roll_pnl);
}
```

---

## Usage Examples

### Weekly Rolling Until Earnings

```bash
./target/debug/cs backtest \
  --earnings-file ./custom_earnings/PENG_2025.parquet \
  --symbols PENG \
  --start 2025-06-10 \
  --end 2025-07-07 \
  --spread straddle \
  --roll-strategy weekly \
  --roll-day friday \
  --exit-at-expiry \
  --hedge --hedge-strategy time --hedge-interval-hours 24 \
  --output ./peng_q3_weekly.json
```

**Expected Output:**
```
Rolling Straddle Results
Symbol: PENG
Period: 2025-06-10 to 2025-07-07
Roll Policy: weekly
Number of Rolls: 4

Individual Roll Periods:
   # Entry      Exit         Strike  P&L        Hedge      Reason
   1 2025-06-10 2025-06-13   $20.00  $-5.23     $+2.15     Scheduled
   2 2025-06-16 2025-06-20   $19.00  $+8.45     $-1.32     Expiry
   3 2025-06-23 2025-06-27   $20.00  $-3.12     $+0.89     Scheduled
   4 2025-06-30 2025-07-07   $20.00  $+12.34    $-4.21     EndOfCampaign

Summary:
  Total Option P&L:  $+12.44
  Total Hedge P&L:   $-2.49
  Net P&L:           $+9.95
  Win Rate:          50.0%
  Avg Roll P&L:      $+3.11
```

### Every 5 Trading Days

```bash
./target/debug/cs backtest \
  --symbols PENG \
  --start 2024-12-01 \
  --end 2025-12-31 \
  --spread straddle \
  --roll-strategy days:5 \
  --output ./peng_rolling_5day.json
```

---

## Testing Strategy

### Unit Tests

**File: `cs-backtest/src/rolling_straddle_executor.rs`**

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_weekly_roll_dates() {
        // Test that weekly rolls land on correct day
    }

    #[test]
    fn test_expiry_before_roll() {
        // Test that positions close at expiry if before roll date
    }

    #[test]
    fn test_roll_aggregation() {
        // Test that P&L aggregates correctly across rolls
    }
}
```

### Integration Tests

1. **Q3 Static vs Rolling:**
   - Run static (current): Expected -$9.29
   - Run weekly rolling: Expected improvement due to maintained vega

2. **Full Year Rolling:**
   - Run all 4 PENG earnings with weekly rolling
   - Compare total P&L to static strategy

---

## Files to Create/Modify

| File | Action | Lines | Description |
|------|--------|-------|-------------|
| `cs-domain/src/roll/policy.rs` | Modify | +50 | Add Weekly, TradingDays variants |
| `cs-domain/src/entities/rolling_result.rs` | Create | +150 | Rolling result structures |
| `cs-domain/src/entities/mod.rs` | Modify | +2 | Export rolling types |
| `cs-domain/src/lib.rs` | Modify | +1 | Re-export rolling types |
| `cs-backtest/src/rolling_straddle_executor.rs` | Create | +300 | Main rolling executor |
| `cs-backtest/src/lib.rs` | Modify | +2 | Export rolling executor |
| `cs-cli/src/commands/backtest.rs` | Modify | +80 | Add CLI flags and routing |
| `cs-cli/src/commands/display.rs` | Modify | +60 | Display rolling results |

**Total Estimate:** ~645 lines of new code

---

## Implementation Order

1. ✅ Write plan document
2. Domain: Add `RollPolicy::Weekly` and `RollPolicy::TradingDays`
3. Domain: Create `RollingStraddleResult` and `RollPeriod` structures
4. Backtest: Implement `RollingStraddleExecutor`
5. CLI: Add flags and parse logic
6. CLI: Add output formatting
7. Test: Run Q3 weekly rolling comparison
8. Document: Update README with rolling examples

---

## Expected Benefits

### Vega Maintenance
- **Static**: Vega degrades from 0.051 → 0.017 over 27 days (-67%)
- **Rolling**: Vega resets to ~0.05 each week (maintains high sensitivity)

### P&L Comparison (Q3 Example)
- **Static**: -$9.29 (theta dominates)
- **Rolling**: Expected +$15-25 (captures IV expansion with maintained vega)

### Risk Management
- Weekly rebalancing provides natural stop-loss points
- Can adjust strategy based on weekly performance
- Avoids concentration risk in single expiration

---

## Future Enhancements (Out of Scope)

1. **Conditional Rolling:**
   - Roll only if IV > threshold
   - Skip roll if position profitable

2. **Strike Selection:**
   - Roll to different deltas (e.g., 0.30 delta instead of ATM)
   - Dynamic strike selection based on IV skew

3. **Position Sizing:**
   - Scale position size based on IV level
   - Reduce size after losses

4. **Cost Optimization:**
   - Minimize transaction costs by selective rolling
   - Skip roll if cost > expected benefit
