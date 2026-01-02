# Architecture Refactoring Plan

**Date:** 2025-12-31
**Status:** Draft
**Scope:** Full DDD/Clean Architecture alignment

---

## Executive Summary

This plan addresses architectural violations discovered during code review. The primary issues are:
1. Infrastructure code living inside the domain crate
2. Repository traits leaking infrastructure types (Polars DataFrame)
3. Presentation layer directly importing infrastructure
4. External dependencies in domain entities

The refactoring follows DDD principles and Clean Architecture, ensuring:
- Domain remains pure (no I/O, no external dependencies)
- Clear dependency flow: Presentation → Application → Domain ← Infrastructure
- Testability through dependency injection

---

## Phase 1: Extract Infrastructure Crate

**Priority:** HIGH
**Estimated Complexity:** Medium
**Breaking Changes:** Yes (import paths change)

### Current State

```
cs-domain/
├── entities.rs
├── value_objects.rs
├── repositories.rs
├── strategies/
├── services/
└── infrastructure/          # ❌ Violates DDD
    ├── finq_options_repo.rs
    ├── finq_equity_repo.rs
    ├── earnings_repo.rs
    ├── earnings_reader_adapter.rs
    └── parquet_results_repo.rs
```

### Target State

```
cs-domain/                   # Pure domain, no infrastructure
├── entities.rs
├── value_objects.rs
├── ports.rs                 # Repository traits (renamed from repositories.rs)
├── strategies/
└── services/

cs-infrastructure/           # New crate
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── finq/
│   │   ├── mod.rs
│   │   ├── options_repo.rs
│   │   └── equity_repo.rs
│   ├── earnings/
│   │   ├── mod.rs
│   │   ├── parquet_repo.rs
│   │   └── reader_adapter.rs
│   └── results/
│       └── parquet_repo.rs
```

### Steps

1. **Create new crate `cs-infrastructure`**
   ```bash
   cargo new cs-infrastructure --lib
   ```

2. **Update workspace `Cargo.toml`**
   ```toml
   [workspace]
   members = [
       "cs-analytics",
       "cs-domain",
       "cs-infrastructure",  # Add
       "cs-backtest",
       "cs-cli",
       "cs-python",
   ]
   ```

3. **Configure `cs-infrastructure/Cargo.toml`**
   ```toml
   [package]
   name = "cs-infrastructure"
   version = "0.1.0"
   edition = "2021"

   [dependencies]
   cs-domain = { path = "../cs-domain" }
   async-trait = "0.1"
   chrono = "0.4"
   polars = { version = "0.46", features = ["lazy", "parquet"] }
   finq-core = { path = "../../finq-rs/finq-core" }
   finq-flatfiles = { path = "../../finq-rs/finq-flatfiles" }
   earnings-rs = { path = "../../trading_project/earnings-rs" }
   rust_decimal = "1.36"
   thiserror = "2.0"
   tokio = { version = "1", features = ["rt-multi-thread"] }
   ```

4. **Move files**
   ```bash
   # From cs-domain/src/infrastructure/ to cs-infrastructure/src/
   mv cs-domain/src/infrastructure/finq_options_repo.rs cs-infrastructure/src/finq/options_repo.rs
   mv cs-domain/src/infrastructure/finq_equity_repo.rs cs-infrastructure/src/finq/equity_repo.rs
   mv cs-domain/src/infrastructure/earnings_repo.rs cs-infrastructure/src/earnings/parquet_repo.rs
   mv cs-domain/src/infrastructure/earnings_reader_adapter.rs cs-infrastructure/src/earnings/reader_adapter.rs
   mv cs-domain/src/infrastructure/parquet_results_repo.rs cs-infrastructure/src/results/parquet_repo.rs
   ```

5. **Update imports in moved files**
   - Change `use crate::` to `use cs_domain::`
   - Update module paths

6. **Remove `infrastructure` module from `cs-domain`**
   - Delete `cs-domain/src/infrastructure/mod.rs`
   - Remove `pub mod infrastructure;` from `cs-domain/src/lib.rs`

