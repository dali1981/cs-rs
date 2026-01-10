# Detailed Implementation Plan: IBKR-Aligned Margin & Buying Power (BPR)

This document turns the earlier spec into an implementation blueprint with **module layout**, **traits**, **structs**, and **design patterns** that make the work straightforward and testable.

> Primary objectives
> 1) Keep existing trade logic unchanged.  
> 2) Add **opt-in** BPR computation via config/CLI.  
> 3) Provide per-trade and portfolio BPR timelines + metrics (peak/avg) suitable for “return on buying power”.

---

## 0) Guiding architecture

### Design patterns used
- **Strategy pattern:** select margin regime at runtime (`Off | RegT | Cash | PM`) via `MarginEngine`.
- **Adapter pattern:** wrap your existing `MarginCalculator` into an `OptionMarginEngine` interface without rewriting formulas.
- **Builder pattern:** construct a per-trade `BprTimeline` incrementally from event timestamps.
- **Pipeline / “ports & adapters”:** `cs-backtest` produces normalized `BprInputs`; `cs-domain` computes BPR.

### Separation of concerns
- `cs-backtest` owns **when** to compute (timestamps, events, open trades).
- `cs-domain` owns **how** to compute margin (options + stock), given normalized inputs.

---

## 1) File/module layout (recommended)

### `cs-domain`
- `cs-domain/src/accounting/bpr/mod.rs`
  - `BprSnapshot`, `BprTimeline`, `BprSummary`, `BprInputs`, `BprConfig`
- `cs-domain/src/accounting/bpr/engine.rs`
  - core traits: `MarginEngine`, `OptionMarginEngine`, `StockMarginEngine`
- `cs-domain/src/accounting/bpr/engines/`
  - `off.rs` (no-op)
  - `regt.rs` (Reg-T proxy, uses adapter to existing MarginCalculator)
  - `cash.rs` (cash-style conservative)
  - `pm.rs` (stub)
- `cs-domain/src/accounting/bpr/aggregate.rs`
  - portfolio aggregator: conservative sum, optional netting for stocks
- `cs-domain/src/accounting/margin.rs`
  - existing MarginCalculator (kept as-is)

### `cs-backtest`
- `cs-backtest/src/bpr/`
  - `inputs.rs`: convert trade+hedge state to normalized `BprInputs`
  - `timeline.rs`: build per-trade BPR timelines from events
  - `portfolio.rs`: build portfolio BPR timeseries
- `cs-backtest/src/reporting/` (or existing output module)
  - print BPR sections, dump CSV timelines

---

## 2) Config additions (opt-in; defaults preserve old behavior)

### Add to config schema
```toml
[margin]
mode = "off"               # off | regt | cash | pm
use_maintenance = true     # denom uses maint if true, otherwise initial

[margin.stock]
stock_margin_mode = "regt"  # regt | cash
long_initial_rate = 0.50
short_initial_rate = 1.50
long_maint_rate = 0.50
short_maint_rate = 1.50

[margin.options]
regt_variant = "cboe_like"  # documents formula family; maps to MarginCalculator adapter config
```

### Accounting denominator switch
```toml
[accounting]
capital_base = "premium"   # premium | bpr_peak | bpr_avg
```

**Behavior guarantees**
- If `margin.mode = off`, no new computations are required and output remains unchanged.
- If `capital_base = premium`, the old ROC stays; BPR is extra reporting only.

---

## 3) Domain data types (cs-domain)

### 3.1 Core timeline types
```rust
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct BprSnapshot {
    pub ts: DateTime<Utc>,
    pub option_initial: f64,
    pub option_maint: f64,
    pub hedge_initial: f64,
    pub hedge_maint: f64,
}

impl BprSnapshot {
    pub fn total_initial(&self) -> f64 { self.option_initial + self.hedge_initial }
    pub fn total_maint(&self) -> f64 { self.option_maint + self.hedge_maint }
}

#[derive(Debug, Clone)]
pub struct BprSummary {
    pub max_total_initial: f64,
    pub max_total_maint: f64,
    pub avg_total_initial: f64,
    pub avg_total_maint: f64,
    pub max_option_maint: f64,
    pub max_hedge_maint: f64,
}

#[derive(Debug, Clone)]
pub struct BprTimeline {
    pub snapshots: Vec<BprSnapshot>,
    pub summary: BprSummary,
}
```

