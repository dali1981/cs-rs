# Canonical Domain Vocabulary

Date: 2026-04-07
Status: Accepted (living document — update when a term is stabilized or retired)

This document is the single source of truth for domain naming in `cs-rs`.
When a name is listed here as canonical, that name is used everywhere: in struct fields,
in doc comments, in test helpers, in PR descriptions, and in conversations.

Everything else is migration debt. See `domain-migration-checklist.md` for status.

---

## How to use this document

Before adding a new type or field:
1. Check if the concept already exists here.
2. If yes, use the canonical name — do not introduce a synonym.
3. If no, propose an addition to this doc in the same PR as the code.

Before renaming anything:
1. Update the canonical name here first.
2. Update the migration checklist to mark the old name as deprecated.
3. Then change the code.

---

## EarningsEvent

**Canonical name:** `EarningsEvent`
**Defined in:** `cs-domain/src/entities.rs`
**Owning context:** Market Data
**Kind:** Domain entity

**Canonical shape:**

```rust
pub struct EarningsEvent {
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub company_name: Option<String>,
    pub eps_forecast: Option<Decimal>,
    pub market_cap: Option<u64>,
}
```

**Rules:**
- `earnings_date` and `earnings_time` are the canonical field names. The names `date` and
  `time` are deprecated — they appear in stale test helpers but not in the struct itself.
- Provider adapters (Finq, IB, custom file, demo) must map their raw format into this type.
  They must not define alternate public `EarningsEvent` types.
- Tests must construct this type via `EarningsEventBuilder` (does not yet exist — see
  checklist). Until the builder exists, use `EarningsEvent::new()` factory.
- Direct brace initialization (`EarningsEvent { earnings_date: ..., ... }`) is permitted
  only inside the builder itself and inside the `earnings_reader_adapter.rs` mapping.

**Deprecated synonyms:**
- Field `date` → `earnings_date`
- Field `time` → `earnings_time`

---

## EarningsTime

**Canonical name:** `EarningsTime`
**Defined in:** `cs-domain/src/value_objects.rs`
**Owning context:** Market Data
**Kind:** Value object (enum)

**Canonical shape:**

```rust
pub enum EarningsTime {
    BeforeMarketOpen,
    AfterMarketClose,
    Unknown,
}
```

**Rules:**
- Three variants only. Do not add `AfterClose` or `BMO` as shorthand aliases.
- `Unknown` is the correct variant when the time of day cannot be determined — not `None`,
  not a sentinel string.
- When mapping from external sources, prefer `BeforeMarketOpen` or `AfterMarketClose`.
  Use `Unknown` only when the source genuinely does not provide this information.

**Deprecated synonyms:** None currently. `AfterClose` must not be introduced.

---

## TradingCampaign

**Canonical name:** `TradingCampaign`
**Defined in:** `cs-domain/src/campaign/campaign.rs`
**Owning context:** Campaign / Scheduling
**Kind:** Domain aggregate

**Canonical shape:**

```rust
pub struct TradingCampaign {
    pub symbol: String,
    pub strategy: OptionStrategy,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub period_policy: PeriodPolicy,
    pub expiration_policy: ExpirationPolicy,
    pub iron_butterfly_config: Option<IronButterflyConfig>,
    pub multi_leg_strategy_config: Option<MultiLegStrategyConfig>,
    pub trade_direction: TradeDirection,
}
```

**Rules:**
- `TradingCampaign` is a scheduling aggregate — it defines _when_ and _what_ to trade for
  one symbol. It does not contain execution results.
- `iron_butterfly_config` and `multi_leg_strategy_config` are strategy-specific extensions.
  Tests that do not test iron butterfly or multi-leg behavior may omit these fields only if
  they use a builder (otherwise struct update syntax `..Default::default()` is required).
- `TradingCampaignBuilder` does not yet exist. Until it does, use the direct constructor
  approach but include all fields explicitly. Do not rely on `Default` silently.
- The aggregate is built by `CampaignUseCase`, not by the CLI directly.

**Deprecated synonyms:** None. But `CampaignConfig` in `cs-backtest` is a DTO, not this type.
Do not confuse them. See `BacktestConfig` and `CampaignCommand` below.

---

## TradeDirection

**Canonical name:** `TradeDirection`
**Defined in:** `cs-domain/src/value_objects.rs`
**Owning context:** Strategy Definition
**Kind:** Value object (enum)

**Canonical shape:**

```rust
pub enum TradeDirection {
    Long,
    Short,
}
```

**Default:** `Short`

**Rules:**
- Two variants only. `Long` means buying the structure (paying premium).
  `Short` means selling the structure (receiving premium).
- Do not add directional variants for individual legs here — `TradeDirection` is always
  applied at the structure level.
- When serializing to TOML/JSON, use `serde(rename_all = "snake_case")` — `"long"` / `"short"`.

**Deprecated synonyms:** None.

---

## Greeks

**Canonical name:** `Greeks`
**Defined in:** `cs-analytics/src/greeks.rs`
**Owning context:** Accounting / Attribution (analytics primitives)
**Kind:** Value object (pure math)

**Canonical shape:**

```rust
pub struct Greeks {
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,
    pub vega: f64,
    pub rho: f64,
}
```

**Rules:**
- `Greeks` holds per-share, single-contract sensitivities. All values are raw (not scaled
  by contract multiplier).
- Arithmetic traits (`Add`, `Sub`, `Mul<f64>`, `Neg`) are implemented — use them instead
  of field-by-field manual combination.
