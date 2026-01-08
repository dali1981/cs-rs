# CLI Refactoring Phase 2: Clean Architecture & ValueEnum

**Date:** 2026-01-08
**Status:** PLANNING

## Issues Identified

### 1. ❌ Strategy Dispatch in Command Handler (Wrong Layer)
**Current (commands/backtest.rs):**
```rust
match config.spread {
    SpreadType::Calendar => {
        let option_type = Self::parse_option_type(...)?;
        let result = use_case.execute_calendar_spread(start, end, option_type, None).await?;
    }
    SpreadType::Straddle => {
        let result = use_case.execute_straddle(start, end, None).await?;
    }
    // ...
}
```

**Problem:** Command handler shouldn't know about strategy-specific execution. This is domain logic.

**Solution:** BacktestUseCase should have a single `execute()` method that handles strategy dispatch internally.

---

### 2. ❌ Start/End Dates Passed Separately
**Current:**
```rust
let use_case = UseCaseFactory::create_backtest(config, ...)?;
let result = use_case.execute_calendar_spread(start_date, end_date, ...)?;
```

**Problem:** Period is part of the backtest configuration, not execution parameters.

**Solution:** Add `BacktestPeriod` value object to `BacktestConfig`.

---

### 3. ❌ Earnings Repo Not Injected Properly
**Current:**
```rust
let use_case = UseCaseFactory::create_backtest(
    config,
    self.args.earnings_dir.as_ref(),
    self.args.earnings_file.as_ref(),
)?;
```

**Problem:** Earnings repo is treated differently from options/equity repos.

**Solution:** Move earnings repo configuration to `BacktestConfig`, inject it uniformly in factory.

---

### 4. ❌ Manual String Parsing (Anti-Pattern)
**Current:**
```rust
// In StrategyArgs
#[arg(long)]
pub spread: Option<String>,

// In command handler
let spread = if let Some(ref spread_str) = self.args.strategy.spread {
    match spread_str.to_lowercase().as_str() {
        "calendar" => SpreadType::Calendar,
        "straddle" => SpreadType::Straddle,
        "iron-butterfly" => SpreadType::IronButterfly,
        _ => SpreadType::Calendar,  // Silent fallback - BAD!
    }
} else {
    SpreadType::Calendar
};
```

**Problems:**
- No type safety
- Silent fallback on typos
- Manual parsing code
- No auto-generated help text

**Solution:** Use Clap's `ValueEnum` trait.

---

## Refactoring Plan

### Phase 2.1: Add ValueEnum to Domain Types ✅

**Files to modify:**
- `cs-backtest/src/config.rs`
- `cs-cli/src/args/strategy.rs`

#### Step 1: Update SpreadType with ValueEnum

**File: cs-backtest/src/config.rs**

```rust
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SpreadType {
    #[default]
    Calendar,
    Straddle,
    #[value(name = "iron-butterfly")]
    #[serde(rename = "iron-butterfly")]
    IronButterfly,
    #[value(name = "calendar-straddle")]
    #[serde(rename = "calendar-straddle")]
    CalendarStraddle,
    #[value(name = "post-earnings-straddle")]
    #[serde(rename = "post-earnings-straddle")]
    PostEarningsStraddle,
}

// Remove manual from_string - ValueEnum handles this
```

#### Step 2: Update SelectionType with ValueEnum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionType {
    #[default]
    #[value(name = "atm")]
    #[serde(rename = "atm")]
    ATM,
    Delta,
    #[value(name = "delta-scan")]
    #[serde(rename = "delta-scan")]
    DeltaScan,
}

// Remove manual from_string
```

#### Step 3: Add ValueEnum to OptionType

**File: Create cs-cli/src/args/option_type.rs**

```rust
use clap::ValueEnum;
use finq_core::OptionType as DomainOptionType;

/// CLI wrapper for OptionType with ValueEnum
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OptionType {
    Call,
    #[value(alias = "c")]
    C,
    Put,
    #[value(alias = "p")]
    P,
}