7. **Update `cs-cli` imports**
   ```rust
   // Before
   use cs_domain::infrastructure::{...};

   // After
   use cs_infrastructure::{...};
   ```

8. **Update `cs-backtest` if needed**
   - Add `cs-infrastructure` dependency if any adapters are used there

### Verification

```bash
cargo build --workspace
cargo test --workspace
```

---

## Phase 2: Define Domain Types for Repository Returns

**Priority:** HIGH
**Estimated Complexity:** High
**Breaking Changes:** Yes (API changes)

### Current State

Repository traits return Polars DataFrame:

```rust
// cs-domain/src/repositories.rs
pub trait OptionsDataRepository: Send + Sync {
    async fn get_option_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<polars::frame::DataFrame, RepositoryError>;  // ❌ Infrastructure type
}
```

### Target State

Repository traits return domain types:

```rust
// cs-domain/src/ports.rs
pub trait OptionsDataRepository: Send + Sync {
    async fn get_option_chain(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<OptionChain, RepositoryError>;
}
```

### New Domain Types

```rust
// cs-domain/src/aggregates/option_chain.rs

/// A single option bar snapshot
#[derive(Debug, Clone)]
pub struct OptionBar {
    pub ticker: String,
    pub underlying: String,
    pub strike: Strike,
    pub expiration: NaiveDate,
    pub option_type: OptionType,
    pub timestamp: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: u64,
    pub vwap: Option<Decimal>,
    pub implied_volatility: Option<f64>,
    pub delta: Option<f64>,
    pub gamma: Option<f64>,
    pub theta: Option<f64>,
    pub vega: Option<f64>,
}

/// Collection of option bars for a single underlying on a single date
#[derive(Debug, Clone)]
pub struct OptionChain {
    pub underlying: String,
    pub as_of_date: NaiveDate,
    bars: Vec<OptionBar>,
}

impl OptionChain {
    pub fn new(underlying: String, as_of_date: NaiveDate, bars: Vec<OptionBar>) -> Self {
        Self { underlying, as_of_date, bars }
    }

    pub fn bars(&self) -> &[OptionBar] {
        &self.bars
    }

    pub fn expirations(&self) -> Vec<NaiveDate> {
        let mut exps: Vec<_> = self.bars.iter()
            .map(|b| b.expiration)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        exps.sort();
        exps
    }

    pub fn strikes_for_expiration(&self, expiration: NaiveDate) -> Vec<Strike> {
        let mut strikes: Vec<_> = self.bars.iter()
            .filter(|b| b.expiration == expiration)
            .map(|b| b.strike)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        strikes.sort();
        strikes
    }

    pub fn filter_by_expiration(&self, expiration: NaiveDate) -> Vec<&OptionBar> {
        self.bars.iter()
            .filter(|b| b.expiration == expiration)
            .collect()
    }

    pub fn filter_by_strike(&self, strike: Strike) -> Vec<&OptionBar> {
        self.bars.iter()
            .filter(|b| b.strike == strike)
            .collect()
    }

    pub fn get_bar(
        &self,
        strike: Strike,
        expiration: NaiveDate,
        option_type: OptionType,
        near_time: DateTime<Utc>,
    ) -> Option<&OptionBar> {
        self.bars.iter()
            .filter(|b| b.strike == strike && b.expiration == expiration && b.option_type == option_type)
            .min_by_key(|b| (b.timestamp - near_time).num_seconds().abs())
    }
}
```

### Steps

1. **Create domain aggregates**
   - Create `cs-domain/src/aggregates/mod.rs`
   - Create `cs-domain/src/aggregates/option_chain.rs`
   - Add `OptionBar` and `OptionChain` types

2. **Update repository trait**
   ```rust
   // cs-domain/src/ports.rs (renamed from repositories.rs)
   pub trait OptionsDataRepository: Send + Sync {
       async fn get_option_chain(
           &self,
           underlying: &str,
           date: NaiveDate,
       ) -> Result<OptionChain, RepositoryError>;
   }
   ```

