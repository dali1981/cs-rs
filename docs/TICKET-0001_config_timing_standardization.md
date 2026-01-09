# Phase 4: Standardize Config Timing to Use TradingPeriodSpec

**Date**: 2026-01-09
**Status**: 🔵 Planned
**Priority**: Medium
**Component**: Config, CLI Arguments, Timing System
**Related**: Phase 1-3 (Trade-centric execution refactoring)

---

## Problem

The config system exposes **legacy, spread-specific timing parameters** that contradict the effort put into building a generic `TradingPeriodSpec` system.

**Current state**:
```toml
# straddle_1m_before_15d_cap1b.toml
spread = "straddle"
straddle_entry_days = 20      # ← Straddle-specific
straddle_exit_days = 5        # ← Straddle-specific
```

**What we should have**:
```toml
# Same config, generic timing
timing_strategy = "PreEarnings"   # Reusable across spreads
entry_days_before = 20
exit_days_before = 5
```

**Why this matters**:
- Built `TradingPeriodSpec` enum with PreEarnings, PostEarnings, CrossEarnings
- Implemented generic event discovery and timing resolution
- But config still forces spread-specific thinking
- Users can't easily swap timing strategies across spread types
- Maintenance burden: add new spread type = add new timing parameters

---

## Current Architecture

### What We Built (Phases 1-3)

**Generic timing system** (`TradingPeriodSpec`):
```rust
pub enum TradingPeriodSpec {
    PreEarnings {
        entry_days_before: u16,
        exit_days_before: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },
    PostEarnings {
        entry_offset: i16,
        holding_days: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },
    CrossEarnings {
        entry_days_before: u16,
        exit_days_after: u16,
        entry_time: NaiveTime,
        exit_time: NaiveTime,
    },
    // ... other variants
}
```

**How it works**:
1. User defines timing (when to trade relative to earnings)
2. `TradingPeriodSpec` resolves concrete dates
3. Works with ANY spread type (straddle, calendar, iron butterfly, etc.)

### Current Config (Backward Compatible Shim)

**What exists** (`BacktestConfig::timing_spec()`):
```rust
pub fn timing_spec(&self) -> TradingPeriodSpec {
    match self.spread {
        SpreadType::Straddle => {
            // Convert straddle_entry_days, straddle_exit_days → PreEarnings
            TradingPeriodSpec::PreEarnings {
                entry_days_before: self.straddle_entry_days as u16,
                exit_days_before: self.straddle_exit_days as u16,
                // ...
            }
        }
        SpreadType::PostEarningsStraddle => {
            // Convert post_earnings_holding_days → PostEarnings
            TradingPeriodSpec::PostEarnings {
                entry_offset: 0,
                holding_days: self.post_earnings_holding_days as u16,
                // ...
            }
        }
        // Other spreads → CrossEarnings
        _ => TradingPeriodSpec::CrossEarnings { /* ... */ }
    }
}
```

**The issue**: We convert spread-specific params to generic TradingPeriodSpec, but users interact with the spread-specific config.

---

## Solution

### Step 1: Add TradingPeriodSpec Config Fields

Add these fields to `BacktestConfig`:

```rust
pub struct BacktestConfig {
    // ... existing fields ...

    // NEW: Generic timing specification
    pub timing_strategy: Option<String>,  // "PreEarnings", "PostEarnings", etc.
    pub entry_days_before: Option<u16>,
    pub exit_days_before: Option<u16>,
    pub entry_offset: Option<i16>,
    pub holding_days: Option<u16>,
    pub exit_days_after: Option<u16>,

    // DEPRECATED (kept for backward compatibility):
    pub straddle_entry_days: usize,
    pub straddle_exit_days: usize,
    pub post_earnings_holding_days: usize,
}
```

### Step 2: Update timing_spec() Method

Priority order for backward compatibility:

```rust
pub fn timing_spec(&self) -> TradingPeriodSpec {
    // 1. NEW PATH: Use generic timing_strategy if provided
    if let Some(strategy) = &self.timing_strategy {
        return match strategy.as_str() {
            "PreEarnings" => TradingPeriodSpec::PreEarnings {
                entry_days_before: self.entry_days_before.unwrap_or(5),
                exit_days_before: self.exit_days_before.unwrap_or(1),
                entry_time: self.entry_time(),
                exit_time: self.exit_time(),
            },
            "PostEarnings" => TradingPeriodSpec::PostEarnings {
                entry_offset: self.entry_offset.unwrap_or(0),
                holding_days: self.holding_days.unwrap_or(5),
                entry_time: self.entry_time(),
                exit_time: self.exit_time(),
            },
            // ... other strategies
            _ => panic!("Unknown timing strategy: {}", strategy),
        };
    }

    // 2. LEGACY PATH: Convert spread-specific params (backward compatible)
    match self.spread {
        SpreadType::Straddle => { /* ... */ }
        // ... existing logic
    }
}
```

