# Figment-Based Configuration System Implementation Plan

**Date:** 2026-01-01
**Status:** Approved, ready for implementation

## Overview

Add `--conf <file.toml>` support with layered configuration:
```
Code defaults → ~/.config/cs/system.toml → --conf strategy.toml → CLI args
```

**Env vars:** Keep existing only (`FINQ_DATA_DIR`, `EARNINGS_DATA_DIR`) - no new prefix.
**File organization:** Base config (`system.toml`) + strategy override files. Each `--conf` merges on top.

## Config File Format (TOML)

### System Config / Base Defaults
`~/.config/cs/system.toml`:

```toml
# Paths - system-specific, rarely change
[paths]
data_dir = "~/polygon/data"
earnings_dir = "~/trading_project/nasdaq_earnings/data"

# Default timing
[timing]
entry_hour = 9
entry_minute = 35
exit_hour = 15
exit_minute = 55

# Default selection criteria
[selection]
min_short_dte = 3
max_short_dte = 45
min_long_dte = 14
max_long_dte = 90

# Default strategy
[strategy]
type = "atm"
target_delta = 0.50
delta_range = [0.25, 0.75]
delta_scan_steps = 5

# Default pricing
[pricing]
model = "sticky_strike"
vol_model = "linear"

# Other defaults
parallel = true
```

### Strategy Override Files

**Aggressive Strategy** (`strategies/aggressive.toml`):
```toml
# Aggressive strategy - overrides system.toml defaults
# Only specify values that differ from base

[selection]
min_short_dte = 5
max_short_dte = 30
min_iv_ratio = 1.25

[strategy]
type = "delta_scan"
delta_range = [0.25, 0.75]
delta_scan_steps = 10

[pricing]
model = "sticky_moneyness"
vol_model = "svi"
```

**Conservative Strategy** (`strategies/conservative.toml`):
```toml
# Conservative strategy - tighter filters

[selection]
min_short_dte = 7
max_short_dte = 21
min_iv_ratio = 1.30

[strategy]
type = "delta"
target_delta = 0.50

[pricing]
model = "sticky_strike"
vol_model = "linear"
```

## Implementation Changes

### 1. Dependencies (`cs-cli/Cargo.toml`)
Add:
```toml
figment = { version = "0.10", features = ["toml", "env"] }
```

### 2. New Config Module (`cs-cli/src/config.rs`)

Create `AppConfig` struct with nested sections:
- `PathsConfig`: data_dir, earnings_dir
- `TimingConfig`: entry/exit hours/minutes
- `SelectionConfig`: DTE bounds, delta, IV ratio
- `StrategyConfig`: type, target_delta, delta_range, delta_scan_steps
- `PricingConfig`: model, vol_model

Key functions:
- `load_config(conf_files: &[PathBuf], cli_overrides: CliOverrides) -> Result<AppConfig>`
- `expand_tilde(path: &Path) -> PathBuf`
- `AppConfig::to_backtest_config() -> BacktestConfig`

### 3. CLI Override Structs (`cs-cli/src/cli_args.rs`)

All fields are `Option<T>` with `#[serde(skip_serializing_if = "Option::is_none")]` to ensure only explicitly-provided CLI args get merged.

### 4. BacktestConfig Update (`cs-backtest/src/config.rs`)

Add `earnings_dir` field:
```rust
pub struct BacktestConfig {
    pub data_dir: PathBuf,
    pub earnings_dir: PathBuf,  // NEW
    // ... rest unchanged
}
```

### 5. CLI Main Update (`cs-cli/src/main.rs`)

Add to Backtest command:
```rust
/// Configuration file(s) - can specify multiple, each merges on top of previous
#[arg(long, short = 'c')]
conf: Vec<PathBuf>,
```

Replace manual BacktestConfig construction with:
```rust
let cli_overrides = build_cli_overrides(/* existing args */);
let config = crate::config::load_config(&conf, cli_overrides)?;
let backtest_config = config.to_backtest_config();
```

### 6. Module Exports (`cs-cli/src/lib.rs`)
```rust
pub mod config;
mod cli_args;
```

## Usage Examples

```bash
# Default behavior (unchanged) - uses system.toml if present
cs backtest --start 2024-01-01 --end 2024-06-30

# With strategy config file (merges on top of system.toml)
cs backtest --conf strategies/aggressive.toml --start 2024-01-01 --end 2024-06-30

# Multiple conf files - each merges on top of previous
cs backtest --conf base.toml --conf strategies/aggressive.toml --start 2024-01-01 --end 2024-06-30

# CLI override takes precedence over all config files
cs backtest --conf strategies/aggressive.toml --min-iv-ratio 1.5 --start 2024-01-01 --end 2024-06-30

# Existing env vars still work for paths
FINQ_DATA_DIR=/fast/ssd/data cs backtest --conf strategies/aggressive.toml --start 2024-01-01 --end 2024-06-30
```

## Backward Compatibility

- All existing CLI args work unchanged
- Config files are optional
- Without `--conf`, uses code defaults + env vars (current behavior)
- `FINQ_DATA_DIR` and `EARNINGS_DATA_DIR` env vars still work

## Design Decisions

1. **Single unified config struct** - simpler mental model, one `AppConfig` type
2. **Config module in cs-cli** (not cs-backtest) - config loading is application concern
3. **CliOverrides with skip_serializing_if** - ensures only explicit CLI args override
4. **Post-process tilde expansion** - simpler than custom figment provider
5. **No new env vars** - keep only existing `FINQ_DATA_DIR`, `EARNINGS_DATA_DIR`
6. **Multiple `--conf` flags** - each file merges on top of previous (base + strategy pattern)
7. **Flat config files** - no profile nesting, one strategy per file

## Implementation Order

1. Add figment dependency to `cs-cli/Cargo.toml`
2. Add `earnings_dir` to `BacktestConfig` in `cs-backtest/src/config.rs`
3. Create `cs-cli/src/config.rs` with `AppConfig` and loading logic
4. Create `cs-cli/src/cli_args.rs` with `CliOverrides`
5. Update `cs-cli/src/main.rs`:
   - Add `--conf` arg
   - Add `--earnings-dir` arg (new)
   - Refactor `run_backtest()` to use new config loading
6. Update `cs-cli/src/lib.rs` to export modules
7. Create example config files in `examples/configs/`