3. **Remove redundant methods from trait**
   - `get_available_expirations` → use `OptionChain::expirations()`
   - `get_available_strikes` → use `OptionChain::strikes_for_expiration()`

4. **Update infrastructure adapter**
   ```rust
   // cs-infrastructure/src/finq/options_repo.rs
   impl OptionsDataRepository for FinqOptionsRepository {
       async fn get_option_chain(
           &self,
           underlying: &str,
           date: NaiveDate,
       ) -> Result<OptionChain, RepositoryError> {
           let df = self.repository.get_chain_bars(underlying, date).await?;
           dataframe_to_option_chain(df, underlying, date)
       }
   }

   fn dataframe_to_option_chain(
       df: DataFrame,
       underlying: &str,
       date: NaiveDate,
   ) -> Result<OptionChain, RepositoryError> {
       // Convert DataFrame rows to Vec<OptionBar>
       // ...
   }
   ```

5. **Update consumers**
   - `cs-backtest/src/backtest_use_case.rs`
   - `cs-backtest/src/iv_surface_builder.rs`
   - Any other code using `get_option_bars`

6. **Update `EquityDataRepository` similarly**
   ```rust
   pub trait EquityDataRepository: Send + Sync {
       async fn get_spot_price(
           &self,
           symbol: &str,
           target_time: DateTime<Utc>,
       ) -> Result<SpotPrice, RepositoryError>;

       async fn get_equity_bars(
           &self,
           symbol: &str,
           date: NaiveDate,
       ) -> Result<Vec<EquityBar>, RepositoryError>;  // Domain type
   }
   ```

### Verification

```bash
cargo build --workspace
cargo test --workspace
# Run backtest to verify results match
./target/release/cs backtest --start 2024-11-01 --end 2024-11-30
```

---

## Phase 3: Add Application Factory

**Priority:** MEDIUM
**Estimated Complexity:** Medium
**Breaking Changes:** No (additive)

### Current State

CLI manually wires all dependencies:

```rust
// cs-cli/src/main.rs
let earnings_repo = EarningsReaderAdapter::new(PathBuf::from(earnings_data_dir));
let options_repo = FinqOptionsRepository::new(data_dir.clone());
let equity_repo = FinqEquityRepository::new(data_dir.clone());

let backtest = BacktestUseCase::new(
    earnings_repo,
    options_repo,
    equity_repo,
    config,
);
```

### Target State

Application layer provides factory:

```rust
// cs-cli/src/main.rs
let factory = UseCaseFactory::from_config(&app_config)?;
let backtest = factory.create_backtest_use_case();
```

### Implementation

```rust
// cs-backtest/src/factory.rs

use std::path::PathBuf;
use std::sync::Arc;

use cs_domain::*;
use cs_infrastructure::{
    FinqOptionsRepository, FinqEquityRepository, EarningsReaderAdapter,
};

use crate::config::BacktestConfig;
use crate::BacktestUseCase;

pub struct UseCaseFactory {
    config: BacktestConfig,
    earnings_repo: Arc<dyn EarningsRepository>,
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
}

impl UseCaseFactory {
    pub fn new(
        config: BacktestConfig,
        data_dir: PathBuf,
        earnings_dir: PathBuf,
    ) -> Self {
        Self {
            config,
            earnings_repo: Arc::new(EarningsReaderAdapter::new(earnings_dir)),
            options_repo: Arc::new(FinqOptionsRepository::new(data_dir.clone())),
            equity_repo: Arc::new(FinqEquityRepository::new(data_dir)),
        }
    }

    pub fn from_env(config: BacktestConfig) -> Result<Self, FactoryError> {
        let data_dir = std::env::var("FINQ_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data"));

        let earnings_dir = std::env::var("EARNINGS_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_default()
                    .join("trading_project/nasdaq_earnings/data")
            });

        Ok(Self::new(config, data_dir, earnings_dir))
    }

    pub fn create_backtest_use_case(&self) -> BacktestUseCase<
        Arc<dyn EarningsRepository>,
        Arc<dyn OptionsDataRepository>,
        Arc<dyn EquityDataRepository>,
    > {
        BacktestUseCase::new(
            self.earnings_repo.clone(),
            self.options_repo.clone(),
            self.equity_repo.clone(),
            self.config.clone(),
        )
    }

    /// For testing: inject mock repositories
    pub fn with_earnings_repo(mut self, repo: Arc<dyn EarningsRepository>) -> Self {
        self.earnings_repo = repo;
        self
    }

    pub fn with_options_repo(mut self, repo: Arc<dyn OptionsDataRepository>) -> Self {
        self.options_repo = repo;
        self
    }

    pub fn with_equity_repo(mut self, repo: Arc<dyn EquityDataRepository>) -> Self {
        self.equity_repo = repo;
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FactoryError {
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),
    #[error("Invalid path: {0}")]
    InvalidPath(String),
}
```