### 3.2 Normalized inputs (what engines consume)
Keep the input representation **strategy-agnostic** and based on legs.

```rust
#[derive(Debug, Clone, Copy)]
pub enum OptionRight { Call, Put }

#[derive(Debug, Clone)]
pub struct OptionLegInput {
    pub right: OptionRight,
    pub strike: f64,
    pub expiry: chrono::NaiveDate,
    pub qty: i32,              // signed: +long, -short
    pub mark_premium: f64,     // per-share premium (or per-contract; define convention)
}

#[derive(Debug, Clone)]
pub struct HedgeInput {
    pub symbol: String,
    pub shares: i32,           // signed: +long, -short
    pub spot: f64,
}

#[derive(Debug, Clone)]
pub struct BprInputs {
    pub ts: DateTime<Utc>,
    pub underlying_symbol: String,
    pub underlying_spot: f64,
    pub option_legs: Vec<OptionLegInput>,
    pub hedge: Option<HedgeInput>,
}
```

> Convention recommendation: normalize all monetary values to **per-contract total** (already ×100) inside your adapter, so engines return “account currency” amounts consistently.

---

## 4) Margin engines (traits + implementations)

### 4.1 Strategy selection (Strategy pattern)
```rust
#[derive(Debug, Clone)]
pub enum MarginMode { Off, RegT, Cash, PM }

#[derive(Debug, Clone)]
pub struct MarginConfig {
    pub mode: MarginMode,
    pub use_maintenance: bool,
    pub stock: StockMarginConfig,
    pub options: OptionsMarginConfig,
}

#[derive(Debug, Clone)]
pub struct StockMarginConfig {
    pub stock_margin_mode: StockMarginMode, // RegT | Cash
    pub long_initial_rate: f64,
    pub short_initial_rate: f64,
    pub long_maint_rate: f64,
    pub short_maint_rate: f64,
}

#[derive(Debug, Clone)]
pub enum StockMarginMode { RegT, Cash }

#[derive(Debug, Clone)]
pub struct OptionsMarginConfig {
    pub regt_variant: String,
}
```

### 4.2 Engine interfaces (ports)
```rust
pub trait OptionMarginEngine: Send + Sync {
    fn compute(&self, inputs: &BprInputs) -> (f64 /*initial*/, f64 /*maint*/);
}

pub trait StockMarginEngine: Send + Sync {
    fn compute(&self, hedge: &HedgeInput, cfg: &StockMarginConfig) -> (f64, f64);
}

pub trait MarginEngine: Send + Sync {
    fn compute_snapshot(&self, inputs: &BprInputs, cfg: &MarginConfig) -> BprSnapshot;
}
```

### 4.3 A composite engine (recommended)
```rust
pub struct CompositeMarginEngine {
    pub opt: Box<dyn OptionMarginEngine>,
    pub stock: Box<dyn StockMarginEngine>,
}

impl MarginEngine for CompositeMarginEngine {
    fn compute_snapshot(&self, inputs: &BprInputs, cfg: &MarginConfig) -> BprSnapshot {
        let (opt_i, opt_m) = self.opt.compute(inputs);

        let (hedge_i, hedge_m) = match inputs.hedge.as_ref() {
            Some(h) => self.stock.compute(h, &cfg.stock),
            None => (0.0, 0.0),
        };

        BprSnapshot {
            ts: inputs.ts,
            option_initial: opt_i,
            option_maint: opt_m,
            hedge_initial: hedge_i,
            hedge_maint: hedge_m,
        }
    }
}
```

