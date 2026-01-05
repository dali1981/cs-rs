# Generic Trade Execution Refactor

## Problem Statement

`RollingExecutor<T>` uses unsafe pointer casts to dispatch to type-specific execution methods:

```rust
// rolling_executor.rs:169-194 - UNSAFE CODE
match trade.type_id() {
    "straddle" => {
        let straddle = unsafe {
            &*(trade as *const T as *const Straddle)  // UNSAFE!
        };
        let result = self.unified_executor.execute_straddle(...).await;
        unsafe {
            std::ptr::read(&result as *const StraddleResult as *const T::Result)  // UNSAFE!
        }
    }
    "calendar_spread" => {
        panic!("Calendar spread rolling not yet implemented");  // Can't extend!
    }
}
```

**Issues:**
1. Unsafe pointer casts are error-prone
2. Adding a new trade type requires modifying the executor
3. `TradeTypeId` trait exists only to enable this unsafe dispatch
4. No compile-time safety for trade type / result type correspondence

## Root Cause

`UnifiedExecutor` has type-specific methods (`execute_straddle()`, etc.) instead of a generic `execute<T>()`. But the execution pattern is **identical** across all trade types:

```
1. Get spot prices (entry + exit)           ← Same for all trades
2. Get option chain (entry + exit)          ← Same for all trades
3. Build IV surfaces                        ← Same for all trades
4. Price trade with type-specific pricer    ← DIFFERS: pricer type
5. Validate entry pricing                   ← DIFFERS: validation rules
6. Construct result                         ← DIFFERS: result fields
```

## Solution: Two Traits

### 1. `TradePricer` Trait

Unifies the pricing interface across all pricers:

```rust
// cs-backtest/src/execution/traits.rs

pub trait TradePricer: Send + Sync {
    /// The trade type this pricer handles
    type Trade;

    /// The pricing result type (contains leg prices, IVs, greeks)
    type Pricing;

    /// Price a trade using pre-built IV surface
    fn price_with_surface(
        &self,
        trade: &Self::Trade,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<Self::Pricing, PricingError>;
}
```

### 2. `ExecutableTrade` Trait

Ties trade → pricer → pricing → result with validation:

```rust
// cs-backtest/src/execution/traits.rs

pub trait ExecutableTrade: Sized + Send + Sync {
    /// The pricer type for this trade
    type Pricer: TradePricer<Trade = Self>;

    /// Pricing output from the pricer
    type Pricing;

    /// Final execution result type
    type Result: TradeResult;

    /// Get symbol (for data fetching)
    fn symbol(&self) -> &str;

    /// Validate entry pricing against config
    /// Returns Ok(()) if valid, Err with reason if invalid
    fn validate_entry(
        pricing: &Self::Pricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError>;

    /// Construct success result from entry/exit pricing
    fn to_result(
        &self,
        entry_pricing: Self::Pricing,
        exit_pricing: Self::Pricing,
        ctx: &ExecutionContext,
    ) -> Self::Result;

    /// Construct failure result
    fn to_failed_result(
        &self,
        ctx: &ExecutionContext,
        error: ExecutionError,
    ) -> Self::Result;
}
```

### 3. Supporting Types

```rust
// cs-backtest/src/execution/types.rs

/// Configuration for trade validation
pub struct ExecutionConfig {
    pub max_entry_iv: Option<f64>,
    pub min_entry_cost: Decimal,      // e.g., $0.05 for calendar, $0.50 for straddle
    pub min_credit: Option<Decimal>,  // For credit spreads
}

/// Context passed to result construction
pub struct ExecutionContext<'a> {
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub entry_spot: f64,
    pub exit_spot: f64,
    pub entry_surface_time: Option<DateTime<Utc>>,
    pub exit_surface_time: DateTime<Utc>,
    pub earnings_event: &'a EarningsEvent,
}
```

### 4. Generic Execute Function

