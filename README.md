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

🚧 **In Development** - See [RUST_REWRITE_PLAN.md](../trading_project/options_strategy/docs/RUST_REWRITE_PLAN.md) for detailed roadmap.

**Current Phase**: Phase 1 - Analytics Core (Weeks 1-3)

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
# Run backtest
cs backtest \
  --start 2025-11-01 \
  --end 2025-11-30 \
  --strategy delta \
  --option-type call

# Analyze results
cs analyze --run-dir ./results/backtest_20251130_120000

# Price single spread (debugging)
cs price \
  --symbol AAPL \
  --strike 180 \
  --short-expiry 2025-11-15 \
  --long-expiry 2025-12-20 \
  --date 2025-11-01
```

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
| 10K trades backtest | 20 min | < 4 min | 🚧 |
| BS IV solver | ~2 μs | < 100 ns | 🚧 |
| Memory usage | 4 GB | < 2 GB | 🚧 |
| Result parity | - | 100% match | 🚧 |

## Timeline

| Phase | Duration | Status |
|-------|----------|--------|
| 1. Analytics Core | Weeks 1-3 | 🚧 In Progress |
| 2. Domain Models | Weeks 4-7 | ⏳ Pending |
| 3. finq-rs Integration | Weeks 8-10 | ⏳ Pending |
| 4. Backtest Engine | Weeks 11-15 | ⏳ Pending |
| 5. Python Bindings | Weeks 16-18 | ⏳ Pending |
| 6. CLI + Persistence | Weeks 19-21 | ⏳ Pending |
| 7. Testing | Weeks 22-25 | ⏳ Pending |

## Contributing

This is a personal project for production trading. External contributions are not currently accepted.

## License

MIT
