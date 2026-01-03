# CLI Strategy Separation Plan

## Problem Statement

The current CLI conflates two orthogonal concepts under a single `--strategy` flag:

```bash
# These are SelectionStrategy (HOW to pick strikes)
--strategy atm
--strategy delta
--strategy delta-scan

# This is OptionStrategy (WHAT trade structure)
--strategy iron-butterfly
```

This creates confusion and limits flexibility. For example:
- Iron butterfly is hardcoded to use ATM selection
- Can't combine iron butterfly with delta-based strike selection
- `--option-type call/put` is shown for iron butterfly (which uses both)

## Domain Model (Already Refactored)

The code already has the correct separation:

```rust
// cs-domain/src/strategies/mod.rs

/// WHAT trade structure to use
pub enum OptionStrategy {
    CalendarSpread,    // 2-leg: short front, long back (same strike or diagonal)
    IronButterfly,     // 4-leg: short straddle + protective wings
}

/// HOW to select strikes/expirations
pub trait SelectionStrategy {
    fn select_calendar_spread(...) -> Result<CalendarSpread, StrategyError>;
    fn select_iron_butterfly(...) -> Result<IronButterfly, StrategyError>;
}

// Implementations: ATMStrategy, DeltaStrategy (fixed + scan mode)
```

## Proposed CLI Changes

### New Arguments

| Argument | Values | Description |
|----------|--------|-------------|
| `--spread` | `calendar`, `iron-butterfly` | OptionStrategy - trade structure |
| `--selection` | `atm`, `delta`, `delta-scan` | SelectionStrategy - strike selection method |
| `--option-type` | `call`, `put` | Only for calendar spreads |

### Argument Validation Rules

1. `--spread` defaults to `calendar`
2. `--selection` defaults to `atm`
3. `--option-type` is REQUIRED for `--spread calendar`, INVALID for `--spread iron-butterfly`
4. `--wing-width` is only valid for `--spread iron-butterfly`
5. `--strike-match-mode` is only valid for `--spread calendar`

### Example Commands

```bash
# Calendar spread with ATM selection
cs backtest --start 2025-11-01 --end 2025-11-30 --spread calendar --selection atm --option-type call

# Calendar spread with delta selection
cs backtest --start 2025-11-01 --end 2025-11-30 --spread calendar --selection delta --target-delta 0.5 --option-type put

# Iron butterfly with ATM centering
cs backtest --start 2025-11-01 --end 2025-11-30 --spread iron-butterfly --selection atm --wing-width 10

# Iron butterfly with delta-based centering
cs backtest --start 2025-11-01 --end 2025-11-30 --spread iron-butterfly --selection delta --target-delta 0.5 --wing-width 10

# Iron butterfly scanning for optimal center
cs backtest --start 2025-11-01 --end 2025-11-30 --spread iron-butterfly --selection delta-scan --delta-range 0.4,0.6 --wing-width 10
```

### Breaking Change

The `--strategy` argument is **removed entirely**. This is a clean break from the old design.

## Implementation Phases

### Phase 1: Update CLI Arguments

**File: `cs-cli/src/main.rs`**

Remove `--strategy`, add `--spread` and `--selection`:

```rust
/// Trade structure (calendar, iron-butterfly)
#[arg(long, default_value = "calendar")]
spread: String,

/// Strike selection method (atm, delta, delta-scan)
#[arg(long, default_value = "atm")]
selection: String,

/// Option type (call/put) - required for calendar spreads only
#[arg(long)]
option_type: Option<String>,
```

### Phase 2: Add Validation

**File: `cs-cli/src/main.rs`**

```rust
fn validate_backtest_args(
    spread: &OptionStrategy,
    option_type: &Option<String>,
    wing_width: &Option<f64>,
    strike_match_mode: &Option<String>,
) -> Result<(), String> {
    match spread {
        OptionStrategy::CalendarSpread => {
            if option_type.is_none() {
                return Err("--option-type is required for calendar spreads".into());
            }
            if wing_width.is_some() {
                return Err("--wing-width is only valid for iron-butterfly".into());
            }
        }
        OptionStrategy::IronButterfly => {
            if option_type.is_some() {
                return Err("--option-type is invalid for iron-butterfly (uses both calls and puts)".into());
            }
            if strike_match_mode.is_some() {
                return Err("--strike-match-mode is only valid for calendar spreads".into());
            }
        }
    }
    Ok(())
}
```

### Phase 3: Update BacktestConfig

