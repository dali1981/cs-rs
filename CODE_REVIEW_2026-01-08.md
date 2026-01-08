# Code Review Report - CS-RS Calendar Spread Backtest System

**Date:** 2026-01-08
**Reviewer:** Code Analysis
**Scope:** CLI paths, defaults, replay scripts, visualization, errors, and refactoring opportunities

---

## Executive Summary

The codebase is a mature, well-structured calendar spread options backtesting system with:
- **Rust core** for high-performance execution
- **Python scripts** for replay, visualization, and simulation
- **Layered configuration** system with reasonable defaults

### Key Findings

| Category | Status | Priority |
|----------|--------|----------|
| CLI Commands | 2 unimplemented commands | High |
| Defaults | Some questionable defaults | Medium |
| Replay Scripts | Well designed, functional | Low |
| Simulation | Feature-complete | Low |
| TODOs/Bugs | 15+ active TODOs | Medium |

---

## 1. CLI Command Analysis

### 1.1 Implemented Commands (Working)

| Command | Status | Notes |
|---------|--------|-------|
| `backtest` | Implemented | Full featured with hedging, attribution |
| `campaign` | Implemented | Multi-symbol batch execution |
| `atm-iv` | Implemented | ATM IV time series generation |
| `earnings-analysis` | Implemented | Expected vs actual move analysis |

### 1.2 Unimplemented Commands (CRITICAL)

**`analyze` command** (cs-cli/src/main.rs:500-503)
```rust
Commands::Analyze { run_dir } => {
    println!("Analyze command not yet implemented");
    println!("Run dir: {:?}", run_dir);
}
```
**Impact:** Users cannot analyze results from a run directory.
**Recommendation:** Either implement or remove from CLI.

**`price` command** (cs-cli/src/main.rs:504-514)
```rust
Commands::Price { ... } => {
    println!("Price command not yet implemented");
    // Just prints parameters
}
```
**Impact:** Cannot debug single spread pricing.
**Recommendation:** Critical for debugging - implement using existing `SpreadPricer`.

### 1.3 Partial Implementations

**Parquet export limitation** (cs-cli/src/main.rs:1756-1776)
- Parquet export only works for calendar spreads
- Other strategies fall back to JSON with warning
- Warning message not actionable: suggests `.json` but doesn't auto-convert

**Plotting TODOs** (cs-cli/src/main.rs:1895, 1933)
```rust
// TODO: Add plotting implementation
```
- ATM IV command mentions `--plot` flag but implementation is TODO

---

## 2. Configuration and Defaults Analysis

### 2.1 Problematic Defaults

**Data directory default** (cs-cli/src/config.rs:128-137)
```rust
impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("data"),  // PROBLEMATIC: relative path
            earnings_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("trading_project/nasdaq_earnings/data"),
        }
    }
}
```
**Issue:** `data_dir: "data"` is a relative path that:
- Changes meaning based on working directory
- May not exist
- Hard to trace in error messages

**Recommendation:** Use environment variable with clear error:
```rust
data_dir: std::env::var("FINQ_DATA_DIR")
    .map(PathBuf::from)
    .unwrap_or_else(|_| {
        eprintln!("Warning: FINQ_DATA_DIR not set, using ./data");
        PathBuf::from("data")
    })
```

### 2.2 Hardcoded Straddle Defaults

**straddle_entry_days, straddle_exit_days, min_straddle_dte** (cs-cli/src/main.rs:142-149)
```rust
#[arg(long, default_value = "5")]
straddle_entry_days: usize,
#[arg(long, default_value = "1")]
straddle_exit_days: usize,
#[arg(long, default_value = "7")]
min_straddle_dte: i32,
```
**Issue:** These always override config file values because they have `default_value`.

**Impact:** Cannot set different defaults in config file - CLI always wins.

**Fix:** Change to `Option<usize>` without default_value:
```rust
#[arg(long)]
straddle_entry_days: Option<usize>,
```

### 2.3 Good Default Practices

The following use proper Option<T> pattern:
- `min_market_cap: Option<u64>`
- `max_entry_iv: Option<f64>`
- `min_entry_price: Option<f64>`
- `max_entry_price: Option<f64>`

---

## 3. Python Replay Scripts Analysis

### 3.1 replay_trade.py - Single Trade Replay

**Status:** Well implemented

**Strengths:**
- Clear CLI interface with argparse
- Graceful finq import error handling
- Multi-panel visualization (5 panels)
- Supports both JSON and Parquet input

**Issues:**

1. **Hardcoded data_dir fallback** (line 50)
```python
self.data_dir = data_dir or Path.home() / "finq_data"
```
Should use `FINQ_DATA_DIR` environment variable.