### 4.4 Off engine (Null Object pattern)
```rust
pub struct OffOptionEngine;
impl OptionMarginEngine for OffOptionEngine {
    fn compute(&self, _inputs: &BprInputs) -> (f64, f64) { (0.0, 0.0) }
}

pub struct OffStockEngine;
impl StockMarginEngine for OffStockEngine {
    fn compute(&self, _hedge: &HedgeInput, _cfg: &StockMarginConfig) -> (f64, f64) { (0.0, 0.0) }
}
```

### 4.5 Stock margin engine (Reg-T proxy / Cash)
```rust
pub struct SimpleStockMarginEngine;

impl StockMarginEngine for SimpleStockMarginEngine {
    fn compute(&self, hedge: &HedgeInput, cfg: &StockMarginConfig) -> (f64, f64) {
        let notional = (hedge.shares.abs() as f64) * hedge.spot;

        match cfg.stock_margin_mode {
            StockMarginMode::Cash => (notional, notional),
            StockMarginMode::RegT => {
                if hedge.shares >= 0 {
                    (notional * cfg.long_initial_rate, notional * cfg.long_maint_rate)
                } else {
                    (notional * cfg.short_initial_rate, notional * cfg.short_maint_rate)
                }
            }
        }
    }
}
```

### 4.6 Option margin engine adapter (Adapter pattern to existing MarginCalculator)
Wrap `MarginCalculator` without changing it. The adapter maps normalized legs → the calculator’s expected inputs.

```rust
pub struct RegTOptionEngineAdapter {
    calc: crate::accounting::margin::MarginCalculator,
    cfg: OptionsMarginConfig,
}

impl RegTOptionEngineAdapter {
    pub fn new(calc: crate::accounting::margin::MarginCalculator, cfg: OptionsMarginConfig) -> Self {
        Self { calc, cfg }
    }
}

impl OptionMarginEngine for RegTOptionEngineAdapter {
    fn compute(&self, inputs: &BprInputs) -> (f64, f64) {
        // 1) Build an internal representation compatible with MarginCalculator
        // 2) Call calc methods for the structure (or per-leg and aggregate).
        // 3) Return same value for initial/maint in Phase 1 (or separate if supported).
        //
        // NOTE: exact mapping depends on your MarginCalculator API.
        let req = self.calc.margin_for_legs(
            inputs.underlying_spot,
            &inputs.option_legs,
            /* maybe variant = */ &self.cfg.regt_variant,
        );
        (req, req)
    }
}
```

> If your MarginCalculator is not “legs-based”, add a *pure adapter layer* that:
> - detects common structures (straddle, vertical, ironfly…) from legs
> - routes to the correct calculator function  
> This keeps the rest of the system structure-agnostic.

---

## 5) Timeline builder (Builder pattern)

### 5.1 Summary computation helper
```rust
pub fn summarize_timeline(snaps: &[BprSnapshot]) -> BprSummary {
    let mut max_total_i = 0.0;
    let mut max_total_m = 0.0;
    let mut max_opt_m = 0.0;
    let mut max_hedge_m = 0.0;
    let mut sum_i = 0.0;
    let mut sum_m = 0.0;

    for s in snaps {
        let ti = s.total_initial();
        let tm = s.total_maint();
        max_total_i = max_total_i.max(ti);
        max_total_m = max_total_m.max(tm);
        max_opt_m = max_opt_m.max(s.option_maint);
        max_hedge_m = max_hedge_m.max(s.hedge_maint);
        sum_i += ti;
        sum_m += tm;
    }

    let n = snaps.len().max(1) as f64;
    BprSummary {
        max_total_initial: max_total_i,
        max_total_maint: max_total_m,
        avg_total_initial: sum_i / n,
        avg_total_maint: sum_m / n,
        max_option_maint: max_opt_m,
        max_hedge_maint: max_hedge_m,
    }
}
```