### Step 3: Add CLI Arguments

Add to `BacktestArgs` (cs-cli/src/args/backtest.rs):

```rust
#[derive(Debug, Clone, Args)]
pub struct BacktestArgs {
    // ... existing fields ...

    /// Timing strategy: PreEarnings, PostEarnings, CrossEarnings, FixedDates, HoldingPeriod
    #[arg(long)]
    pub timing_strategy: Option<String>,

    /// Entry days before event (for PreEarnings/CrossEarnings)
    #[arg(long)]
    pub entry_days_before: Option<u16>,

    /// Exit days before event (for PreEarnings)
    #[arg(long)]
    pub exit_days_before: Option<u16>,

    /// Days after event to enter (for PostEarnings)
    #[arg(long)]
    pub entry_offset: Option<i16>,

    /// Holding days (for PostEarnings/HoldingPeriod)
    #[arg(long)]
    pub holding_days: Option<u16>,

    /// Exit days after event (for CrossEarnings)
    #[arg(long)]
    pub exit_days_after: Option<u16>,
}
```

### Step 4: Update Config Merging

Update `CliOverrides` to include new fields:

```rust
pub struct CliOverrides {
    // ... existing fields ...

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing_strategy: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_days_before: Option<u16>,

    // ... other new timing fields
}
```

---

## Usage Examples

### Before (Legacy, Still Works)
```toml
spread = "straddle"
straddle_entry_days = 20
straddle_exit_days = 5
```

### After (New, Recommended)
```toml
timing_strategy = "PreEarnings"
entry_days_before = 20
exit_days_before = 5
```

### CLI Usage
```bash
# New way (generic)
target/debug/cs backtest \
  --start 2025-10-01 --end 2025-10-31 \
  --spread straddle \
  --timing-strategy PreEarnings \
  --entry-days-before 20 \
  --exit-days-before 5 \
  --min-market-cap 1_000_000_000

# Old way (still works)
target/debug/cs backtest \
  --start 2025-10-01 --end 2025-10-31 \
  --spread straddle \
  --straddle-entry-days 20 \
  --straddle-exit-days 5
```

---

## Benefits

1. **Generic config**: Same timing parameters work with any spread type
2. **Clearer intent**: Config expresses "WHAT to trade" (timing strategy) not "HOW to implement it" (spread-specific params)
3. **Better discoverability**: Users learn about PreEarnings/PostEarnings/CrossEarnings concepts
4. **Scalability**: Add new timing strategy = no new config fields needed
5. **Backward compatible**: Old configs still work

---

## Implementation Order

1. Add new config fields to `BacktestConfig` (with defaults)
2. Update `timing_spec()` to prioritize new fields
3. Add CLI args to `BacktestArgs`
4. Update config merging in `CliOverrides`
5. Add tests for new CLI args
6. Document migration path in CLI help text
7. Update example configs

---

## Testing

```bash
# Test new generic config
target/debug/cs backtest -c generic_timing.toml

# Test backward compatibility
target/debug/cs backtest -c legacy_straddle.toml

# Test CLI override of new fields
target/debug/cs backtest \
  --timing-strategy PreEarnings \
  --entry-days-before 20 \
  --exit-days-before 5

# Test mixed: config + CLI override
target/debug/cs backtest -c base_config.toml \
  --timing-strategy PostEarnings \
  --holding-days 10
```

---

## Backward Compatibility

- **All existing configs work unchanged**
- Legacy straddle/calendar/iron-butterfly fields map to generic timing
- New timing_strategy field optional (defaults to behavior based on spread type)
- No breaking changes to result types or execution logic

---

## Related Issues

- Phases 1-3: Trade-centric execution refactoring (builds foundation)
- Issue: Earnings date discrepancy (independent data source problem)

---

## Notes

This is a **polish and standardization** task, not a core feature. The execution logic is already correct. This improves:
- User mental model (timing is generic, not spread-specific)
- Configuration readability and maintainability
- Discoverability of timing options

---

*End of Phase 4 ticket*
