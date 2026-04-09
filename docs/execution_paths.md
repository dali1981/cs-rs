# execution_paths

## Canonical CLI command

```bash
cargo run -p cs-cli --bin cs -- backtest \
  --conf configs/defaults.toml \
  --start 2025-01-01 \
  --end 2025-03-31 \
  --output out/backtest_2025q1.json
```

Canonical production path:

```text
CLI -> config -> use_case -> services -> domain -> adapters
```

## What runs for canonical `cs backtest`

1. `cs-cli/src/main.rs::main` parses CLI and dispatches `Commands::Backtest`.
2. `cs-cli/src/commands/backtest.rs::BacktestCommand::build_command` builds `(RunBacktestCommand, DataSourceConfig, EarningsSourceConfig)` via `BacktestConfigBuilder`.
3. `cs-cli/src/mapping/backtest_command_mapper.rs::map_config_to_command` maps DTO config to typed run command.
4. `cs-cli/src/factory/use_case_factory.rs::UseCaseFactory::create_backtest` wires repositories (`RepositoryFactory` or `IbRepositoryFactory`) and constructs `BacktestUseCase`.
5. `cs-backtest/src/backtest_use_case.rs::BacktestUseCase::execute` selects strategy and calls `execute_with_strategy`.
6. `cs-backtest/src/trade_strategy.rs` + `cs-backtest/src/execution/*` + `cs-backtest/src/rules/evaluator.rs` execute pricing, filtering, and result assembly.
7. `cs-cli/src/output/backtest.rs::{display_unified,save_unified}` emits terminal summary and optional JSON output.

No other command path is part of canonical production execution for `cs backtest`.

## Classification legend

- `production`: used by canonical `cs backtest` execution path.
- `test-support`: only used by tests or test-only assertions.
- `benchmark`: only used by benchmark harnesses.
- `experimental`: intentionally retained, runnable, but non-canonical for production backtest path.
- `dead`: unreachable, obsolete, or misleading relative to current runtime contract.

## Module/function classification inventory

The table below classifies all audited executable surfaces (entrypoints, orchestration modules, strategy dispatch, utility binaries, tests, and known legacy APIs).

