# Implementation Plan: Post-Earnings Long Straddle Strategy

## Overview

Add a new strategy that buys ATM long straddles the day **after** earnings and holds for ~1 week. This captures potential continued stock movement after earnings while avoiding the elevated pre-earnings IV premium.

**Rationale:**
- Post-earnings, IV has crushed → cheaper straddle entry
- Stocks often continue moving in days after earnings (trend continuation or mean reversion)
- 1-week holding provides time for directional moves to materialize

**Key Insight:** Entry date for post-earnings straddle = exit date of `EarningsTradeTiming`

---

## 1. Domain Layer Changes

### 1.1 Add `n_trading_days_after()` to TradingCalendar
**File:** `cs-domain/src/services/trading_calendar.rs`

Add method after `n_trading_days_before()`:

```rust
/// Get N trading days after a date
///
/// Example: n_trading_days_after(2025-01-10, 5)
///          -> 2025-01-17 (skipping weekends)
pub fn n_trading_days_after(date: NaiveDate, n: usize) -> NaiveDate {
    let mut result = date;
    let mut count = 0;
    while count < n {
        result = Self::next_trading_day(result);
        count += 1;
    }
    result
}
```

**Tests to add:**
```rust
#[test]
fn test_n_trading_days_after_same_week() {
    // Monday + 4 days = Friday
    let monday = NaiveDate::from_ymd_opt(2025, 6, 2).unwrap();
    let result = TradingCalendar::n_trading_days_after(monday, 4);
    assert_eq!(result, NaiveDate::from_ymd_opt(2025, 6, 6).unwrap());
}

#[test]
fn test_n_trading_days_after_with_weekend() {
    // Friday + 3 days = Wednesday (skip weekend)
    let friday = NaiveDate::from_ymd_opt(2025, 6, 6).unwrap();
    let result = TradingCalendar::n_trading_days_after(friday, 3);
    assert_eq!(result, NaiveDate::from_ymd_opt(2025, 6, 11).unwrap());
}
```

### 1.2 Create `PostEarningsStraddleTiming` Service
**New file:** `cs-domain/src/services/post_earnings_timing.rs`

