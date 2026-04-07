# Bounded Context Map

Date: 2026-04-07
Status: Accepted (living document — update as modules move)

This document maps every current module in the `cs-rs` workspace to one of six bounded
contexts. It is the semantic map that architectural decisions (see `docs/adr/`) should
enforce. When a module is ambiguous, this document explains why and what the resolution is.

---

## System Diagram

```text
                    +----------------------------------------------+
                    |         Application / Composition            |
                    |  cs-cli: args, commands, factory, output     |
                    |  cs-cli/src/config: builders, DTO assembly   |
                    +---+----------+----------+----------+---------+
                        |          |          |          |
          +-------------+    +-----+----+  +-+--------+ +-----------+
          |                  |          |  |          | |           |
          v                  v          v  v          v v           v
  +---------------+  +-----------+  +--------+  +----------------+
  |  Market Data  |  | Strategy  |  |Campaign|  |   Accounting / |
  |  normalize,   |  | selection,|  |schedule|  |   Attribution  |
  |  IV surfaces, |  | spread    |  |policy  |  |   PnL, margin, |
  |  spot prices  |  | rules     |  |        |  |   BPR, costs   |
  +-------+-------+  +-----+-----+  +---+----+  +-------+--------+
          |                |             |               |
          +----------------+-------------+               |
                           |                             |
                           v                             |
                  +--------+----------+                  |
                  | Execution /       |                  |
                  | Simulation        +------------------+
                  | pricers, hedging, |
                  | session executor  |
                  +-------------------+
```

Data flows top-to-bottom. `Application` wires the other five contexts together.
`Market Data`, `Strategy`, and `Campaign` are peers that feed `Execution`.
`Execution` feeds `Accounting`.

---

## 1. Market Data

**Purpose:** Normalize raw market facts into typed domain values. This context owns the
boundary between external data (Polars DataFrames, parquet files, broker APIs) and the
domain's typed representations.

**Owns:**
- Options chain data (strikes, expirations, prices, greeks)
- Equity spot prices and bars
- IV surface construction and interpolation
- ATM IV computation
- Historical volatility (HV)
- Earnings event facts (date, time, symbol, market cap) — _not_ the trading rules around them

**Does not own:**
- Strike selection logic (that is Strategy Definition)
- When to enter a trade around earnings (that is Campaign / Scheduling)
- How to price a spread (that is Execution)

**Inputs:** Parquet files, broker APIs, fixture files
**Outputs:** `EarningsEvent`, `SpotPrice`, IV surfaces, option chain DataFrames

**Existing code in this context:**

| Module | Crate | Notes |
|--------|-------|-------|
| `infrastructure/finq_options_repo.rs` | `cs-domain` | ❌ wrong crate — belongs here, not in domain |
| `infrastructure/finq_equity_repo.rs` | `cs-domain` | ❌ wrong crate |
| `infrastructure/ib_options_repo.rs` | `cs-domain` | ❌ wrong crate |
| `infrastructure/ib_equity_repo.rs` | `cs-domain` | ❌ wrong crate |
| `infrastructure/demo_repos.rs` | `cs-domain` | ❌ wrong crate |
| `infrastructure/earnings_repo.rs` | `cs-domain` | ❌ wrong crate |
| `infrastructure/earnings_reader_adapter.rs` | `cs-domain` | ❌ wrong crate |
| `infrastructure/custom_file_earnings.rs` | `cs-domain` | ❌ wrong crate |
| `iv_surface_builder.rs` | `cs-backtest` | ?? should live here or in `cs-analytics` |
| `atm_iv_use_case.rs` | `cs-backtest` | use case orchestration — stays in Application |
| `atm_iv_computer.rs` | `cs-analytics` | ✅ correct |
| `iv_surface.rs`, `iv_model.rs`, `vol_slice.rs` | `cs-analytics` | ✅ correct |
| `realized_volatility.rs` | `cs-analytics` | ✅ correct |
| `iv_statistics.rs` | `cs-analytics` | ✅ correct |
| `entities.rs` → `EarningsEvent` | `cs-domain` | ✅ correct type location; adapters are not |
| `repositories.rs` traits | `cs-domain` | ✅ correct — trait definitions belong in domain |
| `value_objects.rs` → `AtmIvObservation`, `AtmIvConfig` | `cs-domain` | ✅ |
| `value_objects.rs` → `EarningsOutcome`, `EarningsSummaryStats` | `cs-domain` | ?? borderline — outcome analysis is closer to Accounting |

