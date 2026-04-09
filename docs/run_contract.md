# Run Contract

This document defines the explicit run contract types used by canonical backtest execution.

Source of truth:
- `cs-backtest/src/run_contract.rs`

## Overview

The canonical run contract is split into three Rust types:
- `RunInput`: what is required to execute a run
- `RunSummary`: required metrics emitted by every completed run
- `RunOutput`: completed run payload (`input + summary`)

## `RunInput`

```rust
pub struct RunInput {
    pub command: RunBacktestCommand,
    pub data_source: DataSourceConfig,
    pub earnings_source: EarningsSourceConfig,
}
```

Semantics:
- `command`: validated business intent (period, strategy, execution, filters, risk)
- `data_source`: market data adapter configuration (`finq` or `ib` + data dir)
- `earnings_source`: earnings adapter configuration (provider/dir or explicit file)

## `RunSummary`

```rust
pub struct RunSummary {
    pub strategy_family: StrategyFamily,
    pub strategy: SpreadType,
    pub selection_strategy: SelectionType,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub sessions_processed: usize,
    pub total_entries: usize,
    pub total_opportunities: usize,
    pub trade_count: usize,
    pub dropped_event_count: usize,
    pub win_rate_pct: Decimal,
    pub total_pnl: Decimal,
    pub hedging_enabled: bool,
    pub total_hedge_pnl: Option<Decimal>,
    pub total_pnl_with_hedge: Option<Decimal>,
    pub return_basis: ReturnBasis,
}
```

Required summary guarantees:
- always includes strategy identity (`strategy_family`, `strategy`, `selection_strategy`)
- always includes run window (`start_date`, `end_date`)
- always includes participation counts (`total_entries`, `total_opportunities`, `trade_count`, `dropped_event_count`)
- always includes performance basis (`return_basis`) and PnL summary (`total_pnl`)
- hedging metrics are populated only when hedging was used

## `RunOutput`

```rust
pub struct RunOutput {
    pub input: RunInput,
    pub summary: RunSummary,
}
```

This is the canonical completion payload for a run contract consumer.

## `StrategyFamily`

```rust
pub enum StrategyFamily {
    CalendarSpread,
    IronButterfly,
    Straddle,
    CalendarStraddle,
    PostEarningsStraddle,
}
```

Mapping from `SpreadType`:
- `Calendar` -> `CalendarSpread`
- `IronButterfly` and `LongIronButterfly` -> `IronButterfly`
- `Straddle` and `ShortStraddle` -> `Straddle`
- `CalendarStraddle` -> `CalendarStraddle`
- `PostEarningsStraddle` -> `PostEarningsStraddle`

## Construction

### Build input
- construct `RunInput` with a struct literal

### Build summary directly from backtest result
- `RunSummary::from_backtest_result(...)`

### Build output from unified result
- `RunOutput::from_result(input, &unified_result)`

## Validation Notes

Contract-level deterministic tests exist in:
- `cs-backtest/tests/run_contract_spec.rs` (`strategy_family_maps_supported_spreads`, `run_summary_captures_required_fields`, `spec_docs_exist_with_required_sections`)
