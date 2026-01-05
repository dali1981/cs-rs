# Refactoring: Fix Rolling Straddle Fake Expiration Bug

**Date:** 2026-01-05
**Status:** In Progress
**Type:** Architecture Debt Resolution

---

## Executive Summary

The `RollingStraddleExecutor` uses a hardcoded 7-day expiration rule (`date + Duration::days(7)`) instead of querying real expiration dates from the options repository. This architectural debt stems from missing abstraction—the rolling executor was built without the necessary service/repository integration that exists in the regular backtest flow.

**Impact:** All rolling straddle backtest results are invalid—options may not exist at the fake expiration dates, and pricing is incorrect.

---

## Root Cause

### The Core Problem: Separation of Concerns Violation

The architecture has two distinct concerns:

| Concern | Purpose | Requires |
|---------|---------|----------|
| **Trade Selection** | Find valid strikes/expirations from market data | `OptionsDataRepository`, `IVSurface`, `StrikeSelector` |
| **Trade Execution** | Price trades, calculate P&L | Pre-built trade entity, `OptionsDataRepository` (for pricing) |

`UnifiedExecutor` handles BOTH via two methods:
- `execute_with_selection()` - selection + execution (used by regular backtest)
- `execute_straddle()` - execution only (expects pre-built `Straddle`)

`RollingStraddleExecutor` tries to use `execute_straddle()` but needs **selection first**. It can't do selection because:
1. No access to `OptionsDataRepository`
2. No access to `IVSurface` building infrastructure
3. No access to `StrikeSelector`

### The Shortcut That Created Debt

```rust
// cs-backtest/src/rolling_straddle_executor.rs:136
let expiration = date + chrono::Duration::days(7);  // Assume 1-week expiry
```

The author manually constructed a `Straddle` with a **fake expiration** instead of implementing proper selection. This was likely due to:
1. **Time pressure** - selection infrastructure is complex
2. **Incorrect mental model** - assuming `UnifiedExecutor.execute_straddle()` would handle everything
3. **Hidden dependency** - `UnifiedExecutor` has `options_repo` internally but doesn't expose selection capabilities

---

## How Regular Backtest Works (Correct Pattern)

```
BacktestUseCase.process_event_unified()
  ↓
1. Get option chain DataFrame from OptionsRepository
   options_repo.get_option_bars_at_time(symbol, entry_time)

2. Build IVSurface from the DataFrame
   build_iv_surface_minute_aligned(&entry_chain, ...)

3. IVSurface CONTAINS available expirations
   surface.expirations() → Vec<NaiveDate>

4. StrikeSelector queries surface for expirations
   ATMStrategy::select_straddle(spot, surface, min_expiration)

5. Selector filters expirations for minimum DTE
   surface.expirations()
     .filter(|exp| exp > min_expiration)
     .min()  // Select soonest valid

6. Create Straddle with REAL expiration date
7. Execute via UnifiedExecutor
```

**Key insight:** The regular backtest has a full pipeline from raw data → IV surface → selection → execution.

---

## How Rolling Executor Works (Broken)

```
Rolling Loop at date D
    ↓
RollingStraddleExecutor.find_atm_straddle(symbol, D)
    ↓
Get spot from EquityRepository
    ↓
Round spot to nearest strike
    ↓
HARDCODED: expiration = D + 7 days  ← BUG!
    ↓
Create Straddle with FAKE expiration
    ↓
UnifiedExecutor.execute_straddle(fake_straddle, ...)
    ↓
Prices using wrong expiration!
```

**Missing:** The entire selection pipeline (option chain query → IV surface → expiration selection).

---

## Chosen Solution: Option B - TradeFactory Abstraction

### Why This Approach?

Follows DDD principles from `~/.claude/ARCHITECTURE_RULES.md`:
- **Domain Service**: `TradeFactory` is stateless, pure business logic
- **Separation of concerns**: Factory creates, Executor executes
- **Repository pattern**: Factory uses repositories, doesn't expose them

### New Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ Domain Layer (cs-domain)                                        │
│   ├─► TradeFactory trait (NEW)                                 │
│   │     ├─► create_atm_straddle(symbol, date, min_exp)          │
│   │     └─► available_expirations(symbol, date)                 │
└─────────────────────────────────────────────────────────────────┘
         ▲ implemented by
┌─────────────────────────────────────────────────────────────────┐
│ Infrastructure Layer (cs-backtest)                              │
│   ├─► DefaultTradeFactory (NEW)                                │
│   │     ├─► owns OptionsDataRepository                          │
│   │     ├─► owns EquityDataRepository                           │
│   │     ├─► owns ATMStrategy (StrikeSelector)                   │
│   │     └─► implements create_atm_straddle:                     │
│   │           1. Query option chain                             │
│   │           2. Build IV surface                               │
│   │           3. Select straddle with REAL expirations          │
│   │                                                             │
│   └─► RollingStraddleExecutor (REFACTORED)                     │
│         ├─► uses TradeFactory instead of manual construction    │
│         └─► delegates selection to factory                      │
└─────────────────────────────────────────────────────────────────┘
```

---

## Implementation Plan

### Step 1: Create TradeFactory Trait (Domain Layer)

**File:** `cs-domain/src/ports/trade_factory.rs` (NEW)

```rust
use crate::entities::Straddle;
use chrono::{DateTime, NaiveDate, Utc};
use thiserror::Error;