**Known debt:**
- All 8 infrastructure adapters live inside `cs-domain` despite having Polars, async, and
  file I/O dependencies. They violate ADR-0001. Target location: a future `cs-infrastructure`
  crate or an `infrastructure/` layer in `cs-backtest`.

---

## 2. Strategy Definition

**Purpose:** Define _how_ trades are constructed from market data. This context owns the
rules for selecting strikes, building spreads, and determining structural validity of a trade.

**Owns:**
- Strike selection algorithms (ATM, delta-based, iron butterfly wings)
- Spread construction (calendar, straddle, iron butterfly, multi-leg)
- Trade structure types (`CalendarSpread`, `IronButterfly`, `LongStraddle`, `OptionLeg`)
- Entry/exit rules evaluated against market data
- Expiration selection policy

**Does not own:**
- When to trade (that is Campaign / Scheduling)
- How to price an existing spread (that is Execution)
- Accounting for a completed trade (that is Accounting)

**Inputs:** Options chain data (from Market Data), `EarningsEvent` (shared with Market Data)
**Outputs:** Constructed spread objects, rule evaluation results

**Existing code in this context:**

| Module | Crate | Notes |
|--------|-------|-------|
| `strike_selection/atm.rs` | `cs-domain` | ✅ |
| `strike_selection/delta.rs` | `cs-domain` | ✅ |
| `strike_selection/iron_butterfly.rs` | `cs-domain` | ✅ |
| `strike_selection/straddle.rs` | `cs-domain` | ✅ |
| `strike_selection/multi_leg.rs` | `cs-domain` | ✅ |
| `entities.rs` → `CalendarSpread`, `IronButterfly`, `LongStraddle`, `OptionLeg` | `cs-domain` | ✅ |
| `expiration/` | `cs-domain` | ✅ |
| `rules/event.rs` | `cs-domain` | ?? event rule evaluated on `EarningsEvent` — Market Data boundary |
| `rules/market.rs` | `cs-domain` | ✅ — market conditions for entry |
| `rules/trade.rs` | `cs-domain` | ✅ — post-construction trade filters |
| `rules/config.rs` | `cs-domain` | ✅ |
| `strategy/` | `cs-domain` | ✅ — `TradeStrategy`, `TradeStructureConfig` |
| `value_objects.rs` → `SpreadType`, `IronButterflyConfig`, `MultiLegStrategyConfig` | `cs-domain` | ✅ |
| `selection_model.rs` | `cs-analytics` | ✅ |
| `opportunity.rs` | `cs-analytics` | ✅ |

**Known debt:**
- `rules/event.rs` evaluates `EventRule` against `EarningsEvent`. The rule types belong here
  (they constrain which events trigger trading) but `EarningsEvent` is a Market Data type.
  The boundary is correct — the rule _consumes_ an event, it does not own it.
- `entities.rs` is a single flat file containing both Market Data types (`EarningsEvent`) and
  Strategy types (`CalendarSpread`, `OptionLeg`). This makes it a merge conflict hotspot.

---

## 3. Campaign / Scheduling

**Purpose:** Define _when_ to trade. This context owns the sequencing of trading sessions,
the policy for entering around earnings events, and the relationship between an event calendar
and the generated trading windows.

**Owns:**
- Trading period definitions (pre-earnings, post-earnings, monthly)
- Session generation from an event list
- `TradingCampaign` — the symbol-level container for a sequence of sessions
- `TradingSession` — one entry/exit window
- Period policies, roll policies
- Timing calculations (days before earnings, entry/exit times)

**Does not own:**
- Strike selection within a session (that is Strategy Definition)
- Pricing and execution within a session (that is Execution)
- Results accounting (that is Accounting)

**Inputs:** `EarningsEvent` list (from Market Data), date range, policy configuration
**Outputs:** `TradingCampaign`, `Vec<TradingSession>`, `SessionSchedule`

**Existing code in this context:**

| Module | Crate | Notes |
|--------|-------|-------|
| `campaign/campaign.rs` | `cs-domain` | ✅ |
| `campaign/session.rs` | `cs-domain` | ✅ |
| `campaign/schedule.rs` | `cs-domain` | ✅ |
| `campaign/period_policy.rs` | `cs-domain` | ✅ |
| `trading_period/` | `cs-domain` | ✅ |
| `roll/` | `cs-domain` | ✅ — roll policy is a scheduling concern |
| `timing/` | `cs-domain` | ✅ — entry/exit time calculations |
| `campaign_config.rs` | `cs-backtest` | ?? DTO+domain mixed — see ADR-0003 |
| `campaign_use_case.rs` | `cs-backtest` | ❌ orchestration — belongs in Application |
| `timing_strategy.rs` | `cs-backtest` | ?? timing dispatch for backtest — borderline Execution |

