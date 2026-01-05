# CS-RS Architecture Documentation

**Last Updated:** 2026-01-04
**Status:** Production-ready after Phase 1-6 refactoring

---

## Overview

CS-RS is an options trading backtest engine built in Rust, following Domain-Driven Design (DDD) principles with three orthogonal architectural dimensions:

- **WHEN**: Timing strategies (entry/exit timing)
- **WHERE**: Strike selection (ATM, Delta-based)
- **WHAT**: Trade structures (Calendar, Straddle, Iron Butterfly, etc.)

---

## Project Structure

```
cs-rs/
├── cs-domain/          # Domain entities, value objects, traits
├── cs-analytics/       # Pure analytics (pricing, IV, PnL)
├── cs-backtest/        # Backtest execution engine
└── cs-cli/             # Command-line interface
```

---

## Domain Layer (cs-domain)

### Core Entities

**Location**: `cs-domain/src/entities.rs`

Trade result entities for each strategy:
- `CalendarSpreadResult`
- `StraddleResult`
- `CalendarStraddleResult`
- `IronButterflyResult`

### Value Objects

**Location**: `cs-domain/src/value_objects.rs`

Immutable domain primitives:
- `Strike` - Option strike price with validation
- `SpotPrice` - Equity spot price with timestamp
- `Greeks` - Delta, gamma, theta, vega, rho
- `TimingConfig` - Entry/exit timing configuration
- `TradeSelectionCriteria` - DTE ranges, IV filters

### Timing Strategies (WHEN)

**Location**: `cs-domain/src/timing/`

```rust
/// Trait for calculating trade entry/exit timing
pub trait TradeTiming: Send + Sync {
    fn entry_date(&self, event: &EarningsEvent) -> NaiveDate;
    fn exit_date(&self, event: &EarningsEvent) -> NaiveDate;
    fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc>;
    fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc>;
}
```

**Implementations**:
- `EarningsTradeTiming` - Enter before earnings, exit after
- `StraddleTradeTiming` - Enter N days before, exit M days before earnings
- `PostEarningsStraddleTiming` - Enter after earnings, hold for N days

### Strike Selection (WHERE)

**Location**: `cs-domain/src/strike_selection/`

```rust
/// Trait for selecting strikes and trade structures
pub trait StrikeSelector: Send + Sync {
    fn select_calendar_spread(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        option_type: OptionType,
        criteria: &ExpirationCriteria,
    ) -> Result<CalendarSpread, SelectionError>;

    fn select_straddle(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        min_dte: i32,
    ) -> Result<Straddle, SelectionError>;

    // ... other trade types
}
```

**Implementations**:
- `ATMStrategy` - Selects at-the-money strikes
- `DeltaStrategy` - Selects strikes at target delta (calendar spreads only)
  - Delegates to `ATMStrategy` for straddles/butterflies

**Key Design**: Uses `IVSurface` directly with helper methods:
```rust
impl IVSurface {
    pub fn expirations(&self) -> Vec<NaiveDate>;
    pub fn strikes(&self) -> Vec<Strike>;
}
```

---

## Analytics Layer (cs-analytics)

**Location**: `cs-analytics/src/`

### IV Surface

**File**: `iv_surface.rs`

```rust
pub struct IVSurface {
    points: Vec<IVPoint>,
    spot_price: f64,
    timestamp: DateTime<Utc>,
}

pub struct IVPoint {
    strike: f64,
    expiration: NaiveDate,
    option_type: OptionType,
    iv: f64,
    delta: Option<f64>,
}
```

**Helper Methods** (Added in Phase 3):
- `expirations()` - Get unique expirations from surface
- `strikes()` - Get unique strikes from surface

### Pricing Models

**File**: `iv_model.rs`

```rust
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingModel {
    StickyStrike,
    #[default]
    StickyMoneyness,
    StickyDelta,
}
```

Determines how to interpolate IV when pricing options at strikes/expirations without direct market quotes.

### Interpolation Modes

**File**: `vol_slice.rs`

```rust
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterpolationMode {
    #[default]
    Linear,  // Linear interpolation in delta-space
    SVI,     // SVI parametric fit
}
```

### PnL Attribution

**File**: `pnl_attribution.rs` (Moved from cs-domain in Phase 1)

```rust
pub fn calculate_pnl_attribution(
    entry_greeks: &Greeks,
    exit_greeks: &Greeks,
    entry_price: Decimal,
    exit_price: Decimal,
    spot_entry: f64,
    spot_exit: f64,
    days_elapsed: f64,
) -> PnLAttribution;
```

