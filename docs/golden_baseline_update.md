# golden_baseline_update

This project uses deterministic golden baselines to catch silent business regressions.

## What is frozen

Each golden case under `tests/golden/` freezes these outputs:

- `trade_count`
- `win_rate_pct`
- `net_pnl`
- `max_drawdown`
- `profit_factor`
- JSON summary snapshot (`*.summary.json`)
- CSV summary snapshot (`*.summary.csv`)

## Directory layout

- `tests/golden/configs/`: canonical run configurations
- `tests/golden/datasets/`: deterministic input trade fixtures
- `tests/golden/baselines/`: expected JSON/CSV outputs

## Default behavior (CI/local)

By default, tests compare current outputs to committed baselines and fail on any mismatch with a readable diff.

No baseline file is updated automatically.

## Intentional baseline update process

1. Verify the behavior change is intentional and approved.
2. Regenerate baselines explicitly:

```bash
CS_GOLDEN_UPDATE=1 cargo test -p cs-backtest --test golden_regression_suite
```

3. Re-run in normal compare mode:

```bash
cargo test -p cs-backtest --test golden_regression_suite
```

4. Review generated diffs in `tests/golden/baselines/`.
5. Commit baseline updates with a message that explains why outputs changed.

## Guardrail

If outputs changed unexpectedly, do not regenerate baselines. Fix the regression first.