**Known debt:**
- `campaign_config.rs` in `cs-backtest` mixes `PathBuf data_dir` (infrastructure) with
  `OptionStrategy`, `PeriodPolicy` (domain). Per ADR-0003, the config part is a DTO and
  the policy fields should flow into an application command.
- `campaign_use_case.rs` is orchestration: it loads earnings, builds campaigns, runs sessions.
  This belongs in Application, not the scheduling context.

---

## 4. Execution / Simulation

**Purpose:** Execute a defined trade structure over a historical or live data stream. This
context owns pricing, fill simulation, hedging, rolling, and session lifecycle management.

**Owns:**
- Spread pricing (`SpreadPricer`, `IronButterflyPricer`, `StraddlePricer`, etc.)
- Trade execution simulation (`TradeExecutor`, `SessionExecutor`)
- Hedging execution and simulation (`HedgingExecutor`, `HedgingSimulator`)
- Delta providers for hedging decisions
- IV surface building at execution time
- Roll execution

**Does not own:**
- Strike selection (that is Strategy Definition)
- Session scheduling (that is Campaign)
- PnL aggregation and statistics (that is Accounting)

**Inputs:** `TradingSession`, options chain (Market Data), trade structure (Strategy)
**Outputs:** Filled trades, hedge records, `TradeResult` objects

**Existing code in this context:**

| Module | Crate | Notes |
|--------|-------|-------|
| `spread_pricer.rs` | `cs-backtest` | ✅ |
| `composite_pricer.rs` | `cs-backtest` | ✅ |
| `straddle_pricer.rs` | `cs-backtest` | ✅ |
| `iron_butterfly_pricer.rs` | `cs-backtest` | ✅ |
| `calendar_straddle_pricer.rs` | `cs-backtest` | ✅ |
| `multi_leg_pricer.rs` | `cs-backtest` | ✅ |
| `trade_executor.rs` | `cs-backtest` | ✅ |
| `trade_executor_factory.rs` | `cs-backtest` | ✅ |
| `session_executor.rs` | `cs-backtest` | ✅ |
| `trade_factory_impl.rs` | `cs-backtest` | ✅ |
| `trade_strategy.rs` | `cs-backtest` | ✅ |
| `hedging_executor.rs` | `cs-backtest` | ✅ |
| `hedging_simulator.rs` | `cs-backtest` | ✅ |
| `delta_providers/` | `cs-backtest` | ✅ |
| `iv_surface_builder.rs` | `cs-backtest` | ?? used at execution time for hedging — stays here for now |
| `timing_strategy.rs` | `cs-backtest` | ?? dispatch of session timing — borderline Campaign |
| `execution/` | `cs-backtest` | ✅ — generic execution traits |
| `greeks_helpers.rs` | `cs-backtest` | ✅ |
| `iv_validation.rs` | `cs-backtest` | ✅ |
| `domain/src/trade/` | `cs-domain` | ✅ — `RollableTrade`, `CompositeTrade`, `LegPosition` |
| `domain/src/hedging.rs` | `cs-domain` | ✅ — hedging domain types |
| `domain/src/position/` | `cs-domain` | ?? position snapshot is shared with Accounting |

**Known debt:**
- `position/daily_snapshot.rs` and `position/position_attribution.rs` in `cs-domain` are
  consumed by both the Execution context (for tracking open positions mid-session) and the
  Accounting context (for daily attribution). The types are in the right crate; the question
  is which context _owns_ them. Resolution: Execution produces `PositionSnapshot`; Accounting
  consumes it. Types stay in domain; ownership is Execution.

---

## 5. Accounting / Attribution

**Purpose:** Measure the financial result of completed trades. This context owns PnL
calculation, margin requirements, BPR, trading cost modeling, and post-trade attribution.