- `Greeks::ZERO` is the canonical zero value. Use it instead of `Greeks { delta: 0.0, ... }`.
- `PositionGreeks` (in `cs-domain/src/position/daily_snapshot.rs`) is the position-level
  wrapper that applies contract multiplier scaling. When working with a portfolio or spread,
  use `PositionGreeks`, not raw `Greeks`.
- Do not add `charm`, `vanna`, or second-order Greeks to this struct without a dedicated
  task — they change the trait implementations and serialization.

**Deprecated synonyms:** None. But do not introduce a parallel `SpreadGreeks` type —
use `Greeks::spread()` factory method or sum via `Add`.

---

## TradeIntent

**Canonical name:** `TradeIntent`
**Status:** Does not exist yet — reserved name for a future type.
**Owning context (target):** Execution / Simulation
**Kind (target):** Value object representing a decision to enter a trade before execution

**Rules when introduced:**
- `TradeIntent` will represent a pre-execution decision: "try to enter this spread at this
  time with these parameters." It is distinct from `TradeOpportunity` (which holds market
  data used for selection) and from an executed result.
- Do not use `TradeOpportunity` for this purpose. `TradeOpportunity` is the current closest
  concept; it may be renamed to `TradeIntent` in a future task.

**Current closest equivalent:** `TradeOpportunity` in `cs-domain/src/entities.rs`

---

## ExecutedTrade

**Canonical name:** `ExecutedTrade`
**Status:** Does not exist yet — reserved name for a future type.
**Owning context (target):** Execution / Simulation
**Kind (target):** Entity representing a trade that has been filled (entry confirmed)

**Rules when introduced:**
- `ExecutedTrade` is the handoff from Execution to Accounting. It carries entry price, legs,
  and execution metadata — but not PnL (that is computed by Accounting).
- `TradePnlRecord` (in `cs-domain/src/pnl/record.rs`) is the current closest equivalent for
  the Accounting view. `CalendarSpreadResult` / `StraddleResult` etc. in `entities.rs`
  combine execution output and PnL fields — they are the current de facto `ExecutedTrade`.

**Current closest equivalents:**
- `CalendarSpreadResult`, `StraddleResult`, `IronButterflyResult` in `cs-domain/src/entities.rs`
- `TradePnlRecord` in `cs-domain/src/pnl/record.rs`

---

## TradeAccounting

**Canonical name:** `TradeAccounting`
**Defined in:** `cs-domain/src/accounting/trade_accounting.rs`
**Owning context:** Accounting / Attribution
**Kind:** Value object (computed, immutable after construction)

**Canonical shape:**

```rust
pub struct TradeAccounting {
    pub capital_required: CapitalRequirement,
    pub entry_cash_flow: Decimal,
    pub exit_cash_flow: Decimal,
    pub transaction_costs: Decimal,
    pub hedge_pnl: Option<Decimal>,
    pub realized_pnl: Decimal,
    pub return_on_capital: f64,
    pub max_loss: Option<Decimal>,
}
```

**Rules:**
- `TradeAccounting` is computed, not mutated. All factory methods return a fully-constructed
  value: `for_debit_trade()`, `for_credit_trade()`, `from_pnl()`, `from_cashflows()`.
- Do not add mutable setters. If a field changes post-construction (e.g., hedge PnL is
  updated), compute a new `TradeAccounting` value.
- `hedge_pnl: Option<Decimal>` is `None` for unhedged trades. Do not use `Decimal::ZERO`
  as a sentinel — use `None`.

**Deprecated synonyms:** None. `TradeStatistics` is a different type (aggregate stats over
many trades, not accounting for one trade).

---

## BacktestCommand

**Canonical name:** `BacktestCommand`
**Defined in:** `cs-cli/src/commands/backtest.rs`
**Owning context:** Application / Composition
**Kind:** CLI dispatch handler (not a domain type)

**Canonical shape:**

```rust
pub struct BacktestCommand {
    args: BacktestArgs,
    global: GlobalArgs,
}
```

**Rules:**
- `BacktestCommand` is a CLI handler, not an application command in the DDD sense. It wraps
  parsed CLI args and delegates to `BacktestConfigBuilder` and then to the use case.
- The _application command_ that carries business intent does not yet exist as a distinct type.
  Per ADR-0003, it will be a struct like:
  ```rust
  pub struct RunBacktestCommand {
      pub period: BacktestPeriod,
      pub strategy: StrategySpec,
      pub filters: FilterSet,
      pub output: OutputOptions,
  }
  ```
  Until that type exists, `BacktestConfig` (in `cs-backtest/src/config/`) is the effective
  handoff object between Application and Execution.
- `BacktestConfig.data_dir` is **deprecated**. Use `BacktestConfig.data_source` instead.
  The field exists only for TOML backward compatibility with old config files.

**Deprecated synonyms:**
- `BacktestConfig.data_dir` → `BacktestConfig.data_source`

---

## CampaignCommand

**Canonical name:** `CampaignCommand`
**Defined in:** `cs-cli/src/commands/campaign.rs`
**Owning context:** Application / Composition
**Kind:** CLI dispatch handler (not a domain type)

**Canonical shape:**

```rust
pub struct CampaignCommand {
    args: CampaignArgs,
    global: GlobalArgs,
}
```

**Rules:**
- Same rules as `BacktestCommand`. `CampaignCommand` wraps CLI args and delegates to
  `CampaignConfigBuilder`, which produces `CampaignConfig`.
- `CampaignConfig` (in `cs-backtest/src/campaign_config.rs`) is a DTO/domain hybrid per
  the debt noted in ADR-0003. It will be split in a follow-up task.
- When the split happens, the domain part becomes input to the `CampaignUseCase`; the
  infrastructure part (`data_dir`, `earnings_source`) stays in a config DTO.

**Deprecated synonyms:** None currently.
