# CLI Update Plan: Post-Orchestrator Deletion

**Date**: 2026-01-05
**Status**: Draft
**Depends on**: CompositeTrade refactoring (complete)

## Context

After deleting `TradeOrchestrator` and `RollingExecutor`, the CLI has compilation errors:

1. `RollingExecutor` import no longer exists
2. `backtest.execute()` method removed
3. `TradeResult` enum removed (was used for pattern matching)

## Errors to Fix

### 1. RollingExecutor Import

```rust
// BROKEN
use cs_backtest::RollingExecutor;

// FIX: Use TradeExecutor instead
use cs_backtest::TradeExecutor;
```

### 2. Backtest Execution

```rust
// BROKEN - generic execute() with TradeStructure enum
let result = backtest.execute(structure, start, end, progress).await?;

// FIX - call specific method based on config
let result = match config.spread_type {
    SpreadType::CalendarSpread => {
        let option_type = config.option_type.unwrap_or(OptionType::Call);
        backtest.run_calendar_spread_backtest(option_type, start, end, progress).await?
    }
    SpreadType::Straddle => {
        backtest.run_straddle_backtest(start, end, progress).await?
    }
    SpreadType::IronButterfly => {
        backtest.run_iron_butterfly_backtest(start, end, progress).await?
    }
    SpreadType::CalendarStraddle => {
        backtest.run_calendar_straddle_backtest(start, end, progress).await?
    }
};
```

### 3. Result Handling

```rust
// BROKEN - pattern matching on TradeResult enum
match result {
    TradeResult::CalendarSpread(r) => { ... }
    TradeResult::Straddle(r) => { ... }
    TradeResult::Failed(f) => { ... }
}

// FIX - each method returns typed result, handle directly
// For BacktestResult<CalendarSpreadResult>:
for trade in result.results {
    // trade is CalendarSpreadResult, not TradeResult enum
    println!("PnL: {}", trade.pnl);
}
```

## Implementation Strategy

### Option A: Duplicate Output Logic Per Type (Simple)

Each spread type has its own output formatting:

```rust
match config.spread_type {
    SpreadType::CalendarSpread => {
        let result = backtest.run_calendar_spread_backtest(...).await?;
        print_calendar_spread_results(&result);
    }
    SpreadType::Straddle => {
        let result = backtest.run_straddle_backtest(...).await?;
        print_straddle_results(&result);
    }
    // ...
}
```

**Pros**: Simple, explicit, type-safe
**Cons**: Some duplication in output formatting

### Option B: Trait-Based Output (DRY)

Define a trait for result formatting:

```rust
trait DisplayableResult {
    fn symbol(&self) -> &str;
    fn pnl(&self) -> Decimal;
    fn pnl_pct(&self) -> Decimal;
    fn entry_time(&self) -> DateTime<Utc>;
    fn exit_time(&self) -> DateTime<Utc>;
}

impl DisplayableResult for CalendarSpreadResult { ... }
impl DisplayableResult for StraddleResult { ... }

fn print_results<R: DisplayableResult>(results: &BacktestResult<R>) {
    for trade in &results.results {
        println!("{}: {:.2}%", trade.symbol(), trade.pnl_pct());
    }
}
```

**Pros**: DRY, extensible
**Cons**: More abstraction, trait already exists (`TradeResult` in cs-domain)

### Recommendation: Option A for CLI, Leverage Existing TradeResult Trait

The `cs_domain::TradeResult` trait already has `pnl()`, `symbol()`, etc. Use that:

```rust
use cs_domain::TradeResult; // The TRAIT, not an enum

fn print_backtest_summary<R: TradeResult>(result: &BacktestResult<R>) {
    let total_pnl: Decimal = result.results.iter().map(|r| r.pnl()).sum();
    let winners = result.results.iter().filter(|r| r.pnl() > Decimal::ZERO).count();
    // ...
}
```

## TradeExecutorFactory (New)

To avoid repeating dependency cloning, introduce a factory:

**File**: `cs-backtest/src/trade_executor_factory.rs`

