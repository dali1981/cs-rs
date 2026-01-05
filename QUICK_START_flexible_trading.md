# Quick Start: Flexible Trading Period System

**Date**: 2026-01-05

## TL;DR

The new flexible trading system lets you specify:
- **WHAT**: Straddle, Calendar Spread, etc.
- **WHEN**: Pre-earnings, Post-earnings, Fixed dates
- **HOW LONG**: Weekly, Monthly, or custom expiration cycles
- **ROLL**: Auto-renew positions weekly/monthly

---

## Common Use Cases

### 1. Pre-Earnings Straddle (IV Expansion)

Buy straddle 20 days before earnings, sell 1 day before:

```rust
use cs_domain::strategy::presets::pre_earnings_straddle;

let strategy = pre_earnings_straddle();
```

**What it does**:
- Entry: 20 trading days before earnings
- Exit: 1 trading day before earnings
- Expiration: First available after exit date
- No hedging

---

### 2. Pre-Earnings Straddle with Delta Hedging

Same as above, but with systematic delta hedging:

```rust
use cs_domain::strategy::presets::pre_earnings_straddle_hedged;

let strategy = pre_earnings_straddle_hedged();
```

**What it does**:
- Entry: 20 trading days before earnings
- Exit: 1 trading day before earnings
- Expiration: First available after exit date
- **Hedging**: Rehedge when |delta| > 0.15

---

### 3. Weekly Roll Straddle

Buy weekly straddle, roll to next weekly each week:

```rust
use cs_domain::strategy::presets::weekly_roll_straddle;

let strategy = weekly_roll_straddle();
```

**What it does**:
- Entry: 28 trading days (~4 weeks) before earnings
- Expiration: Prefer weekly expirations
- **Roll**: On each weekly expiration, roll to next weekly
- Exit: 1 day before earnings

---

### 4. Post-Earnings Straddle (Momentum)

Enter after earnings, capture momentum:

```rust
use cs_domain::strategy::presets::post_earnings_straddle;

let strategy = post_earnings_straddle();
```

**What it does**:
- Entry: Day after earnings (AMC) or same day (BMO)
- Exit: 5 trading days after entry
- Expiration: First available after exit

---

### 5. Earnings Calendar Spread (IV Crush)

Monthly-only calendar spread crossing earnings:

```rust
use cs_domain::strategy::presets::earnings_calendar_spread;
use finq_core::OptionType;

let strategy = earnings_calendar_spread(OptionType::Call);
```

**What it does**:
- Entry: 1 day before earnings
- Exit: 1 day after earnings
- Short: First monthly after earnings
- Long: Second monthly after earnings
- Filter: Only enter if short IV > long IV * 1.1

---

## Building Custom Strategies

### Example: Custom Pre-Earnings Window

Enter 15 days before, exit 2 days before:

```rust
use cs_domain::{
    TradeStrategy, TradeStructureConfig,
    TradingPeriodSpec, ExpirationPolicy,
};
use chrono::NaiveTime;

let strategy = TradeStrategy::new(TradeStructureConfig::Straddle)
    .with_timing(TradingPeriodSpec::PreEarnings {
        entry_days_before: 15,
        exit_days_before: 2,
        entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
        exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
    })
    .with_expiration_policy(ExpirationPolicy::FirstAfter {
        min_date: chrono::NaiveDate::MIN, // Will be set dynamically
    });
```

---

### Example: Target Specific DTE

Target 45 DTE (+/- 5 days tolerance):

```rust
use cs_domain::{
    TradeStrategy, TradeStructureConfig,
    TradingPeriodSpec, ExpirationPolicy,
};
use chrono::{NaiveDate, NaiveTime};

let entry_date = NaiveDate::from_ymd_opt(2025, 10, 1).unwrap();

let strategy = TradeStrategy::new(TradeStructureConfig::Straddle)
    .with_timing(TradingPeriodSpec::HoldingPeriod {
        entry_date,
        holding_days: 20,
        entry_time: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        exit_time: NaiveTime::from_hms_opt(15, 30, 0).unwrap(),
    })
    .with_expiration_policy(ExpirationPolicy::TargetDte {
        target_dte: 45,
        tolerance: 5,
        entry_date,
    });
```

---

### Example: Roll When DTE < 7

