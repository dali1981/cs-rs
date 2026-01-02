# Delta-Space Option Selection Implementation Plan

## Problem Statement

Currently, calendar spreads are formed at the **same strike** for both legs. The user wants the ability to form spreads at the **same delta** instead, which means:

- **Current (Same Strike)**: Short 7 DTE @ $100, Long 30 DTE @ $100
- **Proposed (Same Delta)**: Short 7 DTE 30-delta @ $100, Long 30 DTE 30-delta @ $105

The delta-matched approach results in different strikes due to forward drift and term structure effects. This is technically a **diagonal spread**, not a pure calendar.

## Current Architecture

### Key Components

| Component | Location | Current Behavior |
|-----------|----------|------------------|
| `CalendarSpread` | `cs-domain/src/entities.rs:92-119` | Validates symbol/expiration, assumes same strike |
| `DeltaStrategy` | `cs-domain/src/strategies/delta.rs` | Maps delta → strike (short expiry), uses same strike both legs |
| `SelectionModel` | `cs-analytics/src/selection_model.rs` | `StrikeSpace` vs `DeltaSpace` for IV comparison only |
| `BacktestConfig` | `cs-backtest/src/config.rs` | `StrategyType::Delta` with target_delta |
| `SpreadPricer` | `cs-backtest/src/spread_pricer.rs` | Prices each leg by its own strike |

### Current Flow (Delta Strategy)

```
1. Target delta = 0.30 (config)
2. DeltaVolSurface.delta_to_strike(0.30, short_exp) → Strike $100
3. CalendarSpread created with:
   - short_leg: Strike $100, short_exp
   - long_leg:  Strike $100, long_exp  ← SAME strike
```

### Proposed Flow (Delta-Matched)

```
1. Target delta = 0.30 (config)
2. DeltaVolSurface.delta_to_strike(0.30, short_exp) → Strike $100
3. DeltaVolSurface.delta_to_strike(0.30, long_exp)  → Strike $105
4. Spread created with:
   - short_leg: Strike $100, short_exp
   - long_leg:  Strike $105, long_exp  ← DIFFERENT strikes
```

## Design Decisions

### Decision 1: Entity Model

**Options:**
- A) Create new `DiagonalSpread` entity (different strikes allowed)
- B) Relax `CalendarSpread` to optionally allow different strikes
- C) Keep `CalendarSpread` unchanged, pricing already handles different strikes

**Recommendation: Option C**

The `CalendarSpread` entity already supports different strikes:
- `short_leg.strike` and `long_leg.strike` are independent
- Only validation is symbol match + expiration order
- `strike()` method returns `short_leg.strike` (convention, not constraint)
- `SpreadPricer` already prices each leg independently by its own strike

No entity changes needed.

### Decision 2: Strike Matching Mode

**New Enum:**
```rust
pub enum StrikeMatchMode {
    /// Same strike for both legs (true calendar spread)
    SameStrike,
    /// Same delta for both legs (diagonal spread)
    SameDelta,
}
```

**Location:** `cs-domain/src/strategies/mod.rs` (alongside `TradeSelectionCriteria`)

### Decision 3: Configuration

Add to `BacktestConfig`:
```rust
/// How to match strikes between legs
#[serde(default)]
pub strike_match_mode: StrikeMatchMode,  // Default: SameStrike
```

Add CLI flag:
```
--strike-match <MODE>   Strike matching: same-strike (default), same-delta
```

### Decision 4: Result Storage

The `CalendarSpreadResult` currently has:
```rust
pub strike: Strike,  // Single strike (assumes same)
```

For delta-matched spreads, we need both strikes. Options:
- A) Add `long_strike: Option<Strike>` (None = same as short)
- B) Rename to `short_strike` + add `long_strike`
- C) Keep `strike` as primary, add optional `long_strike` for diagonals

**Recommendation: Option A** - backward compatible, clear semantics

```rust
pub strike: Strike,           // Short leg strike (primary)
pub long_strike: Option<Strike>,  // Long leg strike if different (diagonal)
```

## Implementation Steps

### Phase 1: Core Infrastructure

#### 1.1 Add `StrikeMatchMode` enum

**File:** `cs-domain/src/strategies/mod.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StrikeMatchMode {
    /// Same strike for both legs (true calendar spread)
    #[default]
    SameStrike,
    /// Same delta for both legs (diagonal spread)
    SameDelta,
}

impl StrikeMatchMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "same_strike" | "samestrike" | "calendar" => Some(Self::SameStrike),
            "same_delta" | "samedelta" | "diagonal" => Some(Self::SameDelta),
            _ => None,
        }
    }
}
```

#### 1.2 Update `BacktestConfig`

**File:** `cs-backtest/src/config.rs`

```rust
pub struct BacktestConfig {
    // ... existing fields ...

    /// Strike matching mode for calendar spreads
    #[serde(default)]
    pub strike_match_mode: StrikeMatchMode,
}
```

#### 1.3 Update `CalendarSpreadResult`

**File:** `cs-domain/src/entities.rs`

```rust
pub struct CalendarSpreadResult {
    pub strike: Strike,
    /// Long leg strike if different from short (diagonal spread)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_strike: Option<Strike>,
    // ... rest unchanged
}
```

Add helper:
```rust
impl CalendarSpreadResult {
    /// Get effective long strike (falls back to short strike for calendars)
    pub fn long_strike_effective(&self) -> Strike {
        self.long_strike.unwrap_or(self.strike)
    }

    /// Whether this is a diagonal spread (different strikes)
    pub fn is_diagonal(&self) -> bool {
        self.long_strike.is_some_and(|ls| ls != self.strike)
    }
}
```

