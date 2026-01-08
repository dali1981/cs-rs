# CLI Refactoring Recommendation: Command Handler Pattern

**Date:** 2026-01-08
**Issue:** Functions with 50+ parameters, monolithic main.rs
**Recommendation:** Command Handler Pattern with Flattened Args

---

## 1. Current Problem Analysis

### Current Structure (Anti-Pattern)

```
main.rs (2800+ lines)
├── Commands enum (200+ lines of inline args)
├── match cli.command {
│   └── run_backtest(50+ positional params)  ← PROBLEM
├── run_backtest() function (200+ lines)
├── run_campaign_command() function (350+ lines)
└── build_cli_overrides() (133 lines, 45 params)
```

**Issues:**
1. `Commands::Backtest { ... }` has 50+ inline fields
2. Functions pass 50+ individual parameters
3. All command logic in single file
4. Hard to test individual commands
5. Hard to add new commands

---

## 2. Recommended Pattern: Command Handler with Flattened Args

### Target Structure

```
cs-cli/src/
├── main.rs              # 50 lines - just routing
├── cli.rs               # App + Commands enum (using flatten)
├── args/
│   ├── mod.rs
│   ├── common.rs        # CommonArgs (data_dir, verbose)
│   ├── backtest.rs      # BacktestArgs (flattened groups)
│   ├── campaign.rs      # CampaignArgs
│   └── atm_iv.rs        # AtmIvArgs
├── commands/
│   ├── mod.rs
│   ├── backtest.rs      # BacktestCommand::execute()
│   ├── campaign.rs      # CampaignCommand::execute()
│   ├── atm_iv.rs        # AtmIvCommand::execute()
│   └── price.rs         # PriceCommand::execute()
└── config/
    ├── mod.rs
    ├── builder.rs       # ConfigBuilder from args
    └── hedge.rs         # HedgeConfigBuilder
```

---

## 3. Implementation Pattern

### 3.1 Flattened Args Groups

**File: `args/backtest.rs`**

```rust
use clap::Args;
use std::path::PathBuf;

/// Timing-related arguments
#[derive(Args, Debug, Clone)]
pub struct TimingArgs {
    /// Entry time in HH:MM format
    #[arg(long)]
    pub entry_time: Option<String>,

    /// Exit time in HH:MM format
    #[arg(long)]
    pub exit_time: Option<String>,
}

/// DTE selection criteria
#[derive(Args, Debug, Clone)]
pub struct SelectionArgs {
    #[arg(long)]
    pub min_short_dte: Option<i32>,
    #[arg(long)]
    pub max_short_dte: Option<i32>,
    #[arg(long)]
    pub min_long_dte: Option<i32>,
    #[arg(long)]
    pub max_long_dte: Option<i32>,
    #[arg(long)]
    pub target_delta: Option<f64>,
    #[arg(long)]
    pub min_iv_ratio: Option<f64>,
}

/// Hedging configuration
#[derive(Args, Debug, Clone)]
pub struct HedgeArgs {
    /// Enable delta hedging
    #[arg(long)]
    pub hedge: bool,

    /// Hedging strategy: time, delta, gamma
    #[arg(long, default_value = "delta")]
    pub hedge_strategy: String,

    /// Rehedge interval in hours (for time-based)
    #[arg(long, default_value = "24")]
    pub hedge_interval_hours: u64,

    /// Delta threshold to trigger rehedge
    #[arg(long, default_value = "0.10")]
    pub delta_threshold: f64,

    /// Maximum rehedges per trade
    #[arg(long)]
    pub max_rehedges: Option<usize>,

    /// Transaction cost per share
    #[arg(long, default_value = "0.01")]
    pub hedge_cost_per_share: f64,

    /// Delta computation mode
    #[arg(long, default_value = "gamma")]
    pub hedge_delta_mode: String,

    /// HV window for HV-based modes
    #[arg(long, default_value = "20")]
    pub hv_window: u32,

    /// Track realized volatility
    #[arg(long)]
    pub track_realized_vol: bool,
}

/// Straddle-specific arguments
#[derive(Args, Debug, Clone)]
pub struct StraddleArgs {
    /// Entry N days before earnings
    #[arg(long)]
    pub straddle_entry_days: Option<usize>,

    /// Exit N days before earnings
    #[arg(long)]
    pub straddle_exit_days: Option<usize>,

    /// Min DTE from entry to expiration
    #[arg(long)]
    pub min_straddle_dte: Option<i32>,

    /// Minimum entry price
    #[arg(long)]
    pub min_entry_price: Option<f64>,

    /// Maximum entry price
    #[arg(long)]
    pub max_entry_price: Option<f64>,
}

/// Complete backtest arguments using flatten
#[derive(Args, Debug, Clone)]
pub struct BacktestArgs {
    /// Configuration files
    #[arg(long, short = 'c')]
    pub conf: Vec<PathBuf>,

    /// Start date
    #[arg(long)]
    pub start: String,

    /// End date
    #[arg(long)]
    pub end: String,

    /// Trade structure
    #[arg(long)]
    pub spread: Option<String>,

    /// Selection method
    #[arg(long)]
    pub selection: Option<String>,

    /// Option type (call/put)
    #[arg(long)]
    pub option_type: Option<String>,

    /// Output file
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Symbols filter
    #[arg(long)]
    pub symbols: Option<Vec<String>>,

    #[command(flatten)]
    pub timing: TimingArgs,

    #[command(flatten)]
    pub selection_criteria: SelectionArgs,

    #[command(flatten)]
    pub hedge: HedgeArgs,

    #[command(flatten)]
    pub straddle: StraddleArgs,

    // ... other flattened groups
}
```

