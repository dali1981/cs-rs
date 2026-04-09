# ARCHITECTURE_AND_RUN_SPEC

## Goal
Freeze the intended behavior of a canonical backtest run so a new reader can understand how runs work without reading implementation code first.

## Canonical Entrypoint

### Canonical command
```bash
cargo run -p cs-cli --bin cs -- backtest \
  -c configs/defaults.toml \
  --start 2025-01-01 \
  --end 2025-03-31 \
  --output out/backtest_2025q1.json
```

This is the canonical runtime path for backtests:

```text
CLI -> config -> use_case -> services -> domain -> adapters
```

Code path:
- `cs-cli/src/main.rs`
- `cs-cli/src/commands/backtest.rs`
- `cs-cli/src/config/builder.rs`
- `cs-cli/src/mapping/backtest_command_mapper.rs`
- `cs-cli/src/factory/use_case_factory.rs`
- `cs-backtest/src/backtest_use_case.rs`
- `cs-cli/src/output/backtest.rs`

### Example output (representative)
```text
Running backtest...
  Data source: Finq { data_dir: ".../polygon/data" }
  Earnings source: TradingView (.../nasdaq_earnings/data)
  Strategy: Calendar
  Selection: ATM
  Period: 2025-01-01 to 2025-03-31

Results:
Sessions Processed: ...
Total Opportunities: ...
Trades Entered: ...
Win Rate: ...
Total P&L: ...
```

## Run Lifecycle

The run lifecycle is intentionally split into four phases.

### 1) Init
- Parse CLI and global args in `cs-cli/src/main.rs` and `cs-cli/src/cli.rs`.
- Build `BacktestCommand` in `cs-cli/src/commands/backtest.rs`.
- Build merged config in `BacktestConfigBuilder` (`cs-cli/src/config/builder.rs`), including layering from defaults/system/TOML/CLI.
- Convert to typed application command `RunBacktestCommand` in `cs-cli/src/mapping/backtest_command_mapper.rs`.
- Wire repositories via `UseCaseFactory` in `cs-cli/src/factory/use_case_factory.rs`.

### 2) Validate
- Dates are parsed as `YYYY-MM-DD` and fail fast in `BacktestConfigBuilder::parse_date`.
- `--data-source ib` requires `--ib-data-dir` or `IB_DATA_DIR` and fails fast in `BacktestConfigBuilder::build_raw_config`.
- Earnings provider values are validated by `EarningsProvider::from_str` in `cs-backtest/src/config/earnings_source.rs`.
- Timing strategy and times are validated by `BacktestConfig::timing_spec` (`cs-backtest/src/config/mod.rs`), returning `TimingSpecError`.
- Invalid config is returned as `BacktestError::Config`, repository/data failures as `BacktestError::Repository` (`cs-backtest/src/backtest_use_case.rs`).

### 3) Execute
- `BacktestUseCase::execute` dispatches strategy from config spread type (`cs-backtest/src/backtest_use_case.rs`).
- `execute_with_strategy` performs:
  - event search range derivation from timing spec
  - earnings loading
  - tradable event discovery
  - event-level filtering (symbols, market cap, configured rules)
  - trade execution batch (parallel/sequential)
  - post-execution filter checks and dropped-event accounting
- Results are collected in `BacktestResult<R>` and unified into `UnifiedBacktestResult`.

### 4) Emit
- Terminal summary and metrics are emitted via `BacktestOutputHandler::display_unified` (`cs-cli/src/output/backtest.rs`).
- Optional persisted output is emitted with `BacktestOutputHandler::save_unified` (`--output`).
- Canonical summary contract is represented by `RunSummary` in `cs-backtest/src/run_contract.rs`.

## Config Contract

The runtime contract is a layered config that materializes into:
- `RunBacktestCommand` (business intent)
- `DataSourceConfig` and `EarningsSourceConfig` (infrastructure)

Required CLI fields for canonical `backtest` execution:
- `--start <YYYY-MM-DD>`
- `--end <YYYY-MM-DD>`

Core source fields:
- `--data-source`: `finq | ib` (default: `finq`)
- `--ib-data-dir`: required when `--data-source ib`
- `--earnings-source`: `nasdaq | tradingview | yahoo` (default: `tradingview`)
- `--earnings-dir` or `--earnings-file` (mutually exclusive)

Strategy fields:
- `--spread`: `calendar | iron-butterfly | long-iron-butterfly | straddle | short-straddle | calendar-straddle | post-earnings-straddle`
- `--selection`: `atm | delta | delta-scan`
- Timing and strike-selection overrides are optional but validated if present.

Risk/metrics fields:
- `--hedge`, hedging mode fields, and attribution fields
- `--return-basis`
- Optional entry rule flags (`--entry-iv-slope`, `--entry-iv-vs-hv`)

Reference structs:
- `cs-cli/src/config/app.rs` (`AppConfig` and nested sections)
- `cs-backtest/src/config/mod.rs` (`BacktestConfig`)
- `cs-backtest/src/commands.rs` (`RunBacktestCommand`)

## Input / Output Contract

Canonical explicit run contract types now exist in Rust:
- `RunInput`
- `RunOutput`
- `RunSummary`

Location:
- `cs-backtest/src/run_contract.rs`

Detailed field-level contract:
- `docs/run_contract.md`

## Invariants

Each invariant is intentionally testable and tied to concrete code paths.

1. Determinism envelope:
- Invariant: same dataset + same `RunInput` values produce the same `RunSummary` fields (trade_count/opportunity_count/win_rate/total_pnl), assuming identical underlying data files and deterministic providers.
- Verification point: use fixed fixtures and compare serialized `RunSummary`.

2. Fast config failure:
- Invariant: malformed dates, unknown timing strategy, or missing IB dir fail before event loading/execution.
- Enforcement: `BacktestConfigBuilder::parse_date`, `BacktestConfigBuilder::build_raw_config`, `BacktestConfig::timing_spec`.

3. Completed run always emits summary metrics:
- Invariant: every successful run yields a summary including strategy, date range, trade count, and PnL metrics.
- Enforcement: `BacktestResult` + `BacktestOutputHandler::display_summary` + `RunSummary` contract.

4. Transaction cost model is explicit:
- Invariant: costs are only applied through explicit `TradingCostConfig`; there is no implicit cost deduction.
- Enforcement: `BacktestConfig.trading_costs` and cost-specific aggregation paths.

5. Missing required data is hard failure:
- Invariant: repository/data-loading failures return errors and stop execution instead of silently skipping the whole run.
- Enforcement: repository errors mapped to `BacktestError::Repository`.

## Strategy Support Matrix

### Officially supported (canonical `cs backtest`)
- `calendar`
- `iron-butterfly`
- `long-iron-butterfly`
- `straddle`
- `short-straddle`
- `calendar-straddle`
- `post-earnings-straddle`

### Experimental or non-canonical run surfaces
- Session/campaign-only trade structures in `SessionExecutor` paths (`strangle`, `butterfly`, `condor`, `iron_condor`) are not part of the canonical `cs backtest` contract.
- Legacy direct strategy wrapper methods in `BacktestUseCase` were removed in DAL-153; canonical execution routes through `execute`.