```rust
use chrono::{DateTime, NaiveDate, Utc};
use crate::datetime::eastern_to_utc;
use crate::entities::EarningsEvent;
use crate::value_objects::{EarningsTime, TimingConfig};
use crate::services::TradingCalendar;

/// Calculates entry/exit timing for post-earnings straddle trades
///
/// Unlike EarningsTradeTiming (which enters BEFORE earnings for IV crush plays),
/// this service enters AFTER earnings to capture continued momentum while
/// benefiting from lower IV entry prices.
pub struct PostEarningsStraddleTiming {
    config: TimingConfig,
    holding_days: usize,  // Default: 5 (one trading week)
}

impl PostEarningsStraddleTiming {
    pub fn new(config: TimingConfig) -> Self {
        Self {
            config,
            holding_days: 5,
        }
    }

    pub fn with_holding_days(mut self, days: usize) -> Self {
        self.holding_days = days;
        self
    }

    /// Entry date: Day AFTER earnings announcement
    ///
    /// - BMO: Same day as earnings (earnings already happened before open)
    /// - AMC: Next trading day (earnings happened after previous close)
    /// - Unknown: Default to AMC behavior (next day)
    pub fn entry_date(&self, event: &EarningsEvent) -> NaiveDate {
        match event.earnings_time {
            EarningsTime::BeforeMarketOpen => {
                // Earnings happened before market open, can enter same day
                event.earnings_date
            }
            EarningsTime::AfterMarketClose | EarningsTime::Unknown => {
                // Earnings happened after close, enter next day
                TradingCalendar::next_trading_day(event.earnings_date)
            }
        }
    }

    /// Exit date: N trading days after entry (default: 5 = 1 week)
    pub fn exit_date(&self, event: &EarningsEvent) -> NaiveDate {
        let entry = self.entry_date(event);
        TradingCalendar::n_trading_days_after(entry, self.holding_days)
    }

    pub fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let entry_date = self.entry_date(event);
        eastern_to_utc(entry_date, self.config.entry_time())
    }

    pub fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let exit_date = self.exit_date(event);
        eastern_to_utc(exit_date, self.config.exit_time())
    }

    /// Get holding period in trading days
    pub fn holding_period(&self) -> usize {
        self.holding_days
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_timing() -> PostEarningsStraddleTiming {
        PostEarningsStraddleTiming::new(TimingConfig {
            entry_hour: 9,
            entry_minute: 35,
            exit_hour: 10,
            exit_minute: 0,
        })
    }

    #[test]
    fn test_amc_entry_next_day() {
        let timing = default_timing().with_holding_days(5);
        let event = EarningsEvent::new(
            "AAPL".into(),
            NaiveDate::from_ymd_opt(2025, 1, 30).unwrap(),  // Thursday AMC
            EarningsTime::AfterMarketClose,
        );

        // Entry: Next day (Friday Jan 31)
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 1, 31).unwrap());

        // Exit: 5 trading days later = Friday Feb 7
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 2, 7).unwrap());

        assert_eq!(timing.holding_period(), 5);
    }

    #[test]
    fn test_bmo_entry_same_day() {
        let timing = default_timing().with_holding_days(5);
        let event = EarningsEvent::new(
            "AAPL".into(),
            NaiveDate::from_ymd_opt(2025, 2, 3).unwrap(),  // Monday BMO
            EarningsTime::BeforeMarketOpen,
        );

        // Entry: Same day (Monday Feb 3)
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 2, 3).unwrap());

        // Exit: 5 trading days later = Monday Feb 10
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 2, 10).unwrap());
    }

    #[test]
    fn test_friday_amc_enters_monday() {
        let timing = default_timing().with_holding_days(5);
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),  // Friday AMC
            EarningsTime::AfterMarketClose,
        );

        // Entry: Monday Nov 10 (skip weekend)
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 10).unwrap());

        // Exit: Friday Nov 14
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 11, 14).unwrap());
    }

    #[test]
    fn test_unknown_defaults_to_amc() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 3).unwrap(),
            EarningsTime::Unknown,
        );

        // Should behave like AMC: entry next day
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 4).unwrap());
    }
}
```

### 1.3 Export New Module
**File:** `cs-domain/src/services/mod.rs`

Add after existing timing modules:

```rust
mod post_earnings_timing;
pub use post_earnings_timing::PostEarningsStraddleTiming;
```

---

## 2. Backtest Config Changes

### 2.1 Add SpreadType Variant
**File:** `cs-backtest/src/config.rs`