```rust
// cs-backtest/src/execution/generic_executor.rs

pub async fn execute_trade<T>(
    trade: &T,
    pricer: &T::Pricer,
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    config: &ExecutionConfig,
    earnings_event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
) -> T::Result
where
    T: ExecutableTrade,
{
    match try_execute_trade(trade, pricer, options_repo, equity_repo, config, earnings_event, entry_time, exit_time).await {
        Ok(result) => result,
        Err(e) => {
            let ctx = ExecutionContext {
                entry_time,
                exit_time,
                entry_spot: 0.0,
                exit_spot: 0.0,
                entry_surface_time: None,
                exit_surface_time: entry_time, // dummy
                earnings_event,
            };
            T::to_failed_result(trade, &ctx, e)
        }
    }
}

async fn try_execute_trade<T>(
    trade: &T,
    pricer: &T::Pricer,
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    config: &ExecutionConfig,
    earnings_event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
) -> Result<T::Result, ExecutionError>
where
    T: ExecutableTrade,
{
    // 1. Get spot prices
    let entry_spot = equity_repo
        .get_spot_price(trade.symbol(), entry_time)
        .await?;
    let exit_spot = equity_repo
        .get_spot_price(trade.symbol(), exit_time)
        .await?;

    // 2. Get option chains
    let entry_chain = options_repo
        .get_option_bars_at_time(trade.symbol(), entry_time)
        .await?;
    let (exit_chain, exit_surface_time) = options_repo
        .get_option_bars_at_or_after_time(trade.symbol(), exit_time, 30)
        .await?;

    // 3. Build IV surfaces
    let entry_surface = build_iv_surface_minute_aligned(
        &entry_chain,
        equity_repo,
        trade.symbol(),
    ).await;
    let entry_surface_time = entry_surface.as_ref().map(|s| s.as_of_time());

    let exit_surface = build_iv_surface_minute_aligned(
        &exit_chain,
        equity_repo,
        trade.symbol(),
    ).await;

    // 4. Price at entry
    let entry_pricing = pricer.price_with_surface(
        trade,
        &entry_chain,
        entry_spot.to_f64(),
        entry_time,
        entry_surface.as_ref(),
    )?;

    // 5. Validate entry
    T::validate_entry(&entry_pricing, config)?;

    // 6. Price at exit
    let exit_pricing = pricer.price_with_surface(
        trade,
        &exit_chain,
        exit_spot.to_f64(),
        exit_time,
        exit_surface.as_ref(),
    )?;

    // 7. Construct result
    let ctx = ExecutionContext {
        entry_time,
        exit_time,
        entry_spot: entry_spot.to_f64(),
        exit_spot: exit_spot.to_f64(),
        entry_surface_time,
        exit_surface_time,
        earnings_event,
    };

    Ok(T::to_result(trade, entry_pricing, exit_pricing, &ctx))
}
```

---

## Implementation Plan

### Phase 1: Create Execution Module Structure

**Files to create:**
- `cs-backtest/src/execution/mod.rs` - Module exports
- `cs-backtest/src/execution/traits.rs` - `TradePricer`, `ExecutableTrade`
- `cs-backtest/src/execution/types.rs` - `ExecutionConfig`, `ExecutionContext`
- `cs-backtest/src/execution/generic_executor.rs` - `execute_trade()` function

**Task 1.1:** Create `cs-backtest/src/execution/mod.rs`
```rust
mod traits;
mod types;
mod generic_executor;

pub use traits::{TradePricer, ExecutableTrade};
pub use types::{ExecutionConfig, ExecutionContext};
pub use generic_executor::execute_trade;
```

**Task 1.2:** Create `cs-backtest/src/execution/types.rs`
- Define `ExecutionConfig` struct
- Define `ExecutionContext` struct

**Task 1.3:** Create `cs-backtest/src/execution/traits.rs`
- Define `TradePricer` trait
- Define `ExecutableTrade` trait

**Task 1.4:** Create `cs-backtest/src/execution/generic_executor.rs`
- Implement `execute_trade()` function
- Implement `try_execute_trade()` helper

### Phase 2: Implement TradePricer for Existing Pricers

**Task 2.1:** Implement `TradePricer` for `StraddlePricer`
```rust
// cs-backtest/src/straddle_pricer.rs

impl TradePricer for StraddlePricer {
    type Trade = Straddle;
    type Pricing = StraddlePricing;

    fn price_with_surface(
        &self,
        trade: &Straddle,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<StraddlePricing, PricingError> {
        // Already exists - just rename/expose
        self.price_with_surface(trade, chain_df, spot, timestamp, iv_surface)
    }
}
```

