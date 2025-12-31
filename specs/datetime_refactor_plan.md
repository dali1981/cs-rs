# DateTime Management Refactor Plan

## Problem Statement

The codebase has **inconsistent datetime handling** causing bugs:
- `finq_options_repo.rs` uses Unix epoch days (1970-01-01 = day 0)
- `earnings_repo.rs` uses days from CE (year 1 = day 1)
- `spread_pricer.rs` uses num_days_from_ce for filtering
- Hardcoded market times (16:00 UTC)
- No single source of truth for conversions

**Result**: Expiration dates parsed as year 0056 instead of 2025, causing 0 trades found.

---

## Design Principles

1. **Single Internal Format**: Everything stored/computed as Unix timestamp (i64 nanos or i32 days)
2. **Conversion at Boundaries**: Convert only when reading from / writing to external sources
3. **Display-Only Formatting**: Human-readable dates only at presentation layer
4. **Centralized Logic**: One module handles ALL datetime operations
5. **Type Safety**: Distinct types prevent mixing formats

---

## Architecture

### New Module: `cs-domain/src/datetime.rs`

```
cs-domain/src/datetime.rs
├── Types
│   ├── TradingDate      - Wrapper around i32 (days since Unix epoch)
│   ├── TradingTimestamp - Wrapper around i64 (nanos since Unix epoch)
│   └── MarketTime       - Time of day (hour, minute) for market operations
│
├── Constants
│   ├── UNIX_EPOCH_DATE  - 1970-01-01 as reference
│   ├── NANOS_PER_DAY    - 86_400_000_000_000
│   └── TRADING_TIMEZONE - "America/New_York" (or UTC offset)
│
├── Conversions (From External)
│   ├── from_polars_date(i32) -> TradingDate
│   ├── from_polars_datetime(i64) -> TradingTimestamp
│   ├── from_naive_date(NaiveDate) -> TradingDate
│   ├── from_datetime_utc(DateTime<Utc>) -> TradingTimestamp
│   └── from_ymd(year, month, day) -> TradingDate
│
├── Conversions (To External - Display Only)
│   ├── to_naive_date(&TradingDate) -> NaiveDate
│   ├── to_datetime_utc(&TradingTimestamp) -> DateTime<Utc>
│   ├── to_polars_date(&TradingDate) -> i32
│   └── format_display(&TradingDate, fmt) -> String
│
├── Arithmetic
│   ├── add_days(&TradingDate, i32) -> TradingDate
│   ├── days_between(&TradingDate, &TradingDate) -> i32
│   ├── with_time(&TradingDate, &MarketTime) -> TradingTimestamp
│   └── time_to_expiry(&TradingTimestamp, &TradingDate) -> f64 (years)
│
└── Validation
    ├── is_trading_day(&TradingDate) -> bool
    ├── is_market_hours(&TradingTimestamp) -> bool
    └── dte(&TradingDate, &TradingDate) -> i32
```

---

## Type Definitions

```rust
// cs-domain/src/datetime.rs

use chrono::{NaiveDate, DateTime, Utc, Duration};

/// Days since Unix epoch (1970-01-01).
/// This is the internal representation matching Polars Date type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TradingDate(i32);

/// Nanoseconds since Unix epoch.
/// This is the internal representation matching Polars Datetime type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TradingTimestamp(i64);

/// Time of day for market operations (no date component)
#[derive(Debug, Clone, Copy)]
pub struct MarketTime {
    pub hour: u32,
    pub minute: u32,
}

// Constants
const UNIX_EPOCH: NaiveDate = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
const NANOS_PER_SECOND: i64 = 1_000_000_000;
const NANOS_PER_DAY: i64 = 86_400 * NANOS_PER_SECOND;
```

---

## Implementation Details

### 1. TradingDate Implementation

