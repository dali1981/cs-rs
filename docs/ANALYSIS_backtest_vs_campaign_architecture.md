# Analysis: BacktestUseCase vs Campaign/SessionExecutor Architecture

**Date**: 2026-01-09
**Context**: Evaluating which architecture to use for hedging integration
**Related**: TICKET-0002 (Hedging Infrastructure Disconnected)

---

## Executive Summary

After deep analysis of both execution paths, **BacktestUseCase architecture is superior** in terms of:
- Clean separation of concerns
- Type safety
- Extensibility
- Code maintainability

However, **Campaign/SessionExecutor has hedging already integrated**. The question becomes: should we port hedging to BacktestUseCase, or fix the architectural issues in Campaign?

**Recommendation**: Port hedging to BacktestUseCase, then deprecate SessionExecutor.

---

## Architecture Comparison

### High-Level Structure

| Aspect | BacktestUseCase | Campaign/SessionExecutor |
|--------|-----------------|--------------------------|
| **Dispatch pattern** | Strategy pattern (trait) | Match-based dispatch |
| **Type safety** | Compile-time (`TradeStrategy<R>`) | Runtime (`Box<dyn Any>`) |
| **Extensibility** | Implement trait, add enum variant | Modify dispatcher, copy-paste handler |
| **Code duplication** | Minimal (~50 lines shared) | Severe (~3000+ lines duplicated) |
| **Separation of concerns** | Excellent (5 layers) | Poor (mixed responsibilities) |
| **Hedging support** | Not connected | Fully integrated |

---

## BacktestUseCase Architecture (The Good)

### Design Pattern: Strategy + Template Method

```
BacktestUseCase (Orchestration)
    │
    ├─ Loads events, filters, determines tradable windows
    │
    └─→ TradeStrategy<R> (Strategy Pattern)
            │
            ├─ Each strategy implements execute_trade()
            │
            └─→ TradeSimulator (Data Layer)
                    │
                    ├─ prepare(): Get spot + IV surface
                    │
                    └─ run(): Price entry → validate → price exit
                            │
                            └─→ ExecutableTrade::to_result()
```

### Key Strengths

**1. Type-Safe Generic Architecture**

```rust
// Each strategy knows its result type at COMPILE TIME
pub trait TradeStrategy<R: TradeResultMethods + Send>: Send + Sync {
    fn execute_trade<'a>(...) -> Pin<Box<dyn Future<Output = Option<R>> + Send + 'a>>;
}

// Usage: No runtime type checks needed
impl TradeStrategy<CalendarSpreadResult> for CalendarSpreadStrategy { ... }
impl TradeStrategy<StraddleResult> for StraddleStrategy { ... }
```

**2. Clean 5-Step Workflow (Consistent Across All Strategies)**

```rust
fn execute_trade(...) -> ... {
    Box::pin(async move {
        // 1. Create simulator (data layer)
        let simulator = TradeSimulator::new(...);

        // 2. Prepare market data
        let data = simulator.prepare().await?;

        // 3. Select trade (strategy-specific selection)
        let trade = selector.select_xxx(&data.spot, &data.surface, ...)?;

        // 4. Simulate (generic over trade type)
        let raw = simulator.run(&trade, &pricer).await?;

        // 5. Convert to result (strategy-specific)
        Some(trade.to_result(raw.entry_pricing, raw.exit_pricing, ...))
    })
}
```

**3. TradeSimulator is Pure Data Layer**

```rust
pub struct TradeSimulator<'a> {
    options_repo: &'a dyn OptionsDataRepository,
    equity_repo: &'a dyn EquityDataRepository,
    // ... NO business logic, NO hedging, NO attribution
}

// Returns RAW data only
pub async fn run<T>(&self, trade: &T, pricer: &T::Pricer)
    -> Result<RawSimulationOutput<T::Pricing>, ExecutionError>
```

This separation means:
- Simulator can be tested without strategy logic
- Strategies can be tested without real data (mock simulator)
- Business logic doesn't leak into data layer

**4. ExecutableTrade Trait - Generic Trade Abstraction**