### 5.2 Builder API
```rust
pub struct BprTimelineBuilder<'a> {
    engine: &'a dyn MarginEngine,
    cfg: &'a MarginConfig,
    snapshots: Vec<BprSnapshot>,
}

impl<'a> BprTimelineBuilder<'a> {
    pub fn new(engine: &'a dyn MarginEngine, cfg: &'a MarginConfig) -> Self {
        Self { engine, cfg, snapshots: Vec::new() }
    }

    pub fn push(&mut self, inputs: &BprInputs) {
        let snap = self.engine.compute_snapshot(inputs, self.cfg);
        self.snapshots.push(snap);
    }

    pub fn build(mut self) -> BprTimeline {
        // ensure chronological order if needed
        self.snapshots.sort_by_key(|s| s.ts);
        let summary = summarize_timeline(&self.snapshots);
        BprTimeline { snapshots: self.snapshots, summary }
    }
}
```

---

## 6) Backtest integration plan (no trade-logic changes)

### 6.1 Convert existing trade/hedge logs → `BprInputs`
In `cs-backtest/src/bpr/inputs.rs` implement **pure conversion** functions. These should not compute margin; they just normalize state.

Example “ports”:
```rust
pub trait BprInputSource {
    fn iter_bpr_inputs(&self) -> Box<dyn Iterator<Item = BprInputs> + '_>;
}
```

Concrete implementations:
- `TradeResultBprSource` that reads:
  - option legs at each timestamp (from your mark/pnl timeline)
  - hedge shares at each timestamp (from hedge timeline)

This isolates “how to find legs/marks/shares in your result structs” from the margin engine.

### 6.2 Where to hook in
In `cs-backtest` where you finalize a trade result:
- If `margin.mode != off`:
  - create engine via factory
  - build `BprTimeline` from the result’s timeline events
  - attach `result.bpr = Some(timeline)`

### 6.3 Engine factory (DI-lite)
```rust
pub fn make_engine(cfg: &MarginConfig) -> Box<dyn MarginEngine> {
    match cfg.mode {
        MarginMode::Off => Box::new(CompositeMarginEngine {
            opt: Box::new(OffOptionEngine),
            stock: Box::new(OffStockEngine),
        }),
        MarginMode::RegT => Box::new(CompositeMarginEngine {
            opt: Box::new(RegTOptionEngineAdapter::new(MarginCalculator::default(), cfg.options.clone())),
            stock: Box::new(SimpleStockMarginEngine),
        }),
        MarginMode::Cash => Box::new(CompositeMarginEngine {
            opt: Box::new(RegTOptionEngineAdapter::new(MarginCalculator::default(), cfg.options.clone())),
            stock: Box::new(SimpleStockMarginEngine),
        }),
        MarginMode::PM => Box::new(CompositeMarginEngine {
            opt: Box::new(RegTOptionEngineAdapter::new(MarginCalculator::default(), cfg.options.clone())), // stub until PM exists
            stock: Box::new(SimpleStockMarginEngine),
        }),
    }
}
```

> Note: `Cash` mode can still use RegT option formulas or treat options as premium-only; pick one and document it.

---

## 7) Portfolio aggregation (conservative + optional netting)

### 7.1 Data model
```rust
pub struct PortfolioBprPoint {
    pub ts: DateTime<Utc>,
    pub total_initial: f64,
    pub total_maint: f64,
}

pub struct PortfolioBprSeries {
    pub points: Vec<PortfolioBprPoint>,
    pub max_total_initial: f64,
    pub max_total_maint: f64,
}
```

### 7.2 Conservative sum (Phase 1)
At each timestamp `t`:
- sum each open trade’s `total_maint(t)`  
If a trade doesn’t have a snapshot at `t`, choose one:
- nearest previous snapshot (step function), or
- interpolate (not recommended for margin).

### 7.3 Optional stock netting (high-value improvement)
At each `t`:
- net **signed shares by symbol** across open trades
- compute hedge BPR on net shares
- add option BPR sum (still not netted across options in Phase 1)

This requires you to retain per-trade `HedgeInput` at each snapshot or expose a method to retrieve it.

