# Domain Migration Checklist

Date: 2026-04-07
Status: Living document — update each row when work is completed.

This checklist tracks the migration status of every term in `canonical-domain-vocabulary.md`.
Each row answers: is the codebase consistent with the canonical definition?

See the vocabulary doc for what "canonical" means per concept.

---

## Checklist

| Concept | Canonical? | Old names in code? | Builder exists? | DTO split done? | Priority | Notes |
|---------|-----------|-------------------|-----------------|-----------------|----------|-------|
| `EarningsEvent` | ✅ Yes | ⚠️ Yes — `date`, `time` in stale test mocks | ❌ No | N/A | **High** | Test mocks in `cs-domain/src/rules/event.rs` and `cs-backtest/src/rules/evaluator.rs` use old field names |
| `EarningsTime` | ✅ Yes | ❌ No | N/A | N/A | Low | Stable — 3 clean variants |
| `TradingCampaign` | ✅ Yes | ❌ No | ❌ No | N/A | Medium | Older tests construct directly without `multi_leg_strategy_config` — works today but fragile |
| `TradeDirection` | ✅ Yes | ❌ No | N/A | N/A | Low | Stable — 2 clean variants |
| `Greeks` | ✅ Yes | ❌ No | N/A | N/A | Low | Clean; `PositionGreeks` wrapper is correct |
| `TradeIntent` | ❌ Does not exist | N/A | N/A | N/A | Deferred | Closest current equivalent: `TradeOpportunity` |
| `ExecutedTrade` | ❌ Does not exist | N/A | N/A | N/A | Deferred | Closest: `CalendarSpreadResult` etc. in `entities.rs` |
| `TradeAccounting` | ✅ Yes | ❌ No | ✅ Factory methods | N/A | Low | Well-designed — factory pattern already in place |
| `BacktestCommand` | ⚠️ Partial | `data_dir` deprecated in `BacktestConfig` | ✅ `BacktestConfigBuilder` | ❌ No — `BacktestConfig` is DTO/domain hybrid | **High** | ADR-0003 target |
| `CampaignCommand` | ⚠️ Partial | ❌ No | ✅ `CampaignConfigBuilder` | ❌ No — `CampaignConfig` is DTO/domain hybrid | **High** | ADR-0003 target |

---

## Action items by priority

### High — do next

#### 1. Introduce `EarningsEventBuilder`

**Scope:** Add builder type in `cs-domain`, replace test helpers.

**Files to touch:**
- `cs-domain/src/entities.rs` or a new `cs-domain/src/builders/earnings_event.rs`
- `cs-domain/src/rules/event.rs` — replace `mock_event()` helper
- `cs-backtest/src/rules/evaluator.rs` — replace duplicate `mock_event()` helper
- Any test file using `EarningsEvent { ... }` direct brace init

**Acceptance:** `rg "EarningsEvent {" --type rust` returns zero results outside the builder
and the infrastructure adapter mapping function.

**Linked ADR:** ADR-0005

---

#### 2. Split `BacktestConfig` into DTO + application command

**Scope:** Separate the parsing/IO concerns from the domain intent.

**Files to touch:**
- `cs-backtest/src/config/mod.rs` — extract domain fields into `RunBacktestCommand`
- `cs-cli/src/config/builder.rs` — produce `RunBacktestCommand` from CLI args
- `cs-backtest/src/backtest_use_case.rs` — consume `RunBacktestCommand` instead of `BacktestConfig`

**Acceptance:** `BacktestConfig` contains only serialization fields (TOML-parseable).
`RunBacktestCommand` contains no `PathBuf`, no `Option<String>` timing fields.

**Linked ADR:** ADR-0003

---

#### 3. Split `CampaignConfig` into DTO + application command

**Scope:** Same pattern as `BacktestConfig`.

**Files to touch:**
- `cs-backtest/src/campaign_config.rs` — extract domain fields
- `cs-cli/src/config/campaign_builder.rs` — produce campaign command from CLI args
- `cs-backtest/src/campaign_use_case.rs` — consume command instead of config

**Acceptance:** `CampaignConfig.data_dir` and `CampaignConfig.earnings_source` are in the
DTO only. `OptionStrategy`, `PeriodPolicy`, `ExpirationPolicy` flow through a typed command.

**Linked ADR:** ADR-0003

---

### Medium — after High items

#### 4. Introduce `TradingCampaignBuilder`

**Scope:** Add builder in `cs-domain/src/campaign/`. Update tests that construct directly.

**Files to touch:**
- `cs-domain/src/campaign/campaign.rs` — add builder or separate `builders` module
- `cs-domain/src/campaign/campaign.rs` test section — migrate test construction

**Linked ADR:** ADR-0005

---

#### 5. Remove `BacktestConfig.data_dir` deprecated field

**Scope:** Remove after confirming no config files in production use the old key.

**Prerequisite:** Step 2 above completed. Verify with `rg "data_dir" configs/`.

**Acceptance:** `#[serde(skip_serializing_if = "Option::is_none")] pub data_dir` removed.

---

### Deferred — needs design first

#### 6. Define `TradeIntent`

**Decision needed:** Does `TradeOpportunity` become `TradeIntent`, or is `TradeIntent` a
new type that wraps or replaces it?

**Current state:** `TradeOpportunity` in `cs-domain/src/entities.rs` carries market data
fields used for strike selection. It is not yet a clean "intent to trade" object.

**Approach when ready:** Create `TradeIntent` in Execution context, deprecate `TradeOpportunity`
if it fully replaces it, or keep `TradeOpportunity` as the Market Data input and `TradeIntent`
as the Execution decision.

---

#### 7. Define `ExecutedTrade`

**Decision needed:** Should the `*Result` types (`CalendarSpreadResult`, etc.) be split into
an execution record and an accounting record, or does `ExecutedTrade` simply wrap them?

**Current state:** `CalendarSpreadResult` in `entities.rs` contains both execution fields
(entry/exit prices, leg details) and PnL fields. It crosses the Execution/Accounting boundary.

**Approach when ready:** Introduce `ExecutedTrade` as an Execution output. Pass it to
Accounting, which computes `TradePnlRecord` from it. The `*Result` types become aliases or
are renamed.

---

## How to update this checklist

When you complete one of the action items above:

1. Change the table cell: `❌ No` → `✅ Done (YYYY-MM-DD)`
2. Strike through or remove the action item below the table.
3. Update the linked ADR if its consequences section changes.
4. Note in the PR description which checklist item was completed.
