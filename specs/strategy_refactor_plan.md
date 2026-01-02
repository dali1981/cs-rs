# Strategy Architecture Refactor Plan

## Overview

This plan addresses three key issues:
1. Remove 0.30 fallback IV - fail explicitly when IV cannot be determined
2. Rename concepts: OptionStrategy (trade type) vs SelectionStrategy (strike/expiration selection)
3. Unify `process_event` to delegate to the OptionStrategy

## Current State Analysis

### Issue 1: 0.30 Fallback IV

**Locations**:
- `cs-backtest/src/spread_pricer.rs:176` - Fallback when IV surface interpolation fails
- `cs-analytics/src/iv_model.rs:325` - Initial guess for sticky-delta iteration

**Problem**: Silent fallback masks data quality issues. A 30% IV assumption can lead to:
- Incorrect Greeks calculations
- Wrong P&L attribution
- Trades that should be skipped due to missing data

### Issue 2: Naming Confusion

**Current naming**:
```
"Strategy" (TradingStrategy trait) = How to SELECT strikes/expirations
"CalendarSpread/IronButterfly" = The actual TRADE STRUCTURE
```

**Better naming**:
```
"SelectionStrategy" = How to SELECT strikes/expirations (ATM, Delta, DeltaScan)
"OptionStrategy" = The actual TRADE STRUCTURE (CalendarSpread, IronButterfly)
```

### Issue 3: Inconsistent `session_date` Usage

**process_event (calendar spread)**:
```rust
// Uses session_date for option chain data
let chain_df = self.options_repo.get_option_bars(&event.symbol, session_date).await?;
let expirations = self.options_repo.get_available_expirations(&event.symbol, session_date).await;
```

**process_iron_butterfly_event**:
```rust
// Ignores session_date, uses entry_time.date_naive() instead
async fn process_iron_butterfly_event(&self, event: &EarningsEvent, _session_date: NaiveDate, ...)
let chain_result = self.options_repo.get_option_bars(&event.symbol, entry_time.date_naive()).await;
```

**Problem**: Inconsistent behavior between trade types. Should use entry_time consistently.

### Issue 4: min_by_key Complexity (iron_butterfly.rs:102-104)

**Current code**:
```rust
.min_by_key(|s| {
    let diff = s.value() - spot.value;
    (diff.abs() * Decimal::from(1000)).to_i64().unwrap_or(i64::MAX)
})
```

**What it does**: Converts Decimal to i64 for comparison because `min_by_key` requires `Ord`.
The `* 1000` preserves 3 decimal places.

**Problem**:
- Unnecessary complexity
- `unwrap_or(i64::MAX)` masks overflow issues
- `Decimal` supports `partial_cmp` - use `min_by` instead

**Fix**: Use `min_by` with `partial_cmp`:
```rust
.min_by(|a, b| {
    let a_diff = (a.value() - spot.value).abs();
    let b_diff = (b.value() - spot.value).abs();
    a_diff.partial_cmp(&b_diff).unwrap_or(std::cmp::Ordering::Equal)
})
```

---

## Refactor Plan

### Phase 1: Remove 0.30 Fallback IV

#### Step 1.1: Update spread_pricer.rs

**File**: `cs-backtest/src/spread_pricer.rs`

**Current** (lines 162-201):
```rust
if filtered.is_empty() {
    // No market data, use Black-Scholes with interpolated or estimated IV
    let ttm = self.calculate_ttm(pricing_time, expiration);

    // Use configured pricing model for interpolation, fall back to 30%
    let estimated_iv = iv_surface
        .and_then(|surface| {
            pricing_provider.get_iv(
                surface,
                strike.value(),
                expiration,
                option_type == OptionType::Call,
            )
        })
        .unwrap_or(0.30);  // <-- REMOVE THIS FALLBACK

    // ... rest uses estimated_iv
}
```

**Change to**:
```rust
if filtered.is_empty() {
    // No market data - try to interpolate from IV surface
    let ttm = self.calculate_ttm(pricing_time, expiration);

    let estimated_iv = iv_surface
        .and_then(|surface| {
            pricing_provider.get_iv(
                surface,
                strike.value(),
                expiration,
                option_type == OptionType::Call,
            )
        })
        .ok_or_else(|| PricingError::InvalidIV(format!(
            "Cannot determine IV for {} {} {} - no market data and interpolation failed",
            strike.value(), expiration, if option_type == OptionType::Call { "call" } else { "put" }
        )))?;

    // ... rest uses estimated_iv
}
```

