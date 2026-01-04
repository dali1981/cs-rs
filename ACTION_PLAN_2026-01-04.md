# Action Plan: Code Deduplication and Consistency Fixes

**Created:** 2026-01-04
**Branch:** code/review
**Based on:** CODE_REVIEW_REPORT.md

---

## Overview

This plan addresses code duplication and minor inconsistencies identified in the code review. Tasks are organized by priority and dependency order.

**Estimated Total Effort:** ~4-6 hours of focused work
**Lines to Remove:** ~500-800 (net reduction after refactoring)

---

## Phase 1: Strategy Module Refactoring (High Priority)

### Task 1.1: Extract `select_expirations()` to Shared Module

**Goal:** Remove duplicate function from `atm.rs` and `delta.rs`

**Files to modify:**
- `cs-domain/src/strategies/mod.rs` (add function)
- `cs-domain/src/strategies/atm.rs` (remove function, update imports)
- `cs-domain/src/strategies/delta.rs` (remove function, update imports)

**Implementation:**

1. Add to `cs-domain/src/strategies/mod.rs`:
```rust
/// Select short and long expirations for calendar/diagonal spreads
///
/// # Arguments
/// * `expirations` - Available expiration dates
/// * `reference_date` - Date to calculate DTE from (typically earnings date)
/// * `min_short_dte` / `max_short_dte` - DTE range for short leg
/// * `min_long_dte` / `max_long_dte` - DTE range for long leg
///
/// # Returns
/// Tuple of (short_expiration, long_expiration)
pub fn select_expirations(
    expirations: &[NaiveDate],
    reference_date: NaiveDate,
    min_short_dte: i32,
    max_short_dte: i32,
    min_long_dte: i32,
    max_long_dte: i32,
) -> Result<(NaiveDate, NaiveDate), StrategyError> {
    if expirations.len() < 2 {
        return Err(StrategyError::InsufficientExpirations {
            needed: 2,
            available: expirations.len(),
        });
    }

    let mut sorted: Vec<_> = expirations.iter().collect();
    sorted.sort();

    // Find short expiry
    let short_exp = sorted
        .iter()
        .find(|&&exp| {
            let dte = (*exp - reference_date).num_days();
            dte >= min_short_dte as i64 && dte <= max_short_dte as i64
        })
        .ok_or(StrategyError::NoExpirations)?;

    // Find long expiry
    let long_exp = sorted
        .iter()
        .find(|&&exp| {
            if exp <= short_exp {
                return false;
            }
            let dte = (*exp - reference_date).num_days();
            dte >= min_long_dte as i64 && dte <= max_long_dte as i64
        })
        .ok_or(StrategyError::InsufficientExpirations {
            needed: 2,
            available: 1,
        })?;

    Ok((**short_exp, **long_exp))
}
```

2. Update `atm.rs`:
   - Remove local `select_expirations` function (lines 212-255)
   - Add `use super::select_expirations;` or call as `super::select_expirations(...)`

3. Update `delta.rs`:
   - Remove local `select_expirations` function (lines 196-239)
   - Add `use super::select_expirations;` or call as `super::select_expirations(...)`

**Lines removed:** ~45

---

### Task 1.2: Extract ATM Strike Finder Utility

**Goal:** Create shared function for finding closest strike to spot

**Files to modify:**
- `cs-domain/src/strategies/mod.rs` (add function)
- `cs-domain/src/strategies/atm.rs` (use shared function)
- `cs-domain/src/strategies/delta.rs` (use shared function)
- `cs-domain/src/strategies/straddle.rs` (use shared function)
- `cs-domain/src/strategies/iron_butterfly.rs` (use shared function)

**Implementation:**

1. Add to `cs-domain/src/strategies/mod.rs`:
```rust
/// Find the strike closest to the given spot price
///
/// # Arguments
/// * `strikes` - Available strikes
/// * `spot` - Current spot price
///
/// # Returns
/// The strike closest to spot, or error if no strikes available
pub fn find_closest_strike(strikes: &[Strike], spot: f64) -> Result<Strike, StrategyError> {
    strikes
        .iter()
        .min_by(|a, b| {
            let a_diff = (f64::from(**a) - spot).abs();
            let b_diff = (f64::from(**b) - spot).abs();
            a_diff.partial_cmp(&b_diff).unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .ok_or(StrategyError::NoStrikes)
}
```

2. Update each strategy file to use `super::find_closest_strike()` or import it

**Lines removed:** ~30

---

## Phase 2: Pricer Consistency Fix (Low Priority)

### Task 2.1: Fix Hardcoded Risk-Free Rate

**Goal:** Use configured rate instead of hardcoded `0.0`

**Files to modify:**
- `cs-backtest/src/straddle_pricer.rs`
- `cs-backtest/src/iron_butterfly_pricer.rs`
- `cs-backtest/src/calendar_straddle_pricer.rs`

**Implementation:**