**Owns:**
- PnL records and statistics (`TradePnlRecord`, `PnlStatistics`)
- Trade accounting (`TradeAccounting`, `TradeStatistics`, `CostSummary`)
- Capital and margin (`CapitalRequirement`, `MarginCalculator`, `BprTimeline`)
- Trading costs (`TradingCostCalculator`, all cost models)
- Greeks-based attribution (`GreeksComputer`, `SnapshotCollector`)
- Return basis calculations
- Hedging analytics (post-trade measurement, not hedging decisions)

**Does not own:**
- Whether to enter a trade (that is Strategy / Campaign)
- How to price a spread at execution time (that is Execution)
- Raw market data access (that is Market Data)

**Inputs:** Completed `TradeResult` objects, `PositionSnapshot`, cost config
**Outputs:** `PnlStatistics`, `BprSummary`, attribution reports

**Existing code in this context:**

| Module | Crate | Notes |
|--------|-------|-------|
| `accounting/` | `cs-domain` | ✅ — all of capital, BPR, margin, statistics |
| `trading_costs/` | `cs-domain` | ✅ |
| `pnl/` | `cs-domain` | ✅ |
| `attribution/` | `cs-backtest` | ✅ — greeks-based attribution |
| `hedging_analytics.rs` | `cs-backtest` | ✅ — post-trade hedge measurement |
| `pnl_attribution.rs` | `cs-analytics` | ✅ |
| `greeks.rs` | `cs-analytics` | ✅ — analytics primitives for attribution |
| `black_scholes.rs` | `cs-analytics` | ✅ — pricing math supporting attribution |
| `entities.rs` → `*Result` types | `cs-domain` | ?? `CalendarSpreadResult`, `StraddleResult` etc. are outputs of Execution but inputs to Accounting |
| `value_objects.rs` → `EarningsOutcome`, `EarningsSummaryStats` | `cs-domain` | ?? belongs here, not in value_objects |

**Known debt:**
- `*Result` types (`CalendarSpreadResult`, `IronButterflyResult`, etc.) in `entities.rs`
  contain both execution output fields and PnL fields. They sit at the Execution/Accounting
  boundary. Resolution: they are Execution outputs; Accounting reads them read-only.
- `EarningsOutcome` and `EarningsSummaryStats` in `value_objects.rs` are attribution-analysis
  types, not general value objects. They should move to `pnl/` or `attribution/` in a
  follow-up.

---

## 6. Application / Composition

**Purpose:** Wire the other five contexts together for a specific use case (CLI backtest, CLI
campaign, ATM IV analysis, earnings analysis). This context owns no business logic — it owns
dependency assembly, config resolution, and result formatting.

**Owns:**
- CLI argument parsing and DTO mapping
- Config file loading and merging
- Repository factory (choosing which adapter to wire)
- Use case orchestration (creating and invoking use cases)
- Output formatting and display

**Does not own:**
- Any domain logic
- Any infrastructure adapter implementations (those belong to Market Data adapters)

**Inputs:** CLI args, environment variables, TOML config files
**Outputs:** Formatted output to stdout/stderr; return codes

**Existing code in this context:**

| Module | Crate | Notes |
|--------|-------|-------|
| `cs-cli/src/args/` | `cs-cli` | ✅ — clap wrappers (DTOs) |
| `cs-cli/src/commands/` | `cs-cli` | ✅ — use case orchestration entry points |
| `cs-cli/src/config/` | `cs-cli` | ✅ — DTO builders, config assembly |
| `cs-cli/src/factory/` | `cs-cli` | ✅ — repository and use case wiring |
| `cs-cli/src/output/` | `cs-cli` | ✅ — result formatting |
| `cs-backtest/src/backtest_use_case.rs` | `cs-backtest` | ✅ — use case logic |
| `cs-backtest/src/campaign_use_case.rs` | `cs-backtest` | ✅ — use case logic |
| `cs-backtest/src/atm_iv_use_case.rs` | `cs-backtest` | ✅ — use case logic |
| `cs-backtest/src/earnings_analysis_use_case.rs` | `cs-backtest` | ✅ — use case logic |
| `cs-backtest/src/minute_aligned_iv_use_case.rs` | `cs-backtest` | ✅ |
| `cs-backtest/src/config/` | `cs-backtest` | ?? config types mix DTO and domain — see ADR-0003 |
| `cs-backtest/src/campaign_config.rs` | `cs-backtest` | ?? same issue |
| `cs-backtest/src/rules/` | `cs-backtest` | ✅ — rule evaluation orchestration |