**Task 2.2:** Implement `TradePricer` for `SpreadPricer` (CalendarSpread)
```rust
// cs-backtest/src/spread_pricer.rs

impl TradePricer for SpreadPricer {
    type Trade = CalendarSpread;
    type Pricing = SpreadPricing;

    fn price_with_surface(
        &self,
        trade: &CalendarSpread,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<SpreadPricing, PricingError> {
        self.price_spread_with_surface(trade, chain_df, spot, timestamp, iv_surface)
    }
}
```

**Task 2.3:** Implement `TradePricer` for `CalendarStraddlePricer`

**Task 2.4:** Implement `TradePricer` for `IronButterflyPricer`

### Phase 3: Implement ExecutableTrade for Trade Types

**Task 3.1:** Implement `ExecutableTrade` for `Straddle`
```rust
// cs-backtest/src/execution/straddle_impl.rs

impl ExecutableTrade for Straddle {
    type Pricer = StraddlePricer;
    type Pricing = StraddlePricing;
    type Result = StraddleResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn validate_entry(
        pricing: &StraddlePricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // Min straddle price
        if pricing.total_price < config.min_entry_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Straddle price too small: {} < {}",
                pricing.total_price, config.min_entry_cost
            )));
        }

        // Max IV check
        if let Some(max_iv) = config.max_entry_iv {
            if let Some(iv) = pricing.call.iv {
                if iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "IV too high: {:.1}% > {:.1}%",
                        iv * 100.0, max_iv * 100.0
                    )));
                }
            }
        }

        Ok(())
    }

    fn to_result(
        &self,
        entry_pricing: StraddlePricing,
        exit_pricing: StraddlePricing,
        ctx: &ExecutionContext,
    ) -> StraddleResult {
        // Move result construction logic from StraddleExecutor::try_execute_trade
        // Lines 154-239 of straddle_executor.rs
        ...
    }

    fn to_failed_result(
        &self,
        ctx: &ExecutionContext,
        error: ExecutionError,
    ) -> StraddleResult {
        // Move from StraddleExecutor::create_failed_result
        // Lines 255-313 of straddle_executor.rs
        ...
    }
}
```

**Task 3.2:** Implement `ExecutableTrade` for `CalendarSpread`
- Move validation from `trade_executor.rs:141-183`
- Move result construction from `trade_executor.rs:201-305`
- Move failed result from `trade_executor.rs:308-371`

**Task 3.3:** Implement `ExecutableTrade` for `CalendarStraddle`

**Task 3.4:** Implement `ExecutableTrade` for `IronButterfly`

### Phase 4: Update RollingExecutor

**Task 4.1:** Remove unsafe dispatch in `RollingExecutor`
```rust
// Before (rolling_executor.rs:151-194):
async fn execute_trade(&self, trade: &T, entry: DateTime<Utc>, exit: DateTime<Utc>) -> T::Result {
    match trade.type_id() {
        "straddle" => { unsafe { ... } }  // REMOVE
        ...
    }
}

// After:
async fn execute_trade(&self, trade: &T, entry: DateTime<Utc>, exit: DateTime<Utc>) -> T::Result
where
    T: ExecutableTrade,
{
    execute_trade(
        trade,
        &self.pricer,  // Need to store pricer in RollingExecutor
        self.options_repo.as_ref(),
        self.equity_repo.as_ref(),
        &self.config,
        &self.earnings_event,  // Or construct dummy
        entry,
        exit,
    ).await
}
```

**Task 4.2:** Update `RollingExecutor` struct to hold pricer
```rust
pub struct RollingExecutor<T, O, E>
where
    T: RollableTrade + ExecutableTrade,
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    pricer: T::Pricer,           // NEW: type-safe pricer
    trade_factory: Arc<dyn TradeFactory>,
    roll_policy: RollPolicy,
    config: ExecutionConfig,     // NEW: validation config
}
```