### 3.2 Commands Enum with Flatten

**File: `cli.rs`**

```rust
use clap::{Parser, Subcommand};
use crate::args::{BacktestArgs, CampaignArgs, AtmIvArgs, PriceArgs};

/// Global options available to all commands
#[derive(Args, Debug, Clone)]
pub struct GlobalArgs {
    /// Data directory
    #[arg(long, env = "FINQ_DATA_DIR", global = true)]
    pub data_dir: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(long, short, global = true)]
    pub verbose: bool,
}

#[derive(Parser)]
#[command(name = "cs")]
#[command(about = "Calendar Spread Backtest CLI")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run backtest
    Backtest(BacktestArgs),

    /// Run campaign-based backtest
    Campaign(CampaignArgs),

    /// Generate ATM IV time series
    AtmIv(AtmIvArgs),

    /// Price a single spread
    Price(PriceArgs),

    /// Analyze results
    Analyze { run_dir: PathBuf },
}
```

### 3.3 Command Handler Trait

**File: `commands/mod.rs`**

```rust
use async_trait::async_trait;
use anyhow::Result;

/// Trait for command handlers
#[async_trait]
pub trait CommandHandler {
    type Args;

    /// Execute the command
    async fn execute(args: Self::Args, global: &GlobalArgs) -> Result<()>;
}

// Re-exports
mod backtest;
mod campaign;
mod atm_iv;
mod price;

pub use backtest::BacktestCommand;
pub use campaign::CampaignCommand;
pub use atm_iv::AtmIvCommand;
pub use price::PriceCommand;
```

### 3.4 Individual Command Handler

**File: `commands/backtest.rs`**

