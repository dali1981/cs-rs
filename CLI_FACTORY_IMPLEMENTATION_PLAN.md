# CLI Factory Implementation Plan

**Date:** 2026-01-08
**Goal:** Complete command handler refactoring with UseCase factories

---

## 1. Current State

### Already Done
- `args/` module with flattened groups (TimingArgs, HedgingArgs, etc.)
- `commands/` module with placeholder handlers
- `CommandHandler` trait defined
- BacktestArgs, CampaignArgs properly structured

### To Implement
- UseCaseFactory to construct use cases from args
- Config builders to convert args → BacktestConfig
- Wire handlers to delegate to factories and use cases
- Remove old monolithic functions from main.rs

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                        main.rs                              │
│  match cli.command {                                        │
│      Commands::Backtest(args) =>                           │
│          BacktestCommand::new(args, global).execute()      │
│  }                                                          │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│              commands/backtest.rs                           │
│  impl CommandHandler for BacktestCommand {                  │
│      async fn execute(&self) -> Result<()> {                │
│          let config = ConfigBuilder::from_args(&self.args)  │
│              .with_config_files(&self.args.conf)            │
│              .build()?;                                     │
│                                                             │
│          let use_case = UseCaseFactory::create_backtest(    │
│              &config, &self.data_dir                        │
│          )?;                                                 │
│                                                             │
│          let result = use_case.execute(...).await?;         │
│                                                             │
│          OutputHandler::display(&result)?;                  │
│      }                                                       │
│  }                                                           │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│              factory/use_case_factory.rs                    │
│  impl UseCaseFactory {                                      │
│      pub fn create_backtest(config, data_dir)               │
│          -> Result<BacktestUseCase<...>>                    │
│      {                                                       │
│          let options_repo = FinqOptionsRepository::new(...);│
│          let equity_repo = FinqEquityRepository::new(...);  │
│          let earnings_repo = Self::create_earnings_repo(...)│
│                                                             │
│          Ok(BacktestUseCase::new(...))                      │
│      }                                                       │
│  }                                                           │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Files to Create

### 3.1 Config Builder

**File: `cs-cli/src/config/mod.rs`**
```rust
//! Configuration building from CLI args

mod builder;
mod hedge;
mod timing;

pub use builder::BacktestConfigBuilder;
pub use hedge::HedgeConfigBuilder;
pub use timing::TimingConfigBuilder;
```

**File: `cs-cli/src/config/builder.rs`**
```rust
//! BacktestConfig builder from CLI args

use anyhow::{Context, Result};
use std::path::PathBuf;
use chrono::NaiveDate;

use cs_backtest::config::{BacktestConfig, SpreadType, SelectionType};
use cs_domain::value_objects::TimingConfig;

use crate::args::BacktestArgs;
use crate::args::GlobalArgs;
use super::{HedgeConfigBuilder, TimingConfigBuilder};

/// Builder for BacktestConfig from CLI args
pub struct BacktestConfigBuilder {
    config: BacktestConfig,
}

impl BacktestConfigBuilder {
    /// Create builder with defaults
    pub fn new() -> Self {
        Self {
            config: BacktestConfig::default(),
        }
    }

    /// Apply CLI arguments (highest priority)
    pub fn from_args(args: &BacktestArgs) -> Self {
        let mut builder = Self::new();

        // Strategy
        if let Some(ref spread) = args.strategy.spread {
            builder.config.spread = SpreadType::from_string(spread);
        }
        if let Some(ref selection) = args.strategy.selection {
            builder.config.selection_strategy = SelectionType::from_string(selection);
        }

        // Timing
        builder.config.timing = TimingConfigBuilder::from_args(&args.timing).build();

        // Selection criteria
        if let Some(v) = args.selection.min_short_dte {
            builder.config.selection.min_short_dte = v;
        }
        if let Some(v) = args.selection.max_short_dte {
            builder.config.selection.max_short_dte = v;
        }
        if let Some(v) = args.selection.min_long_dte {
            builder.config.selection.min_long_dte = v;
        }
        if let Some(v) = args.selection.max_long_dte {
            builder.config.selection.max_long_dte = v;
        }
        if let Some(v) = args.selection.target_delta {
            builder.config.target_delta = v;
        }
        if let Some(v) = args.selection.min_iv_ratio {
            builder.config.selection.min_iv_ratio = Some(v);
        }

        // Hedging
        if args.hedging.hedge {
            builder.config.hedge_config = HedgeConfigBuilder::from_args(&args.hedging).build();
        }

        // Symbols and filters
        builder.config.symbols = args.symbols.clone();
        builder.config.parallel = !args.no_parallel;

        builder
    }

    /// Apply global args
    pub fn with_global(mut self, global: &GlobalArgs) -> Self {
        if let Some(ref dir) = global.data_dir {
            self.config.data_dir = dir.clone();
        }
        self
    }

    /// Merge TOML config files (lower priority than CLI)
    pub fn with_config_files(mut self, files: &[PathBuf]) -> Result<Self> {
        for file in files {
            let file_config = crate::config::load_toml_config(file)
                .with_context(|| format!("Failed to load config: {:?}", file))?;
            self.config = self.config.merge_with(file_config);
        }
        Ok(self)
    }

    /// Set data directory
    pub fn with_data_dir(mut self, data_dir: PathBuf) -> Self {
        self.config.data_dir = data_dir;
        self
    }

    /// Set earnings directory
    pub fn with_earnings_dir(mut self, earnings_dir: PathBuf) -> Self {
        self.config.earnings_dir = earnings_dir;
        self
    }

    /// Build and validate the config
    pub fn build(self) -> Result<BacktestConfig> {
        // Validate required fields
        if self.config.data_dir.as_os_str().is_empty() {
            anyhow::bail!("Data directory is required. Set --data-dir or FINQ_DATA_DIR");
        }

        Ok(self.config)
    }
}
```