**Known debt:**
- `cs-backtest` is acting as both Execution and Application. Use cases and config assembly
  live there alongside pricers and executors. Target: use cases stay in `cs-backtest` for
  now; the config types are refactored per ADR-0003.

---

## Current vs Target Summary

| Module | Current crate | Context | Status | Action |
|--------|--------------|---------|--------|--------|
| `infrastructure/finq_*_repo.rs` | `cs-domain` | Market Data | ❌ wrong crate | Move to infra layer |
| `infrastructure/ib_*_repo.rs` | `cs-domain` | Market Data | ❌ wrong crate | Move to infra layer |
| `infrastructure/demo_repos.rs` | `cs-domain` | Market Data | ❌ wrong crate | Move to infra layer |
| `infrastructure/earnings_*.rs` | `cs-domain` | Market Data | ❌ wrong crate | Move to infra layer |
| `entities.rs` (mixed) | `cs-domain` | Market Data + Strategy | ?? monolith | Split into context modules |
| `value_objects.rs` (mixed) | `cs-domain` | Multiple contexts | ?? monolith | Split incrementally |
| `rules/event.rs` | `cs-domain` | Strategy / Market Data boundary | ?? | Keep; type ownership is correct |
| `campaign_config.rs` | `cs-backtest` | Campaign (DTO part) + Application | ?? DTO/domain mixed | Refactor per ADR-0003 |
| `campaign_use_case.rs` | `cs-backtest` | Application | ✅ correct intent | Clarify in module docs |
| `timing_strategy.rs` | `cs-backtest` | Execution / Campaign boundary | ?? | Keep in Execution; document |
| `iv_surface_builder.rs` | `cs-backtest` | Market Data / Execution boundary | ?? | Keep in Execution for now |
| `position/daily_snapshot.rs` | `cs-domain` | Execution (produces) / Accounting (consumes) | ?? | Keep; document ownership |
| `*Result` types in `entities.rs` | `cs-domain` | Execution output / Accounting input | ?? | Keep; document boundary |
| `EarningsOutcome`, `EarningsSummaryStats` | `cs-domain/value_objects.rs` | Accounting | ❌ misplaced | Move to `pnl/` |
| `pnl_attribution.rs` | `cs-analytics` | Accounting | ✅ | Keep |
| `cs-cli/src/config/` | `cs-cli` | Application | ✅ | Keep; complete DTO pattern |
| `cs-cli/src/factory/` | `cs-cli` | Application | ✅ | Keep |

---

## Ambiguity Register

These are the boundary cases that require an explicit resolution before code moves:

### 1. `EarningsEvent` — Market Data or Strategy?

`EarningsEvent` is a Market Data fact (date, time, symbol, cap). It is consumed by Strategy
rules (`rules/event.rs`) and Campaign scheduling. It is a **Market Data type** consumed by
multiple contexts. The type stays in domain entities; adapters that load it are Market Data
infrastructure.

**Resolution:** Market Data owns the type. Strategy and Campaign consume it read-only.

### 2. `*Result` types — Execution or Accounting?

`CalendarSpreadResult`, `StraddleResult`, etc. contain both execution fields (entry price,
exit price, legs) and PnL fields. They are the natural handoff object between contexts.

**Resolution:** Execution produces them. Accounting reads them. They live at the boundary —
currently in `entities.rs`, eventually in a dedicated `results/` module in `cs-domain`.

### 3. `position/` — Execution or Accounting?

`PositionSnapshot` tracks the live state of an open trade mid-session.
`DailyAttribution` measures change over a day.

**Resolution:** Execution produces `PositionSnapshot`. Accounting consumes `DailyAttribution`.
The types stay together in `cs-domain/src/position/` because they are tightly coupled; the
context boundary runs through how they are _used_, not where they are _defined_.

### 4. `iv_surface_builder.rs` — Market Data or Execution?

It builds an IV surface from a market data snapshot. Conceptually Market Data, but in
practice it runs inside `cs-backtest` as an execution-time computation.

**Resolution:** Keep in Execution (`cs-backtest`) for now. Mark it as a Market Data
computation running inside Execution. Target: move to `cs-analytics` when there is a clean
data interface.

### 5. `cs-backtest` as dual Execution + Application

`cs-backtest` hosts both use case orchestration (Application) and execution machinery
(Execution). Splitting the crate is not in scope yet.

**Resolution:** Keep in `cs-backtest`. Document which modules belong to which context.
Do not add new orchestration to the execution modules or new execution logic to use cases.