impl From<OptionType> for DomainOptionType {
    fn from(opt: OptionType) -> Self {
        match opt {
            OptionType::Call | OptionType::C => DomainOptionType::Call,
            OptionType::Put | OptionType::P => DomainOptionType::Put,
        }
    }
}
```

#### Step 4: Update StrategyArgs to Use ValueEnum

**File: cs-cli/src/args/strategy.rs**

```rust
use clap::Args;
use cs_backtest::{SpreadType, SelectionType};
use super::option_type::OptionType;

#[derive(Debug, Clone, Args)]
pub struct StrategyArgs {
    /// Trade structure
    #[arg(long, default_value_t = SpreadType::Calendar)]
    pub spread: SpreadType,

    /// Strike selection method
    #[arg(long, default_value_t = SelectionType::ATM)]
    pub selection: SelectionType,

    /// Option type (call/put) - required for calendar spreads only
    #[arg(long)]
    pub option_type: Option<OptionType>,

    /// Delta range for delta-scan strategy (format: "0.25,0.75")
    #[arg(long)]
    pub delta_range: Option<String>,

    /// Number of delta steps for delta-scan strategy
    #[arg(long)]
    pub delta_scan_steps: Option<usize>,

    /// Wing width for iron butterfly strategy
    #[arg(long)]
    pub wing_width: Option<f64>,

    /// Straddle: Entry N trading days before earnings (default: 5)
    #[arg(long, default_value = "5")]
    pub straddle_entry_days: usize,

    /// Straddle: Exit N trading days before earnings (default: 1)
    #[arg(long, default_value = "1")]
    pub straddle_exit_days: usize,

    /// Straddle: Minimum days from entry to expiration (default: 7)
    #[arg(long, default_value = "7")]
    pub min_straddle_dte: i32,

    /// Straddle: Minimum entry price
    #[arg(long)]
    pub min_entry_price: Option<f64>,

    /// Straddle: Maximum entry price
    #[arg(long)]
    pub max_entry_price: Option<f64>,

    /// Post-earnings straddle: holding period in trading days (default: 5)
    #[arg(long, default_value = "5")]
    pub post_earnings_holding_days: usize,

    /// Rolling strategy (weekly, monthly, or days:N)
    #[arg(long)]
    pub roll_strategy: Option<String>,

    /// Day of week for weekly rolls (monday, tuesday, ..., friday)
    #[arg(long)]
    pub roll_day: Option<String>,
}
```

---

### Phase 2.2: Create BacktestPeriod Value Object

**File: cs-backtest/src/config.rs**

```rust
use chrono::NaiveDate;
use serde::{Serialize, Deserialize};