**Task 4.3:** Remove `TradeTypeId` trait (no longer needed)
- Delete from `cs-domain/src/trade/rollable.rs:62-68`
- Delete implementations from `cs-domain/src/entities/rollable_impls.rs`

### Phase 5: Simplify UnifiedExecutor

**Task 5.1:** Replace type-specific execute methods with generic delegation
```rust
// unified_executor.rs

impl<O, E> UnifiedExecutor<O, E> {
    /// Generic execute for any trade type
    pub async fn execute<T>(&self, trade: &T, event: &EarningsEvent, entry: DateTime<Utc>, exit: DateTime<Utc>) -> T::Result
    where
        T: ExecutableTrade,
    {
        execute_trade(
            trade,
            self.get_pricer::<T>(),
            self.options_repo.as_ref(),
            self.equity_repo.as_ref(),
            &self.config,
            event,
            entry,
            exit,
        ).await
    }

    // Keep execute_straddle() as thin wrapper for backward compatibility
    pub async fn execute_straddle(...) -> StraddleResult {
        self.execute(&straddle, event, entry, exit).await
    }
}
```

### Phase 6: Cleanup

**Task 6.1:** Remove or deprecate old executors
- `straddle_executor.rs` - Extract result construction, then deprecate
- `trade_executor.rs` - Extract result construction, then deprecate
- `calendar_straddle_executor.rs` - Extract result construction, then deprecate
- `iron_butterfly_executor.rs` - Extract result construction, then deprecate

**Task 6.2:** Update `cs-backtest/src/lib.rs` exports

**Task 6.3:** Update tests

---

## File Changes Summary

| File | Action | Description |
|------|--------|-------------|
| `cs-backtest/src/execution/mod.rs` | CREATE | Module structure |
| `cs-backtest/src/execution/traits.rs` | CREATE | `TradePricer`, `ExecutableTrade` traits |
| `cs-backtest/src/execution/types.rs` | CREATE | `ExecutionConfig`, `ExecutionContext` |
| `cs-backtest/src/execution/generic_executor.rs` | CREATE | Generic `execute_trade()` function |
| `cs-backtest/src/execution/straddle_impl.rs` | CREATE | `ExecutableTrade` impl for Straddle |
| `cs-backtest/src/execution/calendar_spread_impl.rs` | CREATE | `ExecutableTrade` impl for CalendarSpread |
| `cs-backtest/src/straddle_pricer.rs` | MODIFY | Add `TradePricer` impl |
| `cs-backtest/src/spread_pricer.rs` | MODIFY | Add `TradePricer` impl |
| `cs-backtest/src/rolling_executor.rs` | MODIFY | Remove unsafe, use generic execute |
| `cs-backtest/src/unified_executor.rs` | MODIFY | Add generic execute, simplify |
| `cs-domain/src/trade/rollable.rs` | MODIFY | Remove `TradeTypeId` trait |
| `cs-backtest/src/lib.rs` | MODIFY | Export new execution module |

---

## Verification Checklist

- [ ] `cargo build` succeeds with no warnings
- [ ] `cargo test` passes all existing tests
- [ ] No `unsafe` blocks in execution code
- [ ] `RollingExecutor<Straddle>` works
- [ ] `RollingExecutor<CalendarSpread>` works (NEW!)
- [ ] Adding a new trade type only requires implementing traits (no executor changes)

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Large refactor touches many files | Implement in phases, test each phase |
| Result construction logic is complex | Extract as-is first, then simplify |
| Backward compatibility | Keep thin wrapper methods initially |
| Performance regression | Profile before/after, should be identical |

---

## Open Questions

1. **Hedging:** Currently straddle-specific. Should `ExecutableTrade` have optional hedging support?
   - **Proposal:** Add `fn supports_hedging() -> bool` method, hedging logic stays in `HedgingExecutor` wrapper

2. **EarningsEvent dependency:** Generic execute needs earnings event for results. For rolling (non-earnings), use dummy?
   - **Proposal:** Make `EarningsEvent` optional in `ExecutionContext`, provide builder pattern

3. **Pricer construction:** Where do pricers get created? Currently in executors.
   - **Proposal:** Add `T::Pricer::default()` or factory method, `ExecutionConfig` holds pricing model