Pure stateless functions for PnL decomposition into:
- Delta P&L (directional move)
- Gamma P&L (curvature)
- Theta P&L (time decay)
- Vega P&L (IV change)

---

## Backtest Engine (cs-backtest)

### Configuration

**File**: `cs-backtest/src/config.rs`

```rust
pub struct BacktestConfig {
    pub spread: SpreadType,           // Enum: Calendar, Straddle, etc.
    pub selection_strategy: SelectionType,  // Enum: ATM, Delta, DeltaScan
    pub pricing_model: PricingModel,   // Enum from cs-analytics
    pub vol_model: InterpolationMode,  // Enum from cs-analytics
    pub strike_match_mode: StrikeMatchMode,  // Enum: SameStrike, SameDelta
    // ... other fields
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpreadType {
    #[default]
    Calendar,
    IronButterfly,
    Straddle,
    CalendarStraddle,
    PostEarningsStraddle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionType {
    #[default]
    #[serde(rename = "atm")]
    ATM,
    Delta,
    DeltaScan,
}
```

**Key Improvement (Phase 5)**: All configuration uses proper Rust enums instead of strings, providing compile-time type safety and eliminating runtime parsing.

### Unified Executor (Phase 4)

**File**: `cs-backtest/src/unified_executor.rs`

The unified executor implements a **facade pattern** that delegates to specialized executors while providing a single entry point.

```rust
pub struct UnifiedExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    calendar_executor: TradeExecutor<O, E>,
    straddle_executor: StraddleExecutor<O, E>,
    calendar_straddle_executor: CalendarStraddleExecutor<O, E>,
    iron_butterfly_executor: IronButterflyExecutor<O, E>,
    pricing_model: PricingModel,
    max_entry_iv: Option<f64>,
}

pub enum TradeStructure {
    CalendarSpread(OptionType),
    Straddle,
    CalendarStraddle,
    IronButterfly { wing_width: Decimal },
}

pub enum TradeResult {
    CalendarSpread(CalendarSpreadResult),
    Straddle(StraddleResult),
    CalendarStraddle(CalendarStraddleResult),
    IronButterfly(IronButterflyResult),
}
```

**Key Method**:
```rust
pub async fn execute_with_selection(
    &self,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    entry_surface: &IVSurface,  // KEY: Pre-built, passed in
    selector: &dyn StrikeSelector,
    structure: TradeStructure,
    criteria: &ExpirationCriteria,
) -> TradeResult
```

### IV Surface Optimization (Phase 4)

**Critical Performance Improvement**: IV surface is built **once** at entry time and reused for both strike selection AND entry pricing.

**Before (wasteful)**:
```
process_event()
  ├─ build_iv_surface(entry_time)  // Build #1 - for selection
  ├─ selector.select(surface)
  └─ executor.execute()
      ├─ build_iv_surface(entry_time)  // Build #2 - for entry pricing (DUPLICATE!)
      └─ build_iv_surface(exit_time)   // Build #3 - for exit pricing
```

**After (optimized)**:
```
process_event_unified()
  ├─ build_iv_surface(entry_time)  // Build #1 - for selection AND entry
  ├─ selector.select(entry_surface)  // REUSE
  └─ executor.execute(..., entry_surface)  // REUSE
      └─ build_iv_surface(exit_time)   // Build #2 - for exit (different time)
```

**Savings**: **33% reduction** in IV surface builds (from 3 to 2 per trade)

**Implementation**:
```rust
// In BacktestUseCase
pub async fn process_event_unified(
    &self,
    event: &EarningsEvent,
    selector: &dyn StrikeSelector,
    structure: TradeStructure,
) -> TradeResult {
    let entry_time = self.earnings_timing.entry_datetime(event);
    let exit_time = self.earnings_timing.exit_datetime(event);

    // Build IV surface ONCE for entry
    let entry_surface = match build_iv_surface_minute_aligned(
        &entry_chain,
        self.equity_repo.as_ref(),
        &event.symbol,
    ).await {
        Some(surface) => surface,
        None => return self.create_failed_result(...),
    };

    // Create unified executor
    let executor = UnifiedExecutor::new(...)
        .with_pricing_model(self.config.pricing_model)
        .with_max_entry_iv(self.config.max_entry_iv);

    // Execute with pre-built entry surface (REUSE!)
    executor.execute_with_selection(
        event,
        entry_time,
        exit_time,
        &entry_surface,  // Passed in, no rebuild
        selector,
        structure,
        &criteria,
    ).await
}
```

### Specialized Executors (Still Used)

**Files**:
- `trade_executor.rs` - Calendar spread execution
- `straddle_executor.rs` - Straddle execution
- `calendar_straddle_executor.rs` - Calendar straddle execution
- `iron_butterfly_executor.rs` - Iron butterfly execution

