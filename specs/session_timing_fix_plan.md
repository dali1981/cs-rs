# Session Timing Fix Plan

## Executive Summary

The Rust backtest produces drastically different results from Python (5% vs 54% win rate) because it **exits trades on the same day as entry**, missing the earnings announcement entirely. Python holds trades **overnight through earnings** to capture the IV crush.

---

## 1. Understanding the Session Concept

### What is a "Session"?

A **trading session** in the calendar spread strategy is NOT a single calendar day. It spans the period from **trade entry** to **trade exit**, which crosses overnight for most earnings trades.

The strategy profits from **IV crush** - the volatility collapse that occurs AFTER earnings are announced. To capture this:

| Earnings Time | Entry Date | Exit Date |
|--------------|------------|-----------|
| AMC (After-Market-Close) | Earnings day (before close) | NEXT trading day |
| BMO (Before-Market-Open) | PREVIOUS trading day | Earnings day |

### Python's EarningsTradeTiming Logic

```python
# From earnings_timing.py
class EarningsTradeTiming:
    def entry_dt(self, event: EarningsEvent) -> datetime:
        if event.earnings_time == EarningsTime.BEFORE_MARKET:
            entry_date = TradingCalendar.previous_trading_day(event.earnings_date)
        elif event.earnings_time == EarningsTime.AFTER_MARKET:
            entry_date = event.earnings_date
        # Convert to Eastern then UTC
        entry_eastern = datetime.combine(entry_date, self.config.get_entry_time(), tzinfo=EASTERN_TZ)
        return ensure_utc_datetime(entry_eastern)

    def exit_dt(self, event: EarningsEvent) -> datetime:
        if event.earnings_time == EarningsTime.BEFORE_MARKET:
            exit_date = event.earnings_date  # Same day as earnings
        elif event.earnings_time == EarningsTime.AFTER_MARKET:
            exit_date = TradingCalendar.next_trading_day(event.earnings_date)  # NEXT DAY
        exit_eastern = datetime.combine(exit_date, self.config.get_exit_time(), tzinfo=EASTERN_TZ)
        return ensure_utc_datetime(exit_eastern)
```

---

## 2. Current Rust Bug

### Location: `cs-backtest/src/backtest_use_case.rs`

```rust
async fn process_event(
    &self,
    event: &EarningsEvent,
    session_date: NaiveDate,  // This is the ENTRY date
    ...
) -> Result<CalendarSpreadResult, TradeGenerationError> {
    // BUG: Entry uses session_date - CORRECT
    let entry_time = self.config.timing.entry_datetime(session_date);

    // ...process trade...

    // BUG: Exit ALSO uses session_date - WRONG!
    let exit_time = self.config.timing.exit_datetime(session_date);  // <-- THE BUG
}
```

### Impact

For an AMC event on Nov 3, 2025:
- **Rust**: Entry 09:35 Nov 3, Exit 15:55 Nov 3 (SAME DAY - before earnings!)
- **Python**: Entry 15:00 Nov 3, Exit 10:00 Nov 4 (holds through earnings announcement)

The Rust backtest exits BEFORE earnings happen, so it's essentially just trading random daily noise, not capturing IV crush.

### The `should_enter_today` Function is Correct

```rust
fn should_enter_today(&self, event: &EarningsEvent, session_date: NaiveDate) -> bool {
    match event.earnings_time {
        EarningsTime::AfterMarketClose => event.earnings_date == session_date,  // CORRECT
        EarningsTime::BeforeMarketOpen => {
            TradingCalendar::previous_trading_day(event.earnings_date) == session_date  // CORRECT
        }
        EarningsTime::Unknown => false,
    }
}
```

This correctly determines WHETHER to enter. But we don't have logic to determine the correct EXIT date.

---

## 3. Fix Plan

### Step 1: Create `EarningsTradeTiming` in cs-domain

**File**: `cs-domain/src/services/earnings_timing.rs`