**File: `cs-backtest/src/config.rs`**

Replace `StrategyType` with two separate fields:

```rust
/// Trade structure - WHAT to trade
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpreadType {
    #[default]
    Calendar,
    IronButterfly,
}

/// Selection method - HOW to select strikes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionType {
    #[default]
    ATM,
    Delta,
    DeltaScan,
}

pub struct BacktestConfig {
    pub spread: SpreadType,
    pub selection: SelectionType,
    // Remove: pub strategy: StrategyType,
    // ... rest unchanged
}
```

### Phase 4: Update BacktestUseCase

**File: `cs-backtest/src/backtest_use_case.rs`**

Update `create_strategy()`:

```rust
fn create_strategy(&self) -> Box<dyn SelectionStrategy> {
    match self.config.selection {
        SelectionType::ATM => Box::new(
            ATMStrategy::new(self.config.selection_criteria.clone())
                .with_strike_match_mode(self.config.strike_match_mode)
        ),
        SelectionType::Delta => Box::new(
            DeltaStrategy::fixed(self.config.target_delta, self.config.selection_criteria.clone())
                .with_strike_match_mode(self.config.strike_match_mode)
        ),
        SelectionType::DeltaScan => Box::new(
            DeltaStrategy::scanning(
                self.config.delta_range,
                self.config.delta_scan_steps,
                self.config.selection_criteria.clone(),
            )
            .with_strike_match_mode(self.config.strike_match_mode)
        ),
    }
}
```

Update `execute()`:

```rust
pub async fn execute(...) -> Result<BacktestResult, BacktestError> {
    match self.config.spread {
        SpreadType::Calendar => {
            self.execute_calendar_spread(start, end, option_type, on_progress).await
        }
        SpreadType::IronButterfly => {
            self.execute_iron_butterfly(start, end, on_progress).await
        }
    }
}
```

### Phase 5: Update Display Output

**File: `cs-cli/src/main.rs`**

```rust
// For calendar spread
println!("  Spread:        Calendar");
println!("  Option type:   {}", option_type);
println!("  Selection:     {}", selection_display);

// For iron butterfly
println!("  Spread:        Iron Butterfly");
println!("  Wing width:    ${:.2}", wing_width);
println!("  Selection:     {}", selection_display);
// NO option-type line
```

### Phase 6: Update TOML Config Schema

**File: example config**

```toml
[backtest]
spread = "calendar"        # or "iron-butterfly"
selection = "atm"          # or "delta", "delta-scan"

# Calendar-specific
option_type = "call"
strike_match_mode = "same-strike"

# Iron butterfly-specific
wing_width = 10.0

# Delta selection parameters
target_delta = 0.50
delta_range = [0.25, 0.75]
delta_scan_steps = 10
```

## Files to Modify

| File | Changes |
|------|---------|
| `cs-cli/src/main.rs` | Remove `--strategy`, add `--spread`/`--selection`, validation, display |
| `cs-backtest/src/config.rs` | Replace `StrategyType` with `SpreadType` + `SelectionType` |
| `cs-backtest/src/backtest_use_case.rs` | Update `create_strategy()` and `execute()` |
| `cs-cli/src/config.rs` | Update `CliOverrides` and TOML parsing |

## Testing

Run all combinations after implementation:

```bash
# Calendar spreads
cs backtest --spread calendar --selection atm --option-type call ...
cs backtest --spread calendar --selection delta --target-delta 0.5 --option-type put ...
cs backtest --spread calendar --selection delta-scan --delta-range 0.3,0.7 --option-type call ...

# Iron butterflies
cs backtest --spread iron-butterfly --selection atm --wing-width 10 ...
cs backtest --spread iron-butterfly --selection delta --target-delta 0.5 --wing-width 10 ...

# Error cases (should fail with clear message)
cs backtest --spread iron-butterfly --option-type call ...  # Error: option-type invalid
cs backtest --spread calendar ...  # Error: option-type required
cs backtest --spread calendar --wing-width 10 ...  # Error: wing-width invalid
```

## Summary

| Phase | Scope | Files |
|-------|-------|-------|
| 1 | CLI arguments | `cs-cli/src/main.rs` |
| 2 | Validation | `cs-cli/src/main.rs` |
| 3 | Config types | `cs-backtest/src/config.rs` |
| 4 | Use case | `cs-backtest/src/backtest_use_case.rs` |
| 5 | Display | `cs-cli/src/main.rs` |
| 6 | TOML schema | `cs-cli/src/config.rs` |