### Steps

1. **Create factory module**
   - Add `cs-backtest/src/factory.rs`
   - Export from `cs-backtest/src/lib.rs`

2. **Update `BacktestUseCase` to accept trait objects**
   - Change generic parameters to accept `Arc<dyn Trait>`

3. **Simplify CLI**
   ```rust
   // cs-cli/src/main.rs
   let factory = UseCaseFactory::from_env(config)?;
   let backtest = factory.create_backtest_use_case();
   let result = backtest.execute(...).await?;
   ```

4. **Add test helpers**
   ```rust
   #[cfg(test)]
   impl UseCaseFactory {
       pub fn with_mock_repos() -> Self {
           Self {
               config: BacktestConfig::default(),
               earnings_repo: Arc::new(MockEarningsRepo::new()),
               options_repo: Arc::new(MockOptionsRepo::new()),
               equity_repo: Arc::new(MockEquityRepo::new()),
           }
       }
   }
   ```

### Verification

```bash
cargo build --workspace
cargo test --workspace
```

---

## Phase 4: Internalize OptionType in Domain

**Priority:** MEDIUM
**Estimated Complexity:** Low
**Breaking Changes:** Yes (type changes at boundaries)

### Current State

Domain uses external type:

```rust
// cs-domain/src/entities.rs
use finq_core::OptionType;

pub struct OptionLeg {
    pub option_type: OptionType,  // External dependency
}
```

### Target State

Domain defines its own type:

```rust
// cs-domain/src/value_objects.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OptionType {
    Call,
    Put,
}

impl OptionType {
    pub fn is_call(&self) -> bool {
        matches!(self, Self::Call)
    }

    pub fn is_put(&self) -> bool {
        matches!(self, Self::Put)
    }
}
```

### Steps

1. **Define `OptionType` in domain**
   - Add to `cs-domain/src/value_objects.rs`

2. **Remove `finq_core` dependency from `cs-domain`**
   - Update `cs-domain/Cargo.toml`

3. **Add conversion in infrastructure**
   ```rust
   // cs-infrastructure/src/conversions.rs
   impl From<finq_core::OptionType> for cs_domain::OptionType {
       fn from(t: finq_core::OptionType) -> Self {
           match t {
               finq_core::OptionType::Call => cs_domain::OptionType::Call,
               finq_core::OptionType::Put => cs_domain::OptionType::Put,
           }
       }
   }

   impl From<cs_domain::OptionType> for finq_core::OptionType {
       fn from(t: cs_domain::OptionType) -> Self {
           match t {
               cs_domain::OptionType::Call => finq_core::OptionType::Call,
               cs_domain::OptionType::Put => finq_core::OptionType::Put,
           }
       }
   }
   ```

4. **Update CLI to convert at boundary**
   ```rust
   let option_type = match option_type_str.to_lowercase().as_str() {
       "call" => cs_domain::OptionType::Call,
       "put" => cs_domain::OptionType::Put,
       _ => bail!("Invalid option type"),
   };
   ```