/// Backtest time period
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BacktestPeriod {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl BacktestPeriod {
    pub fn new(start: NaiveDate, end: NaiveDate) -> Result<Self> {
        if end < start {
            anyhow::bail!("End date must be >= start date");
        }
        Ok(Self { start, end })
    }

    pub fn from_strings(start: &str, end: &str) -> Result<Self> {
        let start = NaiveDate::parse_from_str(start, "%Y-%m-%d")
            .with_context(|| format!("Invalid start date: {}", start))?;
        let end = NaiveDate::parse_from_str(end, "%Y-%m-%d")
            .with_context(|| format!("Invalid end date: {}", end))?;
        Self::new(start, end)
    }

    pub fn days(&self) -> i64 {
        (self.end - self.start).num_days()
    }
}
```

---

### Phase 2.3: Update BacktestConfig

**File: cs-backtest/src/config.rs**

```rust
pub struct BacktestConfig {
    pub data_dir: PathBuf,
    pub earnings_dir: Option<PathBuf>,      // NEW: Move here
    pub earnings_file: Option<PathBuf>,     // NEW: Move here
    pub period: BacktestPeriod,             // NEW: Add period
    pub timing: TimingConfig,
    pub selection: TradeSelectionCriteria,
    pub spread: SpreadType,
    pub selection_strategy: SelectionType,
    pub symbols: Option<Vec<String>>,
    pub min_market_cap: Option<u64>,
    pub parallel: bool,
    pub pricing_model: PricingModel,
    pub target_delta: f64,
    pub delta_range: (f64, f64),
    pub delta_scan_steps: usize,
    pub vol_model: InterpolationMode,
    pub strike_match_mode: StrikeMatchMode,
    pub max_entry_iv: Option<f64>,
    pub wing_width: f64,
    pub straddle_entry_days: usize,
    pub straddle_exit_days: usize,
    pub min_notional: Option<f64>,
    pub min_straddle_dte: i32,
    pub min_entry_price: Option<f64>,
    pub max_entry_price: Option<f64>,
    pub post_earnings_holding_days: usize,
    pub hedge_config: HedgeConfig,
}
```

---

### Phase 2.4: Refactor BacktestUseCase

**File: cs-backtest/src/backtest_use_case.rs**

#### Add Single Execute Method

```rust
impl<O, E> BacktestUseCase<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    /// Execute backtest for the configured strategy
    pub async fn execute(&self) -> Result<BacktestResult<impl TradeResult>> {
        let period = self.config.period;

        match self.config.spread {
            SpreadType::Calendar => {
                // Get option type from config or use default
                let option_type = self.config.option_type
                    .unwrap_or(finq_core::OptionType::Call);

                self.execute_calendar_spread_internal(
                    period.start,
                    period.end,
                    option_type,
                    self.config.symbols.as_ref(),
                ).await
            }

            SpreadType::Straddle => {
                self.execute_straddle_internal(
                    period.start,
                    period.end,
                    self.config.symbols.as_ref(),
                ).await
            }

            SpreadType::IronButterfly => {
                self.execute_iron_butterfly_internal(
                    period.start,
                    period.end,
                    self.config.symbols.as_ref(),
                ).await
            }

            SpreadType::CalendarStraddle => {
                self.execute_calendar_straddle_internal(
                    period.start,
                    period.end,
                    self.config.symbols.as_ref(),
                ).await
            }

            SpreadType::PostEarningsStraddle => {
                self.execute_post_earnings_straddle_internal(
                    period.start,
                    period.end,
                    self.config.symbols.as_ref(),
                ).await
            }
        }
    }

    // Rename old public methods to _internal
    async fn execute_calendar_spread_internal(...) -> Result<...> {
        // Existing implementation
    }

    async fn execute_straddle_internal(...) -> Result<...> {
        // Existing implementation
    }

    // ... etc
}
```

---

### Phase 2.5: Update Factory

**File: cs-cli/src/factory/use_case_factory.rs**

```rust
impl UseCaseFactory {
    /// Create a backtest use case with all dependencies
    pub fn create_backtest(
        config: BacktestConfig,
    ) -> Result<BacktestUseCase<FinqOptionsRepository, FinqEquityRepository>> {
        // Create all repos from config
        let options_repo = RepositoryFactory::create_options_repo(&config.data_dir);
        let equity_repo = RepositoryFactory::create_equity_repo(&config.data_dir);
        let earnings_repo = RepositoryFactory::create_earnings_repo(
            config.earnings_dir.as_ref(),
            config.earnings_file.as_ref(),
        );

        Ok(BacktestUseCase::new(
            earnings_repo,
            options_repo,
            equity_repo,
            config,
        ))
    }
}
```

---

### Phase 2.6: Simplify BacktestCommand

**File: cs-cli/src/commands/backtest.rs**

```rust
impl BacktestCommand {
    fn build_config(&self) -> Result<BacktestConfig> {
        use cs_domain::value_objects::TimingConfig as DomainTiming;
        use cs_domain::MarketTime;
        use cs_backtest::BacktestPeriod;
        use crate::parsing::parse_time;

        // Determine data directory
        let data_dir = self.global.data_dir.clone()
            .unwrap_or_else(|| {
                std::env::var("FINQ_DATA_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| std::path::PathBuf::from("data"))
            });

        // Parse timing
        let (entry_hour, entry_minute) = parse_time(self.args.timing.entry_time.clone())?;
        let (exit_hour, exit_minute) = parse_time(self.args.timing.exit_time.clone())?;

        let timing = DomainTiming {
            entry_hour: entry_hour.unwrap_or(MarketTime::DEFAULT_ENTRY.hour),
            entry_minute: entry_minute.unwrap_or(MarketTime::DEFAULT_ENTRY.minute),
            exit_hour: exit_hour.unwrap_or(MarketTime::DEFAULT_HEDGE_CHECK.hour),
            exit_minute: exit_minute.unwrap_or(MarketTime::DEFAULT_HEDGE_CHECK.minute),
        };

        // Parse period
        let period = BacktestPeriod::from_strings(&self.args.start, &self.args.end)?;

        // Build config - NO MORE MANUAL PARSING!
        let config = BacktestConfig {
            data_dir,
            earnings_dir: self.args.earnings_dir.clone(),
            earnings_file: self.args.earnings_file.clone(),
            period,
            timing,
            spread: self.args.strategy.spread,        // Direct enum!
            selection_strategy: self.args.strategy.selection,  // Direct enum!
            parallel: !self.args.no_parallel,
            symbols: self.args.symbols.clone(),
            // Convert CLI OptionType to domain OptionType
            option_type: self.args.strategy.option_type.map(Into::into),
            ..Default::default()
        };

        Ok(config)
    }
}

#[async_trait]
impl CommandHandler for BacktestCommand {
    async fn execute(&self) -> Result<()> {
        println!("{}", style("Running backtest...").bold());

        // 1. Build config from args
        let config = self.build_config()
            .context("Failed to build backtest config")?;

        println!("  Strategy: {:?}", config.spread);
        println!("  Selection: {:?}", config.selection_strategy);
        println!("  Period: {} to {}", config.period.start, config.period.end);
        println!();

        // 2. Create use case via factory (earnings repo now in config!)
        let use_case = UseCaseFactory::create_backtest(config)?;

        // 3. Execute - use case handles strategy dispatch!
        let result = use_case.execute().await
            .context("Backtest execution failed")?;

        // 4. Display results
        BacktestOutputHandler::display(&result);

        // 5. Save if output specified
        if let Some(ref output) = self.args.output {
            BacktestOutputHandler::save(&result, output)?;
        }

        Ok(())
    }
}
```

---

## Benefits Matrix

| Aspect | Before | After |
|--------|--------|-------|
| **Type Safety** | `Option<String>` everywhere | Direct enums with ValueEnum |
| **Validation** | Silent fallback on typos | Clap error with valid options |
| **Help Text** | Manual documentation | Auto-generated: `[calendar, straddle, ...]` |
| **Strategy Dispatch** | In command handler (wrong layer) | In BacktestUseCase (correct layer) |
| **Period Handling** | Passed as params | Part of BacktestConfig |
| **Repo Injection** | Earnings repo special-cased | All repos injected uniformly |
| **Command Handler** | 190+ lines with logic | ~80 lines, just builds config |
| **Testability** | Hard to test strategy dispatch | Easy - just test config building |

---

## Migration Checklist

### Phase 2.1: ValueEnum ✅
- [ ] Add `clap` feature to `cs-backtest/Cargo.toml`
- [ ] Add `ValueEnum` to `SpreadType` in `cs-backtest/src/config.rs`
- [ ] Add `ValueEnum` to `SelectionType` in `cs-backtest/src/config.rs`
- [ ] Create `cs-cli/src/args/option_type.rs` with CLI wrapper
- [ ] Update `StrategyArgs` to use direct enums (not `Option<String>`)
- [ ] Remove manual `from_string` methods

### Phase 2.2: BacktestPeriod ✅
- [ ] Create `BacktestPeriod` value object in `cs-backtest/src/config.rs`
- [ ] Add validation (end >= start)
- [ ] Add `from_strings` constructor

### Phase 2.3: Update BacktestConfig ✅
- [ ] Add `period: BacktestPeriod` field
- [ ] Add `earnings_dir: Option<PathBuf>` field
- [ ] Add `earnings_file: Option<PathBuf>` field
- [ ] Add `option_type: Option<OptionType>` field
- [ ] Update Default implementation

### Phase 2.4: Refactor BacktestUseCase ✅
- [ ] Add single `execute()` method with strategy dispatch
- [ ] Rename old methods to `_internal` (private)
- [ ] Use `config.period` instead of parameters
- [ ] Use `config.spread` for dispatch

### Phase 2.5: Update Factory ✅
- [ ] Remove `earnings_dir` and `earnings_file` parameters
- [ ] Get them from `config` instead
- [ ] Simplify factory method signature

### Phase 2.6: Simplify BacktestCommand ✅
- [ ] Remove manual parsing code
- [ ] Use direct enum fields from args
- [ ] Move period parsing to config building
- [ ] Remove strategy dispatch match
- [ ] Call single `use_case.execute()`

### Phase 2.7: Testing ✅
- [ ] Test all spread types with ValueEnum
- [ ] Test typo handling (should error, not fallback)
- [ ] Test help text generation
- [ ] Test end-to-end execution

---

## Example CLI Usage After Refactoring

### Help Text (Auto-Generated)
```bash
$ cs backtest --help
Options:
  --spread <SPREAD>
          Trade structure [default: calendar]
          [possible values: calendar, straddle, iron-butterfly,
           calendar-straddle, post-earnings-straddle]