### Phase 2: Strategy Updates

#### 2.1 Extend `DeltaStrategy`

**File:** `cs-domain/src/strategies/delta.rs`

```rust
pub struct DeltaStrategy {
    // ... existing fields ...

    /// How to match strikes between legs
    pub strike_match_mode: StrikeMatchMode,
}

impl TradingStrategy for DeltaStrategy {
    fn select(...) -> Result<CalendarSpread, StrategyError> {
        // ... existing delta → strike mapping for short leg ...
        let short_strike = find_closest_strike(&chain_data.strikes, theoretical_strike)?;

        // Determine long leg strike based on mode
        let long_strike = match self.strike_match_mode {
            StrikeMatchMode::SameStrike => short_strike,
            StrikeMatchMode::SameDelta => {
                // Map same delta to strike using LONG expiration
                let theoretical_long = delta_surface
                    .delta_to_strike(target_delta, long_exp, is_call)
                    .ok_or(StrategyError::NoDeltaData)?;
                find_closest_strike(&chain_data.strikes, theoretical_long)?
            }
        };

        let short_leg = OptionLeg::new(symbol, short_strike, short_exp, option_type);
        let long_leg = OptionLeg::new(symbol, long_strike, long_exp, option_type);

        CalendarSpread::new(short_leg, long_leg)
    }
}
```

#### 2.2 Strategy Factory Updates

Update strategy construction in `cs-backtest/src/backtest_use_case.rs` to pass `strike_match_mode` from config.

### Phase 3: CLI Integration

#### 3.1 Add CLI Override

**File:** `cs-cli/src/cli_args.rs`

```rust
pub struct CliStrategy {
    // ... existing ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike_match: Option<String>,  // "same-strike" or "same-delta"
}
```

#### 3.2 Add Clap Argument

**File:** `cs-cli/src/main.rs` (backtest command)

```rust
#[arg(long, value_name = "MODE")]
/// Strike matching mode: same-strike (default), same-delta
strike_match: Option<String>,
```

### Phase 4: Result Recording

#### 4.1 Update Trade Executor

**File:** `cs-backtest/src/trade_executor.rs`

When building `CalendarSpreadResult`:
```rust
CalendarSpreadResult {
    strike: spread.short_leg.strike,
    long_strike: if spread.short_leg.strike != spread.long_leg.strike {
        Some(spread.long_leg.strike)
    } else {
        None
    },
    // ...
}
```

#### 4.2 Update Parquet Schema

**File:** `cs-domain/src/infrastructure/parquet_results_repo.rs`

Add `long_strike` column (nullable f64).

### Phase 5: Testing

1. Unit tests for `StrikeMatchMode` parsing
2. Unit tests for `DeltaStrategy` with both modes
3. Integration test: backtest with `--strike-match same-delta`
4. Verify parquet output includes `long_strike` when applicable

## CLI Usage Examples

```bash
# Traditional calendar (same strike) - default behavior
./target/release/cs backtest --strategy delta --target-delta 0.30

# Delta-matched diagonal (same delta, different strikes)
./target/release/cs backtest --strategy delta --target-delta 0.30 --strike-match same-delta

# Combined with delta scanning
./target/release/cs backtest --strategy delta-scan --delta-range 0.25,0.75 --strike-match same-delta
```

## Config File Example

```toml
[strategy]
type = "delta"
target_delta = 0.30
strike_match_mode = "same_delta"  # or "same_strike" (default)
```

## Migration Notes

- Default behavior unchanged (`SameStrike`)
- Existing results remain valid (no `long_strike` = same as short)
- Parquet schema is backward compatible (nullable column)

## Files to Modify

| File | Changes |
|------|---------|
| `cs-domain/src/strategies/mod.rs` | Add `StrikeMatchMode` enum |
| `cs-domain/src/strategies/delta.rs` | Add `strike_match_mode` field, update `select()` |
| `cs-domain/src/entities.rs` | Add `long_strike` to `CalendarSpreadResult` |
| `cs-backtest/src/config.rs` | Add `strike_match_mode` to `BacktestConfig` |
| `cs-backtest/src/trade_executor.rs` | Populate `long_strike` in results |
| `cs-backtest/src/backtest_use_case.rs` | Pass config to strategy |
| `cs-cli/src/cli_args.rs` | Add `strike_match` override |
| `cs-cli/src/main.rs` | Add `--strike-match` flag |
| `cs-domain/src/infrastructure/parquet_results_repo.rs` | Add column |

## Open Questions

1. **ATM Strategy**: Should `StrikeMatchMode` apply to ATM strategy too?
   - ATM uses spot price, not delta, so "same delta" doesn't directly apply
   - Could interpret as: short=ATM by spot, long=ATM by forward price
   - **Recommendation**: Only support for Delta/DeltaScan strategies initially

2. **IV Ratio Calculation**: When strikes differ, should IV ratio use:
   - Same strike (short leg's strike) for both IVs?
   - Each leg's actual strike?
   - **Recommendation**: Use each leg's actual strike (represents real trade)

3. **Opportunity Scoring**: Should `OpportunityAnalyzer` consider diagonal spreads?
   - Current scoring assumes same strike
   - May need separate scoring logic for diagonals
   - **Recommendation**: Defer to future iteration