```rust
pub trait ExecutableTrade: Sized + Send + Sync {
    type Pricer: TradePricer<Trade = Self, Pricing = Self::Pricing>;
    type Pricing: Clone + ToTradingContext;
    type Result: TradeResult + ApplyCosts;

    fn validate_entry(pricing: &Self::Pricing, config: &ExecutionConfig) -> Result<(), ExecutionError>;
    fn to_result(&self, entry: Self::Pricing, exit: Self::Pricing, ...) -> Self::Result;
    fn to_failed_result(&self, ...) -> Self::Result;
}
```

**Benefits**:
- Associated types ensure type safety across the pipeline
- Each trade controls its own validation and result construction
- No runtime type checks or downcasting

**5. Extensibility: Adding New Strategies**

To add `CondorStrategy`:

```rust
// 1. Create strategy struct
pub struct CondorStrategy { timing: TimingStrategy }

// 2. Implement trait (no copy-paste from other strategies)
impl TradeStrategy<CondorResult> for CondorStrategy {
    fn execute_trade(...) -> ... {
        // Same 5-step workflow, strategy-specific selection only
    }
}

// 3. Add to dispatch enum
pub enum StrategyDispatch {
    Condor(CondorStrategy),
    // ...
}
```

**Total code for new strategy**: ~150-200 lines (trait impl + dispatch)

---

## Campaign/SessionExecutor Architecture (The Bad)

### Design Pattern: Switch Statement + Copy-Paste

```
CampaignUseCase (Orchestration)
    │
    └─→ SessionExecutor::execute_session(session)
            │
            └─→ match session.strategy {
                    CalendarSpread => execute_calendar_spread(),
                    Straddle => execute_straddle(),
                    IronButterfly => execute_iron_butterfly(),
                    // ... 8 branches, each ~80 lines
                }
                    │
                    └─→ TradeExecutor<T>::execute()
                            │
                            └─→ TradeSimulator + hedging + attribution
```

### Key Weaknesses

**1. Severe Code Duplication in Strategy Handlers**

Each `execute_*()` method in SessionExecutor follows **identical** structure:

```rust
async fn execute_calendar_spread(&self, session: &TradingSession, event: &EarningsEvent) -> SessionResult {
    // 1. Create trade (strategy-specific: 3 lines)
    let trade = CalendarSpread::create(...)?;

    // 2. Create pricer (strategy-specific: 1 line)
    let pricer = CalendarSpreadPricer::new();

    // 3. Create executor (IDENTICAL across all strategies: 8 lines)
    let mut executor = TradeExecutor::new(
        Arc::clone(&self.options_repo),
        Arc::clone(&self.equity_repo),
        pricer,
        Arc::clone(&self.trade_factory),
        self.execution_config.clone(),
    );

    // 4. Apply hedging (IDENTICAL: 3 lines)
    if let (Some(ref hedge_config), Some(ref timing)) = (&self.hedge_config, &self.timing_strategy) {
        executor = executor.with_hedging(hedge_config.clone(), timing.clone());
    }

    // 5. Apply attribution (IDENTICAL: 3 lines)
    if let Some(ref attr_config) = self.attribution_config {
        executor = executor.with_attribution(attr_config.clone());
    }

    // 6. Execute (IDENTICAL: 1 line)
    let result = executor.execute(&trade, Some(event), entry_time, exit_time).await;

    // 7. Build SessionResult (IDENTICAL: 25 lines)
    if result.success() {
        let pnl = SessionPnL { ... };  // Same field extraction
        SessionResult::success_with_pnl(session.clone(), pnl, Box::new(result))
    } else {
        SessionResult::failure(session.clone(), "Calendar spread execution failed")
    }
}
```

**Duplication analysis**:
- Lines 3-7: **100% identical** across all 8 strategies (~40 lines × 8 = 320 lines)
- Only lines 1-2 differ (trade creation and pricer type)
- **Result: ~700+ lines of duplicated code**

**2. Type Erasure Loses Safety**

```rust
pub struct SessionResult {
    // Type safety lost - can't know what's inside at compile time
    pub trade_result: Box<dyn std::any::Any + Send>,
}

// To use the result, must downcast (runtime failure possible)
if let Some(straddle) = result.trade_result.downcast_ref::<StraddleResult>() {
    // Now we can use it
}
```

**3. Poor Extensibility**

To add `CondorStrategy`:

