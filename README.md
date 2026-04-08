# cs-rs

A quantitative research and backtesting engine for options volatility strategies,
with realistic execution modeling, capital constraints, and risk attribution.

## Quickstart (Demo)

Run a backtest with embedded sample data (no external dependencies required):

```bash
cargo run --release --no-default-features --features demo -p cs-cli --bin cs -- \
  backtest --conf configs/demo.toml --start 2024-08-14 --end 2024-08-28
```

The demo uses real NVDA options data around the August 2024 earnings event
(2024-08-28 AMC). Canonical parameters are defined in `scripts/demo_command.sh`.

## Features

### Strategies
- 8 spread types: calendar spreads, straddles (long/short), iron butterflies, calendar straddles, strangles, butterflies, condors, iron condors
- Campaign system for declarative multi-symbol trade scheduling (daily, weekly, monthly entry policies)
- Configurable entry/exit rules engine (IV slope, market cap, delta, DTE, bid-ask)

### Analytics
- Volatility term structure analysis (IV7 / IV20 / IV30)
- P&L attribution by Greeks (delta, gamma, vega, theta)
- IBKR-style margin and buying power (BPR) tracking

### Execution
- Event-driven options strategies (earnings-focused)
- Realistic execution with transaction costs and slippage (commission, half-spread, IV-based, percentage models)
- Delta hedging with pluggable providers (entry IV, market IV, historical HV, gamma approximation, historical average IV)
- Portfolio-level aggregation across underlyings (per-symbol breakdown not yet implemented)

### Configuration & Data
- Configurable strategy rules via TOML configs or CLI flags (layered: multiple TOML files + CLI overrides)
- Two data sources: finq-flatfiles and Interactive Brokers
- Demo mode with embedded fixture data for testing without external dependencies

### Integrations
- Python bindings via PyO3 (`cs_rust` module: Black-Scholes pricing, Greeks, IV solver, backtest execution)

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

## Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `full` | Yes | Enables external data sources (`finq-flatfiles`, `earnings-rs`, `ib-data-collector`) |
| `demo` | No | Uses embedded NVDA fixture data; no external dependencies required |

```bash
# Full build (default — requires finq-rs and earnings-rs)
cargo build --release

# Demo build (self-contained, no external data)
cargo build --release --no-default-features --features demo -p cs-cli
```

## Build

```bash
# Build all crates
cargo build --release

# Build specific crate
cargo build --release -p cs-backtest

# Build Python bindings (requires maturin)
cd cs-python && maturin develop --release
```

## Testing

```bash
# Run all tests (demo mode — no external data needed)
cargo test --no-default-features --features demo

# Run all tests (full mode — requires data sources)
cargo test

# Run tests for a specific crate
cargo test -p cs-analytics
cargo test -p cs-domain

# Run with debug logging
RUST_LOG=debug cargo test -- --nocapture
```

**Test organization:**
- **76 inline test modules** (`#[cfg(test)]`) across all crates — heaviest in cs-domain (52) and cs-analytics (17)
- **Integration tests** in `cs-backtest/tests/` (real data execution paths)
- **Fixtures** in `fixtures/` — NVDA options/equity parquet files + earnings CSV
- **Example binaries** in `cs-domain/examples/` and `cs-cli/src/bin/` (data diagnostics, IV visualization)

## License

MIT