2. **Bare except clauses** (lines 65, 76, 117, etc.)
```python
except:
    return None
```
Swallows all errors silently. Should at least log.

3. **Missing pandas import** (line 231)
```python
dates = pd.to_datetime(df.index).date
```
`pd` is not imported at top of file. Will crash.

4. **Polars row iteration** (line 482-483)
```python
row = df[index]
return {col: row[col].item() if hasattr(row[col], 'item') else row[col] ...}
```
This indexing pattern is inefficient and may not work correctly with all Polars versions.

**Recommendations:**
- Add `import pandas as pd` at top
- Replace bare `except:` with specific exceptions
- Use `FINQ_DATA_DIR` environment variable
- Use `df.row(index, named=True)` for Polars

### 3.2 batch_replay_trades.py - Batch Replay

**Status:** Well implemented

**Strengths:**
- Parallel processing with ThreadPoolExecutor
- Filtering options (success/failed, P&L range, symbols)
- HTML index generation
- Progress reporting

**Issues:**

1. **Polars row iteration** (line 54-58)
```python
return [
    {col: row[col].item() if hasattr(row[col], 'item') else row[col]
     for col in df.columns}
    for row in df.iter_rows(named=True)
]
```
`iter_rows(named=True)` returns named tuples, not dicts. The `row[col]` indexing is incorrect.

**Fix:**
```python
return df.to_dicts()  # Correct Polars way
```

2. **Missing error context in results** (line 149)
```python
idx, success, message = future.result()
```
The `future` variable is from the outer loop, not `as_completed()`. This is a bug.

**Fix:**
```python
for future in as_completed(futures):
    idx, success, message = future.result()
    # ...
```

---

## 4. Simulation System Analysis

### 4.1 simulation/config.py

**Status:** Well designed

**Strengths:**
- Immutable dataclasses with `frozen=True`
- Comprehensive validation in `__post_init__`
- Preset patterns for common strategies
- JSON serialization support

**No issues found.**

### 4.2 simulation/run_simulation.py

**Status:** Functional

**Strengths:**
- Rich CLI with argparse
- Multiple example functions
- Extensible architecture

**Minor Issue:** `sys.path.insert(0, ...)` hack (line 28)
```python
sys.path.insert(0, str(Path(__file__).parent))
```
Should use relative imports or proper package structure.

### 4.3 simulation/strategy_simulator_v2.py

**Status:** Recently fixed (per HEDGE_CALCULATION_FIX_REPORT.md)

The hedge calculation was corrected from `delta × spot` to `delta × 100`.

---

## 5. Obvious Errors and Bugs

### 5.1 Active Bugs in Code

| Location | Description | Severity |
|----------|-------------|----------|
| batch_replay_trades.py:54 | Polars iter_rows returns tuples, not dicts | High |
| batch_replay_trades.py:149 | Wrong future variable reference | High |
| replay_trade.py:231 | Missing pandas import | Medium |
| cs-cli/src/main.rs:1764 | Parquet export comment mentions unsafe transmute | Low |

### 5.2 TODOs in Production Code

| File | Line | TODO |
|------|------|------|
| cs-cli/src/main.rs | 1895 | Add plotting implementation |
| cs-cli/src/main.rs | 1933 | Add plotting implementation |
| cs-cli/src/main.rs | 2290 | Migrate to MultiLegStrategyConfig |
| cs-backtest/src/session_executor.rs | 368 | CalendarStraddle doesn't implement RollableTrade |
| cs-backtest/src/session_executor.rs | 622 | Extract realized_vol_metrics from result |
| cs-backtest/src/trade_executor.rs | 529 | Set entry vol for EntryIV/EntryHV modes |
| cs-domain/src/infrastructure/parquet_results_repo.rs | 40 | Convert to Parquet for better performance |
| cs-domain/src/infrastructure/earnings_repo.rs | 38 | Implement actual earnings data loading |

### 5.3 StubEarningsRepository Still in Use

**File:** cs-domain/src/infrastructure/earnings_repo.rs

```rust
pub struct StubEarningsRepository {
    // Returns empty list always!
}

impl EarningsRepository for StubEarningsRepository {
    async fn load_earnings(...) -> Result<Vec<EarningsEvent>, RepositoryError> {
        // TODO: Implement actual earnings data loading
        Ok(Vec::new())  // ALWAYS RETURNS EMPTY!
    }
}
```

**Impact:** If this stub is used instead of real repos, backtests will silently produce no trades.

---

## 6. Refactoring Opportunities

### 6.1 High Priority

**1. Consolidate Trade Result Serialization**