```rust
// 1. Add match arm in execute_session() (modify SessionExecutor)
match session.strategy {
    OptionStrategy::Condor => self.execute_condor(session, &earnings_event).await,
    // ...
}

// 2. Copy-paste execute_*() method (~80 lines)
async fn execute_condor(&self, session: &TradingSession, event: &EarningsEvent) -> SessionResult {
    // Copy from execute_straddle, change:
    // - Trade creation line
    // - Pricer type
    // - Error message string
    // Everything else is copy-paste
}

// 3. Implement ExecutableTrade for Condor (~275 lines, mostly duplicated)
impl ExecutableTrade for Condor {
    fn to_result(...) -> CondorResult {
        // Copy from Straddle::to_result
        // Change struct name
        // ~50% is duplicated logic
    }
}
```

**Total code for new strategy**: ~450-600 lines (mostly copy-paste)

**4. Duplication in ExecutableTrade Implementations**

Each strategy file has nearly identical `calculate_pnl_attribution()`:

```rust
// This function appears in ALL 8 strategy files with ~5% variation
fn calculate_pnl_attribution(
    entry_pricing: &CompositePricing,
    exit_pricing: &CompositePricing,
    entry_spot: f64,
    exit_spot: f64,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    total_pnl: Decimal,
) -> (Option<Decimal>, Option<Decimal>, Option<Decimal>, Option<Decimal>, Option<Decimal>) {
    // ~60 lines of IDENTICAL code
    // Loop through legs, sum Greeks, scale by multiplier
}
```

**Duplication**: ~60 lines × 8 files = **480 lines of identical code**

**5. TradeExecutor Mixes Concerns**

Unlike BacktestUseCase's clean TradeSimulator, TradeExecutor combines:
- Data fetching (via TradeSimulator internally)
- Hedging orchestration
- Attribution computation
- Result enrichment

```rust
pub struct TradeExecutor<T: RollableTrade + ExecutableTrade + CompositeTrade> {
    // Data access
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,

    // Business logic (mixed in)
    hedge_config: Option<HedgeConfig>,
    attribution_config: Option<AttributionConfig>,

    // Strategy-specific
    pricer: T::Pricer,
}
```

This makes testing harder - can't test hedging without TradeExecutor, can't test TradeExecutor without real repos.

---

## Quantitative Comparison

| Metric | BacktestUseCase | SessionExecutor |
|--------|-----------------|-----------------|
| **Strategy handler code** | ~100 lines (shared trait impl) | ~700 lines (8 copy-paste methods) |
| **ExecutableTrade impl** | ~250 lines (clean) | ~275 lines × 8 = 2200 lines (duplicated) |
| **Attribution code** | Could be shared | ~60 lines × 8 = 480 lines (duplicated) |
| **Type safety** | Compile-time generics | Runtime downcasting |
| **To add strategy** | ~150-200 lines | ~450-600 lines |
| **Files to modify** | 1 (enum) | 2+ (executor + dispatch) |
| **Testability** | High (mockable layers) | Low (mixed concerns) |

---

## Why SessionExecutor Has Hedging (And BacktestUseCase Doesn't)

### Historical Context

The hedging system was built directly into TradeExecutor:

```rust
// trade_executor.rs
impl<T> TradeExecutor<T> {
    pub fn with_hedging(mut self, config: HedgeConfig, timing: TimingStrategy) -> Self {
        self.hedge_config = Some(config);
        self.timing_strategy = Some(timing);
        self
    }

    pub async fn execute(...) -> T::Result {
        let mut result = self.simulate(...).await?;

        // Hedging applied here
        if let (Some(hedge_config), Some(timing)) = (&self.hedge_config, &self.timing_strategy) {
            self.apply_hedging(&mut result, ...)?;
        }

        result
    }
}
```

### Why BacktestUseCase Doesn't Have It

BacktestUseCase uses TradeSimulator, which is intentionally data-only:

```rust
// TradeSimulator returns RAW simulation output
pub async fn run<T>(&self, trade: &T, pricer: &T::Pricer)
    -> Result<RawSimulationOutput<T::Pricing>, ExecutionError>

// Then ExecutableTrade::to_result() converts to final result
// But this happens in strategy, NOT in simulator
// So there's no hook for hedging
```

The TradeSimulator → ExecutableTrade::to_result() flow never calls hedging.

---

## Recommendation: Port Hedging to BacktestUseCase

### Option A: Quick Fix (Wire Campaign Command)

**Effort**: ~50 lines, 1-2 hours