#[async_trait::async_trait]
pub trait TradeFactory: Send + Sync {
    /// Create an ATM straddle at the given date with minimum expiration
    ///
    /// # Arguments
    /// * `symbol` - Ticker symbol
    /// * `as_of` - Date/time to query market data
    /// * `min_expiration` - Minimum required expiration date (options must expire AFTER this)
    ///
    /// # Returns
    /// A Straddle with:
    /// - ATM strike (closest to spot price)
    /// - First available expiration after min_expiration
    /// - Both call and put legs at same strike/expiration
    async fn create_atm_straddle(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Straddle, TradeFactoryError>;

    /// Query available expiration dates for a symbol at a given time
    async fn available_expirations(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<NaiveDate>, TradeFactoryError>;
}

#[derive(Debug, Error)]
pub enum TradeFactoryError {
    #[error("No expirations available")]
    NoExpirations,
    #[error("No strikes available")]
    NoStrikes,
    #[error("Data error: {0}")]
    DataError(String),
    #[error("Selection error: {0}")]
    SelectionError(String),
}
```

**File:** `cs-domain/src/ports/mod.rs` (MODIFY)

Add:
```rust
pub mod trade_factory;
pub use trade_factory::{TradeFactory, TradeFactoryError};
```

---

### Step 2: Implement DefaultTradeFactory (Infrastructure Layer)

**File:** `cs-backtest/src/trade_factory_impl.rs` (NEW)

```rust
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_analytics::IVSurface;
use cs_domain::{
    EquityDataRepository, OptionsDataRepository,
    SpotPrice, Straddle, TradeFactory, TradeFactoryError,
};
use cs_domain::strike_selection::{ATMStrategy, StrikeSelector};

use crate::iv_surface_builder::build_iv_surface_minute_aligned;

/// Default implementation of TradeFactory using ATM selection strategy
pub struct DefaultTradeFactory<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    selector: ATMStrategy,
}

impl<O, E> DefaultTradeFactory<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(options_repo: Arc<O>, equity_repo: Arc<E>) -> Self {
        Self {
            options_repo,
            equity_repo,
            selector: ATMStrategy::default(),
        }
    }

    pub fn with_selector(mut self, selector: ATMStrategy) -> Self {
        self.selector = selector;
        self
    }
}

#[async_trait]
impl<O, E> TradeFactory for DefaultTradeFactory<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    async fn create_atm_straddle(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Straddle, TradeFactoryError> {
        // 1. Query option chain
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(e.to_string()))?;

        // 2. Get spot price
        let spot = self.equity_repo
            .get_spot_price(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(e.to_string()))?;

        // 3. Build IV surface
        let surface = build_iv_surface_minute_aligned(
            &chain,
            spot.to_f64(),
            symbol,
            as_of,
        )
        .map_err(|e| TradeFactoryError::SelectionError(e.to_string()))?;

        // 4. Select straddle using real expirations
        let spot_price = SpotPrice::new(Decimal::from_f64(spot.to_f64()).unwrap(), as_of);

        self.selector
            .select_straddle(&spot_price, &surface, min_expiration)
            .map_err(|e| TradeFactoryError::SelectionError(e.to_string()))
    }

    async fn available_expirations(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<NaiveDate>, TradeFactoryError> {
        // Query option chain
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(e.to_string()))?;

        // Get spot price for IV surface building
        let spot = self.equity_repo
            .get_spot_price(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(e.to_string()))?;

        // Build IV surface to extract expirations
        let surface = build_iv_surface_minute_aligned(
            &chain,
            spot.to_f64(),
            symbol,
            as_of,
        )
        .map_err(|e| TradeFactoryError::SelectionError(e.to_string()))?;

        Ok(surface.expirations())
    }
}
```

**File:** `cs-backtest/src/lib.rs` (MODIFY)

Add:
```rust
mod trade_factory_impl;
pub use trade_factory_impl::DefaultTradeFactory;
```

---

### Step 3: Refactor RollingStraddleExecutor

**File:** `cs-backtest/src/rolling_straddle_executor.rs` (MODIFY)

Changes:
1. Add `TradeFactory` dependency
2. Remove manual strike/expiration construction
3. Delegate to factory

```rust
use cs_domain::{TradeFactory, TradeFactoryError};

pub struct RollingStraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    unified_executor: UnifiedExecutor<O, E>,
    trade_factory: Arc<dyn TradeFactory>,  // NEW
    roll_policy: RollPolicy,
}