| Surface | Classification | Evidence / reason |
|---|---|---|
| `cs-cli/src/main.rs::main` | `production` | Canonical CLI entrypoint for `cs` binary. |
| `cs-cli/src/cli.rs::Commands::Backtest` | `production` | Canonical subcommand selection for runtime path. |
| `cs-cli/src/commands/backtest.rs::{build_command,execute}` | `production` | Canonical backtest command handler and executor. |
| `cs-cli/src/config/builder.rs::{build,build_raw_config,parse_date}` | `production` | Canonical config layering/validation before execution. |
| `cs-cli/src/config/app.rs` | `production` | Source for merged `AppConfig` -> `BacktestConfig` conversion used by builder. |
| `cs-cli/src/mapping/backtest_command_mapper.rs::map_config_to_command` | `production` | Canonical DTO -> command handoff boundary. |
| `cs-cli/src/factory/use_case_factory.rs::{create_backtest,create_backtest_with_factory,command_to_config}` | `production` | Repository wiring and use-case construction for canonical run. |
| `cs-cli/src/factory/repository_factory.rs` | `production` | Default (full/demo) repo factory used by canonical create path. |
| `cs-cli/src/factory/ib_repository_factory.rs` | `production` | Canonical IB branch when `--data-source ib` is selected. |
| `cs-backtest/src/commands.rs::{RunBacktestCommand,...}` | `production` | Typed application command consumed by backtest use case path. |
| `cs-backtest/src/config/mod.rs` | `production` | Runtime spread/selection/timing config used by execute path. |
| `cs-backtest/src/backtest_use_case.rs::{execute,execute_with_strategy,execute_tradable_batch}` | `production` | Core orchestration path used by canonical command. |
| `cs-backtest/src/trade_strategy.rs` | `production` | Strategy dispatch + execution adapters used by use case. |
| `cs-backtest/src/execution/*` | `production` | Trade execution implementations used by strategy dispatch. |
| `cs-backtest/src/rules/evaluator.rs` | `production` | Entry-rule gating in canonical backtest execution flow. |
| `cs-backtest/src/trade_executor_factory.rs` | `production` | Production executor/pricer factory used by strategies. |
| `cs-backtest/src/run_contract.rs::{RunInput,RunOutput,RunSummary}` | `production` | Canonical run summary contract emitted from canonical results. |
| `cs-cli/src/output/backtest.rs::{display_unified,save_unified}` | `production` | Canonical output path. |
| `cs-domain/src/repositories.rs` + `cs-domain/src/infrastructure/{finq_*,ib_*,earnings_reader_adapter.rs,custom_file_earnings.rs,earnings_repo.rs}` | `production` | Domain ports and concrete adapters wired by canonical repository factories. |
| `cs-cli/tests/command_mapper.rs` | `test-support` | Verifies command mapping invariants only in tests. |
| `cs-cli/tests/demo_smoke.rs` | `test-support` | Demo smoke contract test, not runtime production code. |
| `cs-backtest/tests/run_contract_spec.rs` | `test-support` | Run-contract and docs guard tests. |
| `cs-backtest/tests/test_crbg_execution.rs` | `test-support` | Data-dependent integration test guarded by feature flags. |
| `cs-cli/src/parsing/time_config.rs` test module (`#[cfg(test)]`) | `test-support` | Test-only validation of parsing helpers. |
| `cs-cli/src/commands/atm_iv.rs` | `experimental` | Runnable command, but outside canonical `cs backtest` production path. |
| `cs-backtest/src/atm_iv_use_case.rs` | `experimental` | Supports `atm-iv` command, not canonical backtest path. |
| `cs-backtest/src/minute_aligned_iv_use_case.rs` | `experimental` | Supports `atm-iv` mode, not canonical backtest path. |
| `cs-cli/src/commands/campaign.rs` | `experimental` | Runnable alternative path, not canonical backtest command. |
| `cs-cli/src/config/campaign_builder.rs` | `experimental` | Campaign builder includes TODO/hardcoded fields; non-canonical path. |
| `cs-cli/src/output/campaign.rs` | `experimental` | Output for campaign path only. |
| `cs-backtest/src/campaign_use_case.rs` | `experimental` | Campaign orchestration path outside canonical command. |
| `cs-backtest/src/session_executor.rs` | `experimental` | Session/campaign executor for non-canonical strategy set. |
| `cs-backtest/src/earnings_analysis_use_case.rs` | `experimental` | Use case retained for analysis surfaces, not canonical backtest path. |
| `cs-cli/src/bin/{debug_idxx.rs,test_load_idxx.rs,test_earnings.rs,test_ib_chain_schema.rs,view_atm_iv.rs,plot_atm_iv.rs}` | `experimental` | Utility/test binaries, not canonical runtime command path. |
| `cs-cli/src/commands/analyze.rs::execute` | `dead` | CLI-exposed but TODO stub; currently prints only and does not perform analysis. |
| `cs-cli/src/commands/price.rs::execute` | `dead` | CLI-exposed but TODO stub; currently prints only and does not perform pricing flow. |
| `cs-cli/src/commands/earnings.rs::execute` | `dead` | CLI-exposed but TODO stub; currently prints only and does not perform analysis use case. |
| `cs-cli/src/handlers/mod.rs` + `cs-cli/src/handlers/earnings_output.rs::{save_earnings_parquet,save_earnings_csv,save_earnings_json}` | `dead` | Unused legacy handler layer; only commented re-export remains. |
| `cs-cli/src/parsing/earnings_loader.rs::{load_earnings_from_file,load_earnings_for_symbols}` | `dead` | No call sites in current runtime/test paths. |
| `cs-cli/src/parsing/roll_policy.rs::{parse_roll_policy,parse_campaign_roll_policy}` | `dead` | No call sites in current runtime/test paths. |
| `cs-cli/src/parsing/time_config.rs::parse_delta_range` | `test-support` | Currently exercised only by local unit tests; not part of runtime path. |
| `cs-backtest/src/backtest_use_case.rs::{execute_batch,load_earnings_for_strategy,report_progress}` | `dead` | Explicitly marked old/deprecated date-centric helpers and unused in canonical execution. |
| `cs-backtest/src/backtest_use_case.rs::{execute_calendar_spread,execute_iron_butterfly,execute_straddle,execute_post_earnings_straddle,execute_calendar_straddle}` | `dead` | Legacy API wrappers; no internal call sites found for canonical runtime. |
| `cs-backtest/src/lib.rs` legacy exports + files `{straddle_pricer.rs,iron_butterfly_pricer.rs,calendar_straddle_pricer.rs}` | `dead` | Deprecated legacy pricer surface superseded by `CompositePricer` family. |
| `workspace benches` | `benchmark` | No benchmark harnesses currently present in this repository. |

## Dead/misleading paths and proposed actions

| Item | Problem | Proposed action |
|---|---|---|
| `analyze`, `price`, `earnings-analysis` command handlers | Exposed in help output but implementations are TODO stubs. | `quarantine`: hide behind non-default feature or mark as experimental in CLI help until implemented. |
| Legacy backtest wrapper APIs in `BacktestUseCase` | Parallel legacy API surface creates ambiguity vs canonical `execute`. | `remove`: delete wrappers after confirming no external consumers. |
| Legacy pricer exports in `cs-backtest/src/lib.rs` | Deprecated surface remains exported and can be imported accidentally. | `quarantine` immediately (feature-gate or stop re-export), then `remove` in follow-up ticket. |
| Unused earnings/parsing helper modules in `cs-cli` | Maintains misleading alternative paths with no current callers. | `remove` if no upcoming ticket depends on them; otherwise `quarantine` under `legacy` module. |

## Benchmark status

`benchmark` classification currently has no members because no benchmark harnesses are checked in. If benchmark files are added later (`*/benches/*`), classify them under `benchmark` only.

## Canonical command clarity statement

Invoking the canonical command (`cs backtest ...`) executes only the production chain listed above; campaign/ATM-IV/utility-bin/stub command paths are outside canonical execution and should not be used as runtime architecture references.
