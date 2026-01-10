# Spec: IBKR-Aligned Margin & Buying Power in Backtest Accounting

**Scope:** Define how to compute and report *broker-like* margin / buying-power requirements (IBKR-style) for each trade and for the full backtest portfolio, **without changing trade logic**. This spec is meant to guide an implementation that can later replace/augment the current premium-based `capital_required()` metrics.

> Key goal: When you run `backtest`, the accounting output should be able to answer:
> - “How much buying power would IBKR require for this position (and through time)?”
> - “What’s my return on buying power, not return on premium?”
> - “How much of the buying power is from option margin vs hedge stock margin?”

---

## 1) Current behavior (baseline)

### 1.1 Option-side capital in summary metrics
Current `HasAccounting::capital_required()` is mostly:
- Long premium paid (`entry_debit`) for long structures (e.g., long straddle)
- Abs cost/credit proxies for others (calendar, ironfly credit proxy)

This is **not broker margin** and makes “Return on Capital” closer to “Return on premium”.

### 1.2 Hedge-side capital
Two notions exist:
- **Peak-based** hedge metrics (`HedgePosition`): peak shares × spot-at-peak (long = 100% notional; short = notional × margin_rate)
- **Heuristic** hedge capital in `HasAccounting` (peak shares × avg hedge price × 50%)

These are not aligned to IBKR Reg‑T or Portfolio Margin.

### 1.3 MarginCalculator exists
There is a `MarginCalculator` module (strategy formulas for options and some stock hedge logic), but it is **not currently used** as the primary capital base for backtest metrics.

---

## 2) Target definitions

### 2.1 Terms
- **Buying Power Requirement (BPR):** the broker-required margin / capital lockup for a position at a point in time.
- **Initial Margin vs Maintenance Margin:** store both when feasible; if only one is practical, store “Maintenance” (more relevant for liquidation risk).
- **Reg‑T (rules-based) vs Portfolio Margin (risk-based):** support at least Reg‑T first; PM later as an extension.

### 2.2 Required outputs
At minimum, for each executed trade (result object):
- `option_bpr_initial`, `option_bpr_maint` (currency)
- `hedge_bpr_initial`, `hedge_bpr_maint` (currency)
- `total_bpr_initial`, `total_bpr_maint` (currency)
- `max_total_bpr_over_life` (peak BPR observed during trade lifetime)
- `roc_on_bpr = pnl / max_total_bpr_over_life` (or use average BPR; see 6.2)

For the portfolio/backtest (across overlapping trades):
- Timeseries of `portfolio_bpr` (initial/maint), with `max_portfolio_bpr`
- Portfolio “Return on BPR” using the chosen denominator (peak/avg)

---

## 3) Margin regimes & formulas

### 3.1 Reg‑T strategy-based option margin (Phase 1)
Implement Reg‑T style requirements using existing `MarginCalculator` strategy methods:

- **Long options:** typically 100% of premium (and sometimes haircut for long-dated, if desired)
- **Short naked call/put:** proceeds + risk add-on:
  - Often: proceeds + max(20% underlying − OTM, minimum floor)
  - Put minimum floors differ (often tied to strike)
- **Spreads / defined risk:** margin = max loss (width − net credit, etc.)
- **Short straddle/strangle:** combined requirement; commonly max(call req, put req) + proceeds (varies by rule-set)

> Note: IBKR may apply “house margin” overlays; Phase 1 aims to be *representative* rather than exact.

### 3.2 Stock hedge margin (Phase 1: Reg‑T representative)
Support **two stock margin modes**:
- `stock_margin_mode = "cash"`: long stock uses 100% notional (conservative)
- `stock_margin_mode = "regt"`: long stock initial ~50% of notional; short stock initial ~150% of notional  
  (maintenance may differ; can use same as initial initially)

Configurable parameters:
- `long_stock_initial_rate` default 0.50 (Reg‑T proxy)
- `short_stock_initial_rate` default 1.50 (Reg‑T proxy)
- optional maintenance overrides:
  - `long_stock_maint_rate`
  - `short_stock_maint_rate`

### 3.3 Portfolio Margin (Phase 2, extension)
PM is scenario-based (TIMS). For backtest purposes, provide a *pluggable engine*:
- Define a set of stress scenarios (e.g., underlying moves ±X%, vol moves ±Y, skew shifts)
- Revalue options with a pricing model under each scenario
- BPR = max projected loss across scenarios net of offsets  
This is larger scope and can be added later without changing the Phase 1 interface.

---

## 4) BPR computation timeline (through trade life)

### 4.1 When to compute
Compute BPR at:
- **Entry**
- **Each hedge rebalance**
- **Each option mark** step (same cadence you currently mark PnL / greeks / hedge)

Store a per-trade timeline:
- `bpr_snapshots: Vec<BprSnapshot>`
  - timestamp
  - option_bpr_initial/maint
  - hedge_bpr_initial/maint
  - total_bpr_initial/maint

Also store summary peaks:
- `max_total_bpr_initial`
- `max_total_bpr_maint`
- `max_total_bpr_over_life` (choose maint by default)

### 4.2 Why peaks matter
Overlapping trades + hedging mean the “true” capital lockup is a function of path.
Peaks answer: “Could I have held these positions without a margin call?”