Update `SpreadType` enum (around line 95):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpreadType {
    #[default]
    Calendar,
    IronButterfly,
    Straddle,
    CalendarStraddle,
    /// Post-earnings straddle: enter day after earnings, hold for ~1 week
    PostEarningsStraddle,
}
```

Update `from_string()` method (around line 106):

```rust
impl SpreadType {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().replace('-', "_").as_str() {
            "iron_butterfly" | "ironbutterfly" | "butterfly" => SpreadType::IronButterfly,
            "straddle" | "long_straddle" => SpreadType::Straddle,
            "calendar_straddle" | "calendarstraddle" => SpreadType::CalendarStraddle,
            "post_earnings_straddle" | "postearningstraddle" | "post_straddle" => SpreadType::PostEarningsStraddle,
            _ => SpreadType::Calendar,
        }
    }
}
```

### 2.2 Add Config Parameters
**File:** `cs-backtest/src/config.rs`

Add field to `BacktestConfig` struct (around line 61):

```rust
/// Post-earnings straddle: holding period in trading days (default: 5)
#[serde(default = "default_post_earnings_holding_days")]
pub post_earnings_holding_days: usize,
```

Add default function (around line 90):

```rust
fn default_post_earnings_holding_days() -> usize {
    5  // 1 trading week
}
```

Update `Default` impl for `BacktestConfig` (around line 165):

```rust
impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            // ... existing fields
            post_earnings_holding_days: default_post_earnings_holding_days(),
        }
    }
}
```

---

## 3. Backtest Execution Changes

### 3.1 Add Execution Branch
**File:** `cs-backtest/src/backtest_use_case.rs`

Add to the match statement in `execute()` (around line 133-150):

```rust
match self.config.spread {
    SpreadType::Calendar => { /* ... */ }
    SpreadType::IronButterfly => { /* ... */ }
    SpreadType::Straddle => { /* ... */ }
    SpreadType::CalendarStraddle => { /* ... */ }
    SpreadType::PostEarningsStraddle => {
        self.execute_post_earnings_straddle(session_date).await
    }
}
```

### 3.2 Implement Execution Method

Add new method to `BacktestUseCase` (after `execute_straddle`, around line 900):

```rust
async fn execute_post_earnings_straddle(
    &self,
    session_date: NaiveDate,
) -> Vec<TradeResult> {
    // Create timing service
    let timing = PostEarningsStraddleTiming::new(self.config.timing.clone())
        .with_holding_days(self.config.post_earnings_holding_days);

    // Load earnings events (check entry window around session_date)
    let events = self.load_earnings_window(session_date).await;

    // Filter for events entering today
    let events_to_enter: Vec<_> = events
        .into_iter()
        .filter(|e| timing.entry_date(e) == session_date)
        .filter(|e| self.passes_market_cap_filter(e))
        .collect();

    if events_to_enter.is_empty() {
        return Vec::new();
    }

    // Process each event
    let mut results = Vec::new();
    for event in events_to_enter {
        match self.process_post_earnings_straddle(&event, &timing).await {
            Ok(result) => results.push(result),
            Err(e) => {
                log::warn!(
                    "Failed to process post-earnings straddle for {} on {}: {}",
                    event.symbol,
                    event.earnings_date,
                    e
                );
            }
        }
    }

    results
}

async fn process_post_earnings_straddle(
    &self,
    event: &EarningsEvent,
    timing: &PostEarningsStraddleTiming,
) -> Result<TradeResult, Box<dyn std::error::Error>> {
    let symbol = &event.symbol;
    let entry_time = timing.entry_datetime(event);
    let exit_time = timing.exit_datetime(event);

    // Get spot price at entry
    let spot = self.get_spot_price(symbol, entry_time).await?;

    // Get option chain at entry
    let chain = self.get_option_chain(symbol, entry_time).await?;

    // Build IV surface (minute-aligned for accurate pricing)
    let iv_surface = build_iv_surface_minute_aligned(
        &chain,
        symbol,
        entry_time,
        self.config.vol_model,
        &self.finq_client,
    )
    .await?;

    // Get available expirations and strikes
    let expirations = extract_expirations(&chain);
    let strikes = extract_strikes(&chain);

    let option_data = OptionChainData {
        expirations,
        strikes,
        iv_surface: Some(iv_surface),
    };

    // Create strategy
    let strategy = StraddleStrategy::new(self.config.min_straddle_dte);

    // Select straddle
    let straddle = strategy.select(
        symbol.clone(),
        spot,
        &option_data,
        entry_time.date_naive(),
    )?;

    // Execute trade using StraddleExecutor
    let executor = StraddleExecutor::new(
        self.finq_client.clone(),
        self.config.pricing_model,
        self.config.vol_model,
    );

    let result = executor
        .execute(
            straddle,
            event.clone(),
            entry_time,
            exit_time,
            spot,
        )
        .await?;

    Ok(TradeResult::Straddle(result))
}
```

---

## 4. CLI Changes

### 4.1 Add CLI Arguments
**File:** `cs-cli/src/cli_args.rs`

Add to `BacktestArgs` struct (around line 60-80):

```rust
/// Holding period for post-earnings straddle in trading days (default: 5)
#[arg(long, default_value = "5")]
pub post_earnings_holding_days: usize,
```

Update config construction in `main.rs` or wherever args are mapped to config:

```rust
post_earnings_holding_days: args.post_earnings_holding_days,
```

---

## 5. Files Summary

| Layer | File | Action | Priority |
|-------|------|--------|----------|
| Domain | `cs-domain/src/services/trading_calendar.rs` | Add `n_trading_days_after()` + tests | 1 |
| Domain | `cs-domain/src/services/post_earnings_timing.rs` | **NEW** - Create timing service | 2 |
| Domain | `cs-domain/src/services/mod.rs` | Export new module | 3 |
| Config | `cs-backtest/src/config.rs` | Add enum variant + config field | 4 |
| Backtest | `cs-backtest/src/backtest_use_case.rs` | Add execution methods | 5 |
| CLI | `cs-cli/src/cli_args.rs` | Add CLI argument | 6 |

---

## 6. Testing Strategy

### Unit Tests
1. `TradingCalendar::n_trading_days_after()` - various scenarios
2. `PostEarningsStraddleTiming` - all earnings timing combinations
3. `SpreadType::from_string()` - verify parsing

### Integration Tests
```bash
# Test with known earnings event
cargo run --bin cs backtest \
    --spread post-earnings-straddle \
    --symbols AAPL \
    --start-date 2024-01-01 \
    --end-date 2024-03-31 \
    --post-earnings-holding-days 5