5. **Update all domain code to use domain `OptionType`**

### Verification

```bash
cargo build --workspace
cargo test --workspace
```

---

## Phase 5: Restructure Ports Module

**Priority:** LOW
**Estimated Complexity:** Low
**Breaking Changes:** Yes (import paths)

### Current State

```
cs-domain/src/
├── repositories.rs  # All traits in one file
```

### Target State

```
cs-domain/src/
├── ports/
│   ├── mod.rs
│   ├── earnings.rs      # EarningsRepository
│   ├── options.rs       # OptionsDataRepository
│   ├── equity.rs        # EquityDataRepository
│   └── results.rs       # ResultsRepository
```

### Steps

1. **Create ports directory**
   ```bash
   mkdir -p cs-domain/src/ports
   ```

2. **Split `repositories.rs`**
   - Move `EarningsRepository` to `ports/earnings.rs`
   - Move `OptionsDataRepository` to `ports/options.rs`
   - Move `EquityDataRepository` to `ports/equity.rs`
   - Move `ResultsRepository` to `ports/results.rs`
   - Keep `RepositoryError` in `ports/mod.rs`

3. **Update `cs-domain/src/lib.rs`**
   ```rust
   pub mod ports;
   // Remove: pub mod repositories;

   pub use ports::*;
   ```

4. **Update all imports**
   ```rust
   // Before
   use cs_domain::repositories::*;

   // After
   use cs_domain::ports::*;
   ```

### Verification

```bash
cargo build --workspace
```

---

## Phase 6: Separate Configuration Concerns

**Priority:** LOW
**Estimated Complexity:** Low
**Breaking Changes:** No (can be done gradually)

### Current State

`BacktestConfig` mixes infrastructure and domain:

```rust
pub struct BacktestConfig {
    pub data_dir: PathBuf,              // Infrastructure
    pub timing: TimingConfig,            // Domain
    pub selection: TradeSelectionCriteria,  // Domain
    pub strategy: StrategyType,          // Domain
    pub parallel: bool,                  // Infrastructure
    // ...
}
```

### Target State

```rust
// Infrastructure configuration
pub struct InfrastructureConfig {
    pub data_dir: PathBuf,
    pub earnings_dir: PathBuf,
    pub parallel: bool,
    pub output_dir: Option<PathBuf>,
}

// Domain configuration (already exists mostly)
pub struct BacktestDomainConfig {
    pub timing: TimingConfig,
    pub selection: TradeSelectionCriteria,
    pub strategy: StrategyType,
    pub iv_model: IVModel,
    pub vol_model: InterpolationMode,
    pub target_delta: f64,
    pub delta_range: (f64, f64),
    pub delta_scan_steps: usize,
}

// Combined for use case (internal)
pub struct BacktestConfig {
    pub infra: InfrastructureConfig,
    pub domain: BacktestDomainConfig,
}
```

### Steps

1. **Define `InfrastructureConfig`**
2. **Define `BacktestDomainConfig`**
3. **Refactor `BacktestConfig` to compose both**
4. **Update factory to accept separated configs**
5. **CLI builds configs separately then combines**

---

## Phase 7: Extract BacktestAnalyzer Service

**Priority:** LOW
**Estimated Complexity:** Low
**Breaking Changes:** No (additive)

### Current State

`BacktestResult` has statistical methods:

```rust
impl BacktestResult {
    pub fn win_rate(&self) -> f64 { ... }
    pub fn total_pnl(&self) -> Decimal { ... }
    pub fn sharpe_ratio(&self) -> f64 { ... }
    // ... many more
}
```

### Target State

Pure data struct + analyzer service:

```rust
// Pure data
pub struct BacktestResult {
    pub results: Vec<CalendarSpreadResult>,
    pub sessions_processed: usize,
    pub total_entries: usize,
    pub total_opportunities: usize,
    pub dropped_events: Vec<TradeGenerationError>,
}

// Analyzer service
pub struct BacktestAnalyzer;

impl BacktestAnalyzer {
    pub fn win_rate(result: &BacktestResult) -> f64 { ... }
    pub fn total_pnl(result: &BacktestResult) -> Decimal { ... }
    pub fn sharpe_ratio(result: &BacktestResult) -> f64 { ... }
    pub fn summary(result: &BacktestResult) -> BacktestSummary { ... }
}

pub struct BacktestSummary {
    pub win_rate: f64,
    pub total_pnl: Decimal,
    pub mean_return: f64,
    pub std_return: f64,
    pub sharpe_ratio: f64,
    pub avg_winner: Decimal,
    pub avg_loser: Decimal,
}
```

### Steps

1. **Create `BacktestAnalyzer` struct**
2. **Move methods from `BacktestResult` impl to `BacktestAnalyzer`**
3. **Add `BacktestSummary` value object**
4. **Update CLI to use analyzer**

---

## Phase 8: Fix EarningsTime FromStr

**Priority:** LOW
**Estimated Complexity:** Trivial
**Breaking Changes:** No

### Current State

```rust
impl EarningsTime {
    pub fn from_str(s: &str) -> Self { ... }  // Shadows trait
}
```

### Target State

```rust
impl std::str::FromStr for EarningsTime {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "bmo" | "before_market_open" | "pre-market" => Self::BeforeMarketOpen,
            "amc" | "after_market_close" | "post-market" => Self::AfterMarketClose,
            _ => Self::Unknown,
        })
    }
}
```

### Steps

1. Replace inherent `from_str` with `FromStr` trait impl
2. Update callers to use `.parse()` or `FromStr::from_str()`

---

## Implementation Order

| Phase | Description | Priority | Dependencies |
|-------|-------------|----------|--------------|
| 1 | Extract Infrastructure Crate | HIGH | None |
| 2 | Domain Types for Repositories | HIGH | Phase 1 |
| 3 | Application Factory | MEDIUM | Phase 1 |
| 4 | Internalize OptionType | MEDIUM | Phase 1 |
| 5 | Restructure Ports | LOW | Phase 1 |
| 6 | Separate Configuration | LOW | Phase 3 |
| 7 | Extract BacktestAnalyzer | LOW | None |
| 8 | Fix EarningsTime FromStr | LOW | None |

**Recommended approach:** Complete Phases 1-4 as a single refactoring effort, then tackle 5-8 incrementally.

---

## Testing Strategy

### Unit Tests
- Each phase should maintain passing unit tests
- Add tests for new types (`OptionChain`, `OptionBar`, factory)

### Integration Tests
- Run full backtest after each phase
- Compare results with baseline:
  ```bash
  # Before refactoring - save baseline
  ./target/release/cs backtest --start 2024-11-01 --end 2024-11-30 --output baseline.parquet

  # After each phase - compare
  ./target/release/cs backtest --start 2024-11-01 --end 2024-11-30 --output phase_N.parquet
  # Verify identical results
  ```

### Regression Prevention
- CI should run full backtest suite
- Document expected results for key test scenarios

---

## Rollback Plan

Each phase should be a separate PR/commit that can be reverted independently.

For Phase 1-2 (high complexity):
1. Keep old code paths available behind feature flag initially
2. Remove old paths only after verification
3. Tag release before and after major changes

---

## Success Criteria

1. **All tests pass** after each phase
2. **Backtest results identical** to pre-refactoring baseline
3. **No Polars imports** in `cs-domain` (after Phase 2)
4. **No `finq_core` imports** in `cs-domain` (after Phase 4)
5. **CLI imports only** from `cs-backtest` and `cs-infrastructure` (after Phase 3)
6. **Domain crate compiles** with minimal dependencies

---

## Future Considerations

After completing this refactoring:

1. **Add more infrastructure adapters** (e.g., different data sources)
2. **Implement read models** for analytics queries
3. **Add event sourcing** for trade audit trail
4. **Consider CQRS** if read/write patterns diverge significantly