Option A (Simple): Store rate in pricer struct
```rust
pub struct StraddlePricer {
    spread_pricer: SpreadPricer,
    risk_free_rate: f64,  // Add field
}

impl StraddlePricer {
    pub fn new(spread_pricer: SpreadPricer) -> Self {
        Self {
            spread_pricer,
            risk_free_rate: 0.05,  // Default
        }
    }

    pub fn with_risk_free_rate(mut self, rate: f64) -> Self {
        self.risk_free_rate = rate;
        self
    }
}
```

Option B (Better): Get rate from SpreadPricer
```rust
// In SpreadPricer, add getter:
pub fn risk_free_rate(&self) -> f64 {
    self.bs_config.risk_free_rate
}

// In other pricers, use:
let pricing_provider = self.spread_pricer
    .pricing_model()
    .to_provider_with_rate(self.spread_pricer.risk_free_rate());
```

**Recommendation:** Option B - single source of truth

**Files to change:**
1. `spread_pricer.rs`: Add `pub fn risk_free_rate(&self) -> f64`
2. `straddle_pricer.rs:63`: Change `0.0` to `self.spread_pricer.risk_free_rate()`
3. `iron_butterfly_pricer.rs:61`: Same change
4. `calendar_straddle_pricer.rs:71`: Same change

---

## Phase 3: Remove Deprecated Code (Low Priority)

### Task 3.1: Remove Deprecated `calculate_leg_attribution`

**Goal:** Remove deprecated wrapper function

**File:** `cs-backtest/src/iron_butterfly_executor.rs`

**Implementation:**

1. Remove deprecated function (lines 346-366)

2. Update `calculate_pnl_attribution` to call domain function directly:

```rust
fn calculate_pnl_attribution(
    entry_pricing: &IronButterflyPricing,
    exit_pricing: &IronButterflyPricing,
    entry_spot: f64,
    exit_spot: f64,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    total_pnl: Decimal,
) -> (Option<Decimal>, Option<Decimal>, Option<Decimal>, Option<Decimal>, Option<Decimal>) {
    let spot_change = exit_spot - entry_spot;
    let days_held = (exit_time - entry_time).num_hours() as f64 / 24.0;

    // Short call
    let sc = cs_domain::calculate_option_leg_pnl(
        entry_pricing.short_call.greeks.as_ref(),
        entry_pricing.short_call.iv,
        exit_pricing.short_call.iv,
        spot_change,
        days_held,
        -1.0,
    );

    // Short put
    let sp = cs_domain::calculate_option_leg_pnl(
        entry_pricing.short_put.greeks.as_ref(),
        entry_pricing.short_put.iv,
        exit_pricing.short_put.iv,
        spot_change,
        days_held,
        -1.0,
    );

    // Long call
    let lc = cs_domain::calculate_option_leg_pnl(
        entry_pricing.long_call.greeks.as_ref(),
        entry_pricing.long_call.iv,
        exit_pricing.long_call.iv,
        spot_change,
        days_held,
        1.0,
    );

    // Long put
    let lp = cs_domain::calculate_option_leg_pnl(
        entry_pricing.long_put.greeks.as_ref(),
        entry_pricing.long_put.iv,
        exit_pricing.long_put.iv,
        spot_change,
        days_held,
        1.0,
    );

    let delta_pnl = sc.delta + sp.delta + lc.delta + lp.delta;
    let gamma_pnl = sc.gamma + sp.gamma + lc.gamma + lp.gamma;
    let theta_pnl = sc.theta + sp.theta + lc.theta + lp.theta;
    let vega_pnl = sc.vega + sp.vega + lc.vega + lp.vega;

    let explained = delta_pnl + gamma_pnl + theta_pnl + vega_pnl;
    let unexplained = total_pnl.to_f64().unwrap_or(0.0) - explained;

    (
        Some(Decimal::try_from(delta_pnl).unwrap_or_default()),
        Some(Decimal::try_from(gamma_pnl).unwrap_or_default()),
        Some(Decimal::try_from(theta_pnl).unwrap_or_default()),
        Some(Decimal::try_from(vega_pnl).unwrap_or_default()),
        Some(Decimal::try_from(unexplained).unwrap_or_default()),
    )
}
```

**Lines removed:** ~20

---

## Phase 4: Backtest Use Case Consolidation (Medium Priority)

### Task 4.1: Consolidate Straddle Event Processing

**Goal:** Merge `process_straddle_event` and `process_post_earnings_straddle_event`

**File:** `cs-backtest/src/backtest_use_case.rs`

**Implementation:**

Create a generic straddle processing function:

```rust
/// Unified straddle event processor
///
/// Handles both pre-earnings and post-earnings straddle strategies
async fn process_straddle_event_generic<T: StraddleTimingProvider>(
    &self,
    event: &EarningsEvent,
    timing: &T,
) -> Result<StraddleResult, TradeGenerationError> {
    let entry_time = timing.entry_datetime(event);
    let exit_time = timing.exit_datetime(event);
    let entry_date = entry_time.date_naive();
    let exit_date = timing.exit_date(event);

    // Create strategy
    let strategy = StraddleStrategy::with_min_dte(
        self.config.min_straddle_dte,
        entry_date
    );

    // Get spot price at entry
    let spot = self.equity_repo
        .get_spot_price(&event.symbol, entry_time)
        .await
        .map_err(|_| TradeGenerationError { /* ... */ })?;

    // Get option chain
    let chain_df = self.options_repo
        .get_option_bars_at_time(&event.symbol, entry_time)
        .await
        .map_err(|_| TradeGenerationError { /* ... */ })?;

    // Notional filter
    if !self.passes_notional_filter(&chain_df, spot.value, event)? {
        return Err(TradeGenerationError { /* ... */ });
    }

    // Get expirations
    let expirations = self.options_repo
        .get_available_expirations(&event.symbol, entry_date)
        .await
        .unwrap_or_default();

    if expirations.is_empty() {
        return Err(TradeGenerationError { /* ... */ });
    }

    // Filter expirations based on timing strategy
    let valid_expirations: Vec<_> = expirations
        .iter()
        .filter(|&&exp| exp > exit_date)  // Use timing's exit_date
        .copied()
        .collect();

    // ... rest of common logic ...
}

/// Trait for straddle timing strategies
trait StraddleTimingProvider {
    fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc>;
    fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc>;
    fn exit_date(&self, event: &EarningsEvent) -> NaiveDate;
}

impl StraddleTimingProvider for StraddleTradeTiming { /* ... */ }
impl StraddleTimingProvider for PostEarningsStraddleTiming { /* ... */ }
```

**Lines removed:** ~150-180

---

### Task 4.2: Consolidate Execute Methods (Future Improvement)

**Goal:** Reduce duplication in `execute_*` methods

**Note:** This is a larger refactoring that may not be worth the complexity trade-off. Consider only if Phase 4.1 is successful.

**Approach:** Extract common loop structure:

```rust
async fn execute_strategy_loop<S, R, F>(
    &self,
    start_date: NaiveDate,
    end_date: NaiveDate,
    strategy: S,
    process_event: F,
    on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
) -> Result<BacktestResult, BacktestError>
where
    S: SelectionStrategy,
    R: Into<TradeResult>,
    F: Fn(&EarningsEvent, &S) -> impl Future<Output = Result<R, TradeGenerationError>>,
{
    // Common loop logic
}
```

**Risk:** May add complexity without sufficient benefit. Evaluate after Phase 4.1.

---

## Phase 5: Testing & Validation

### Task 5.1: Run Existing Tests

```bash
cargo test --workspace
```

All existing tests must pass after each phase.

### Task 5.2: Run Backtest Validation

Run a sample backtest before and after changes to verify identical results:

```bash
# Before changes (on main)
./target/release/cs backtest --start 2025-01-01 --end 2025-03-31 --spread calendar > before.json

# After changes
./target/release/cs backtest --start 2025-01-01 --end 2025-03-31 --spread calendar > after.json

# Compare
diff before.json after.json
```

Results should be byte-identical.

---

## Implementation Order

| Order | Task | Dependencies | Est. Time |
|-------|------|--------------|-----------|
| 1 | Task 1.1: Extract select_expirations | None | 30 min |
| 2 | Task 1.2: Extract find_closest_strike | None | 30 min |
| 3 | Task 5.1: Run tests | Tasks 1.1, 1.2 | 10 min |
| 4 | Task 2.1: Fix risk-free rate | None | 20 min |
| 5 | Task 3.1: Remove deprecated function | None | 15 min |
| 6 | Task 5.1: Run tests | Tasks 2.1, 3.1 | 10 min |
| 7 | Task 4.1: Consolidate straddle processing | Tasks 1.1, 1.2 | 1-2 hours |
| 8 | Task 5.1 & 5.2: Full validation | All above | 30 min |

---

## Commit Strategy

1. **Commit 1:** "Extract select_expirations to shared module"
2. **Commit 2:** "Extract find_closest_strike utility function"
3. **Commit 3:** "Fix hardcoded risk-free rate in pricers"
4. **Commit 4:** "Remove deprecated calculate_leg_attribution"
5. **Commit 5:** "Consolidate straddle event processing" (if implemented)

Each commit should be independently testable and deployable.

---

## Rollback Plan

If issues are discovered:

1. Each phase can be reverted independently
2. Git revert commits in reverse order
3. All changes are additive refactoring (no behavior changes)

---

## Success Criteria

- [ ] All tests pass
- [ ] Backtest results identical before/after
- [ ] No new compiler warnings
- [ ] Net reduction of 200+ lines (Phase 1-3)
- [ ] Net reduction of 400+ lines (if Phase 4 included)