**Important**: These are **NOT deleted** in Phase 4.5. They are still used by `UnifiedExecutor` as delegates (facade pattern). Each contains trade-specific logic for:
- Option contract selection
- Pricing with Greeks
- PnL calculation
- Result construction

### Backtest Use Case

**File**: `cs-backtest/src/backtest_use_case.rs`

Main orchestrator with 5 execute methods (all now using optimized unified flow):
- `execute_calendar_spread()`
- `execute_straddle()`
- `execute_post_earnings_straddle()`
- `execute_calendar_straddle()`
- `execute_iron_butterfly()`

**Pattern** (consistent across all methods):
```rust
async fn execute_calendar_spread(...) -> Result<BacktestResult, BacktestError> {
    // Create selector and structure once
    let selector = self.create_selector();
    let structure = TradeStructure::CalendarSpread(option_type);

    for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
        // Load events...

        // Process events using OPTIMIZED unified executor
        let session_results: Vec<_> = if self.config.parallel {
            let futures: Vec<_> = to_enter
                .iter()
                .map(|event| self.process_event_unified(event, &*selector, structure))
                .collect();
            futures::future::join_all(futures).await
        } else {
            let mut results = Vec::new();
            for event in &to_enter {
                results.push(self.process_event_unified(event, &*selector, structure).await);
            }
            results
        };

        // Collect results...
    }
}
```

---

## CLI Layer (cs-cli)

### Configuration Loading

**File**: `cs-cli/src/config.rs`

Layered configuration with proper enums (Phase 5):

```rust
pub struct AppConfig {
    pub paths: PathsConfig,
    pub timing: TimingConfig,
    pub selection: SelectionConfig,
    pub strategy: StrategyConfig,
    pub pricing: PricingConfig,
    pub strike_match_mode: StrikeMatchMode,  // Enum, not String
    // ...
}

pub struct StrategyConfig {
    pub spread_type: SpreadType,       // Enum, not String
    pub selection_type: SelectionType, // Enum, not String
    pub target_delta: f64,
    // ...
}

pub struct PricingConfig {
    pub model: PricingModel,           // Enum, not String
    pub vol_model: InterpolationMode,  // Enum, not String
}
```

**Configuration Priority** (highest to lowest):
1. CLI arguments
2. Strategy config file (`--conf`)
3. System config (`~/.config/cs/system.toml`)
4. Code defaults

**Conversion to BacktestConfig**:
```rust
impl AppConfig {
    pub fn to_backtest_config(&self) -> cs_backtest::BacktestConfig {
        cs_backtest::BacktestConfig {
            // Direct enum assignment (no string parsing!)
            spread: self.strategy.spread_type,
            selection_strategy: self.strategy.selection_type,
            pricing_model: self.pricing.model,
            vol_model: self.pricing.vol_model,
            strike_match_mode: self.strike_match_mode,
            // ...
        }
    }
}
```

### CLI Overrides

**File**: `cs-cli/src/cli_args.rs`

All fields are `Option<T>` with `skip_serializing_if = "Option::is_none"` to ensure only explicitly-provided CLI args override config file values.

**CLI String Arguments**: While the internal config uses enums, CLI arguments are still strings that must match the serde serialization format:

```bash
# Correct - use underscores to match snake_case enum serialization
--spread calendar_straddle
--selection atm
--pricing-model sticky_moneyness
--vol-model svi

# Incorrect - hyphens in enum values won't deserialize
--spread calendar-straddle  # ❌ Use calendar_straddle
--pricing-model sticky-moneyness  # ❌ Use sticky_moneyness
```

**Valid Enum Values**:
- `--spread`: `calendar`, `straddle`, `calendar_straddle`, `iron_butterfly`, `post_earnings_straddle`
- `--selection`: `atm`, `delta`, `delta_scan`
- `--pricing-model`: `sticky_strike`, `sticky_moneyness`, `sticky_delta`
- `--vol-model`: `linear`, `svi`
- `--strike-match-mode`: `same_strike`, `same_delta`

**Removed in Phase 5**:
- `CliStraddle` struct (duplicate of fields in `CliStrategy`)
- `target_delta` from `SelectionConfig` (duplicate, kept only in `StrategyConfig`)

---

## Design Patterns

### 1. Strategy Pattern
- `TradeTiming` trait with multiple implementations
- `StrikeSelector` trait with ATM and Delta strategies

### 2. Facade Pattern
- `UnifiedExecutor` provides single interface to multiple specialized executors

### 3. Repository Pattern
- `OptionsDataRepository` trait
- `EquityDataRepository` trait
- `EarningsRepository` trait