#### Step 1.2: Update iv_model.rs (StickyDeltaPricing)

**File**: `cs-analytics/src/iv_model.rs`

**Current** (lines 322-325):
```rust
// Get ATM vol as initial guess
let mut sigma = delta_smile
    .get_atm_iv(expiration, is_call)
    .unwrap_or(0.30);
```

**Change to**:
```rust
// Get ATM vol as initial guess - required for iteration
let mut sigma = delta_smile
    .get_atm_iv(expiration, is_call)
    .ok_or_else(|| {
        // Return None to signal IV couldn't be determined
        // Caller will handle the missing IV
    })?;
```

Wait - this is inside `get_iv()` which returns `Option<f64>`. So we should:

**Change to**:
```rust
// Get ATM vol as initial guess - required for iteration
let mut sigma = delta_smile
    .get_atm_iv(expiration, is_call)?;  // Return None if no ATM vol available
```

#### Step 1.3: Add PricingError::NoIVData variant

**File**: `cs-backtest/src/spread_pricer.rs`

Add to `PricingError` enum:
```rust
#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    // ... existing variants ...
    #[error("No IV data available: {0}")]
    NoIVData(String),
}
```

---

### Phase 2: Rename Strategy Concepts

#### Step 2.1: Define New Trait Names

**File**: `cs-domain/src/strategies/mod.rs`

**Rename**:
- `TradingStrategy` -> `SelectionStrategy`
- Add new `OptionStrategy` concept (trait or enum)

**New structure**:
```rust
/// SelectionStrategy: Determines HOW to select strikes/expirations
/// This replaces the old "TradingStrategy" name
pub trait SelectionStrategy: Send + Sync {
    /// Select a calendar spread opportunity
    fn select_calendar_spread(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
        option_type: OptionType,
    ) -> Result<CalendarSpread, StrategyError>;

    /// Select an iron butterfly opportunity (optional, not all strategies support this)
    fn select_iron_butterfly(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
    ) -> Result<IronButterfly, StrategyError> {
        Err(StrategyError::UnsupportedStrategy("Iron butterfly not supported by this selection strategy".into()))
    }
}

/// OptionStrategy: The type of trade to execute
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionStrategy {
    CalendarSpread,
    IronButterfly,
}
```

#### Step 2.2: Update ATMStrategy

**File**: `cs-domain/src/strategies/atm.rs`

**Rename**: Keep struct name, but implement new trait
```rust
impl SelectionStrategy for ATMStrategy {
    fn select_calendar_spread(...) -> Result<CalendarSpread, StrategyError> {
        // Current implementation of select()
    }

    fn select_iron_butterfly(...) -> Result<IronButterfly, StrategyError> {
        // ATM strike selection for iron butterfly
        // Extract from IronButterflyStrategy
    }
}
```

#### Step 2.3: Update DeltaStrategy

**File**: `cs-domain/src/strategies/delta.rs`

Same pattern - implement `SelectionStrategy` trait.

#### Step 2.4: Move Iron Butterfly Selection Logic

**Current**: `IronButterflyStrategy` is a separate struct
**Change**: Selection logic moves into `SelectionStrategy::select_iron_butterfly`

The `IronButterflyStrategy` struct can be removed. Its selection logic (find ATM, calculate wings, snap to strikes) becomes part of `ATMStrategy::select_iron_butterfly()`.

---

### Phase 3: Unify process_event

#### Step 3.1: Create Unified Event Processor

**File**: `cs-backtest/src/backtest_use_case.rs`

