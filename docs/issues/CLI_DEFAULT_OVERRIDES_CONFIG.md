# CLI Default Values Override TOML Configuration

## Severity
**High** - Data integrity issue causing silent incorrect behavior

## Status
Open

## Description
CLI argument default values are unconditionally overriding TOML configuration file values, even when the CLI arguments are not explicitly provided by the user. This violates the expected configuration precedence hierarchy and causes silent failures.

## Steps to Reproduce

1. Create a TOML config file specifying a strategy:
   ```toml
   # straddle_with_costs.toml
   spread = "straddle"
   straddle_entry_days = 5
   straddle_exit_days = 2
   ```

2. Run backtest using the config file WITHOUT specifying `--spread`:
   ```bash
   cs backtest --conf straddle_with_costs.toml --start 2025-10-01 --end 2025-10-31 --output test.json
   ```

3. Examine output - trades are Calendar spreads, not Straddles

## Expected Behavior
Configuration precedence should follow this hierarchy (highest to lowest):
1. Explicit CLI arguments (user provided on command line)
2. TOML configuration files
3. Environment variables
4. System defaults

When a CLI argument is NOT explicitly provided, the TOML config value should be used.

## Actual Behavior
CLI argument defaults ALWAYS override TOML config values, even when not explicitly provided by the user. The hierarchy is:
1. CLI arguments (whether explicit OR default)
2. TOML configuration files ❌ (never used if CLI has defaults)
3. Environment variables
4. System defaults

Result: TOML config files are effectively ignored for any parameter with a CLI default.

## Root Cause

### 1. CLI Args Have Hardcoded Defaults
`cs-cli/src/args/strategy.rs:10`
```rust
#[arg(long, default_value_t = SpreadTypeArg::Calendar)]
pub spread: SpreadTypeArg,  // Always Calendar if not provided

#[arg(long, default_value_t = SelectionTypeArg::ATM)]
pub selection: SelectionTypeArg,  // Always ATM if not provided
```

### 2. Config Builder Unconditionally Uses CLI Values
`cs-cli/src/config/builder.rs:119-127`
```rust
// Apply backtest args
if let Some(ref args) = self.args {
    // Strategy
    let spread_str = format!("{}", args.strategy.spread);  // ❌ Uses default!
    let selection_str = format!("{}", args.strategy.selection);

    overrides.strategy = Some(CliStrategy {
        spread_type: Some(spread_str),  // ❌ Always overrides TOML!
        selection_type: Some(selection_str),
        // ...
    });
}
```

The builder has no way to distinguish between:
- User explicitly provided `--spread calendar`
- User didn't provide `--spread`, so it defaulted to `Calendar`

Both cases produce `args.strategy.spread = SpreadTypeArg::Calendar`.

### 3. CLI Overrides Take Precedence
The figment configuration system correctly prioritizes CLI overrides over TOML, but it receives defaults as if they were explicit user input.

## Impact

### User-Facing
- Silent incorrect behavior - users specify config in TOML, get different results
- No warning or error message
- Debugging requires inspecting output data to notice discrepancy
- Loss of trust in configuration system

### Code Quality
- Violates Principle of Least Surprise
- Makes TOML config files unreliable
- Forces users to duplicate config on command line
- Undermines the point of having config files

## Affected Parameters
Any CLI argument with a default value, including:
- `--spread` (defaults to `Calendar`)
- `--selection` (defaults to `ATM`)
- `--straddle-entry-days` (defaults to `5`)
- `--straddle-exit-days` (defaults to `1`)
- `--min-straddle-dte` (defaults to `7`)
- `--post-earnings-holding-days` (defaults to `5`)
- And likely others throughout the codebase

## Solution Design

### Option 1: Make CLI Args Optional (Recommended)
Remove `default_value_t` and use `Option<T>` for CLI args. Only add to overrides when `Some`.

**Before:**
```rust
// args/strategy.rs
#[arg(long, default_value_t = SpreadTypeArg::Calendar)]
pub spread: SpreadTypeArg,
```

**After:**
```rust
// args/strategy.rs
#[arg(long)]
pub spread: Option<SpreadTypeArg>,
```

**Builder:**
```rust
// config/builder.rs
if let Some(spread) = args.strategy.spread {
    let spread_str = format!("{}", spread);
    if overrides.strategy.is_none() {
        overrides.strategy = Some(CliStrategy::default());
    }
    if let Some(ref mut strategy) = overrides.strategy {
        strategy.spread_type = Some(spread_str);
    }
}
```

**Defaults move to:**
- TOML config files (user's project config)
- System config defaults (in `load_config()` figment setup)
- `BacktestConfig::default()` (code-level defaults)

### Option 2: Track Value Source
Use clap's `value_source()` API to detect if value came from user or default. More complex, requires accessing `ArgMatches`.

### Option 3: Separate Default Structs
Create separate default config structs, merge explicitly. Most complex, most flexibility.

## Recommended Solution
**Option 1** - cleanest architecture, aligns with config precedence best practices.

## Migration Impact
**Breaking Change**: Commands that relied on CLI defaults will need to either:
1. Provide explicit CLI arguments
2. Add values to TOML config
3. Accept system defaults from `BacktestConfig::default()`

Example migration:
```bash
# Before (implicitly used --spread calendar)
cs backtest --start 2025-01-01 --end 2025-12-31

# After - need explicit arg OR config file
cs backtest --start 2025-01-01 --end 2025-12-31 --spread calendar
# OR
cs backtest --conf config.toml --start 2025-01-01 --end 2025-12-31  # config.toml has spread = "calendar"
```

## Related Files
- `cs-cli/src/args/strategy.rs` - CLI argument definitions
- `cs-cli/src/args/timing.rs` - More CLI arguments with defaults
- `cs-cli/src/args/selection.rs` - Selection criteria args
- `cs-cli/src/config/builder.rs` - Config builder that merges sources
- `cs-cli/src/cli_args.rs` - CliOverrides structures
- `cs-backtest/src/config/mod.rs` - BacktestConfig defaults

## Testing Considerations
After fix, verify:
1. TOML config values are respected when CLI args not provided
2. Explicit CLI args still override TOML (highest precedence)
3. Defaults still work when neither TOML nor CLI provided
4. All spread types work correctly: Calendar, Straddle, IronButterfly, etc.
5. Regression test: run same command before/after fix, verify same output

## Workaround
Until fixed, explicitly provide all CLI arguments:
```bash
cs backtest --conf config.toml --spread straddle --start 2025-10-01 --end 2025-10-31
```

## References
- Architecture doc: `~/.claude/ARCHITECTURE_RULES.md` - Presentation layer principles
- Related pattern: CLI wrapper types (domain types vs CLI types)
- Figment docs: https://docs.rs/figment/latest/figment/