```rust
use std::sync::Arc;
use cs_domain::{
    EquityDataRepository, OptionsDataRepository, TradeFactory,
    HedgeConfig, RollPolicy, RollableTrade,
};
use cs_analytics::PricingModel;

use crate::execution::{ExecutableTrade, ExecutionConfig};
use crate::trade_executor::TradeExecutor;
use crate::spread_pricer::SpreadPricer;
use crate::straddle_pricer::StraddlePricer;
use crate::iron_butterfly_pricer::IronButterflyPricer;
use crate::calendar_straddle_pricer::CalendarStraddlePricer;
use crate::timing_strategy::TimingStrategy;

/// Factory for creating type-specific TradeExecutors
///
/// Centralizes dependency management and configuration.
/// Each create_*() method returns a properly configured executor.
pub struct TradeExecutorFactory {
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    trade_factory: Arc<dyn TradeFactory>,
    exec_config: ExecutionConfig,
    pricing_model: PricingModel,
    // Optional configuration
    hedge_config: Option<HedgeConfig>,
    timing_strategy: Option<TimingStrategy>,
    roll_policy: Option<RollPolicy>,
}

impl TradeExecutorFactory {
    pub fn new(
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        trade_factory: Arc<dyn TradeFactory>,
        exec_config: ExecutionConfig,
    ) -> Self {
        Self {
            options_repo,
            equity_repo,
            trade_factory,
            exec_config,
            pricing_model: PricingModel::default(),
            hedge_config: None,
            timing_strategy: None,
            roll_policy: None,
        }
    }

    pub fn with_pricing_model(mut self, model: PricingModel) -> Self {
        self.pricing_model = model;
        self
    }

    pub fn with_hedging(mut self, config: HedgeConfig, timing: TimingStrategy) -> Self {
        self.hedge_config = Some(config);
        self.timing_strategy = Some(timing);
        self
    }

    pub fn with_roll_policy(mut self, policy: RollPolicy) -> Self {
        self.roll_policy = Some(policy);
        self
    }

    pub fn create_straddle_executor(&self) -> TradeExecutor<Straddle> {
        let pricer = StraddlePricer::new(
            SpreadPricer::new().with_pricing_model(self.pricing_model)
        );
        self.build_executor(pricer)
    }

    pub fn create_calendar_spread_executor(&self) -> TradeExecutor<CalendarSpread> {
        let pricer = SpreadPricer::new().with_pricing_model(self.pricing_model);
        self.build_executor(pricer)
    }

    pub fn create_iron_butterfly_executor(&self) -> TradeExecutor<IronButterfly> {
        let pricer = IronButterflyPricer::new(
            SpreadPricer::new().with_pricing_model(self.pricing_model)
        );
        self.build_executor(pricer)
    }

    pub fn create_calendar_straddle_executor(&self) -> TradeExecutor<CalendarStraddle> {
        let pricer = CalendarStraddlePricer::new(
            SpreadPricer::new().with_pricing_model(self.pricing_model)
        );
        self.build_executor(pricer)
    }

    fn build_executor<T>(&self, pricer: T::Pricer) -> TradeExecutor<T>
    where
        T: RollableTrade + ExecutableTrade,
    {
        let mut executor = TradeExecutor::<T>::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.exec_config.clone(),
        );

        if let Some(ref policy) = self.roll_policy {
            executor = executor.with_roll_policy(policy.clone());
        }
        if let (Some(ref hc), Some(ref ts)) = (&self.hedge_config, &self.timing_strategy) {
            executor = executor.with_hedging(hc.clone(), ts.clone());
        }

        executor
    }
}
```

## Files to Update

| File | Changes |
|------|---------|
| `cs-backtest/src/trade_executor_factory.rs` | NEW - Factory for creating executors |
| `cs-backtest/src/lib.rs` | Export TradeExecutorFactory |
| `cs-cli/src/commands/backtest.rs` | Update imports, match on spread type |
| `cs-cli/src/commands/rolling.rs` | Use TradeExecutorFactory |
| `cs-cli/src/output/mod.rs` | Generic print functions using TradeResult trait |

## Step-by-Step

### Step 1: Fix Imports

```rust
// Remove
use cs_backtest::{RollingExecutor, TradeResult};

// Add
use cs_backtest::TradeExecutor;
use cs_domain::TradeResult; // trait for generic output
```

### Step 2: Update Backtest Command