**New method**:
```rust
async fn process_earnings_event(
    &self,
    event: &EarningsEvent,
    strategy: &dyn SelectionStrategy,
    option_strategy: OptionStrategy,
    option_type: Option<OptionType>,  // None for iron butterfly
) -> Result<TradeResult, TradeGenerationError> {
    // Use event-based timing consistently
    let entry_time = self.earnings_timing.entry_datetime(event);
    let entry_date = entry_time.date_naive();  // Use this for all data fetching

    // Get spot price
    let spot = self.equity_repo
        .get_spot_price(&event.symbol, entry_time)
        .await
        .map_err(|_| TradeGenerationError {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            reason: "NO_SPOT_PRICE".into(),
            details: Some(format!("No spot price at {}", entry_time)),
            phase: "spot_price".into(),
        })?;

    // Get option chain data using entry_date (not session_date)
    let chain_df = self.options_repo
        .get_option_bars(&event.symbol, entry_date)
        .await
        .map_err(|_| TradeGenerationError {
            symbol: event.symbol.clone(),
            // ...
        })?;

    // Build IV surface
    let iv_surface = build_iv_surface(&chain_df, spot.to_f64(), entry_time, &event.symbol);

    // Get expirations and strikes
    let expirations = self.options_repo
        .get_available_expirations(&event.symbol, entry_date)
        .await
        .unwrap_or_default();

    let strikes = if !expirations.is_empty() {
        self.options_repo
            .get_available_strikes(&event.symbol, expirations[0], entry_date)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Build chain data
    let chain_data = OptionChainData {
        expirations,
        strikes,
        deltas: None,
        volumes: None,
        iv_ratios: None,
        iv_surface,
    };

    // Delegate to appropriate strategy method based on OptionStrategy
    match option_strategy {
        OptionStrategy::CalendarSpread => {
            let option_type = option_type.expect("CalendarSpread requires option_type");
            let spread = strategy
                .select_calendar_spread(event, &spot, &chain_data, option_type)
                .map_err(|e| TradeGenerationError { /* ... */ })?;

            let exit_time = self.earnings_timing.exit_datetime(event);
            let executor = TradeExecutor::new(...)
                .with_pricing_model(self.config.pricing_model);

            let result = executor.execute_trade(&spread, event, entry_time, exit_time).await;

            if self.passes_iv_filter(&result) {
                Ok(TradeResult::CalendarSpread(result))
            } else {
                Err(TradeGenerationError {
                    reason: "IV_RATIO_FILTER".into(),
                    // ...
                })
            }
        }
        OptionStrategy::IronButterfly => {
            let butterfly = strategy
                .select_iron_butterfly(event, &spot, &chain_data)
                .map_err(|e| TradeGenerationError { /* ... */ })?;

            let exit_time = self.earnings_timing.exit_datetime(event);
            let executor = IronButterflyExecutor::new(...)
                .with_pricing_model(self.config.pricing_model);

            let result = executor.execute_trade(&butterfly, event, entry_time, exit_time).await;

            if result.success {
                Ok(TradeResult::IronButterfly(result))
            } else {
                Err(TradeGenerationError {
                    reason: result.failure_reason.map(|r| format!("{:?}", r)).unwrap_or("UNKNOWN".into()),
                    // ...
                })
            }
        }
    }
}
```

#### Step 3.2: Update execute() to Use Unified Processor

**File**: `cs-backtest/src/backtest_use_case.rs`

Remove separate `execute_calendar_spread()` and `execute_iron_butterfly()` methods.
Update `execute()` to:

```rust
pub async fn execute(
    &self,
    start_date: NaiveDate,
    end_date: NaiveDate,
    option_type: Option<OptionType>,  // None for iron butterfly
    on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
) -> Result<BacktestResult, BacktestError> {
    let strategy = self.create_selection_strategy();
    let option_strategy = self.config.option_strategy;  // New config field

    for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
        // ... load earnings, filter for entry ...

        let session_results: Vec<_> = if self.config.parallel {
            let futures: Vec<_> = to_enter.iter()
                .map(|event| self.process_earnings_event(event, &*strategy, option_strategy, option_type))
                .collect();
            futures::future::join_all(futures).await
        } else {
            // sequential processing
        };

        // ... collect results ...
    }
}
```

---

### Phase 4: Fix min_by_key Complexity

#### Step 4.1: Update iron_butterfly.rs

**File**: `cs-domain/src/strategies/iron_butterfly.rs`

**Current** (lines 100-108):
```rust
fn find_atm_strike(
    &self,
    spot: &SpotPrice,
    strikes: &[Strike],
) -> Result<Strike, StrategyError> {
    strikes
        .iter()
        .min_by_key(|s| {
            let diff = s.value() - spot.value;
            (diff.abs() * Decimal::from(1000)).to_i64().unwrap_or(i64::MAX)
        })
        .copied()
        .ok_or(StrategyError::NoStrikes)
}
```