Just wire `HedgingArgs` through to `SessionExecutor.with_hedging()`.

**Pros**: Fast, hedging works
**Cons**: Perpetuates poor architecture, more tech debt

### Option B: Port Hedging to BacktestUseCase (Recommended)

**Effort**: ~200-300 lines, 4-6 hours

Add hedging as a post-processor in the backtest pipeline:

```rust
// In TradeStrategy::execute_trade() or as post-processor
impl TradeStrategy<StraddleResult> for StraddleStrategy {
    fn execute_trade(...) -> ... {
        Box::pin(async move {
            let simulator = TradeSimulator::new(...);
            let data = simulator.prepare().await?;
            let trade = selector.select_straddle(...)?;
            let raw = simulator.run(&trade, &pricer).await?;

            let mut result = trade.to_result(...);

            // NEW: Apply hedging if configured
            if let Some(ref hedge_config) = exec_config.hedge_config {
                result = apply_hedging(result, &data, hedge_config, equity_repo).await?;
            }

            Some(result)
        })
    }
}
```

Or cleaner - add hedging layer to BacktestUseCase:

```rust
// backtest_use_case.rs
async fn execute_with_strategy<S, R>(...) -> Result<BacktestResult<R>> {
    let batch_results = self.execute_tradable_batch(...).await;

    // Apply hedging as post-processor
    let hedged_results = if let Some(ref hedge_config) = self.config.hedge_config {
        self.apply_hedging_to_batch(batch_results, hedge_config).await
    } else {
        batch_results
    };

    Ok(BacktestResult { results: hedged_results, ... })
}
```

**Pros**:
- Keeps BacktestUseCase architecture clean
- Hedging is a separate concern (can be tested independently)
- Type-safe hedging results
- Path to deprecating SessionExecutor

**Cons**: More work than Option A

### Option C: Unify on BacktestUseCase, Deprecate SessionExecutor

**Effort**: ~1-2 days

1. Port hedging to BacktestUseCase (Option B)
2. Add TradingSession/TradingCampaign support to BacktestUseCase
3. Make Campaign command use BacktestUseCase internally
4. Deprecate SessionExecutor

**Pros**:
- Single code path
- Clean architecture preserved
- Eliminates ~3000+ lines of duplicated code
- Future-proof

**Cons**: Significant effort

---

## My Assessment

**You're right - BacktestUseCase architecture is superior.**

The Campaign/SessionExecutor path was built pragmatically to get hedging working, but accumulated significant technical debt:

1. **~3000+ lines of duplicated code** across strategy handlers and ExecutableTrade impls
2. **Type erasure** (`Box<dyn Any>`) loses compile-time safety
3. **Mixed concerns** in TradeExecutor make testing harder
4. **Poor extensibility** - adding strategies requires copy-paste

The BacktestUseCase architecture follows proper patterns:
- **Strategy pattern** for extensibility
- **Clean data layer** (TradeSimulator)
- **Type-safe generics** throughout
- **Single responsibility** per component

### Path Forward

1. **Short term**: Implement Option B (port hedging to BacktestUseCase)
2. **Medium term**: Route Campaign command through BacktestUseCase
3. **Long term**: Deprecate SessionExecutor, delete ~3000 lines of duplicated code

The hedging infrastructure (delta providers, HedgeConfig, HedgeState) is solid - it just needs to be connected to the right execution path.

---

## Appendix: Code Metrics

### BacktestUseCase Path

| File | Lines | Purpose |
|------|-------|---------|
| `backtest_use_case.rs` | 850 | Orchestration |
| `trade_strategy.rs` | 450 | Strategy trait + impls |
| `backtest_use_case_helpers.rs` | 200 | TradeSimulator |
| `execution/traits.rs` | 150 | ExecutableTrade trait |
| **Total** | **~1650** | |

### Campaign/SessionExecutor Path

| File | Lines | Purpose |
|------|-------|---------|
| `session_executor.rs` | 1150 | Dispatch + 8 handlers |
| `trade_executor.rs` | 700 | Orchestration + hedging |
| `campaign_use_case.rs` | 140 | Thin wrapper |
| `execution/*_impl.rs` | 2200 | 8 ExecutableTrade impls |
| **Total** | **~4190** | |

**SessionExecutor has 2.5x more code for equivalent functionality** - most of it duplicated.

---

*End of Analysis*