```rust
pub async fn run_backtest(config: BacktestConfig) -> Result<(), CliError> {
    let backtest = BacktestUseCase::new(...);

    match config.spread_type {
        SpreadType::CalendarSpread => {
            let opt_type = config.option_type.unwrap_or(OptionType::Call);
            let result = backtest.run_calendar_spread_backtest(opt_type, start, end, None).await?;
            print_backtest_result(&result, "Calendar Spread");
        }
        SpreadType::Straddle => {
            let result = backtest.run_straddle_backtest(start, end, None).await?;
            print_backtest_result(&result, "Straddle");
        }
        SpreadType::IronButterfly => {
            let result = backtest.run_iron_butterfly_backtest(start, end, None).await?;
            print_backtest_result(&result, "Iron Butterfly");
        }
        SpreadType::CalendarStraddle => {
            let result = backtest.run_calendar_straddle_backtest(start, end, None).await?;
            print_backtest_result(&result, "Calendar Straddle");
        }
    }

    Ok(())
}

fn print_backtest_result<R: cs_domain::TradeResult>(result: &BacktestResult<R>, name: &str) {
    println!("\n{} Backtest Results", name);
    println!("==================");
    println!("Total trades: {}", result.results.len());

    let total_pnl: Decimal = result.results.iter().map(|r| r.pnl()).sum();
    println!("Total P&L: ${:.2}", total_pnl);

    let winners = result.results.iter().filter(|r| r.pnl() > Decimal::ZERO).count();
    let win_rate = if result.results.is_empty() {
        0.0
    } else {
        winners as f64 / result.results.len() as f64 * 100.0
    };
    println!("Win rate: {:.1}%", win_rate);
}
```

### Step 3: Update Rolling Command

Use `TradeExecutorFactory` to simplify executor creation:

```rust
pub async fn run_rolling(config: RollingConfig) -> Result<(), CliError> {
    // Create factory with shared dependencies
    let factory = TradeExecutorFactory::new(
        options_repo,
        equity_repo,
        trade_factory,
        exec_config,
    )
    .with_pricing_model(config.pricing_model)
    .with_roll_policy(config.roll_policy)
    .with_hedging(config.hedge_config, config.timing_strategy);

    // Each trade type: create executor, execute, print
    match config.trade_type {
        TradeType::Straddle => {
            let result = factory.create_straddle_executor()
                .execute_rolling(&config.symbol, start, end, entry, exit).await;
            print_rolling_result(&result, "Straddle");
        }
        TradeType::CalendarSpread => {
            let result = factory.create_calendar_spread_executor()
                .execute_rolling(&config.symbol, start, end, entry, exit).await;
            print_rolling_result(&result, "Calendar Spread");
        }
        TradeType::IronButterfly => {
            let result = factory.create_iron_butterfly_executor()
                .execute_rolling(&config.symbol, start, end, entry, exit).await;
            print_rolling_result(&result, "Iron Butterfly");
        }
        TradeType::CalendarStraddle => {
            let result = factory.create_calendar_straddle_executor()
                .execute_rolling(&config.symbol, start, end, entry, exit).await;
            print_rolling_result(&result, "Calendar Straddle");
        }
    }

    Ok(())
}

fn print_rolling_result(result: &RollingResult, name: &str) {
    println!("\n{} Rolling Results", name);
    println!("==================");
    println!("Symbol: {}", result.symbol);
    println!("Rolls: {}", result.rolls.len());
    println!("Total P&L: ${:.2}", result.total_pnl);
    println!("Win rate: {:.1}%", result.win_rate * 100.0);

    if let Some(hedge_pnl) = result.total_hedge_pnl {
        println!("Hedge P&L: ${:.2}", hedge_pnl);
        println!("Total with hedge: ${:.2}", result.total_pnl + hedge_pnl);
    }
}
```

**Benefits of factory pattern:**
- Dependencies configured once, reused across all trade types
- Hedging/roll policy applied uniformly
- Each `create_*()` method handles type-specific pricer
- Clean separation: factory setup vs execution

## Testing

After changes:
```bash
cargo build -p cs-cli

# Test each spread type
./target/debug/cs backtest --spread calendar-spread --start 2024-01-01 --end 2024-03-01
./target/debug/cs backtest --spread straddle --start 2024-01-01 --end 2024-03-01
./target/debug/cs backtest --spread iron-butterfly --start 2024-01-01 --end 2024-03-01

# Test rolling
./target/debug/cs rolling --symbol AAPL --start 2024-01-01 --end 2024-03-01
```

## Notes

- The `TradeResult` trait in cs-domain provides common methods (`pnl()`, `symbol()`, etc.)
- Each specific result type (`StraddleResult`, etc.) implements this trait
- Generic functions can use `R: TradeResult` bound for shared output logic
- Type-specific details (like `iv_ratio` for calendars) need dedicated handling