**Change to**:
```rust
fn find_atm_strike(
    &self,
    spot: &SpotPrice,
    strikes: &[Strike],
) -> Result<Strike, StrategyError> {
    strikes
        .iter()
        .min_by(|a, b| {
            let a_diff = (a.value() - spot.value).abs();
            let b_diff = (b.value() - spot.value).abs();
            a_diff.partial_cmp(&b_diff).unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .ok_or(StrategyError::NoStrikes)
}
```

#### Step 4.2: Update snap_to_strike

**Current** (lines 115-125):
```rust
fn snap_to_strike(
    &self,
    target: Strike,
    available: &[Strike],
    round_up: bool,
) -> Result<Strike, StrategyError> {
    available
        .iter()
        .filter(|s| if round_up { **s >= target } else { **s <= target })
        .min_by_key(|s| {
            (s.value() - target.value()).abs() * Decimal::from(1000)
        })
        .copied()
        .ok_or(StrategyError::NoStrikes)
}
```

**Change to**:
```rust
fn snap_to_strike(
    &self,
    target: Strike,
    available: &[Strike],
    round_up: bool,
) -> Result<Strike, StrategyError> {
    available
        .iter()
        .filter(|s| if round_up { **s >= target } else { **s <= target })
        .min_by(|a, b| {
            let a_diff = (a.value() - target.value()).abs();
            let b_diff = (b.value() - target.value()).abs();
            a_diff.partial_cmp(&b_diff).unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .ok_or(StrategyError::NoStrikes)
}
```

#### Step 4.3: Similar Updates in atm.rs

**File**: `cs-domain/src/strategies/atm.rs`

Update any similar `min_by_key` patterns to use `min_by` with `partial_cmp`.

---

### Phase 5: Update Config and CLI

#### Step 5.1: Update BacktestConfig

**File**: `cs-backtest/src/config.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    // ... existing fields ...

    /// The option strategy (trade structure type)
    pub option_strategy: OptionStrategy,

    /// The selection strategy (how to pick strikes)
    pub selection_strategy: SelectionStrategyType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrategyType {
    #[default]
    ATM,
    Delta,
    DeltaScan,
}

// Remove IronButterfly from StrategyType - it's now an OptionStrategy
```

#### Step 5.2: Update CLI

**File**: `cs-cli/src/main.rs`

```rust
/// Option strategy (trade type)
#[arg(long, default_value = "calendar-spread")]
option_strategy: String,  // "calendar-spread" or "iron-butterfly"

/// Selection strategy (strike selection)
#[arg(long)]
selection_strategy: Option<String>,  // "atm", "delta", "delta-scan"
```

---

## Migration Steps

### Order of Implementation

1. **Phase 4** first (min_by_key fix) - Low risk, isolated change
2. **Phase 1** (Remove 0.30 fallback) - May cause more trade failures, test thoroughly
3. **Phase 2** (Rename strategies) - Breaking change to public API
4. **Phase 3** (Unify process_event) - Depends on Phase 2
5. **Phase 5** (Config/CLI) - Depends on Phase 2 and 3

### Breaking Changes

- `TradingStrategy` trait renamed to `SelectionStrategy`
- `TradingStrategy::select()` -> `SelectionStrategy::select_calendar_spread()`
- `IronButterflyStrategy` struct removed (logic merged into `ATMStrategy`)
- `StrategyType::IronButterfly` moved to `OptionStrategy::IronButterfly`
- More trades may fail due to IV fallback removal (desired behavior)

### Testing Requirements

1. Unit tests for new `SelectionStrategy` implementations
2. Integration tests comparing old vs new behavior
3. Backtest comparison to ensure no unexpected P&L differences
4. Test IV fallback removal with edge cases (missing data scenarios)

---

## Summary

| Change | Risk | Benefit |
|--------|------|---------|
| Remove 0.30 fallback | Medium | Explicit failures for bad data |
| Rename TradingStrategy | Low | Clearer domain model |
| Add OptionStrategy enum | Low | Better type safety |
| Unify process_event | Medium | DRY code, consistent behavior |
| Fix min_by_key | Low | Cleaner code, no overflow risk |