```rust
use chrono::{DateTime, NaiveDate, Utc};
use crate::entities::EarningsEvent;
use crate::value_objects::{EarningsTime, TimingConfig};
use crate::services::TradingCalendar;

/// Calculates entry/exit timing for earnings-based trades
pub struct EarningsTradeTiming {
    config: TimingConfig,
}

impl EarningsTradeTiming {
    pub fn new(config: TimingConfig) -> Self {
        Self { config }
    }

    /// Calculate entry datetime based on earnings timing
    pub fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let entry_date = self.entry_date(event);
        self.config.entry_datetime(entry_date)
    }

    /// Calculate exit datetime based on earnings timing
    pub fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let exit_date = self.exit_date(event);
        self.config.exit_datetime(exit_date)
    }

    /// Entry date: When we enter the trade
    /// - BMO: Previous trading day (enter day before earnings)
    /// - AMC: Same day (enter before close, earnings after close)
    pub fn entry_date(&self, event: &EarningsEvent) -> NaiveDate {
        match event.earnings_time {
            EarningsTime::BeforeMarketOpen => {
                TradingCalendar::previous_trading_day(event.earnings_date)
            }
            EarningsTime::AfterMarketClose => {
                event.earnings_date
            }
            EarningsTime::Unknown => {
                // Default to AMC behavior
                event.earnings_date
            }
        }
    }

    /// Exit date: When we exit the trade (AFTER earnings)
    /// - BMO: Same day as earnings (earnings already happened)
    /// - AMC: Next trading day (exit morning after earnings)
    pub fn exit_date(&self, event: &EarningsEvent) -> NaiveDate {
        match event.earnings_time {
            EarningsTime::BeforeMarketOpen => {
                event.earnings_date  // Exit same day
            }
            EarningsTime::AfterMarketClose => {
                TradingCalendar::next_trading_day(event.earnings_date)  // Exit next day
            }
            EarningsTime::Unknown => {
                TradingCalendar::next_trading_day(event.earnings_date)
            }
        }
    }
}
```

### Step 2: Create services module in cs-domain

**File**: `cs-domain/src/services/mod.rs`

```rust
pub mod earnings_timing;
pub mod trading_calendar;

pub use earnings_timing::EarningsTradeTiming;
pub use trading_calendar::TradingCalendar;
```

Move `TradingCalendar` from wherever it currently lives into `cs-domain/src/services/trading_calendar.rs`.

### Step 3: Update `BacktestUseCase::process_event`

**File**: `cs-backtest/src/backtest_use_case.rs`

```rust
use cs_domain::services::EarningsTradeTiming;

// In the struct, add timing calculator
pub struct BacktestUseCase<Earn, Opt, Eq> {
    // ...existing fields...
    earnings_timing: EarningsTradeTiming,  // Add this
}

// In new():
pub fn new(...) -> Self {
    Self {
        // ...
        earnings_timing: EarningsTradeTiming::new(config.timing),
    }
}

// In process_event():
async fn process_event(
    &self,
    event: &EarningsEvent,
    _session_date: NaiveDate,  // Keep for compatibility but don't use for exit
    strategy: &dyn TradingStrategy,
    option_type: finq_core::OptionType,
) -> Result<CalendarSpreadResult, TradeGenerationError> {
    // Use event-based timing, not session_date
    let entry_time = self.earnings_timing.entry_datetime(event);
    let exit_time = self.earnings_timing.exit_datetime(event);

    // Get spot price at entry time
    let spot_result = self.equity_repo.get_spot_price(&event.symbol, entry_time).await;

    // ... rest of processing ...

    let executor = TradeExecutor::new(
        self.options_repo.clone(),
        self.equity_repo.clone(),
    );

    let result = executor.execute_trade(&spread, event, entry_time, exit_time).await;
    Ok(result)
}
```

### Step 4: Simplify `should_enter_today`

The logic can be simplified since `EarningsTradeTiming` handles date calculation:

```rust
fn should_enter_today(&self, event: &EarningsEvent, session_date: NaiveDate) -> bool {
    self.earnings_timing.entry_date(event) == session_date
}
```