### 3.2 UseCase Factory

**File: `cs-cli/src/factory/mod.rs`**
```rust
//! Factory for creating use cases with proper dependencies

mod use_case_factory;
mod repository_factory;

pub use use_case_factory::UseCaseFactory;
pub use repository_factory::RepositoryFactory;
```

**File: `cs-cli/src/factory/repository_factory.rs`**
```rust
//! Repository factory for creating data access components

use std::path::PathBuf;
use std::sync::Arc;

use cs_domain::{
    OptionsDataRepository, EquityDataRepository, EarningsRepository,
    infrastructure::{
        FinqOptionsRepository, FinqEquityRepository,
        EarningsReaderAdapter, ParquetEarningsRepository,
    },
};

/// Factory for creating repository instances
pub struct RepositoryFactory;

impl RepositoryFactory {
    /// Create options repository
    pub fn create_options_repo(data_dir: &PathBuf) -> FinqOptionsRepository {
        FinqOptionsRepository::new(data_dir.clone())
    }

    /// Create equity repository
    pub fn create_equity_repo(data_dir: &PathBuf) -> FinqEquityRepository {
        FinqEquityRepository::new(data_dir.clone())
    }

    /// Create earnings repository based on configuration
    pub fn create_earnings_repo(
        earnings_dir: Option<&PathBuf>,
        earnings_file: Option<&PathBuf>,
    ) -> Box<dyn EarningsRepository> {
        if let Some(file) = earnings_file {
            // Custom file takes precedence
            Box::new(ParquetEarningsRepository::new(file.clone()))
        } else if let Some(dir) = earnings_dir {
            // Use earnings-rs adapter
            Box::new(EarningsReaderAdapter::new(dir.clone()))
        } else {
            // Default location
            let default_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("trading_project/nasdaq_earnings/data");
            Box::new(EarningsReaderAdapter::new(default_dir))
        }
    }
}
```

