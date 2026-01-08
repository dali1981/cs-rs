# Rust Code Review - CS-RS Calendar Spread Backtest System

**Date:** 2026-01-08
**Scope:** cs-analytics, cs-domain, cs-backtest, cs-cli
**Total LOC:** ~14,000 lines across 129 Rust files

---

## Executive Summary

| Aspect | Rating | Notes |
|--------|--------|-------|
| Architecture | A | Clean layered design, proper DDD |
| Type Safety | A- | Strong types, some unwrap() issues |
| Error Handling | B+ | thiserror used, some gaps |
| Performance | B+ | Good structure, could optimize IV surface |
| Testing | B | Unit tests good, integration sparse |
| Maintainability | B+ | Well-modularized, some complex bounds |

**Overall Grade: B+ (85/100)**

---

## 1. Architecture Analysis

### 1.1 Crate Dependency Graph (Clean)

```
cs-cli
   │
   └──► cs-backtest
           │
           ├──► cs-domain ◄──── finq-rs, earnings-rs
           │        │
           │        └──► cs-analytics (pure math, no deps)
           │
           └──► cs-analytics
```

**Verdict:** Clean layered architecture. Analytics is pure (no I/O), domain encapsulates business rules, backtest orchestrates execution.

### 1.2 Module Organization

| Crate | Files | Responsibility |
|-------|-------|----------------|
| `cs-analytics` | 18 | Pure math: BS pricing, IV surfaces, Greeks, P&L attribution |
| `cs-domain` | 52 | Entities, value objects, repositories, timing strategies |
| `cs-backtest` | 45 | Execution engine, pricers, hedging, use cases |
| `cs-cli` | 14 | Command parsing, config loading, output formatting |

---

## 2. Type System Analysis

### 2.1 Strong Patterns (Good)

**Value Object Validation** (`cs-domain/src/entities.rs:105-119`)
```rust
impl CalendarSpread {
    pub fn new(short: OptionLeg, long: OptionLeg) -> Result<Self, ValidationError> {
        if short.symbol != long.symbol {
            return Err(ValidationError::SymbolMismatch(...));
        }
        if short.expiration >= long.expiration {
            return Err(ValidationError::ExpirationMismatch {...});
        }
        Ok(Self { short_leg: short, long_leg: long })
    }
}
```
**Verdict:** Construction validates invariants - impossible to create invalid spreads.

**Position Sign Abstraction** (`cs-domain/src/trade/composite.rs:6-28`)
```rust
pub enum LegPosition {
    Long,   // +1
    Short,  // -1
}

impl LegPosition {
    pub fn sign(&self) -> f64 {
        match self {
            LegPosition::Long => 1.0,
            LegPosition::Short => -1.0,
        }
    }

    pub fn sign_decimal(&self) -> Decimal {
        match self {
            LegPosition::Long => Decimal::ONE,
            LegPosition::Short => Decimal::NEGATIVE_ONE,
        }
    }
}
```
**Verdict:** Eliminates sign errors in position math.

### 2.2 Trait-Based Dispatch (Excellent)

**Generic Execution Traits** (`cs-backtest/src/execution/traits.rs`)
```rust
pub trait TradePricer: Send + Sync {
    type Trade;
    type Pricing;

    fn price_with_surface(
        &self,
        trade: &Self::Trade,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<Self::Pricing, PricingError>;
}

pub trait ExecutableTrade: Sized + Send + Sync {
    type Pricer: TradePricer<Trade = Self, Pricing = Self::Pricing>;
    type Pricing;
    type Result: TradeResult;

    fn validate_entry(pricing: &Self::Pricing, config: &ExecutionConfig) -> Result<(), ExecutionError>;
    fn to_result(&self, entry: Self::Pricing, exit: Self::Pricing, ctx: &ExecutionContext) -> Self::Result;
    fn to_failed_result(&self, ctx: &ExecutionContext, error: ExecutionError) -> Self::Result;
}
```
**Verdict:** Enables single `execute_trade()` function for all trade types. Excellent generics design.

### 2.3 Complex Type Bounds (Concern)

**TradeExecutor Bound** (`cs-backtest/src/trade_executor.rs:81-84`)
```rust
pub struct TradeExecutor<T>
where
    T: RollableTrade + ExecutableTrade + CompositeTrade + Clone,
```
**Issue:** 4-trait bound is hard to understand at a glance.