  --selection <SELECTION>
          Strike selection method [default: atm]
          [possible values: atm, delta, delta-scan]

  --option-type <OPTION_TYPE>
          Option type (call/put)
          [possible values: call, c, put, p]
```

### Typo Handling
```bash
$ cs backtest --spread calendr --start 2024-01-01 --end 2024-12-31
error: invalid value 'calendr' for '--spread <SPREAD>'
  [possible values: calendar, straddle, iron-butterfly, ...]
```

### Clean Execution
```bash
$ cs backtest --spread straddle --start 2024-01-01 --end 2024-12-31
Calendar Spread Backtest - Rust Edition

Running backtest...
  Strategy: Straddle
  Selection: ATM
  Period: 2024-01-01 to 2024-12-31

[Executes correctly with no manual parsing!]
```

---

## Files to Modify

### Domain Layer (cs-backtest)
1. `src/config.rs` - Add ValueEnum, BacktestPeriod, update BacktestConfig
2. `src/backtest_use_case.rs` - Add execute(), refactor strategy dispatch
3. `Cargo.toml` - Add clap dependency

### CLI Layer (cs-cli)
4. `src/args/strategy.rs` - Use direct enums, not Option<String>
5. `src/args/option_type.rs` - NEW: CLI wrapper for OptionType
6. `src/commands/backtest.rs` - Simplify, remove manual parsing
7. `src/factory/use_case_factory.rs` - Simplify signature

---

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| Breaking changes to BacktestConfig | Add new fields with defaults, deprecate old methods gradually |
| BacktestUseCase API changes | Keep old methods as deprecated wrappers initially |
| ValueEnum serialization issues | Use `#[serde(rename_all = "kebab-case")]` for compatibility |
| Period validation failures | Provide clear error messages with date formats |

---

## Conclusion

This refactoring addresses all four architectural issues:

1. ✅ **Strategy dispatch** → Moved to BacktestUseCase (correct layer)
2. ✅ **Period handling** → BacktestPeriod value object in config
3. ✅ **Earnings repo** → Injected uniformly via config
4. ✅ **Manual parsing** → Replaced with ValueEnum (type-safe)

**Result:**
- Cleaner architecture
- Better type safety
- Auto-generated help text
- Proper validation
- Easier testing
- Less code in command handlers

**Next Step:** Implement Phase 2.1 (ValueEnum migration)