impl<O, E> RollingStraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(
        unified_executor: UnifiedExecutor<O, E>,
        trade_factory: Arc<dyn TradeFactory>,  // NEW PARAMETER
        roll_policy: RollPolicy,
    ) -> Self {
        Self {
            unified_executor,
            trade_factory,
            roll_policy,
        }
    }

    /// Find ATM straddle at given date (REFACTORED)
    async fn find_atm_straddle(
        &self,
        symbol: &str,
        date: NaiveDate,
    ) -> Result<Straddle, String> {
        // Use 3:45pm ET as reference time for querying market data
        let dt = self.to_datetime(date, MarketTime { hour: 15, minute: 45 });

        // Require options to expire at least 1 day after entry
        let min_expiration = date + chrono::Duration::days(1);

        // Delegate to factory - uses REAL expirations from market data
        self.trade_factory
            .create_atm_straddle(symbol, dt, min_expiration)
            .await
            .map_err(|e| e.to_string())
    }
}
```

**Delete:** Lines 115-155 (old `find_atm_straddle` implementation with fake expiration)

---

### Step 4: Update CLI Wiring

**File:** `cs-cli/src/main.rs` (MODIFY)

```rust
use cs_backtest::DefaultTradeFactory;

pub async fn run_rolling_straddle(
    // ... existing parameters ...
) -> Result<()> {
    // ... existing repository setup ...

    let options_repo = Arc::new(options_repo);
    let equity_repo = Arc::new(equity_repo);

    // Create trade factory (NEW)
    let trade_factory = Arc::new(DefaultTradeFactory::new(
        Arc::clone(&options_repo),
        Arc::clone(&equity_repo),
    )) as Arc<dyn cs_domain::TradeFactory>;

    // Create unified executor
    let mut unified_executor = UnifiedExecutor::new(
        Arc::clone(&options_repo),
        Arc::clone(&equity_repo),
    )
    .with_pricing_model(backtest_config.pricing_model.clone())
    .with_hedge_config(backtest_config.hedge_config.clone());

    // ... existing timing strategy setup ...

    // Create rolling executor with factory (MODIFIED)
    let rolling_executor = RollingStraddleExecutor::new(
        unified_executor,
        trade_factory,  // NEW PARAMETER
        roll_policy,
    );

    // ... rest of function unchanged ...
}
```

---

## Benefits of This Refactoring

1. **Correctness** - Uses real expiration dates from market data
2. **Separation of Concerns** - Factory handles creation, Executor handles execution
3. **Testability** - Can mock `TradeFactory` for unit tests
4. **Reusability** - `TradeFactory` can be used in other contexts (live trading, other strategies)
5. **DDD Compliance** - Follows port/adapter pattern, domain service abstraction
6. **No God Objects** - Each component has a single, clear responsibility

---

## Testing Strategy

1. **Unit tests for DefaultTradeFactory:**
   - Mock repositories
   - Verify real expirations are selected
   - Test error handling (no expirations, no strikes)

2. **Integration test for rolling executor:**
   - Run a small rolling backtest (1 month)
   - Verify all expirations are valid dates from the options chain
   - Compare with old results to detect pricing differences

3. **Validation query:**
   ```rust
   // After backtest, verify all expirations were real
   for roll in result.rolls {
       let available = trade_factory.available_expirations(symbol, roll.entry_date).await?;
       assert!(available.contains(&roll.expiration),
               "Expiration {} not in available list: {:?}", roll.expiration, available);
   }
   ```

---

## Migration Checklist

- [ ] Create `cs-domain/src/ports/trade_factory.rs`
- [ ] Update `cs-domain/src/ports/mod.rs` to export trait
- [ ] Create `cs-backtest/src/trade_factory_impl.rs`
- [ ] Update `cs-backtest/src/lib.rs` to export impl
- [ ] Refactor `cs-backtest/src/rolling_straddle_executor.rs`
- [ ] Update `cs-cli/src/main.rs` wiring
- [ ] Add unit tests for `DefaultTradeFactory`
- [ ] Run integration test with rolling backtest
- [ ] Validate all expirations are real
- [ ] Update documentation

---

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Breaking changes to RollingStraddleExecutor API | Update all call sites (only 1: CLI) |
| Performance regression (extra IV surface builds) | Profile before/after; IV surface is cached in factory |
| Different results than old backtest | Expected! Old results were WRONG. Document differences. |
| Missing expirations in sparse data | Factory returns error; rolling executor logs and skips roll |

---

## References

- Original bug: `cs-backtest/src/rolling_straddle_executor.rs:136`
- Regular backtest selection: `cs-backtest/src/backtest_use_case.rs:1360-1402`
- StrikeSelector trait: `cs-domain/src/strike_selection/mod.rs:164-216`
- ATMStrategy implementation: `cs-domain/src/strike_selection/atm.rs:345-394`

---

## Appendix: Alternative Approaches Considered

### Option A: Minimal Fix (Rejected)

Add `options_repo` directly to `RollingStraddleExecutor`.

**Why rejected:** Violates single responsibility; executor would know too much about IV surface building.

### Option C: UnifiedExecutor Facade (Rejected)

Add `build_straddle()` method to `UnifiedExecutor`.

**Why rejected:** Makes `UnifiedExecutor` a god object with too many responsibilities (selection + execution).