**Recommendation:**
```rust
/// A trade that can be rolled, executed, and composed into multi-leg strategies
pub trait FullyExecutableTrade: RollableTrade + ExecutableTrade + CompositeTrade + Clone {}
impl<T> FullyExecutableTrade for T where T: RollableTrade + ExecutableTrade + CompositeTrade + Clone {}
```

---

## 3. Error Handling Analysis

### 3.1 Good: thiserror Usage

**BSError** (`cs-analytics/src/black_scholes.rs:6-12`)
```rust
#[derive(Error, Debug)]
pub enum BSError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("IV solver failed to converge")]
    ConvergenceFailure,
}
```

**ExecutionError** (`cs-backtest/src/execution/types.rs`)
```rust
#[derive(Error, Debug, Clone)]
pub enum ExecutionError {
    #[error("No data available: {0}")]
    NoData(String),
    #[error("Option expired")]
    Expired,
    #[error("Invalid IV: {0}")]
    InvalidIV(String),
    #[error("Pricing failed: {0}")]
    PricingFailed(String),
}
```

### 3.2 Issue: unwrap() in Production Paths

**Count by crate:**
| Crate | unwrap() count | expect() count | In tests only? |
|-------|----------------|----------------|----------------|
| cs-analytics | ~15 | 8 | Mostly tests |
| cs-backtest | ~50 | 0 | Mixed |
| cs-domain | ~5 | 2 | Mostly tests |
| cs-cli | ~20 | 0 | Some in production |

**Problematic Examples:**

1. **Float Comparison** (`cs-backtest/src/minute_aligned_iv_use_case.rs:424`)
```rust
unique_strikes.sort_by(|a, b| a.partial_cmp(b).unwrap());
```
**Risk:** NaN values will panic. Should use `total_cmp()` or handle None.

2. **Date Construction** (`cs-backtest/src/iv_surface_builder.rs:262-263`)
```rust
let exp_date = NaiveDate::from_ymd_opt(2025, 2, 21).unwrap();
```
**Context:** Test code - acceptable.

3. **Silent Failure** (`cs-domain/src/entities.rs:77-81`)
```rust
let strike_int = strike_millis.round().to_string()
    .split('.')
    .next()
    .and_then(|s| s.parse::<u64>().ok())
    .unwrap_or(0);  // Silent fallback to 0!
```
**Risk:** Invalid OCC ticker generated silently.

**Recommendation:** Replace with explicit error:
```rust
let strike_int = strike_millis.round()
    .to_u64()
    .ok_or_else(|| OccTickerError::InvalidStrike(self.strike))?;
```

### 3.3 Issue: expect() in Non-Test Code

**Selection Model** (`cs-analytics/src/selection_model.rs:242-246`)
```rust
.expect("StrikeSpace should return result");
.expect("DeltaSpace should return result");
```
**Risk:** Panic in production if algorithm assumptions violated.

---

## 4. Specific Bug Analysis

### 4.1 Incomplete Implementations (TODOs)

| Location | TODO | Impact |
|----------|------|--------|
| `session_executor.rs:368` | CalendarStraddle doesn't implement RollableTrade | Rolling broken for calendar straddles |
| `trade_executor.rs:529` | Set entry vol for EntryIV/EntryHV modes | Delta hedging may use wrong vol |
| `earnings_repo.rs:38` | Implement actual earnings loading | StubEarningsRepository returns empty! |
| `parquet_results_repo.rs:40` | Convert to Parquet for better performance | JSON fallback used |

### 4.2 StubEarningsRepository Returns Empty

**File:** `cs-domain/src/infrastructure/earnings_repo.rs:30-42`
```rust
impl EarningsRepository for StubEarningsRepository {
    async fn load_earnings(...) -> Result<Vec<EarningsEvent>, RepositoryError> {
        // TODO: Implement actual earnings data loading
        Ok(Vec::new())  // ALWAYS EMPTY!
    }
}
```
**Impact:** If used instead of real adapters, backtests silently produce zero trades.

### 4.3 IV Validation Gap

