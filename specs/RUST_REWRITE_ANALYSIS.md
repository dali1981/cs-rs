# Rust Rewrite Analysis: Calendar Spread Backtest

## Executive Summary

**Recommendation**: Full Rust rewrite is now viable with `finq-rs` development in progress.

**Key Finding**: With `finq-rs` providing Rust-native market data access, the main integration blocker is removed. The remaining Python dependencies (IB connector, ML pipeline) can be addressed incrementally.

---

## Codebase Profile

| Metric | Value |
|--------|-------|
| Total Python LOC | ~59,353 |
| Analytics module (compute-heavy) | 3,313 |
| Domain/Application (business logic) | ~25,000 |
| Infrastructure (I/O) | ~15,000 |
| External dependencies | 3 local packages (finq, ib-connector, nasdaq-earnings) |

---

## Current Architecture Layers

```
┌─────────────────────────────────────────────────────────────┐
│  SDK/Presentation (CLI, CalendarSpreadClient)               │
├─────────────────────────────────────────────────────────────┤
│  Application (BacktestUseCase, TradeExecutor, UseCaseFactory)│
├─────────────────────────────────────────────────────────────┤
│  Services (ValuationService, HistoricalReplayService)       │
├─────────────────────────────────────────────────────────────┤
│  Domain (CalendarSpread, Greeks, TradingSession, Strategies)│
├─────────────────────────────────────────────────────────────┤
│  Infrastructure (FINQ adapter, IB adapter, Persistence)     │
├─────────────────────────────────────────────────────────────┤
│  Analytics (Black-Scholes, IV Surface, Price Interpolation) │
└─────────────────────────────────────────────────────────────┘
```

---

## Computational Hotspots

### Currently Rust-Backed (via Polars)
- DataFrame operations (filter, groupby, join)
- Parquet I/O (via PyArrow)
- Time series filtering

### Pure Python Compute (Rewrite Targets)
| Component | File | Lines | Call Frequency |
|-----------|------|-------|----------------|
| `bs_implied_volatility` | `analytics/black_scholes.py` | 55 | 4x per trade |
| `bs_price` + Greeks | `analytics/black_scholes.py` | 150 | 8x per trade |
| IV Surface interpolation | `analytics/iv_surface.py` | 515 | 1x per pricing |
| Historical IV calculation | `analytics/historical_iv.py` | 406 | 1x per trade |
| Price interpolation | `analytics/price_interpolation.py` | 405 | variable |

---

## Rust Ecosystem Assessment

### Available Libraries

| Library | Purpose | Maturity | Use Case |
|---------|---------|----------|----------|
| [RustQuant](https://github.com/avhz/RustQuant) | Full quant library | High | Black-Scholes, Greeks |
| [Polars](https://pola.rs/) | DataFrames | Production | Data manipulation |
| [Arrow-rs](https://github.com/apache/arrow-rs) | Columnar memory | Production | Parquet I/O |
| [Tokio](https://tokio.rs/) | Async runtime | Production | Streaming, IB connection |
| [PyO3](https://pyo3.rs/) | Python bindings | Production | Python interop |
| [Serde](https://serde.rs/) | Serialization | Production | Config, persistence |

### finq-rs Status
- Location: `~/finq-rs`
- Status: In development
- Provides: Rust-native market data access (options bars, equity prices, chains)

---

## Performance Projections

### Current Python Performance (per 10K trades)
```
Operation               Time        % of Total
─────────────────────────────────────────────────
Load option bars        ~5 min      25%
Build option chain      ~8 min      40%
BS IV solver            ~3 min      15%
BS pricing + Greeks     ~2 min      10%
Domain logic            ~2 min      10%
─────────────────────────────────────────────────
Total                   ~20 min     100%
```

### Projected Rust Performance
```
Operation               Python      Rust        Speedup
─────────────────────────────────────────────────────────
Load option bars        5 min       2 min       2.5x (native parquet)
Build option chain      8 min       1 min       8x (native polars)
BS IV solver            3 min       0.2 min     15x (native math)
BS pricing + Greeks     2 min       0.1 min     20x (SIMD)
Domain logic            2 min       0.5 min     4x (no GIL)
─────────────────────────────────────────────────────────
Total                   20 min      ~4 min      5x overall
```

---

## Effort Estimation (Full Rewrite)

| Phase | Duration | Dependencies |
|-------|----------|--------------|
| Phase 1: Analytics Core | 2-3 weeks | None |
| Phase 2: Domain Models | 3-4 weeks | Phase 1 |
| Phase 3: finq-rs Integration | 2-3 weeks | finq-rs completion |
| Phase 4: Backtest Engine | 4-5 weeks | Phases 1-3 |
| Phase 5: Python Bindings | 2-3 weeks | Phase 4 |
| Phase 6: CLI + Persistence | 2-3 weeks | Phase 4 |
| Phase 7: Testing + Validation | 3-4 weeks | All phases |
| **Total** | **18-25 weeks** | |

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| finq-rs delays | Medium | High | PyO3 fallback to Python finq |
| IB integration complexity | Medium | Medium | Keep Python IB adapter initially |
| ML pipeline incompatibility | Low | Medium | Keep ML in Python, call via PyO3 |
| Performance regression | Low | High | Benchmark suite, A/B testing |
| Feature parity gaps | Medium | Medium | Incremental migration, feature flags |

---

## Migration Strategy

### Recommended: Parallel Development
1. Keep Python codebase operational
2. Build Rust core in parallel
3. Expose Rust via PyO3 for gradual adoption
4. Switch CLI/SDK to Rust when stable

### Alternative: Big Bang
- Higher risk, faster completion
- Requires feature freeze on Python
- Not recommended for active trading

---

## Success Criteria

1. **Performance**: 5x backtest speedup (20 min → 4 min for 10K trades)
2. **Correctness**: 100% result parity with Python implementation
3. **Memory**: 50% reduction in peak memory usage
4. **Maintainability**: Single codebase (Rust) with Python bindings
5. **Extensibility**: Easy to add new strategies, data sources

---

## Next Steps

1. Review detailed implementation plan in `RUST_REWRITE_PLAN.md`
2. Coordinate with finq-rs development timeline
3. Set up Rust workspace structure
4. Begin Phase 1 (Analytics Core)