Roll position when DTE drops below 7 days:

```rust
use cs_domain::{
    TradeStrategy, TradeStructureConfig,
    RollPolicy, ExpirationPolicy,
    TradingPeriodSpec,
};

let strategy = TradeStrategy::new(TradeStructureConfig::Straddle)
    .with_timing(TradingPeriodSpec::pre_earnings_default())
    .with_expiration_policy(ExpirationPolicy::PreferMonthly {
        min_date: chrono::NaiveDate::MIN,
        months_out: 0,
    })
    .with_roll_policy(RollPolicy::DteThreshold {
        min_dte: 7,
        to_policy: ExpirationPolicy::PreferMonthly {
            min_date: chrono::NaiveDate::MIN,
            months_out: 0,
        },
    });
```

---

## Core Types Reference

### TradeStructureConfig

What to trade:

```rust
enum TradeStructureConfig {
    Straddle,
    CalendarSpread { option_type: OptionType, strike_match: StrikeMatchMode },
    CalendarStraddle,
    IronButterfly { wing_width: Decimal },
}
```

### TradingPeriodSpec

When to enter/exit:

```rust
enum TradingPeriodSpec {
    PreEarnings { entry_days_before: u16, exit_days_before: u16, ... },
    PostEarnings { entry_offset: i16, holding_days: u16, ... },
    CrossEarnings { entry_days_before: u16, exit_days_after: u16, ... },
    FixedDates { entry_date: NaiveDate, exit_date: NaiveDate, ... },
    HoldingPeriod { entry_date: NaiveDate, holding_days: u16, ... },
}
```

### ExpirationPolicy

Which expiration to select:

```rust
enum ExpirationPolicy {
    FirstAfter { min_date: NaiveDate },
    PreferWeekly { min_date: NaiveDate, fallback_to_monthly: bool },
    PreferMonthly { min_date: NaiveDate, months_out: u8 },
    TargetDte { target_dte: i32, tolerance: i32, entry_date: NaiveDate },
    Calendar { short: Box<ExpirationPolicy>, long: Box<ExpirationPolicy> },
}
```

### RollPolicy

When to roll position:

```rust
enum RollPolicy {
    None,
    OnExpiration { to_next: ExpirationPolicy },
    DteThreshold { min_dte: i32, to_policy: ExpirationPolicy },
    TimeInterval { interval_days: u16, to_policy: ExpirationPolicy },
}
```

---

## Expiration Cycle Detection

The system automatically classifies expirations:

```rust
use cs_domain::ExpirationCycle;
use chrono::NaiveDate;

let date = NaiveDate::from_ymd_opt(2025, 10, 17).unwrap(); // 3rd Friday
let cycle = ExpirationCycle::classify(date);
// -> ExpirationCycle::Monthly

let date = NaiveDate::from_ymd_opt(2025, 10, 24).unwrap(); // 4th Friday
let cycle = ExpirationCycle::classify(date);
// -> ExpirationCycle::Weekly
```

**Classification Rules**:
- **Weekly**: Any Friday except 3rd Friday
- **Monthly**: 3rd Friday of any month (except quarterly)
- **Quarterly**: 3rd Friday of Mar/Jun/Sep/Dec
- **LEAPS**: January expiration 1+ year out
- **Non-Standard**: Not a Friday

---

## Hedging Configuration

Add delta hedging to any strategy:

```rust
use cs_domain::{HedgeConfig, HedgeStrategy};
use rust_decimal::Decimal;

let hedge_config = HedgeConfig {
    strategy: HedgeStrategy::DeltaThreshold { threshold: 0.10 },
    max_rehedges: Some(15),
    min_hedge_size: 10,
    transaction_cost_per_share: Decimal::new(1, 2), // $0.01
    contract_multiplier: 100,
};

let strategy = pre_earnings_straddle()
    .with_hedge_config(hedge_config);
```

**Hedge Strategies**:
- `DeltaThreshold { threshold }` - Rehedge when |delta| > threshold
- `GammaDollar { threshold }` - Rehedge when gamma $ exposure > threshold
- `TimeBased { interval }` - Rehedge at fixed time intervals
- `None` - No hedging

---

## Filters

Add entry filters to any strategy:

```rust
use cs_domain::TradeFilters;

let filters = TradeFilters {
    min_iv: Some(0.30),           // 30% min IV
    max_iv: Some(2.0),            // 200% max IV
    min_iv_ratio: Some(1.1),      // Short IV > Long IV * 1.1 (calendars)
    min_volume: Some(100),         // Min 100 contracts volume
    max_bid_ask_pct: Some(0.10),  // Max 10% bid-ask spread
};

let strategy = pre_earnings_straddle()
    .with_filters(filters);
```

---

## Integration with Backtest (Future)

Once integrated with `UnifiedExecutor`, usage will look like:

```rust
// Create strategy
let strategy = weekly_roll_straddle();

// Execute backtest
let result = executor.execute_strategy(
    &strategy,
    event,
    entry_surface,
).await;
```

---

## Design Decisions Summary

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Weekly detection** | Calendar logic (non-3rd Fridays) | Simple, predictable |
| **Roll execution** | Single trade with roll events | Best for tracking P&L |
| **Hedging during rolls** | Close hedges on roll | Simpler accounting |

---

## Common Patterns

### Pattern 1: Pre-Earnings + Weeklies

```rust
let strategy = TradeStrategy::new(TradeStructureConfig::Straddle)
    .with_timing(TradingPeriodSpec::pre_earnings_default())
    .with_expiration_policy(ExpirationPolicy::prefer_weekly(NaiveDate::MIN));
```

### Pattern 2: Post-Earnings + Monthlies

```rust
let strategy = TradeStrategy::new(TradeStructureConfig::Straddle)
    .with_timing(TradingPeriodSpec::post_earnings_default())
    .with_expiration_policy(ExpirationPolicy::prefer_monthly(NaiveDate::MIN, 0));
```

### Pattern 3: Calendar Spread + Roll Monthly

```rust
use finq_core::OptionType;

let strategy = TradeStrategy::new(TradeStructureConfig::CalendarSpread {
    option_type: OptionType::Call,
    strike_match: StrikeMatchMode::SameStrike,
})
.with_timing(TradingPeriodSpec::cross_earnings_default())
.with_expiration_policy(ExpirationPolicy::Calendar {
    short: Box::new(ExpirationPolicy::prefer_monthly(NaiveDate::MIN, 0)),
    long: Box::new(ExpirationPolicy::prefer_monthly(NaiveDate::MIN, 1)),
})
.with_roll_policy(RollPolicy::OnExpiration {
    to_next: ExpirationPolicy::prefer_monthly(NaiveDate::MIN, 0),
});
```

---

## FAQ

### Q: How do I avoid weeklies and only use monthlies?

```rust
.with_expiration_policy(ExpirationPolicy::PreferMonthly {
    min_date: NaiveDate::MIN,
    months_out: 0,
})
```

### Q: How do I roll weekly positions?

```rust
.with_roll_policy(RollPolicy::OnExpiration {
    to_next: ExpirationPolicy::prefer_weekly(NaiveDate::MIN),
})
```

### Q: How do I use a fixed date range (not earnings)?

```rust
.with_timing(TradingPeriodSpec::FixedDates {
    entry_date: NaiveDate::from_ymd_opt(2025, 10, 1).unwrap(),
    exit_date: NaiveDate::from_ymd_opt(2025, 10, 31).unwrap(),
    entry_time: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
    exit_time: NaiveTime::from_hms_opt(15, 30, 0).unwrap(),
})
```

### Q: How do I add delta hedging?

```rust
use cs_domain::{HedgeConfig, HedgeStrategy};

.with_hedge_config(HedgeConfig {
    strategy: HedgeStrategy::DeltaThreshold { threshold: 0.15 },
    max_rehedges: Some(10),
    ..Default::default()
})
```

---

## Next Steps

1. **Try the presets**: Start with `pre_earnings_straddle()` or `weekly_roll_straddle()`
2. **Customize**: Use the builder pattern to adjust timing, expirations, or rolls
3. **Add hedging**: Experiment with delta hedging configurations
4. **Test**: Run backtests to validate your strategies

For more details, see:
- `IMPLEMENTATION_PLAN_flexible_trading.md` - Full implementation plan
- `IMPLEMENTATION_COMPLETE.md` - Implementation summary
- `RESEARCH_flexible_trading_periods.md` - Original research and design