Currently, save_campaign_json_per_symbol (main.rs:2544-2600+) has repetitive downcasting:
```rust
if let Some(sr) = trade_result.downcast_ref::<StraddleResult>() { ... }
if let Some(cr) = trade_result.downcast_ref::<CalendarSpreadResult>() { ... }
if let Some(ir) = trade_result.downcast_ref::<IronButterflyResult>() { ... }
```

**Recommendation:** Add a `Serializable` trait to TradeResult types or use serde_json directly on the trait object.

**2. Extract CLI Builder Functions**

`build_cli_overrides` (main.rs:1016-1149) has 50+ parameters.

**Recommendation:** Use a builder pattern or derive from Clap args directly.

**3. Unify Earnings Repository Pattern**

Multiple earnings repository implementations exist:
- `StubEarningsRepository` (does nothing)
- `ParquetEarningsRepository`
- `EarningsReaderAdapter`
- `CustomFileEarningsReader`

**Recommendation:** Remove stub, consolidate into single configurable repository.

### 6.2 Medium Priority

**4. Extract Display Functions**

`display_backtest_results` and `display_rolling_results` duplicate formatting logic.

**Recommendation:** Create a `ResultsFormatter` trait/struct.

**5. Configuration Validation**

Validation happens at runtime in multiple places. Some invalid combinations silently fallback.

**Recommendation:** Add comprehensive config validation in `load_config()`.

### 6.3 Low Priority

**6. Python Package Structure**

The simulation directory uses `sys.path` hacks.

**Recommendation:** Make it a proper Python package with `__init__.py` exports.

**7. Error Message Consistency**

Some errors use `anyhow::bail!`, others use custom types.

**Recommendation:** Standardize on domain-specific error types.

---

## 7. Replay-ability Assessment

### 7.1 Output Formats for Replay

| Format | Replay Support | Notes |
|--------|----------------|-------|
| JSON | Excellent | Full trade details, human-readable |
| Parquet | Limited | Only calendar spreads, needs conversion |
| CSV | Limited | Summary data only, loses nested fields |

### 7.2 Required Fields for Replay

The replay scripts expect these fields in results:
- `symbol` (required)
- `entry_time` (required)
- `exit_time` (required)
- `earnings_date` (optional but used for markers)
- `pnl` (required for filtering)
- `pnl_pct` (optional)
- `success` (required for filtering)
- `spot_at_entry`, `spot_at_exit` (optional for charts)
- `iv_entry`, `iv_exit` (optional for IV evolution)
- `net_delta`, `net_gamma`, `net_theta`, `net_vega` (optional for Greeks panel)

### 7.3 Gaps in Replay Support

1. **No replay for rolling results** - Rolling straddle outputs different schema
2. **Campaign results need conversion** - CSV output loses detail
3. **No Greeks history** - Only entry Greeks stored, not evolution

---

## 8. Recommendations Summary

### Immediate Actions (Block release)

1. Fix `batch_replay_trades.py` Polars iteration bug
2. Add missing `pandas` import to `replay_trade.py`
3. Either implement or remove `analyze` and `price` CLI commands

### Short-term (Next sprint)

4. Change straddle timing args to `Option<T>` to respect config files
5. Use `FINQ_DATA_DIR` consistently across Python scripts
6. Add plotting implementation for ATM IV command

### Medium-term (Backlog)

7. Remove `StubEarningsRepository`
8. Consolidate trade result serialization
9. Add comprehensive config validation
10. Create proper Python package structure for simulation

---

## Appendix A: File Statistics

| Component | Files Reviewed | Issues Found |
|-----------|----------------|--------------|
| cs-cli/src/main.rs | 1 | 4 |
| cs-cli/src/config.rs | 1 | 1 |
| cs-cli/src/cli_args.rs | 1 | 0 |
| replay_trade.py | 1 | 4 |
| batch_replay_trades.py | 1 | 2 |
| simulation/*.py | 4 | 1 |
| cs-domain/src/infrastructure/*.rs | 2 | 2 |

## Appendix B: Commands to Test

```bash
# Test backtest (should work)
cargo run --bin cs -- backtest --start 2025-01-01 --end 2025-03-31 \
  --spread calendar --selection atm --option-type call

# Test unimplemented commands (will fail gracefully)
cargo run --bin cs -- analyze --run-dir ./results
cargo run --bin cs -- price --symbol AAPL --strike 150 \
  --short-expiry 2025-02-21 --long-expiry 2025-03-21 --date 2025-02-01

# Test Python replay (after fixes)
uv run python3 replay_trade.py --result results.json --index 0
uv run python3 batch_replay_trades.py --result results.parquet --max-trades 10
```

---

**End of Report**
