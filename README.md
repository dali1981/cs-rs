# cs-rs

A quantitative research and backtesting engine for earnings-driven options volatility strategies.
It models calendar spreads, straddles, iron butterflies, and related structures with realistic
execution costs, margin accounting, and Greeks-based P&L attribution. The engine runs in demo
mode (self-contained, no external data) and in full mode (connected to `finq-rs` and `earnings-rs`
market data sources).

## Supported Modes

| Mode | Purpose | Support level |
|------|---------|---------------|
| `demo` | Self-contained backtest using embedded NVDA fixture data | **Supported** — no external dependencies |
| `full` | Live market data via `finq-flatfiles` and `earnings-rs` | **Experimental** — requires external data setup |

## Golden Path

Run a complete backtest using the embedded demo data (no setup required):

```bash
cargo run --release --no-default-features --features demo -p cs-cli --bin cs -- \
  backtest --conf configs/demo.toml --start 2024-08-14 --end 2024-08-28
```

The demo uses real NVDA options data around the August 2024 earnings event (2024-08-28 AMC).
Canonical parameters are in `scripts/demo_command.sh`.

## Architecture

`cs-rs` is a modular monolith organized around six bounded contexts:

- **Market Data** — normalizes earnings events, spot prices, and option chain data (IV surfaces, strikes, expirations)
- **Strategy Definition** — strike selection algorithms, spread construction rules, and trade structure invariants
- **Campaign / Scheduling** — policy-driven session generation around earnings events (pre/post-earnings, monthly)
- **Execution / Simulation** — fill simulation, pricing, hedging, roll execution, and session lifecycle
- **Accounting / Attribution** — P&L decomposition, Greeks attribution, margin, BPR, and trading costs
- **Application / Composition** — CLI wiring, TOML config loading, repository factory, and output formatting

```
External inputs → DTOs → Commands → Use cases → Domain contexts → Reports
```

Crate layout:

```
cs-rs/
├── cs-analytics/   # Black-Scholes, Greeks, IV surface interpolation, PnL attribution
├── cs-domain/      # Domain models, traits, repository interfaces, accounting, scheduling
├── cs-backtest/    # Execution engine, pricers, strike selection, use cases
├── cs-cli/         # Command-line interface, arg parsing, config assembly
└── cs-python/      # PyO3 bindings (optional)
```

Dependency order: `cs-analytics` → `cs-domain` → `cs-backtest` → `cs-cli`.
`cs-domain` must not depend on `cs-analytics` (enforced by `scripts/check_dependencies.py`).

See [`docs/architecture/bounded-context-map.md`](docs/architecture/bounded-context-map.md) for
the full module-to-context mapping and known debt.

## Current Limitations

- **Infrastructure adapters in `cs-domain`**: All 8 data adapters (`finq_*_repo.rs`,
  `ib_*_repo.rs`, `demo_repos.rs`, `earnings_repo.rs`) live inside `cs-domain` despite having
  Polars/async/file I/O dependencies. They violate ADR-0001 and are tracked for migration to a
  future `cs-infrastructure` crate.
- **Full mode requires external setup**: The `full` feature requires `finq-rs`, `earnings-rs`,
  and `ib-data-collector` to be available as workspace dependencies. These are not bundled.
  The demo mode works without them.
- **Test migration in progress**: Most unit tests use direct struct initialization rather than
  the builder pattern mandated by ADR-0005. Migration is ongoing.
- **`cs-backtest` is a dual-context crate**: It hosts both Execution machinery (pricers,
  executors, strike selection) and Application use cases. Splitting is not planned near-term.
- **`entities.rs` and `value_objects.rs` are flat monolith files**: They mix types from
  multiple bounded contexts and are being split incrementally.

## Strategies

- 8 spread types: calendar spreads, straddles (long/short), iron butterflies, calendar straddles, strangles, butterflies, condors, iron condors
- Campaign system for declarative multi-symbol trade scheduling (daily, weekly, monthly entry policies)
- Configurable entry/exit rules engine (IV slope, market cap, delta, DTE, bid-ask spread)

## Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `full` | Yes | Enables external data sources (`finq-flatfiles`, `earnings-rs`, `ib-data-collector`) |
| `demo` | No | Uses embedded NVDA fixture data; no external dependencies required |

```bash
# Demo build (self-contained, no external data)
cargo build --release --no-default-features --features demo -p cs-cli

# Full build (default — requires finq-rs and earnings-rs)
cargo build --release
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

# Demo smoke test
cargo test --no-default-features --features demo -p cs-cli --test demo_smoke

# Run tests for a specific crate
cargo test -p cs-analytics
cargo test -p cs-domain

# Run with debug logging
RUST_LOG=debug cargo test -- --nocapture
```

**Test organization:**
- Inline test modules (`#[cfg(test)]`) across all crates — heaviest in cs-domain and cs-analytics
- Integration tests in `cs-backtest/tests/` (real data execution paths)
- Fixtures in `fixtures/` — NVDA options/equity parquet files + earnings CSV
- Example binaries in `cs-domain/examples/` and `cs-cli/src/bin/` (data diagnostics, IV visualization)

## Architecture Decision Records

- [ADR-0001](docs/adr/0001-bounded-contexts.md) — Bounded Contexts
- [ADR-0002](docs/adr/0002-demo-is-first-class.md) — Demo is First Class
- [ADR-0003](docs/adr/0003-cli-config-types-are-dtos.md) — CLI Config Types are DTOs
- [ADR-0004](docs/adr/0004-repositories-speak-business-queries.md) — Repositories Speak Business Queries
- [ADR-0005](docs/adr/0005-tests-use-builders.md) — Tests Use Builders

## License

MIT