```rust
use async_trait::async_trait;
use anyhow::Result;

use crate::args::BacktestArgs;
use crate::cli::GlobalArgs;
use crate::commands::CommandHandler;
use crate::config::BacktestConfigBuilder;

pub struct BacktestCommand;

#[async_trait]
impl CommandHandler for BacktestCommand {
    type Args = BacktestArgs;

    async fn execute(args: Self::Args, global: &GlobalArgs) -> Result<()> {
        // 1. Build config from args (single responsibility)
        let config = BacktestConfigBuilder::new()
            .with_args(&args)
            .with_global(global)
            .with_config_files(&args.conf)
            .build()?;

        // 2. Create dependencies
        let repos = Self::create_repositories(&config)?;

        // 3. Execute use case
        let use_case = BacktestUseCase::new(repos);
        let results = use_case.execute(&config).await?;

        // 4. Display/save results
        Self::handle_output(results, &args.output)?;

        Ok(())
    }
}

impl BacktestCommand {
    fn create_repositories(config: &BacktestConfig) -> Result<Repositories> {
        let options_repo = FinqOptionsRepository::new(config.data_dir.clone());
        let equity_repo = FinqEquityRepository::new(config.data_dir.clone());
        // ...
        Ok(Repositories { options_repo, equity_repo })
    }

    fn handle_output(results: BacktestResult, output: &Option<PathBuf>) -> Result<()> {
        // Display logic
        // Save to file if output provided
        Ok(())
    }
}
```

### 3.5 Config Builder Pattern

**File: `config/builder.rs`**

```rust
use crate::args::BacktestArgs;
use crate::cli::GlobalArgs;
use cs_backtest::config::BacktestConfig;

pub struct BacktestConfigBuilder {
    config: BacktestConfig,
}

impl BacktestConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: BacktestConfig::default(),
        }
    }

    /// Apply CLI args (highest priority)
    pub fn with_args(mut self, args: &BacktestArgs) -> Self {
        // Timing
        if let Some(ref t) = args.timing.entry_time {
            let (h, m) = parse_time(t).unwrap_or((9, 35));
            self.config.timing.entry_hour = h;
            self.config.timing.entry_minute = m;
        }

        // Selection
        if let Some(v) = args.selection_criteria.min_short_dte {
            self.config.selection.min_short_dte = v;
        }

        // Hedging - delegate to specialized builder
        if args.hedge.hedge {
            self.config.hedge_config = HedgeConfigBuilder::from_args(&args.hedge).build();
        }

        self
    }

    /// Apply global options
    pub fn with_global(mut self, global: &GlobalArgs) -> Self {
        if let Some(ref dir) = global.data_dir {
            self.config.data_dir = dir.clone();
        }
        self
    }

    /// Merge config files (lower priority than CLI)
    pub fn with_config_files(mut self, files: &[PathBuf]) -> Self {
        for file in files {
            if let Ok(file_config) = load_toml_config(file) {
                self.config = self.config.merge(file_config);
            }
        }
        self
    }

    pub fn build(self) -> Result<BacktestConfig> {
        self.config.validate()?;
        Ok(self.config)
    }
}
```

### 3.6 Clean Main.rs

**File: `main.rs`**

```rust
mod cli;
mod args;
mod commands;
mod config;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Commands};
use commands::{BacktestCommand, CampaignCommand, AtmIvCommand, PriceCommand, CommandHandler};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging based on verbose flag
    if cli.global.verbose {
        tracing_subscriber::init();
    }

    // Route to command handler
    match cli.command {
        Commands::Backtest(args) => {
            BacktestCommand::execute(args, &cli.global).await?;
        }
        Commands::Campaign(args) => {
            CampaignCommand::execute(args, &cli.global).await?;
        }
        Commands::AtmIv(args) => {
            AtmIvCommand::execute(args, &cli.global).await?;
        }
        Commands::Price(args) => {
            PriceCommand::execute(args, &cli.global).await?;
        }
        Commands::Analyze { run_dir } => {
            println!("Analyze: {:?}", run_dir);
        }
    }

    Ok(())
}
```

---

## 4. Benefits of This Pattern

| Aspect | Before | After |
|--------|--------|-------|
| main.rs | 2800 lines | ~50 lines |
| Parameter count | 50+ per function | 3-5 per function |
| Testability | Hard (coupled) | Easy (isolated handlers) |
| Adding commands | Edit massive enum | Add new file |
| Config merging | Scattered | Centralized builder |
| Code navigation | Scroll forever | Clear file structure |

---

## 5. Migration Strategy