---

## 5) Portfolio aggregation (overlapping trades)

### 5.1 Aggregation rule (Phase 1)
Since IBKR offsets across positions (especially PM), Phase 1 should implement a **conservative aggregation**:

- Portfolio BPR at time *t* = sum over open trades of `trade_total_bpr_maint(t)`  
  (option + hedge BPR), **minus** any simple offsets you already model explicitly (none today).

Later improvement (still Reg‑T):
- Net stock hedges across trades by symbol before applying stock margin rates.
- Net option positions by symbol/expiry/strike/right before applying defined-risk rules (complex).

### 5.2 Required portfolio outputs
- `portfolio_bpr_timeseries`
- `max_portfolio_bpr`
- `roc_portfolio_on_bpr = portfolio_pnl / max_portfolio_bpr` (or average)

---

## 6) Metrics & reporting

### 6.1 Per-trade metrics to add
- `pnl`, `pnl_pct_on_premium` (existing style)
- `roc_on_bpr_peak = pnl / max_total_bpr_over_life`
- `roc_on_bpr_avg = pnl / avg(total_bpr_over_life)` (optional)
- `bpr_efficiency = premium_capital_required / max_total_bpr_over_life`  
  (shows how “levered” the trade is versus broker BPR)

### 6.2 Portfolio metrics
- `max_drawdown_on_equity` (existing)
- `max_portfolio_bpr`
- `equity / bpr` utilization ratio over time
- Sharpe on returns normalized by BPR (optional)

### 6.3 UI / output formats
Add a *new section* in backtest output:
- `Margin & Buying Power (IBKR-like)`
  - per-trade table: entry date, exit date, pnl, peak BPR, roc_on_bpr_peak, option vs hedge BPR share
  - portfolio summary: max_portfolio_bpr, roc_portfolio_on_bpr

---

## 7) Config surface (CLI / file)

Add a `margin` block in config:

```toml
[margin]
mode = "regt"                # "regt" | "cash" | "pm" (pm later)
use_maintenance = true       # if false, use initial for denominators

[margin.stock]
stock_margin_mode = "regt"   # "cash" | "regt"
long_initial_rate = 0.50
short_initial_rate = 1.50
long_maint_rate = 0.50       # optional
short_maint_rate = 1.50      # optional

[margin.options]
regt_variant = "cboe_like"   # documents which formula set you’re approximating
min_call_floor_rate = 0.10   # if used by MarginCalculator variant
min_put_floor_rate = 0.10    # or strike-based logic depending on your calculator
```

Defaults should preserve **existing backtest behavior** unless user opts in:
- `margin.mode = "off"` (or omit block) => use current premium-based capital_required
- if `margin.mode != "off"` => compute & report BPR metrics and use them for “Return on Capital” denominators (optional gate)

---

## 8) API / data model changes (high level)

### 8.1 New struct
```rust
pub struct BprSnapshot {
    pub ts: DateTime<Utc>,
    pub option_initial: f64,
    pub option_maint: f64,
    pub hedge_initial: f64,
    pub hedge_maint: f64,
}
```

### 8.2 Extend trade result
Each trade result gets:
- `bpr: Vec<BprSnapshot>`
- `max_bpr_initial`, `max_bpr_maint`

### 8.3 Computation entry points
- `compute_option_bpr(snapshot_inputs) -> (initial, maint)`
  - uses `MarginCalculator` (Reg‑T)
- `compute_hedge_bpr(symbol, shares, spot, rates) -> (initial, maint)`
  - long vs short
- `compute_trade_bpr(ts) -> snapshot`

---

## 9) Validation plan (what “correct” means)

### 9.1 Unit tests (deterministic)
- Short call/put margin numbers against hand-calculated examples for the implemented formula.
- Defined-risk spreads => max loss.
- Stock hedge long/short => notional × configured rates.

### 9.2 Golden tests (integration)
- Run backtest on a small known dataset and assert:
  - BPR snapshots non-negative
  - Peak BPR ≥ entry BPR
  - Portfolio peak BPR equals max(sum of open trades BPR) for Phase 1.

### 9.3 Reality checks vs IBKR
For a handful of real option chains:
- Compare your computed Reg‑T proxy margin to IBKR’s reported requirement (manual or exported)
- Record differences and tune configurable rates/floors (documented)

---

## 10) Non-goals (for now)
- Exact reproduction of IBKR house-margin overlays, concentration, hard-to-borrow, dividend risk adjustments.
- Full PM TIMS equivalence (Phase 2).
- Cross-trade offsets for options (Phase 1 will be conservative).

---

## Appendix A: Rationale for “peak BPR over life”
Backtests are path-dependent:
- hedging can temporarily increase exposure
- margin spikes can cause real-world liquidation even if final PnL is positive
Peak BPR is the cleanest single-number denominator for “could I have held it?”

---

## Appendix B: Mapping to your existing modules
- Use `cs-domain/src/accounting/margin.rs` as the authoritative option margin engine (Reg‑T proxy).
- Use `HedgePosition` peak tracking to compute hedge BPR with configurable Reg‑T rates.
- Add BPR reporting to backtest output alongside existing premium-based accounting.