**Black-Scholes accepts any IV** (`cs-analytics/src/black_scholes.rs:46-52`)
```rust
if time_to_expiry <= 0.0 || volatility <= 0.0 {
    return if is_call {
        (spot - strike).max(0.0)
    } else {
        (strike - spot).max(0.0)
    };
}
```
**Good:** Handles edge cases.

**Missing:** No upper bound validation. IV > 500% would produce garbage prices.

**Recommendation:** Add in `BSConfig`:
```rust
if volatility > config.max_iv {
    return Err(BSError::InvalidInput(format!("IV {} exceeds max {}", volatility, config.max_iv)));
}
```

### 4.4 CompositeIV Division Potential Issue

**File:** `cs-domain/src/trade/composite.rs:77-83`
```rust
pub fn calendar(short_iv: f64, long_iv: f64) -> Self {
    Self {
        primary: short_iv,
        ratio: Some(short_iv / long_iv),  // Division by zero if long_iv == 0
        by_expiration: Some((short_iv, long_iv)),
    }
}
```
**Risk:** `long_iv = 0.0` causes division by zero → Infinity/NaN.

**Fix:**
```rust
ratio: if long_iv > 0.0 { Some(short_iv / long_iv) } else { None },
```

---

## 5. Performance Analysis

### 5.1 IV Surface Rebuilding

**Issue:** IV surface rebuilt on every price request.

**Location:** `spread_pricer.rs` calls `build_iv_surface()` repeatedly.

**Impact:** For hedging with 30 rehedge points, rebuilds surface 30× per trade.

**Recommendation:** Cache IV surface by (spot_bucket, timestamp_bucket):
```rust
struct IVSurfaceCache {
    cache: HashMap<(i32, i64), IVSurface>,  // (spot*100, timestamp/60)
    max_size: usize,
}
```

### 5.2 Clone Requirements

**TradeExecutor requires Clone:**
```rust
T: RollableTrade + ExecutableTrade + CompositeTrade + Clone
```
**Issue:** Forces cloning trade structures during execution.

**Recommendation:** Use `Arc<T>` or borrowing where possible.

### 5.3 Spot History Accumulation

**RealizedVolatilityTracker** (`trade_executor.rs:29-63`)
```rust
struct RealizedVolatilityTracker {
    spot_history: Vec<(DateTime<Utc>, f64)>,  // Unbounded growth
    ...
}
```
**Issue:** For long trades with frequent hedging, this grows unbounded.

**Recommendation:** Add capacity limit or sliding window.

---

## 6. Code Quality Issues

### 6.1 Repetitive TradeResultMethods Implementations

**File:** `cs-backtest/src/backtest_use_case.rs:39-121`
```rust
impl TradeResultMethods for CalendarSpreadResult { ... }
impl TradeResultMethods for StraddleResult { ... }
impl TradeResultMethods for IronButterflyResult { ... }
impl TradeResultMethods for CalendarStraddleResult { ... }
```
**Issue:** 4 nearly identical implementations.

**Recommendation:** Use macro:
```rust
macro_rules! impl_trade_result_methods {
    ($type:ty) => {
        impl TradeResultMethods for $type {
            fn is_winner(&self) -> bool { self.is_winner() }
            fn pnl(&self) -> Decimal { self.pnl }
            fn pnl_pct(&self) -> Decimal { self.pnl_pct }
            fn has_hedge_data(&self) -> bool { self.hedge_pnl.is_some() }
            fn hedge_pnl(&self) -> Option<Decimal> { self.hedge_pnl }
            fn total_pnl_with_hedge(&self) -> Option<Decimal> { self.total_pnl_with_hedge }
        }
    };
}

impl_trade_result_methods!(CalendarSpreadResult);
impl_trade_result_methods!(StraddleResult);
impl_trade_result_methods!(IronButterflyResult);
impl_trade_result_methods!(CalendarStraddleResult);
```

### 6.2 Magic Numbers

**Timing Strategy** (`cs-backtest/src/timing_strategy.rs`)
```rust
NaiveTime::from_hms_opt(15, 45, 0).unwrap()  // What is 15:45?
```
**Recommendation:** Use named constants:
```rust
const DEFAULT_EXIT_TIME: NaiveTime = NaiveTime::from_hms_opt(15, 45, 0).unwrap();
const MARKET_CLOSE: NaiveTime = NaiveTime::from_hms_opt(16, 0, 0).unwrap();
```