```rust
impl TradingDate {
    /// Create from days since Unix epoch (Polars Date format)
    pub fn from_polars_date(days: i32) -> Self {
        Self(days)
    }

    /// Create from NaiveDate
    pub fn from_naive_date(date: NaiveDate) -> Self {
        let days = (date - UNIX_EPOCH).num_days() as i32;
        Self(days)
    }

    /// Create from year/month/day
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Option<Self> {
        NaiveDate::from_ymd_opt(year, month, day)
            .map(Self::from_naive_date)
    }

    /// Convert to Polars Date format (for filtering)
    pub fn to_polars_date(&self) -> i32 {
        self.0
    }

    /// Convert to NaiveDate (for display or external APIs)
    pub fn to_naive_date(&self) -> NaiveDate {
        UNIX_EPOCH + Duration::days(self.0 as i64)
    }

    /// Days to expiry from another date
    pub fn dte(&self, from: &TradingDate) -> i32 {
        self.0 - from.0
    }

    /// Add days
    pub fn add_days(&self, days: i32) -> Self {
        Self(self.0 + days)
    }

    /// Combine with time to create timestamp
    pub fn with_time(&self, time: &MarketTime) -> TradingTimestamp {
        let day_nanos = (self.0 as i64) * NANOS_PER_DAY;
        let time_nanos = (time.hour as i64 * 3600 + time.minute as i64 * 60) * NANOS_PER_SECOND;
        TradingTimestamp(day_nanos + time_nanos)
    }
}
```

### 2. TradingTimestamp Implementation

```rust
impl TradingTimestamp {
    /// Create from nanoseconds since Unix epoch
    pub fn from_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    /// Create from DateTime<Utc>
    pub fn from_datetime_utc(dt: DateTime<Utc>) -> Self {
        Self(dt.timestamp_nanos_opt().unwrap_or(0))
    }

    /// Convert to DateTime<Utc> (for display)
    pub fn to_datetime_utc(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_nanos(self.0)
    }

    /// Get the date component
    pub fn date(&self) -> TradingDate {
        TradingDate((self.0 / NANOS_PER_DAY) as i32)
    }

    /// Time to expiry in years (for Black-Scholes)
    pub fn time_to_expiry(&self, expiry: &TradingDate, market_close: &MarketTime) -> f64 {
        let expiry_ts = expiry.with_time(market_close);
        let diff_nanos = expiry_ts.0 - self.0;
        let diff_seconds = diff_nanos as f64 / NANOS_PER_SECOND as f64;
        diff_seconds / (365.25 * 86400.0)
    }
}
```

### 3. Display Formatting (Presentation Layer Only)

```rust
impl std::fmt::Display for TradingDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_naive_date().format("%Y-%m-%d"))
    }
}

impl std::fmt::Display for TradingTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_datetime_utc().format("%Y-%m-%d %H:%M:%S UTC"))
    }
}
```

---

## Migration Plan

### Phase 1: Create datetime module (no breaking changes)

1. Create `cs-domain/src/datetime.rs` with types and conversions
2. Add `pub mod datetime;` to `cs-domain/src/lib.rs`
3. Re-export types: `pub use datetime::{TradingDate, TradingTimestamp, MarketTime};`
4. Write unit tests for all conversions

### Phase 2: Fix infrastructure layer

Update these files to use new types at Polars boundaries:

| File | Change |
|------|--------|
| `finq_options_repo.rs:56-70` | Use `TradingDate::from_polars_date()` |
| `finq_options_repo.rs:82-87` | Use `TradingDate::to_polars_date()` |
| `finq_equity_repo.rs:43-88` | Use `TradingTimestamp::from_nanos()` |
| `earnings_repo.rs:90-135` | Use `TradingDate` (verify parquet format first!) |

### Phase 3: Update domain layer

Update internal calculations to use new types:

| File | Change |
|------|--------|
| `entities.rs:85-86` | Use `TradingDate::dte()` |
| `value_objects.rs:76-82` | Use `TradingDate::with_time()` |
| `strategies/atm.rs` | Use `TradingDate::dte()` |

### Phase 4: Update analytics/pricing

| File | Change |
|------|--------|
| `spread_pricer.rs:99` | Use `TradingDate::to_polars_date()` |
| `spread_pricer.rs:188-191` | Use `TradingTimestamp::time_to_expiry()` |
| `iv_surface.rs:201` | Use `TradingDate::dte()` |

### Phase 5: Remove hardcoded times

Replace hardcoded `16:00 UTC` with `TimingConfig`:

```rust
// Before (spread_pricer.rs:189)
let market_close = NaiveTime::from_hms_opt(16, 0, 0).unwrap();

// After
let market_close = MarketTime { hour: timing_config.exit_hour, minute: timing_config.exit_minute };
let ttm = entry_timestamp.time_to_expiry(&expiration_date, &market_close);
```

---

## Files to Modify

### New Files
- `cs-domain/src/datetime.rs` - New datetime module

### Modified Files (by priority)

**P0 - Blocking bugs**
1. `cs-domain/src/infrastructure/finq_options_repo.rs`
2. `cs-domain/src/infrastructure/finq_equity_repo.rs`
3. `cs-domain/src/infrastructure/earnings_repo.rs`

**P1 - Functionality**
4. `cs-backtest/src/spread_pricer.rs`
5. `cs-domain/src/value_objects.rs`
6. `cs-domain/src/entities.rs`

**P2 - Cleanup**
7. `cs-domain/src/strategies/atm.rs`
8. `cs-analytics/src/iv_surface.rs`
9. `cs-domain/src/services/trading_calendar.rs`
10. `cs-backtest/src/trade_executor.rs`

---

## Testing Strategy

### Unit Tests (datetime.rs)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_naive_date() {
        let original = NaiveDate::from_ymd_opt(2025, 11, 3).unwrap();
        let trading = TradingDate::from_naive_date(original);
        assert_eq!(trading.to_naive_date(), original);
    }

    #[test]
    fn test_polars_date_format() {
        // Nov 3, 2025 = 20,395 days since 1970-01-01
        let trading = TradingDate::from_ymd(2025, 11, 3).unwrap();
        let polars_days = trading.to_polars_date();
        // Verify against known value
        assert!(polars_days > 20000 && polars_days < 21000);
    }

    #[test]
    fn test_dte_calculation() {
        let nov_3 = TradingDate::from_ymd(2025, 11, 3).unwrap();
        let nov_21 = TradingDate::from_ymd(2025, 11, 21).unwrap();
        assert_eq!(nov_21.dte(&nov_3), 18);
    }

    #[test]
    fn test_with_time() {
        let date = TradingDate::from_ymd(2025, 11, 3).unwrap();
        let time = MarketTime { hour: 9, minute: 35 };
        let ts = date.with_time(&time);
        let dt = ts.to_datetime_utc();
        assert_eq!(dt.hour(), 9);
        assert_eq!(dt.minute(), 35);
    }
}
```

### Integration Tests

1. Load actual IDXX options data, verify expirations parse correctly
2. Load equity bars, verify timestamps are correct (not year 1970)
3. Run backtest with new datetime module, verify trades are found

---

## Verification Checklist

After implementation, verify:

- [ ] `test_load_idxx` shows correct expiration dates (2025-11-21, not 0056-11-20)
- [ ] `test_load_idxx` shows correct spot price timestamp (2025-11-03, not 1970-01-01)
- [ ] Backtest finds > 0 trades for Nov 3-4, 2025
- [ ] All unit tests pass
- [ ] No `num_days_from_ce()` calls remain outside datetime.rs
- [ ] No hardcoded `1970, 1, 1` dates outside datetime.rs

---

## Estimated Effort

| Phase | Files | Complexity | Estimate |
|-------|-------|------------|----------|
| Phase 1: Create module | 1 new | Medium | Core implementation |
| Phase 2: Infrastructure | 3 | Low | Swap function calls |
| Phase 3: Domain | 3 | Low | Swap function calls |
| Phase 4: Analytics | 2 | Medium | Update TTM calculation |
| Phase 5: Config | 1 | Low | Pass config through |

---

## Open Questions

1. **Earnings data format**: Verify if `earnings_repo.rs` parquet uses Unix epoch or CE days
2. **Timezone handling**: Should we support EST/EDT for US market hours?
3. **Weekend/holiday handling**: Should `TradingDate` validate trading days?

---

## Summary

This refactor creates a **single source of truth** for all datetime operations:

- **Internal**: Unix epoch timestamps (i32 days or i64 nanos)
- **External**: Convert at Polars boundaries
- **Display**: Human-readable only at presentation layer
- **Centralized**: All logic in `datetime.rs`

No more guessing which format a function expects - the types make it explicit.
