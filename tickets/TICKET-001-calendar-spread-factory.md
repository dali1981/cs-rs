# TICKET-001: Implement CalendarSpread::create() in TradeFactory

**Status**: Open
**Priority**: High
**Created**: 2026-01-06
**Blocks**: `cs campaign --strategy calendar-spread`

---

## Problem

`CalendarSpread::create()` in `cs-domain/src/entities/rollable_impls.rs:133-144` returns a hardcoded error:

```rust
// For now, return an error as calendar spread factory method doesn't exist yet
// This will be implemented in Phase 2.4
Err(TradeConstructionError::FactoryError(
    "create_calendar_spread not yet implemented in TradeFactory".to_string()
))
```

This prevents the `campaign` command from executing calendar spread sessions.

---

## Current Behavior

```bash
cs campaign --strategy calendar-spread --period-policy earnings-only ...
# Result: 0% success rate, all sessions fail silently
```

---

## Required Changes

### 1. Add method to `TradeFactory` trait

File: `cs-domain/src/trade/factory.rs`

```rust
async fn create_calendar_spread(
    &self,
    symbol: &str,
    entry_dt: DateTime<Utc>,
    min_short_expiration: NaiveDate,
    option_type: OptionType,  // Call or Put
) -> Result<CalendarSpread, TradeConstructionError>;
```

### 2. Implement in `DefaultTradeFactory`

File: `cs-backtest/src/default_trade_factory.rs`

- Use `ATMStrategy` to select strike at entry time
- Use `ExpirationPolicy::Calendar` to select short/long expirations
- Query options chain for both expirations
- Build `CalendarSpread` from selected contracts

### 3. Update `CalendarSpread::create()`

File: `cs-domain/src/entities/rollable_impls.rs`

```rust
async fn create(
    factory: &dyn TradeFactory,
    symbol: &str,
    dt: DateTime<Utc>,
    min_expiration: NaiveDate,
) -> Result<Self, TradeConstructionError> {
    // Default to Call for calendar spreads
    factory.create_calendar_spread(symbol, dt, min_expiration, OptionType::Call).await
}
```

### 4. Update `SessionExecutor`

File: `cs-backtest/src/session_executor.rs`

Add option_type parameter to session context or default to Call.

---

## Workaround

Use `cs backtest` command instead of `cs campaign`:

```bash
cs backtest --spread calendar --option-type call \
    --symbols PENG \
    --earnings-file custom_earnings/PENG_2025.parquet \
    --start 2025-01-01 --end 2025-12-31
```

---

## Acceptance Criteria

- [ ] `cs campaign --strategy calendar-spread` executes successfully
- [ ] P&L results match `cs backtest --spread calendar` for same parameters
- [ ] Unit tests for `TradeFactory::create_calendar_spread()`