---

## 8) Metric computation and reporting

### 8.1 Per-trade metrics
Given `pnl` and `BprTimeline.summary`:
- `peak_bpr = summary.max_total_maint` (if `use_maintenance`)
- `avg_bpr = summary.avg_total_maint`
- `roc_on_bpr_peak = pnl / peak_bpr` (guard divide by zero)
- `roc_on_bpr_avg = pnl / avg_bpr`

### 8.2 Portfolio metrics
- `portfolio_peak_bpr = series.max_total_maint`
- `portfolio_roc_on_bpr = portfolio_pnl / portfolio_peak_bpr`

### 8.3 Output section
Add:
- margin config echo (mode, stock rates)
- per-trade table: pnl, peak_bpr, roc_on_bpr_peak, option/hedge peak split
- portfolio: max BPR and ROC

### 8.4 Debug dumps (recommended)
- `--dump-bpr trades.csv`
  - One row per snapshot (trade_id, ts, option_maint, hedge_maint, total_maint)
- `--dump-portfolio-bpr portfolio.csv`

---

## 9) Testing plan (what to implement)

### 9.1 Unit tests (cs-domain)
- `SimpleStockMarginEngine`:
  - long 100 @ 100 => 5,000 with 0.5 rates; 10,000 with cash
  - short 100 @ 100 => 15,000 with 1.5 rates
- `summarize_timeline`:
  - peak/avg correctness

### 9.2 Option adapter tests
Construct minimal leg sets and assert margin equals `MarginCalculator` outputs.
- long call: margin == premium
- short put: margin matches calculator formula
- vertical spread: margin == max loss

### 9.3 Integration tests (cs-backtest)
- Run a tiny deterministic scenario with known hedges.
- Assert:
  - BPR timeline exists when margin enabled
  - peak BPR >= entry BPR
  - portfolio series monotonic behavior is *not assumed* (can go up/down), but peak is correct

### 9.4 Golden test: “margin off” preserves output
- ensure that enabling code paths doesn’t change existing summary outputs when `margin.mode=off`.

---

## 10) Implementation sequence (PR-by-PR)

### PR1 — Config + domain types
- Add `MarginConfig`, enums, `BprSnapshot/BprTimeline` in `cs-domain`
- Add parsing + defaults in CLI/config layer
- No behavior changes (margin off)

### PR2 — Stock BPR + timeline builder scaffolding
- Implement `StockMarginEngine`, `BprTimelineBuilder`, summary helper
- Backtest: attach hedge-only BPR (option BPR = 0) to validate pipeline

### PR3 — Option margin adapter
- Implement `RegTOptionEngineAdapter` using existing `MarginCalculator`
- Backtest: attach full BPR to trade results

### PR4 — Portfolio aggregation
- Conservative sum series + reporting
- Optional stock netting by symbol

### PR5 — Reporting + dumps + docs
- Add CLI flags, CSV dumps
- Add “Margin & Buying Power” output section

### PR6 — Validation notes
- A markdown doc with 5–10 IBKR comparisons and tuning notes

---

## Appendix A: Common pitfalls and how this design avoids them

- **Pitfall:** “Margin rules differ per strategy.”  
  **Fix:** normalize to legs and delegate to an adapter routing layer.

- **Pitfall:** “Hedge margin differs for long vs short.”  
  **Fix:** explicit signed shares + separate rates; no single 0.5 heuristic.

- **Pitfall:** “Overlapping trades distort capital.”  
  **Fix:** portfolio aggregator is a first-class module with a timeseries.

- **Pitfall:** “Hard to test.”  
  **Fix:** domain engines are pure functions over inputs; backtest conversion is isolated.

---

## Appendix B: Minimal stub for PM mode (future)
Keep the same interface but swap engines:
- `PmEngine` computes BPR as max loss under scenario grid:
  - underlying move ±X%
  - IV move ±Y
  - reprice legs with your pricing model
- This slots in via the same `MarginEngine` factory.