```

### Build Commands
```bash
# Clean build
cargo clean
cargo build --release

# Run tests
cargo test --workspace

# Run specific tests
cargo test -p cs-domain trading_calendar
cargo test -p cs-domain post_earnings_timing
```

---

## 7. Design Decisions & Trade-offs

### ✅ Decided

1. **Naming:** `PostEarningsStraddle` - clear and descriptive
2. **Entry timing:** Same as other strategies (9:35 ET default) - consistent behavior
3. **Holding period:** Default 5 days (1 trading week) - configurable via CLI
4. **Expiration selection:** Reuse `min_straddle_dte` (default 7 days) - sufficient for 5-day hold
5. **Executor:** Reuse `StraddleExecutor` - pricing logic is identical

### 🤔 Open Questions

1. **IV filtering:** Should we filter if post-earnings IV is still > threshold?
   - Decision: Use existing `max_entry_iv` if configured

2. **Movement filter:** Only enter if stock moved > X% on earnings?
   - Decision: Not implementing initially - can add later if needed

3. **Expiration buffer:** Should we enforce `min_straddle_dte >= holding_days + 2`?
   - Decision: Let user configure via `min_straddle_dte` flag

---

## 8. Expected Behavior

### Example: AAPL Thursday AMC Earnings

```
Earnings: Thursday Jan 30, 2025 AMC
Entry:    Friday Jan 31, 9:35 ET (day after)
Exit:     Friday Feb 7, 10:00 ET (5 trading days later)
Hold:     Jan 31 -> Feb 3, 4, 5, 6, 7 (5 trading days)
```

### Example: MSFT Monday BMO Earnings

```
Earnings: Monday Feb 3, 2025 BMO
Entry:    Monday Feb 3, 9:35 ET (same day, post-announcement)
Exit:     Monday Feb 10, 10:00 ET (5 trading days later)
Hold:     Feb 3 -> Feb 4, 5, 6, 7, 10 (5 trading days)
```

---

## 9. Next Steps

1. Implement in order of priority (files 1-6)
2. Test each layer as you go (unit tests after each file)
3. Integration test with full backtest run
4. Analyze results vs pre-earnings straddle strategy

---

## 10. Potential Future Enhancements

- Variable holding period based on upcoming earnings calendar
- Movement-based entry filter (only if stock moved > X%)
- Dynamic exit (exit early if IV normalizes)
- Compare vs pre-earnings straddle for same stock
- Greeks-based position sizing
