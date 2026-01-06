# TICKET-002: Add Hedging Support to Campaign Command

**Status**: Open
**Priority**: Medium
**Created**: 2026-01-06

---

## Problem

The `campaign` command doesn't support hedging while `backtest` does.

---

## Current Workaround

Use `backtest` command for hedged straddles:

```bash
export FINQ_DATA_DIR=~/polygon/data && ./target/debug/cs backtest \
    --symbols PENG \
    --spread straddle \
    --straddle-entry-days 14 \
    --straddle-exit-days 1 \
    --earnings-file custom_earnings/PENG_2025.parquet \
    --start 2025-01-01 --end 2025-12-31 \
    --hedge --hedge-strategy time --hedge-interval-hours 24
```

---

## Required Changes

1. Add `--hedge`, `--hedge-strategy`, `--hedge-interval-hours` flags to Campaign command (done but incomplete)

2. SessionExecutor needs to pass hedge config to TradeExecutor (partially done)

3. Fix API mismatch between CLI and domain types:
   - `HedgeStrategy::DeltaThreshold` not `DeltaBased`
   - `HedgeConfig.transaction_cost_per_share` not `cost_per_share`
   - `TimingStrategy` creation from `HedgeStrategy`

4. Add hedge P&L to SessionPnL struct

---

## Files to Modify

- `cs-cli/src/main.rs`: Fix hedge config construction
- `cs-backtest/src/session_executor.rs`: Ensure hedging is applied correctly
- `cs-backtest/src/session_executor.rs`: Add hedge_pnl to SessionPnL