### 4. Dependency Injection
- Repositories injected into use cases
- Configured via constructor

### 5. Builder Pattern
- `UnifiedExecutor::new().with_pricing_model(...).with_max_entry_iv(...)`

---

## Testing Strategy

### Unit Tests

**Location**: `cs-backtest/tests/test_unified_executor.rs`

Tests unified executor with real data:
- `test_unified_executor_calendar_spread()` - Verifies calendar spread execution
- `test_unified_executor_straddle()` - Verifies straddle execution

**Key Verification**: Both tests confirm IV surface optimization is working (build once, reuse).

### Integration Tests

Run full backtest via CLI:
```bash
./target/debug/cs backtest \
  --start 2025-11-01 --end 2025-11-07 \
  --symbols CRBG \
  --spread calendar \
  --selection atm \
  --option-type call
```

Verifies:
- Enum deserialization from CLI args
- Configuration loading and conversion
- End-to-end execution with optimized flow

---

## Performance Characteristics

### IV Surface Building
- **Before**: 3 builds per trade (selection + entry + exit)
- **After**: 2 builds per trade (entry+selection reuse + exit)
- **Improvement**: 33% reduction in expensive computation

### Parallel Execution
- Configurable via `parallel: bool` in config
- Uses `futures::join_all` for concurrent event processing
- Sequential fallback for debugging

### Memory Efficiency
- IV surfaces contain only necessary points
- Results stored as enums (type-safe, no boxing overhead)
- Decimal arithmetic for precise monetary calculations

---

## Type Safety Improvements (Phase 5)

### Before: String-Based Configuration
```rust
pub struct StrategyConfig {
    pub spread_type: String,           // Runtime parsing required
    pub selection_type: String,        // Runtime parsing required
}

// In conversion:
spread: cs_backtest::SpreadType::from_string(&self.strategy.spread_type),  // Can fail at runtime
```

### After: Enum-Based Configuration
```rust
pub struct StrategyConfig {
    pub spread_type: SpreadType,       // Compile-time type safety
    pub selection_type: SelectionType, // Compile-time type safety
}

// In conversion:
spread: self.strategy.spread_type,     // Direct assignment, no parsing
```

**Benefits**:
- Compile-time validation
- No runtime string parsing overhead
- Impossible to pass invalid values
- Better IDE autocomplete and error messages

---

## TradeResult Simplification (Phase 6)

### Problem: Over-Engineered 3-Level Structure

**Before**: Nested enums created verbose pattern matching
```rust
pub enum TradeResult {
    Success(SuccessfulTrade),    // Wrapper layer
    Failure(FailedTrade),
}

pub enum SuccessfulTrade {       // Redundant middle layer
    CalendarSpread(CalendarSpreadResult),
    Straddle(StraddleResult),
    CalendarStraddle(CalendarStraddleResult),
    IronButterfly(IronButterflyResult),
}
```

**Usage**: Extremely verbose, hard to read
```rust
// 3 levels deep - ugly!
match result {
    TradeResult::Success(SuccessfulTrade::CalendarSpread(r)) => { ... }
    TradeResult::Success(SuccessfulTrade::Straddle(r)) => { ... }
    TradeResult::Failure(f) => { ... }
}
```

### Solution: Flattened 2-Level Structure

**After**: Direct enum variants, no wrapper
```rust
pub enum TradeResult {
    CalendarSpread(CalendarSpreadResult),
    Straddle(StraddleResult),
    CalendarStraddle(CalendarStraddleResult),
    IronButterfly(IronButterflyResult),
    Failed(FailedTrade),         // No dummy values needed
}
```

**Usage**: Clean and readable
```rust
// 2 levels - much better!
match result {
    TradeResult::CalendarSpread(r) => { ... }
    TradeResult::Straddle(r) => { ... }
    TradeResult::Failed(f) => { ... }
}
```

### Key Improvement: No Dummy Values

**Failed Trade Structure**:
```rust
pub struct FailedTrade {
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub trade_structure: TradeStructure,
    pub reason: FailureReason,
    pub phase: String,              // "selection", "entry_pricing", etc.
    pub details: Option<String>,
}
// Note: No Strike, no prices - only metadata!
```

**Before Phase 6**: Failed trades required dummy values
```rust
// ❌ Had to use fake data
let dummy_strike = Strike::new(Decimal::ONE).unwrap();  // Not a real strike!
let dummy_price = Decimal::ZERO;                         // Not a real price!
```

