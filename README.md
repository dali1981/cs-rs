# cs-rs: Calendar Spread Backtest Engine

High-performance Rust implementation of calendar spread options backtesting around earnings announcements.

## Overview

This project is a complete rewrite of the Python `calendar-spread-backtest` in Rust, targeting:
- **5x performance improvement** (20 min → 4 min for 10K trades)
- **15-20x faster Black-Scholes calculations**
- **50% memory reduction**
- Single-language codebase with optional Python bindings

## Architecture

```
cs-rs/
├── cs-analytics/      # Black-Scholes, Greeks, IV surface (pure math)
├── cs-domain/         # CalendarSpread, Strategies, Repositories (business logic)
├── cs-backtest/       # BacktestUseCase, TradeExecutor (execution engine)
├── cs-python/         # PyO3 Python bindings
└── cs-cli/            # Command-line interface
```

## Dependencies

- **[finq-rs](../finq-rs)** - Market data layer (options, equities, IV surfaces)
- Polars - Fast DataFrame operations
- Tokio - Async runtime
- Rayon - Parallel processing

## Status

🚧 **In Development**

**Current Phase**: Delta-space strategy implementation (M1 complete, M2 in progress)

## Documentation

| Document | Description |
|----------|-------------|
| [Trade Selection](specs/trade_selection.md) | How trades are selected: expiry filtering, opportunity scoring, delta-to-strike mapping |
| [Delta Strategy Plan](specs/delta_strategy_plan.md) | Full design for delta-space strategies, SVI fitting, arbitrage detection |
| [IV Models Design](specs/iv_models_design.md) | Sticky-strike vs sticky-moneyness vs sticky-delta interpolation |
| [Rust Rewrite Plan](specs/RUST_REWRITE_PLAN.md) | Original migration plan from Python |

## Quick Start

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify finq-rs is available
ls ../finq-rs/crates/finq-core
```

### Build

```bash
# Build all crates
cargo build --release

# Run tests
cargo test

# Run benchmarks
cargo bench
```

### Usage (Python)

```python
from cs_rust import bs_price, bs_implied_volatility, bs_greeks

# Calculate implied volatility (15-20x faster than scipy)
iv = bs_implied_volatility(5.0, 100.0, 100.0, 0.08, True)
print(f"IV: {iv:.2%}")

# Calculate Greeks
greeks = bs_greeks(100.0, 100.0, 0.08, 0.30, True)
print(f"Delta: {greeks.delta:.3f}, Vega: {greeks.vega:.3f}")
```

### Usage (CLI)

```bash
# Simple ATM backtest
./target/release/cs backtest \
  --start 2025-11-01 \
  --end 2025-11-30 \
  --strategy atm

# Delta-scan strategy (finds best delta)
./target/release/cs backtest \
  --start 2025-11-01 \
  --end 2025-11-30 \
  --strategy delta-scan \
  --delta-range "0.40,0.60" \
  --min-iv-ratio 1.10

# Fixed delta with sticky-delta IV model
./target/release/cs backtest \
  --start 2025-11-01 \
  --end 2025-11-30 \
  --strategy delta \
  --target-delta 0.50 \
  --iv-model sticky-delta

# With SVI interpolation (M2)
./target/release/cs backtest \
  --start 2025-11-01 \
  --end 2025-11-30 \
  --strategy delta-scan \
  --vol-model svi
```

### Key CLI Options

| Option | Description | Default |
|--------|-------------|---------|
| `--strategy` | `atm`, `delta`, `delta-scan` | `atm` |
| `--target-delta` | Fixed delta for `delta` strategy | `0.50` |
| `--delta-range` | Range for `delta-scan` (e.g., "0.25,0.75") | `0.25,0.75` |
| `--iv-model` | `sticky-strike`, `sticky-moneyness`, `sticky-delta` | `sticky-strike` |
| `--vol-model` | `linear` (M1), `svi` (M2) | `linear` |
| `--min-iv-ratio` | Minimum short/long IV ratio | none |

## Development

### Project Structure

Each crate follows clean architecture principles:

- **cs-analytics**: Pure functions, no I/O, highly testable
- **cs-domain**: Domain models, strategies, repository traits
- **cs-backtest**: Use cases, parallel execution
- **cs-python**: PyO3 bindings for Python interop
- **cs-cli**: Command-line interface

### Running Individual Crates

```bash
# Test analytics (Black-Scholes)
cd cs-analytics
cargo test
cargo bench

# Test domain logic
cd cs-domain
cargo test

# Run CLI locally
cd cs-cli
cargo run -- backtest --help
```

### Python Development

```bash
# Build Python bindings with maturin
cd cs-python
pip install maturin
maturin develop

# Now use in Python
python -c "from cs_rust import bs_price; print(bs_price(100, 100, 0.08, 0.3, True, 0.05))"
```

## Performance Targets

| Metric | Python | Rust Target | Status |
|--------|--------|-------------|--------|
| 10K trades backtest | 20 min | < 4 min | ✅ Achieved |
| BS IV solver | ~2 μs | < 100 ns | ✅ Achieved |
| Memory usage | 4 GB | < 2 GB | ✅ Achieved |
| Result parity | - | 100% match | ✅ Verified |

## Milestones

| Milestone | Description | Status |
|-----------|-------------|--------|
| **M1** | Delta-space linear interpolation | ✅ Complete |
| **M2** | SVI fitting, arbitrage detection | 🚧 In Progress |
| **M3** | SSVI surface consistency | ⏳ Planned |

## Contributing

This is a personal project for production trading. External contributions are not currently accepted.

## License

MIT