**File: `cs-cli/src/factory/use_case_factory.rs`**
```rust
//! UseCase factory for creating fully-configured use case instances

use anyhow::Result;
use std::path::PathBuf;

use cs_backtest::{
    BacktestUseCase, BacktestConfig,
    GenerateIvTimeSeriesUseCase,
    EarningsAnalysisUseCase,
};
use cs_domain::infrastructure::{FinqOptionsRepository, FinqEquityRepository};

use super::RepositoryFactory;

/// Factory for creating use case instances
pub struct UseCaseFactory;

impl UseCaseFactory {
    /// Create a backtest use case with all dependencies wired up
    pub fn create_backtest(
        config: BacktestConfig,
        earnings_dir: Option<&PathBuf>,
        earnings_file: Option<&PathBuf>,
    ) -> Result<BacktestUseCase<FinqOptionsRepository, FinqEquityRepository>> {
        let options_repo = RepositoryFactory::create_options_repo(&config.data_dir);
        let equity_repo = RepositoryFactory::create_equity_repo(&config.data_dir);
        let earnings_repo = RepositoryFactory::create_earnings_repo(earnings_dir, earnings_file);

        Ok(BacktestUseCase::new(
            earnings_repo,
            options_repo,
            equity_repo,
            config,
        ))
    }

    /// Create ATM IV generation use case
    pub fn create_atm_iv(
        data_dir: &PathBuf,
    ) -> Result<GenerateIvTimeSeriesUseCase<FinqOptionsRepository, FinqEquityRepository>> {
        let options_repo = RepositoryFactory::create_options_repo(data_dir);
        let equity_repo = RepositoryFactory::create_equity_repo(data_dir);

        Ok(GenerateIvTimeSeriesUseCase::new(options_repo, equity_repo))
    }

    /// Create earnings analysis use case
    pub fn create_earnings_analysis(
        data_dir: &PathBuf,
        earnings_dir: Option<&PathBuf>,
    ) -> Result<EarningsAnalysisUseCase<FinqOptionsRepository, FinqEquityRepository>> {
        let options_repo = RepositoryFactory::create_options_repo(data_dir);
        let equity_repo = RepositoryFactory::create_equity_repo(data_dir);
        let earnings_repo = RepositoryFactory::create_earnings_repo(earnings_dir, None);

        Ok(EarningsAnalysisUseCase::new(
            earnings_repo,
            options_repo,
            equity_repo,
        ))
    }
}
```

### 3.3 Updated Command Handler

**File: `cs-cli/src/commands/backtest.rs`**
```rust
//! Backtest command handler

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::NaiveDate;
use console::style;

use cs_backtest::{BacktestConfig, SpreadType};
use finq_core::OptionType;

use crate::args::{BacktestArgs, GlobalArgs};
use crate::config::BacktestConfigBuilder;
use crate::factory::UseCaseFactory;
use crate::output::BacktestOutputHandler;
use super::CommandHandler;

/// Backtest command handler
pub struct BacktestCommand {
    args: BacktestArgs,
    global: GlobalArgs,
}

impl BacktestCommand {
    /// Create a new backtest command
    pub fn new(args: BacktestArgs, global: GlobalArgs) -> Self {
        Self { args, global }
    }

    /// Parse date string to NaiveDate
    fn parse_date(s: &str) -> Result<NaiveDate> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .with_context(|| format!("Invalid date format: {}. Use YYYY-MM-DD", s))
    }

    /// Parse option type string
    fn parse_option_type(s: Option<&str>) -> Result<OptionType> {
        match s.map(|s| s.to_lowercase()).as_deref() {
            Some("call") | Some("c") => Ok(OptionType::Call),
            Some("put") | Some("p") => Ok(OptionType::Put),
            None => Ok(OptionType::Call), // Default
            Some(other) => anyhow::bail!("Invalid option type: {}. Use 'call' or 'put'", other),
        }
    }
}

#[async_trait]
impl CommandHandler for BacktestCommand {
    async fn execute(&self) -> Result<()> {
        // 1. Build config from args
        let config = BacktestConfigBuilder::from_args(&self.args)
            .with_global(&self.global)
            .with_config_files(&self.args.conf)?
            .build()
            .context("Failed to build backtest config")?;

        // 2. Parse dates
        let start_date = Self::parse_date(&self.args.start)?;
        let end_date = Self::parse_date(&self.args.end)?;

        // 3. Create use case via factory
        let use_case = UseCaseFactory::create_backtest(
            config.clone(),
            self.args.earnings_dir.as_ref(),
            self.args.earnings_file.as_ref(),
        )?;

        // 4. Execute based on strategy type
        println!("{}", style("Running backtest...").bold());
        println!("  Strategy: {:?}", config.spread);
        println!("  Selection: {:?}", config.selection_strategy);
        println!("  Period: {} to {}", start_date, end_date);
        println!();

        match config.spread {
            SpreadType::Calendar => {
                let option_type = Self::parse_option_type(
                    self.args.strategy.option_type.as_deref()
                )?;

                let result = use_case
                    .execute_calendar_spread(start_date, end_date, option_type, None)
                    .await
                    .context("Calendar spread backtest failed")?;

                BacktestOutputHandler::display_calendar_results(&result);

                if let Some(ref output) = self.args.output {
                    BacktestOutputHandler::save_calendar_results(&result, output)?;
                }
            }

            SpreadType::Straddle => {
                let result = use_case
                    .execute_straddle(start_date, end_date, None)
                    .await
                    .context("Straddle backtest failed")?;

                BacktestOutputHandler::display_straddle_results(&result);

                if let Some(ref output) = self.args.output {
                    BacktestOutputHandler::save_straddle_results(&result, output)?;
                }
            }

            SpreadType::IronButterfly => {
                let result = use_case
                    .execute_iron_butterfly(start_date, end_date, None)
                    .await
                    .context("Iron butterfly backtest failed")?;

                BacktestOutputHandler::display_iron_butterfly_results(&result);

                if let Some(ref output) = self.args.output {
                    BacktestOutputHandler::save_iron_butterfly_results(&result, output)?;
                }
            }

            SpreadType::CalendarStraddle => {
                let result = use_case
                    .execute_calendar_straddle(start_date, end_date, None)
                    .await
                    .context("Calendar straddle backtest failed")?;

                BacktestOutputHandler::display_calendar_straddle_results(&result);

                if let Some(ref output) = self.args.output {
                    BacktestOutputHandler::save_calendar_straddle_results(&result, output)?;
                }
            }

            SpreadType::PostEarningsStraddle => {
                let result = use_case
                    .execute_post_earnings_straddle(start_date, end_date, None)
                    .await
                    .context("Post-earnings straddle backtest failed")?;

                BacktestOutputHandler::display_straddle_results(&result);

                if let Some(ref output) = self.args.output {
                    BacktestOutputHandler::save_straddle_results(&result, output)?;
                }
            }
        }

        println!();
        println!("{}", style("Done!").bold().green());
        Ok(())
    }
}
```