### 6.3 Long Function: build_cli_overrides

**File:** `cs-cli/src/main.rs:1016-1149` (133 lines, 50+ parameters)

**Recommendation:** Use builder pattern or derive from clap args:
```rust
#[derive(Default)]
struct CliOverridesBuilder {
    paths: Option<CliPaths>,
    timing: Option<CliTiming>,
    // ...
}

impl CliOverridesBuilder {
    fn with_data_dir(mut self, dir: Option<PathBuf>) -> Self {
        if let Some(d) = dir {
            self.paths.get_or_insert_with(Default::default).data_dir = Some(d);
        }
        self
    }
    // ...
}
```

---

## 7. Testing Coverage

### 7.1 Good Coverage

- Black-Scholes pricing ✓
- IV surface interpolation ✓
- Greeks calculations ✓
- Attribution calculations ✓
- Selection strategies ✓

### 7.2 Missing Coverage

| Component | Issue |
|-----------|-------|
| Session Executor | `#[ignore]` with "Need mock repos" |
| Snapshot Collector | `#[ignore]` with "Need mock repos" |
| CLI commands | No tests |
| Error paths | Sparse |

### 7.3 Recommendation: Mock Repository Trait

```rust
#[cfg(test)]
mod test_utils {
    use async_trait::async_trait;

    pub struct MockOptionsRepository {
        pub chain_data: HashMap<(String, NaiveDate), DataFrame>,
    }

    #[async_trait]
    impl OptionsDataRepository for MockOptionsRepository {
        async fn get_options_chain(...) -> Result<DataFrame, RepositoryError> {
            self.chain_data.get(&(symbol, date))
                .cloned()
                .ok_or(RepositoryError::NotFound)
        }
    }
}
```

---

## 8. Security Considerations

### 8.1 Input Validation

**Good:** Strike validation at construction.
**Good:** Date parsing with explicit error handling.
**Concern:** Custom earnings file loading could be hardened.

### 8.2 Division Safety

**Found 3 potential division by zero locations:**
1. `composite.rs:80` - IV ratio calculation
2. `backtest_use_case.rs:129-130` - win rate calculation (handled)
3. Various percentage calculations in results

### 8.3 Float Comparisons

**Pattern found throughout:**
```rust
.sort_by(|a, b| a.partial_cmp(b).unwrap())
```
**Risk:** Panics on NaN. Use `total_cmp()` from Rust 1.62+.

---

## 9. Recommendations Summary

### 9.1 Critical (Fix Before Release)

1. **Remove StubEarningsRepository** or make it fail loudly
2. **Implement CalendarStraddle RollableTrade** trait
3. **Add IV bounds validation** in Black-Scholes
4. **Handle division by zero** in CompositeIV::calendar()

### 9.2 High Priority

5. **Replace unwrap() with proper error handling** in production paths
6. **Add mock repositories** for integration testing
7. **Cache IV surfaces** for hedging performance
8. **Use total_cmp()** for float sorting

### 9.3 Medium Priority

9. **Macro for TradeResultMethods** implementations
10. **Builder pattern for CLI overrides**
11. **Named constants** for magic numbers
12. **Type alias** for complex trait bounds

### 9.4 Low Priority

13. **Add tracing spans** for performance profiling
14. **Document TimingStrategy** usage patterns
15. **Consider Arc<T>** instead of Clone bounds

---

## 10. Appendix: File-by-File Issues

| File | Line | Issue | Severity |
|------|------|-------|----------|
| `black_scholes.rs` | 59 | Normal::new().unwrap() | Low (always valid) |
| `composite.rs` | 80 | Division by zero risk | Medium |
| `entities.rs` | 81 | Silent fallback to 0 | Medium |
| `session_executor.rs` | 368 | TODO: CalendarStraddle | High |
| `trade_executor.rs` | 529 | TODO: entry vol | Medium |
| `earnings_repo.rs` | 38 | TODO: returns empty | Critical |
| `selection_model.rs` | 242-299 | expect() in non-test | Medium |
| `minute_aligned_iv.rs` | 424 | partial_cmp().unwrap() | Medium |
| `main.rs` | 1016-1149 | 50+ param function | Low |

---

**End of Rust Code Review**