**After Phase 6**: Type system prevents accessing non-existent data
```rust
// ✅ Failed trades don't have strikes
pub fn strike(&self) -> Option<Strike> {
    match self {
        TradeResult::CalendarSpread(r) => Some(r.strike),
        TradeResult::Straddle(r) => Some(r.strike),
        TradeResult::CalendarStraddle(r) => Some(r.short_strike),
        TradeResult::IronButterfly(r) => Some(r.center_strike),
        TradeResult::Failed(_) => None,  // No dummy value!
    }
}
```

### Benefits

**Code Quality**:
- ✅ **50% shorter** pattern matches (from ~100 chars to ~50 chars per line)
- ✅ **No redundant layer** - `SuccessfulTrade` provided zero value
- ✅ **Cleaner imports** - no need to import intermediate enum
- ✅ **Easier to read** - pattern matching is straightforward

**Type Safety**:
- ✅ **No dummy values** - Failed trades don't have strikes/prices
- ✅ **Compile-time checking** - Can't access strike on failed trade without Option
- ✅ **Self-documenting** - `Option<Strike>` clearly shows strike may not exist

**Files Modified**:
- `cs-backtest/src/unified_executor.rs` - Removed `SuccessfulTrade` enum
- `cs-backtest/src/backtest_use_case.rs` - Updated all pattern matches
- `cs-backtest/tests/test_unified_executor.rs` - Simplified test assertions
- `cs-cli/src/main.rs` - Cleaned up 11 pattern match locations

**Code Reduction**: ~470 lines removed (redundant enum + verbose pattern matches)

---

## Key Files Summary

| File | Purpose | Key Trait/Struct |
|------|---------|------------------|
| `cs-domain/src/timing/mod.rs` | Timing trait + implementations | `TradeTiming` |
| `cs-domain/src/strike_selection/mod.rs` | Strike selection trait | `StrikeSelector` |
| `cs-analytics/src/pnl_attribution.rs` | PnL decomposition | `calculate_pnl_attribution()` |
| `cs-analytics/src/iv_surface.rs` | IV surface with helpers | `IVSurface` |
| `cs-backtest/src/unified_executor.rs` | Unified facade | `UnifiedExecutor` |
| `cs-backtest/src/backtest_use_case.rs` | Main orchestrator | `BacktestUseCase` |
| `cs-backtest/src/config.rs` | Type-safe config | `BacktestConfig` |
| `cs-cli/src/config.rs` | CLI config with enums | `AppConfig` |

---

## Migration Notes

### Phase 1-5 Refactoring Timeline

1. **Phase 1**: Moved `pnl_calculator` to `cs-analytics` (pure stateless functions)
2. **Phase 2**: Created `TradeTiming` trait, unified timing implementations
3. **Phase 3**: Refactored `StrikeSelector` trait, added `IVSurface` helper methods
4. **Phase 4**: Created `UnifiedExecutor`, implemented IV surface optimization (33% reduction)
5. **Phase 5**: Converted all config strings to proper Rust enums
6. **Phase 6**: Simplified `TradeResult` from 3-level to 2-level structure, eliminated dummy values

### Breaking Changes (All Committed)

- Import paths changed for PnL attribution (now in `cs-analytics`)
- `TradeTiming` trait required for custom timing implementations
- `StrikeSelector` trait signature changed (uses `IVSurface` directly)
- Configuration file format changed (strings → enums in TOML)
- Old `from_string()` methods still exist for backwards compatibility but are deprecated
- **Phase 6**: `TradeResult` enum flattened (removed `SuccessfulTrade` wrapper)
  - Pattern matches changed from `TradeResult::Success(SuccessfulTrade::X(r))` to `TradeResult::X(r)`
  - `TradeResult::Failure` renamed to `TradeResult::Failed`
  - `strike()` method now returns `Option<Strike>` instead of `Strike`

---

## Future Enhancements

### Potential Optimizations
- [ ] Cache IV surfaces by (symbol, timestamp) key
- [ ] Parallel IV surface building across multiple symbols
- [ ] Incremental Greeks calculation (reuse computations)

### Potential Features
- [ ] Live trading mode (real-time data ingestion)
- [ ] Custom strike selection strategies (plugin system)
- [ ] Advanced PnL attribution (higher-order Greeks)

---

## References

### Design Patterns
- Fowler, Martin. "Patterns of Enterprise Application Architecture"
- Gang of Four. "Design Patterns: Elements of Reusable Object-Oriented Software"

### Domain-Driven Design
- Evans, Eric. "Domain-Driven Design: Tackling Complexity in the Heart of Software"
- Vernon, Vaughn. "Implementing Domain-Driven Design"

### Rust Best Practices
- [The Rust Book](https://doc.rust-lang.org/book/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