### 3.4 Output Handler

**File: `cs-cli/src/output/mod.rs`**
```rust
//! Output handling for command results

mod backtest_output;
mod table_formatter;

pub use backtest_output::BacktestOutputHandler;
pub use table_formatter::TableFormatter;
```

**File: `cs-cli/src/output/backtest_output.rs`**
```rust
//! Output handling for backtest results

use anyhow::Result;
use console::style;
use std::path::PathBuf;
use tabled::{Table, Tabled};

use cs_backtest::{BacktestResult, TradeResultMethods};
use cs_domain::{CalendarSpreadResult, StraddleResult, IronButterflyResult, CalendarStraddleResult};

/// Handler for backtest output display and persistence
pub struct BacktestOutputHandler;

impl BacktestOutputHandler {
    /// Display calendar spread results
    pub fn display_calendar_results(result: &BacktestResult<CalendarSpreadResult>) {
        Self::display_summary(result);
        Self::display_statistics(result);
    }

    /// Display straddle results
    pub fn display_straddle_results(result: &BacktestResult<StraddleResult>) {
        Self::display_summary(result);
        Self::display_statistics(result);
    }

    /// Display iron butterfly results
    pub fn display_iron_butterfly_results(result: &BacktestResult<IronButterflyResult>) {
        Self::display_summary(result);
        Self::display_statistics(result);
    }

    /// Display calendar straddle results
    pub fn display_calendar_straddle_results(result: &BacktestResult<CalendarStraddleResult>) {
        Self::display_summary(result);
        Self::display_statistics(result);
    }

    fn display_summary<R: TradeResultMethods>(result: &BacktestResult<R>) {
        println!("{}", style("Summary:").bold());
        println!("  Sessions processed: {}", result.sessions_processed);
        println!("  Total opportunities: {}", result.total_opportunities);
        println!("  Successful trades: {}", result.successful_trades());
        println!("  Win rate: {:.1}%", result.win_rate() * 100.0);
        println!("  Total P&L: ${:.2}", result.total_pnl());
        println!();
    }

    fn display_statistics<R: TradeResultMethods>(result: &BacktestResult<R>) {
        println!("{}", style("Statistics:").bold());
        println!("  Mean return: {:.2}%", result.mean_return() * 100.0);
        println!("  Std deviation: {:.2}%", result.std_return() * 100.0);
        println!("  Sharpe ratio: {:.2}", result.sharpe_ratio());
        println!("  Avg winner: ${:.2} ({:.2}%)",
            result.avg_winner(),
            result.avg_winner_pct() * 100.0);
        println!("  Avg loser: ${:.2} ({:.2}%)",
            result.avg_loser(),
            result.avg_loser_pct() * 100.0);
        println!();
    }

    /// Save calendar spread results to file
    pub fn save_calendar_results(
        result: &BacktestResult<CalendarSpreadResult>,
        output: &PathBuf,
    ) -> Result<()> {
        // Determine format from extension
        let ext = output.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("json");

        match ext {
            "json" => Self::save_as_json(&result.results, output),
            "parquet" => Self::save_calendar_as_parquet(&result.results, output),
            "csv" => Self::save_as_csv(&result.results, output),
            _ => Self::save_as_json(&result.results, output),
        }
    }

    /// Save straddle results to file
    pub fn save_straddle_results(
        result: &BacktestResult<StraddleResult>,
        output: &PathBuf,
    ) -> Result<()> {
        Self::save_as_json(&result.results, output)
    }

    /// Save iron butterfly results to file
    pub fn save_iron_butterfly_results(
        result: &BacktestResult<IronButterflyResult>,
        output: &PathBuf,
    ) -> Result<()> {
        Self::save_as_json(&result.results, output)
    }

    /// Save calendar straddle results to file
    pub fn save_calendar_straddle_results(
        result: &BacktestResult<CalendarStraddleResult>,
        output: &PathBuf,
    ) -> Result<()> {
        Self::save_as_json(&result.results, output)
    }

    fn save_as_json<T: serde::Serialize>(results: &[T], output: &PathBuf) -> Result<()> {
        let json = serde_json::to_string_pretty(results)?;
        std::fs::write(output, json)?;
        println!("  Saved to: {:?}", output);
        Ok(())
    }

    fn save_calendar_as_parquet(
        results: &[CalendarSpreadResult],
        output: &PathBuf,
    ) -> Result<()> {
        // Use existing parquet serialization from cs-domain
        use cs_domain::infrastructure::ParquetResultsRepository;
        ParquetResultsRepository::save_calendar_results(results, output)?;
        println!("  Saved to: {:?}", output);
        Ok(())
    }

    fn save_as_csv<T: serde::Serialize>(results: &[T], output: &PathBuf) -> Result<()> {
        let mut wtr = csv::Writer::from_path(output)?;
        for result in results {
            wtr.serialize(result)?;
        }
        wtr.flush()?;
        println!("  Saved to: {:?}", output);
        Ok(())
    }
}
```

