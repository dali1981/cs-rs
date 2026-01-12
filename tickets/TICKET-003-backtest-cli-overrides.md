# TICKET-003: Backtest CLI Overrides Ignored (Pricing + Filters)

**Status**: Open
**Priority**: High
**Created**: 2026-01-08
**Blocks**: `cs backtest` CLI correctness

---

## Problem

The backtest CLI defines flags for pricing and entry filters, but these values never make it into
`CliOverrides` during config building. This means several CLI options are silently ignored.

Defined flags:
- `--pricing-model`, `--vol-model`, `--strike-match-mode` in `cs-cli/src/args/backtest.rs`
- `--min-market-cap`, `--min-notional`, `--max-entry-iv` in `cs-cli/src/args/selection.rs`
- `--delta-range`, `--delta-scan-steps`, `--wing-width`, `--straddle-entry-days`,
  `--straddle-exit-days`, `--min-straddle-dte`, `--min-entry-price`, `--max-entry-price`,
  `--post-earnings-holding-days` in `cs-cli/src/args/strategy.rs`
- `--track-realized-vol` in `cs-cli/src/args/hedging.rs`

Missing wiring:
- `BacktestConfigBuilder::build_cli_overrides` in `cs-cli/src/config/builder.rs` does not set
  `CliOverrides.pricing`, `CliOverrides.strike_match_mode`, `CliOverrides.min_market_cap`,
  `CliOverrides.min_notional`, or `CliOverrides.max_entry_iv`.
- Strategy fields above are not parsed or forwarded into `CliOverrides.strategy`.
- Hedging `track_realized_vol` is ignored (always `None`).

---

## Current Behavior

Running:
```
cs backtest --pricing-model sticky-moneyness --vol-model svi --strike-match-mode same-delta \
    --min-market-cap 5000000000 --min-notional 100000 --max-entry-iv 1.5 ...
```
produces the same effective config as running without these flags.

---

## Required Changes

1. Update `cs-cli/src/config/builder.rs` to populate:
   - `CliOverrides.pricing` from `args.pricing_model` and `args.vol_model`
   - `CliOverrides.strike_match_mode` from `args.strike_match_mode`
   - `CliOverrides.min_market_cap`, `CliOverrides.min_notional`, `CliOverrides.max_entry_iv`
     from `args.selection`
   - `CliOverrides.strategy.delta_range` (parsed via `parse_delta_range`)
   - `CliOverrides.strategy.delta_scan_steps`, `wing_width`, `straddle_*`,
     `min_entry_price`, `max_entry_price`, `post_earnings_holding_days`
   - `CliOverrides.hedging.track_realized_vol` when `--track-realized-vol` is set

2. Add a focused test that verifies these CLI overrides take precedence over TOML defaults.

---

## Acceptance Criteria

- [ ] `--pricing-model` changes `BacktestConfig.pricing_model`
- [ ] `--vol-model` changes `BacktestConfig.vol_model`
- [ ] `--strike-match-mode` changes `BacktestConfig.strike_match_mode`
- [ ] `--min-market-cap`, `--min-notional`, `--max-entry-iv` change the corresponding filters
- [ ] Strategy flags (`--delta-range`, `--wing-width`, `--straddle-*`, etc.) change `BacktestConfig`
- [ ] `--track-realized-vol` enables hedging RV tracking