### Step 5: Update `load_earnings_window`

Currently loads events for previous_day to next_day. This should be based on what could be entered OR exited on session_date:

```rust
async fn load_earnings_window(&self, session_date: NaiveDate) -> Result<Vec<EarningsEvent>, BacktestError> {
    // For AMC: entry_date == earnings_date, so we need events where earnings_date == session_date
    // For BMO: entry_date == previous(earnings_date), so we need events where previous(earnings_date) == session_date
    //         which means earnings_date == next(session_date)
    // Combined: we need events from session_date to next_trading_day(session_date)
    let start = session_date;
    let end = TradingCalendar::next_trading_day(session_date);

    self.earnings_repo
        .load_earnings(start, end, self.config.symbols.as_deref())
        .await
        .map_err(|e| BacktestError::Repository(e.to_string()))
}
```

---

## 4. Test Cases

### Unit Tests for EarningsTradeTiming

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn default_timing() -> EarningsTradeTiming {
        EarningsTradeTiming::new(TimingConfig {
            entry_hour: 9,
            entry_minute: 35,
            exit_hour: 10,
            exit_minute: 0,
        })
    }

    #[test]
    fn test_amc_entry_exit_dates() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 3).unwrap(),
            EarningsTime::AfterMarketClose,
        );

        // AMC: Enter same day, exit next day
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 3).unwrap());
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 11, 4).unwrap());
    }

    #[test]
    fn test_bmo_entry_exit_dates() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 4).unwrap(),  // Earnings on Nov 4
            EarningsTime::BeforeMarketOpen,
        );

        // BMO: Enter previous day, exit same day as earnings
        assert_eq!(timing.entry_date(&event), NaiveDate::from_ymd_opt(2025, 11, 3).unwrap());
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 11, 4).unwrap());
    }

    #[test]
    fn test_amc_friday_exits_monday() {
        let timing = default_timing();
        let event = EarningsEvent::new(
            "TEST".into(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),  // Friday
            EarningsTime::AfterMarketClose,
        );

        // Friday AMC should exit Monday
        assert_eq!(timing.exit_date(&event), NaiveDate::from_ymd_opt(2025, 11, 10).unwrap());
    }
}
```

### Integration Test

```rust
#[tokio::test]
async fn test_backtest_holds_overnight_for_amc() {
    // Run backtest for a single AMC event
    // Verify that entry_time is on Day N and exit_time is on Day N+1
}
```

---

## 5. Additional Issues to Address (Later)

These are related but can be fixed separately:

### 5.1 IV Interpolation
- Rust uses fixed 30% IV fallback
- Python interpolates from IV surface
- Impact: Mispriced options

### 5.2 Timezone Handling
- Python converts Eastern -> UTC explicitly
- Rust uses naive UTC times
- Impact: Off by 4-5 hours depending on DST

### 5.3 Negative Entry Costs
- Some trades show short_price > long_price
- Root cause likely related to IV/pricing
- Impact: Invalid trade economics

---

## 6. Implementation Order

1. **Create `EarningsTradeTiming`** in cs-domain
2. **Add unit tests** for timing logic
3. **Update `BacktestUseCase`** to use event-based timing
4. **Run comparison test** against Python for Nov 3, 2025
5. **Verify overnight holding** through trace logs
6. **Compare specific trade results** with Python output

---

## 7. Expected Outcome

After fix:
- Win rate should approach Python's ~54%
- Trades should show entry on Day N, exit on Day N+1 for AMC events
- P&L should be positive (capturing IV crush)

---

## 8. Files to Modify

| File | Action |
|------|--------|
| `cs-domain/src/services/mod.rs` | Create (new module) |
| `cs-domain/src/services/earnings_timing.rs` | Create |
| `cs-domain/src/services/trading_calendar.rs` | Move/create |
| `cs-domain/src/lib.rs` | Export services module |
| `cs-backtest/src/backtest_use_case.rs` | Update process_event |
| `cs-backtest/src/lib.rs` | Update imports if needed |