### 3.5 Updated main.rs

**File: `cs-cli/src/main.rs`**
```rust
//! Calendar Spread Backtest CLI
//!
//! Clean entry point that routes commands to handlers.

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod args;
mod cli;
mod commands;
mod config;
mod factory;
mod output;
mod parsing;

use cli::{Cli, Commands};
use commands::{
    BacktestCommand, CampaignCommand, AtmIvCommand,
    EarningsCommand, PriceCommand, AnalyzeCommand,
    CommandHandler,
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    if cli.global.verbose {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
    }

    // Route to command handler
    match cli.command {
        Commands::Backtest(args) => {
            BacktestCommand::new(args, cli.global.clone())
                .execute()
                .await?;
        }

        Commands::Campaign(args) => {
            CampaignCommand::new(args, cli.global.clone())
                .execute()
                .await?;
        }

        Commands::AtmIv(args) => {
            AtmIvCommand::new(args, cli.global.clone())
                .execute()
                .await?;
        }

        Commands::EarningsAnalysis(args) => {
            EarningsCommand::new(args, cli.global.clone())
                .execute()
                .await?;
        }

        Commands::Price(args) => {
            PriceCommand::new(args, cli.global.clone())
                .execute()
                .await?;
        }

        Commands::Analyze(args) => {
            AnalyzeCommand::new(args)
                .execute()
                .await?;
        }
    }

    Ok(())
}
```

---

## 4. Migration Steps

### Phase 1: Create Infrastructure (No Breaking Changes)

```bash
# Create new modules
touch cs-cli/src/config/mod.rs
touch cs-cli/src/config/builder.rs
touch cs-cli/src/config/hedge.rs
touch cs-cli/src/config/timing.rs
touch cs-cli/src/factory/mod.rs
touch cs-cli/src/factory/use_case_factory.rs
touch cs-cli/src/factory/repository_factory.rs
touch cs-cli/src/output/mod.rs
touch cs-cli/src/output/backtest_output.rs
```

### Phase 2: Implement Factories

1. Implement `RepositoryFactory` (no deps on old code)
2. Implement `UseCaseFactory` (wraps existing use cases)
3. Implement `BacktestConfigBuilder`
4. Add unit tests for factories

