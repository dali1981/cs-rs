# cs-rs

A quantitative research and backtesting engine for options volatility strategies,
with realistic execution modeling, capital constraints, and risk attribution.

## Quickstart (Demo)

Run a backtest with embedded sample data (no external dependencies required):

```bash
cargo run --release --no-default-features --features demo -p cs-cli --bin cs -- \
  backtest --conf configs/demo.toml --start 2024-11-06 --end 2024-11-20
```

Expected output:
```
Sample Trades:
  1. NVDA | P&L: $-60.00 (-3.82%)
```

The demo uses real NVDA options data around the November 2024 earnings event
(entry Nov 12, exit Nov 20). The small loss demonstrates realistic IV crush
after earnings when the stock move doesn't exceed implied volatility expectations.

## Features

- Volatility term structure analysis (IV7 / IV20 / IV30)
- Event-driven options strategies (earnings-focused)
- Multiple spread types: calendar spreads, straddles, iron butterflies
- Configurable strategy rules via TOML configs or CLI flags
- Realistic execution with transaction costs and slippage
- Delta hedging simulation
- Portfolio-level aggregation across underlyings

## Full Usage Example

With market data connected (requires `finq-rs` and `earnings-rs`):

```bash
cargo run --release -- backtest \
  --start 2025-01-01 \
  --end 2025-12-31 \
  --timing-strategy PreEarnings \
  --entry-days-before 7 \
  --exit-days-before 2 \
  --min-iv-ratio 1.10 \
  --target-delta 0.50
```

## Architecture

```
cs-rs/
├── cs-analytics/   # Black-Scholes, Greeks, IV surface interpolation
├── cs-domain/      # Domain models, strategy traits, repository interfaces
├── cs-backtest/    # Execution engine, pricers, use cases
├── cs-cli/         # Command-line interface
└── cs-python/      # PyO3 bindings (optional)
```

Each crate follows clean architecture: domain logic is pure and testable,
infrastructure concerns are isolated at the boundaries.

## Tech Stack

- Rust (decimal arithmetic for financial precision)
- Polars (DataFrame operations)
- Rayon (parallel backtesting)
- CLI-based workflows

## Build

```bash
cargo build --release
cargo test
```

## License

MIT