### Phase 1: Extract Args (Low Risk)
1. Create `args/` module with flattened structs
2. Update `Commands` enum to use `#[command(flatten)]`
3. Keep existing functions, just restructure args

### Phase 2: Extract Handlers (Medium Risk)
1. Create `commands/` module with trait
2. Move `run_backtest()` → `BacktestCommand::execute()`
3. Move `run_campaign_command()` → `CampaignCommand::execute()`
4. Simplify main.rs to routing only

### Phase 3: Config Builders (Low Risk)
1. Create `config/builder.rs`
2. Replace `build_cli_overrides()` with builder pattern
3. Centralize validation

---

## 6. File-by-File Changes

### New Files to Create

```
args/mod.rs           # ~20 lines
args/common.rs        # ~30 lines
args/backtest.rs      # ~150 lines (from Commands::Backtest)
args/campaign.rs      # ~80 lines (from Commands::Campaign)
args/atm_iv.rs        # ~50 lines
commands/mod.rs       # ~20 lines
commands/backtest.rs  # ~200 lines (from run_backtest)
commands/campaign.rs  # ~300 lines (from run_campaign_command)
commands/atm_iv.rs    # ~80 lines
config/builder.rs     # ~150 lines (from build_cli_overrides)
```

### Files to Modify

```
main.rs               # 2800 → 50 lines
cli.rs (new)          # ~60 lines (Commands enum)
```

### Files to Delete/Archive

```
# After migration complete:
# - Remove inline arg definitions from Commands enum
# - Remove build_cli_overrides function
# - Remove run_* functions from main.rs
```

---

## 7. Example: Full Backtest Args

```rust
// args/backtest.rs - Complete example

#[derive(Args, Debug, Clone)]
pub struct BacktestArgs {
    // === Core Required ===
    #[arg(long)]
    pub start: String,
    #[arg(long)]
    pub end: String,

    // === Config Files ===
    #[arg(long, short = 'c')]
    pub conf: Vec<PathBuf>,

    // === Data Sources ===
    #[arg(long, env = "EARNINGS_DATA_DIR", conflicts_with = "earnings_file")]
    pub earnings_dir: Option<PathBuf>,
    #[arg(long, conflicts_with = "earnings_dir")]
    pub earnings_file: Option<PathBuf>,

    // === Strategy Selection ===
    #[arg(long)]
    pub spread: Option<String>,
    #[arg(long)]
    pub selection: Option<String>,
    #[arg(long)]
    pub option_type: Option<String>,

    // === Output ===
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub symbols: Option<Vec<String>>,
    #[arg(long)]
    pub min_market_cap: Option<u64>,

    // === Flattened Groups ===
    #[command(flatten)]
    pub timing: TimingArgs,

    #[command(flatten)]
    pub selection_criteria: SelectionArgs,

    #[command(flatten)]
    pub delta: DeltaArgs,

    #[command(flatten)]
    pub hedge: HedgeArgs,

    #[command(flatten)]
    pub straddle: StraddleArgs,

    #[command(flatten)]
    pub pricing: PricingArgs,

    #[command(flatten)]
    pub attribution: AttributionArgs,
}
```

---

## 8. References

- [Rain's Rust CLI Recommendations - Handling Arguments](https://rust-cli-recommendations.sunshowers.io/handling-arguments.html)
- [Clap Derive Tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html)
- [Shuttle Blog - Writing CLI Tool in Rust](https://www.shuttle.dev/blog/2023/12/08/clap-rust)

---

## 9. Summary

**Yes, your suggestion is the adapted pattern.** The recommended approach:

1. **Commands as structs** (not inline fields in enum)
2. **Flatten for arg groups** (timing, hedge, selection, etc.)
3. **Command handlers in dedicated files**
4. **Config builders** instead of 50-param functions
5. **Clean routing in main.rs**

This matches the patterns used by:
- `cargo` (Rust's own package manager)
- `ripgrep` (popular Rust CLI tool)
- `bat` (cat clone with syntax highlighting)

The refactoring can be done incrementally - start with extracting args, then handlers, then builders.