### Phase 3: Wire Command Handlers

1. Update `BacktestCommand::execute()` to use factories
2. Test end-to-end with: `cargo run -- backtest --start 2025-01-01 --end 2025-03-31`
3. Repeat for each command

### Phase 4: Clean Up

1. Remove `run_backtest()` function from main.rs
2. Remove `run_campaign_command()` function
3. Remove `build_cli_overrides()` function
4. Remove old `mod config` from main.rs (use new config module)

---

## 5. Testing Strategy

### Unit Tests

```rust
// config/builder.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_from_args() {
        let args = BacktestArgs {
            start: "2025-01-01".to_string(),
            end: "2025-03-31".to_string(),
            strategy: StrategyArgs {
                spread: Some("calendar".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let config = BacktestConfigBuilder::from_args(&args).build().unwrap();
        assert_eq!(config.spread, SpreadType::Calendar);
    }
}
```

### Integration Tests

```rust
// tests/integration/backtest_command.rs
#[tokio::test]
async fn test_backtest_command_execution() {
    let args = BacktestArgs {
        start: "2025-01-01".to_string(),
        end: "2025-01-31".to_string(),
        ..Default::default()
    };
    let global = GlobalArgs {
        data_dir: Some(PathBuf::from("test_data")),
        verbose: false,
    };

    let cmd = BacktestCommand::new(args, global);
    // This would require test fixtures
    // assert!(cmd.execute().await.is_ok());
}
```

---

## 6. File Structure After Migration

```
cs-cli/src/
├── main.rs              # ~50 lines - clean routing
├── cli.rs               # Cli + Commands enum
├── args/
│   ├── mod.rs
│   ├── common.rs        # GlobalArgs
│   ├── timing.rs        # TimingArgs
│   ├── selection.rs     # SelectionArgs
│   ├── strategy.rs      # StrategyArgs
│   ├── hedging.rs       # HedgingArgs
│   ├── attribution.rs   # AttributionArgs
│   ├── backtest.rs      # BacktestArgs
│   ├── campaign.rs      # CampaignArgs
│   ├── atm_iv.rs
│   ├── earnings.rs
│   ├── price.rs
│   └── analyze.rs
├── commands/
│   ├── mod.rs           # CommandHandler trait + re-exports
│   ├── handler.rs       # Trait definition
│   ├── backtest.rs      # BacktestCommand
│   ├── campaign.rs      # CampaignCommand
│   ├── atm_iv.rs        # AtmIvCommand
│   ├── earnings.rs      # EarningsCommand
│   ├── price.rs         # PriceCommand
│   └── analyze.rs       # AnalyzeCommand
├── config/
│   ├── mod.rs
│   ├── builder.rs       # BacktestConfigBuilder
│   ├── hedge.rs         # HedgeConfigBuilder
│   └── timing.rs        # TimingConfigBuilder
├── factory/
│   ├── mod.rs
│   ├── use_case_factory.rs
│   └── repository_factory.rs
├── output/
│   ├── mod.rs
│   ├── backtest_output.rs
│   └── table_formatter.rs
└── parsing/
    ├── mod.rs           # Keep existing parsing utilities
    ├── roll_policy.rs
    ├── earnings_loader.rs
    └── time_config.rs
```

---

## 7. Benefits Summary

| Aspect | Before | After |
|--------|--------|-------|
| main.rs | 2800 lines | ~50 lines |
| Testing | Impossible | Per-command unit tests |
| Dependencies | Hidden in functions | Explicit via factories |
| Adding commands | Edit massive file | Add new file |
| Config handling | 50-param functions | Builder pattern |

---

## 8. Checklist

- [ ] Create `config/` module with builders
- [ ] Create `factory/` module with UseCase factories
- [ ] Create `output/` module for result display
- [ ] Update `BacktestCommand::execute()` to use factories
- [ ] Update `CampaignCommand::execute()`
- [ ] Update `AtmIvCommand::execute()`
- [ ] Update `EarningsCommand::execute()`
- [ ] Implement `PriceCommand::execute()` (was placeholder)
- [ ] Implement `AnalyzeCommand::execute()` (was placeholder)
- [ ] Add unit tests for config builders
- [ ] Add integration tests for commands
- [ ] Remove old `run_backtest()` function
- [ ] Remove old `run_campaign_command()` function
- [ ] Remove `build_cli_overrides()` function
- [ ] Update documentation
