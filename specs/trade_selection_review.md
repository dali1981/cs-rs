# Trade Selection Review

**Date:** 2025-12-31
**Scope:** End-to-end analysis of trade selection logic

---

## Overview

The trade selection system selects calendar spread opportunities around earnings events. The flow is:

```
Earnings Events → Filtering → Strategy Selection → Pricing → Execution
```

---

## 1. Entry Point: BacktestUseCase

**File:** `cs-backtest/src/backtest_use_case.rs`

### Session Loop (lines 229-303)

```rust
for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
    let events = self.load_earnings_window(session_date).await?;
    let to_enter = self.filter_for_entry(&events, session_date);
    // ... process events
}
```

### Pre-Selection Filters

1. **Market Cap Filter** (lines 486-491)
   ```rust
   fn passes_market_cap_filter(&self, event: &EarningsEvent) -> bool {
       match (self.config.min_market_cap, event.market_cap) {
           (Some(min), Some(cap)) => cap >= min,
           (Some(_), None) => false,  // Reject if no market cap data
           (None, _) => true,
       }
   }
   ```

2. **Entry Date Filter** (lines 481-484)
   - Uses `EarningsTradeTiming` to determine if event should be entered today
   - AMC events: enter same day
   - BMO events: enter previous trading day

### Post-Selection Filter

**IV Ratio Filter** (lines 455-461)
```rust
fn passes_iv_filter(&self, result: &CalendarSpreadResult) -> bool {
    match (self.config.selection.min_iv_ratio, result.iv_ratio()) {
        (Some(min), Some(ratio)) => ratio >= min,
        (Some(_), None) => false,  // Reject if no IV data
        (None, _) => true,
    }
}
```

**Issue:** IV ratio is calculated AFTER trade execution, meaning we execute trades then filter them out. This is wasteful.

---

## 2. Earnings Timing Logic

**File:** `cs-domain/src/services/earnings_timing.rs`

### Session Concept

The strategy profits from IV crush after earnings, so trades must span the announcement:

| Timing | Entry Date | Exit Date | Rationale |
|--------|------------|-----------|-----------|
| AMC | Same day | Next trading day | Earnings after close, exit after crush |
| BMO | Previous day | Same day | Earnings before open, exit same day |
| Unknown | Same day | Next day | Default to AMC behavior |

### Weekend Handling

```rust
// Friday AMC → Enter Friday, Exit Monday
// Monday BMO → Enter Friday, Exit Monday
```

Uses `TradingCalendar::previous_trading_day()` and `next_trading_day()` to skip weekends.

**Potential Issue:** No holiday handling visible - relies on `TradingCalendar` implementation.

---

## 3. Strategy Pattern

**File:** `cs-domain/src/strategies/mod.rs`

### TradingStrategy Trait

```rust
pub trait TradingStrategy: Send + Sync {
    fn select(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
        option_type: OptionType,
    ) -> Result<CalendarSpread, StrategyError>;
}
```

### Available Strategies

| Strategy | Selection Logic | Use Case |
|----------|----------------|----------|
| `ATMStrategy` | Strike closest to spot | Simple baseline |
| `DeltaStrategy::fixed()` | Strike at target delta | Precise delta targeting |
| `DeltaStrategy::scanning()` | Scan delta range for best IV ratio | Opportunity optimization |

---

## 4. ATM Strategy

**File:** `cs-domain/src/strategies/atm.rs`

### Strike Selection (lines 29-38)

```rust
let atm_strike = chain_data.strikes
    .iter()
    .min_by(|a, b| {
        let a_diff = (f64::from(**a) - spot_f64).abs();
        let b_diff = (f64::from(**b) - spot_f64).abs();
        a_diff.partial_cmp(&b_diff).unwrap()
    })
    .ok_or(StrategyError::NoStrikes)?;
```

**Simple minimum distance to spot.**

### Expiration Selection (lines 67-110)

```rust
fn select_expirations(
    expirations: &[NaiveDate],
    reference_date: NaiveDate,
    min_short_dte: i32,
    max_short_dte: i32,
    min_long_dte: i32,
    max_long_dte: i32,
) -> Result<(NaiveDate, NaiveDate), StrategyError>
```

**Logic:**
1. Sort expirations
2. Find first expiration with `min_short_dte <= DTE <= max_short_dte` → **short leg**
3. Find first expiration after short with `min_long_dte <= DTE <= max_long_dte` → **long leg**

**Issue:** Uses `earnings_date` as reference, not entry date. For BMO earnings, the reference should be entry date (previous day).

**Defaults:**
- Short: 3-45 DTE
- Long: 14-90 DTE

---

## 5. Delta Strategy

**File:** `cs-domain/src/strategies/delta.rs`

### Two Modes

1. **Fixed Delta** (lines 66-74)
   ```rust
   pub fn fixed(target_delta: f64, criteria: TradeSelectionCriteria) -> Self
   ```
   Uses a single target delta (e.g., 0.50 for ATM).

2. **Scanning Mode** (lines 77-89)
   ```rust
   pub fn scanning(delta_range: (f64, f64), steps: usize, criteria: TradeSelectionCriteria) -> Self
   ```
   Scans a range of deltas and picks the best opportunity.

### Selection Flow (lines 98-169)

```rust
fn select(&self, event, spot, chain_data, option_type) -> Result<CalendarSpread> {
    // 1. Get IV surface
    let iv_surface = chain_data.iv_surface.as_ref()?;

    // 2. Build delta-parameterized surface
    let delta_surface = DeltaVolSurface::from_iv_surface(iv_surface, self.risk_free_rate);

    // 3. Select expirations
    let (short_exp, long_exp) = select_expirations(...)?;

    // 4. Determine target delta (fixed or via scanning)
    let target_delta = match self.scan_mode {
        DeltaScanMode::Fixed => self.target_delta,
        DeltaScanMode::Scan { steps } => {
            let analyzer = OpportunityAnalyzer::new(config);
            let opportunities = analyzer.find_opportunities(&delta_surface, short_exp, long_exp);
            opportunities.first().map(|o| o.target_delta).unwrap_or(0.50)
        }
    };

    // 5. Map delta to theoretical strike
    let theoretical_strike = delta_surface.delta_to_strike(target_delta, short_exp, is_call)?;

    // 6. Find closest tradable strike
    let closest_strike = find_closest_strike(&chain_data.strikes, theoretical_strike)?;

    // 7. Build spread
    CalendarSpread::new(short_leg, long_leg)
}
```

**Key Dependency:** Requires `iv_surface` in `OptionChainData`. If not present, fails with `NoDeltaData`.

### Delta → Contract Translation

When a delta is selected (e.g., 0.375), it must be translated to an actual tradable contract:

```
Selected Delta (0.375)
        │
        ▼
┌───────────────────────────────────────┐
│ 1. Interpolate IV at delta            │
│    iv = delta_surface.get_iv(0.375)   │
│    → e.g., 0.525 (52.5% IV)           │
└───────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────┐
│ 2. Black-Scholes Inversion            │
│    K = S × exp(-(d1×σ√T - (r+σ²/2)T)) │
│    d1 = N⁻¹(Δ) = N⁻¹(0.375) = -0.319 │
│    → e.g., $98.06 theoretical strike  │
└───────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────┐
│ 3. Snap to nearest tradable strike    │
│    Available: [90, 95, 100, 105, 110] │
│    min(|strike - 98.06|) → $100       │
└───────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────┐
│ 4. Build Calendar Spread              │
│    Short: AAPL 100C Jan 10            │
│    Long:  AAPL 100C Jan 24            │
└───────────────────────────────────────┘
```

**Delta Drift:** The snap-to-strike introduces drift. A target of 0.375 delta might result in a 0.45 or 0.50 delta contract depending on strike granularity. For stocks with $1 increments, drift is minimal; for $5 increments, it can be significant.

### Delta Scan Steps

For `--delta-range "0.25,0.75" --delta-scan-steps 5`:

```rust
// linspace(0.25, 0.75, 5) generates:
step = (0.75 - 0.25) / (5 - 1) = 0.125

Delta targets: [0.250, 0.375, 0.500, 0.625, 0.750]
```

| Steps | Delta Range 0.25–0.75 |
|-------|----------------------|
| 3 | `[0.25, 0.50, 0.75]` |
| 5 | `[0.25, 0.375, 0.50, 0.625, 0.75]` |
| 11 | `[0.25, 0.30, 0.35, ..., 0.70, 0.75]` |

---

## 6. Opportunity Analyzer

**File:** `cs-analytics/src/opportunity.rs`

### Scoring Function (lines 141-152)

```rust
fn score_opportunity(&self, delta: f64, ratio: f64, short_iv: f64) -> f64 {
    // Higher IV ratio = more edge
    let ratio_score = (ratio - 1.0) * 10.0;

    // Higher absolute IV = more theta
    let iv_score = short_iv * 2.0;

    // Prefer deltas closer to ATM (more liquid)
    let liquidity_score = 1.0 - (delta - 0.5).abs() * 2.0;

    ratio_score + iv_score + liquidity_score
}
```

**Scoring Weights:**
| Factor | Weight | Rationale |
|--------|--------|-----------|
| IV Ratio | 10x | Core edge - short IV > long IV |
| Absolute IV | 2x | Higher IV = more premium to capture |
| Delta proximity to ATM | 1x | ATM is most liquid |

**Example Scores:**
- Delta=0.50, Ratio=1.20, IV=0.40: `2.0 + 0.8 + 1.0 = 3.8`
- Delta=0.25, Ratio=1.30, IV=0.50: `3.0 + 1.0 + 0.5 = 4.5`

**Issue:** No consideration for bid-ask spread, even though `max_bid_ask_spread_pct` exists in criteria.

### Critical Bug: Delta vs Strike Mismatch

The scoring compares IVs at the **same delta** for both expirations, but the actual trade uses the **same strike**. These are not equivalent.

**Delta-space reality:**

```
Delta 0.50 @ 7 DTE  → Strike $100
Delta 0.50 @ 30 DTE → Strike $102 (forward drift from r, div)
```

**What OpportunityAnalyzer scores:**

```rust
short_iv = surface.get_iv(delta=0.50, short_exp);  // IV at $100
long_iv  = surface.get_iv(delta=0.50, long_exp);   // IV at $102 ← DIFFERENT STRIKE
ratio    = short_iv / long_iv;                      // Apples vs oranges
```

**What the trade actually does:**

```rust
// Strike mapped from SHORT expiry only
let strike = delta_to_strike(0.50, short_exp);  // → $100

// Calendar spread = SAME strike for both legs
short_leg: $100 strike, 7 DTE,  delta ≈ 0.50
long_leg:  $100 strike, 30 DTE, delta ≈ 0.48  // NOT 0.50!
```

**The mismatch:**

| Leg | Scoring basis | Actual trade |
|-----|---------------|--------------|
| Short | IV @ $100 (Δ=0.50) | IV @ $100 (Δ=0.50) ✓ |
| Long | IV @ $102 (Δ=0.50) | IV @ $100 (Δ≈0.48) ✗ |

**The scored IV ratio doesn't match the traded IV ratio.**

**Fix:** Score at the same STRIKE, not same delta:

```rust
// Current (wrong for calendar spreads):
let short_iv = delta_surface.get_iv(delta, short_exp);  // Strike A
let long_iv  = delta_surface.get_iv(delta, long_exp);   // Strike B ≠ A

// Correct:
let strike = delta_to_strike(target_delta, short_exp);
let short_iv = iv_surface.get_iv_at_strike(strike, short_exp);
let long_iv  = iv_surface.get_iv_at_strike(strike, long_exp);  // Same strike!
```

**Additional consideration:** This also affects the IV model choice:
- Delta-scan implicitly assumes sticky-delta behavior
- But calendar spreads trade at fixed strikes
- Using `--iv-model sticky-strike` with `--strategy delta-scan` is conceptually inconsistent

---

## 7. Delta-Vol Surface

**File:** `cs-analytics/src/delta_surface.rs`

### Construction (lines 42-76)

```rust
pub fn from_iv_surface(surface: &IVSurface, risk_free_rate: f64) -> Self {
    // Group points by expiration
    // For each expiration, create VolSlice
    // VolSlice converts (strike, IV) → (delta, IV)
}
```

### Delta-to-Strike Mapping (lines 199-215)

```rust
pub fn delta_to_strike(&self, delta: f64, expiration: NaiveDate, is_call: bool) -> Option<f64> {
    let iv = self.get_iv(delta, expiration)?;
    let tte = self.tte(expiration)?;
    delta_to_strike_with_iv(delta, iv, self.spot, tte, self.risk_free_rate, is_call)
}
```

Uses Black-Scholes inversion: `K = S * exp(-(d1 * σ√T - (r + σ²/2)T))`

---

## 8. Vol Slice Interpolation

**File:** `cs-analytics/src/vol_slice.rs`

### Interpolation Modes

1. **Linear** (default): Linear interpolation in delta-space
2. **SVI**: Parametric SVI fit (M2 feature)

### Linear Interpolation (lines 223-251)

```rust
fn linear_interp(&self, target_delta: f64) -> Option<f64> {
    // Find bracketing points
    // Linear interpolation between them
    // Flat extrapolation outside range
}
```

**Extrapolation:** Uses flat extrapolation (last known value).

**Issue:** Flat extrapolation can be inaccurate for deep OTM options.

---

## 9. Trade Execution

**File:** `cs-backtest/src/trade_executor.rs`

### Execution Flow (lines 70-81)

```rust
pub async fn execute_trade(&self, spread, event, entry_time, exit_time) -> CalendarSpreadResult {
    match self.try_execute_trade(...).await {
        Ok(result) => result,
        Err(e) => self.create_failed_result(spread, event, entry_time, exit_time, e),
    }
}
```

### Validation Checks (lines 119-139)

1. **Positive Entry Cost:**
   ```rust
   if entry_pricing.net_cost <= Decimal::ZERO {
       return Err(ExecutionError::InvalidSpread("Negative entry cost"));
   }
   ```
   Calendar spread should cost money (long > short).

2. **Minimum Entry Cost:**
   ```rust
   let min_entry_cost = Decimal::new(5, 2); // $0.05
   if entry_pricing.net_cost < min_entry_cost {
       return Err(ExecutionError::InvalidSpread("Entry cost too small"));
   }
   ```
   Avoid degenerate spreads with near-zero cost.

---

## 10. Spread Pricing

**File:** `cs-backtest/src/spread_pricer.rs`

### IV Models

| Model | Description | Use Case |
|-------|-------------|----------|
| `StickyStrike` | IV indexed by absolute strike K | Default, simplest |
| `StickyMoneyness` | IV indexed by K/S | Floats with spot |
| `StickyDelta` | IV indexed by delta | Most accurate for rolling smile |

### Pricing Logic (lines 130-241)

1. Try to find exact match in chain data
2. If no match, use Black-Scholes with interpolated IV
3. Calculate Greeks alongside price

**Fallback IV:** 30% if no interpolation available.

---

## 11. IV Surface Building

**File:** `cs-backtest/src/iv_surface_builder.rs`

### Filters Applied (lines 47-77)

1. Skip if close <= 0 or strike <= 0
2. Skip if TTM <= 0 (expired)
3. Skip if IV < 0.01 or IV > 5.0 (unreasonable)

---

## Issues & Improvement Opportunities

### High Priority

1. **CRITICAL: Delta vs Strike Mismatch in Scoring**
   - `OpportunityAnalyzer` compares IVs at same DELTA (different strikes)
   - But calendar spread trades at same STRIKE (different deltas)
   - The scored IV ratio doesn't match the actual traded IV ratio
   - Impact: Delta-scan strategy may select suboptimal trades
   - Fix: Score IVs at the same strike, not same delta

2. **IV Ratio Filter Applied Too Late**
   - Currently: Execute trade → Calculate IV ratio → Filter
   - Should: Calculate IV ratio → Filter → Execute trade
   - Impact: Wasted computation on filtered trades

3. **Expiration Reference Date**
   - Uses `earnings_date` for DTE calculation
   - Should use `entry_date` (which differs for BMO events)
   - Impact: Off-by-one DTE for BMO events

4. **Duplicate Code: `select_expirations`**
   - Identical function in `atm.rs` and `delta.rs`
   - Should be extracted to shared utility

### Medium Priority

5. **No Bid-Ask Spread Filtering**
   - `TradeSelectionCriteria.max_bid_ask_spread_pct` exists but is never used
   - Could filter illiquid options

6. **Flat Extrapolation in VolSlice**
   - Deep OTM options get inaccurate IV estimates
   - Consider wing extrapolation models

7. **Opportunity Scoring Doesn't Consider Greeks**
   - No theta/vega consideration in scoring
   - Calendar spreads are primarily theta plays

### Low Priority

8. **Hardcoded Risk-Free Rate**
   - 5% hardcoded in `DeltaStrategy::default()`
   - Should be configurable

9. **No Position Sizing**
   - All trades are 1 contract
   - No Kelly criterion or volatility targeting

10. **No Stop-Loss Logic**
    - Trade always held to exit time
    - No early exit on adverse moves

---

## Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           BacktestUseCase                               │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  1. Load Earnings Window (session_date ± 1 day)                         │
│     └── EarningsRepository.load_earnings()                              │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  2. Pre-Filter Events                                                   │
│     ├── Market Cap Filter (min_market_cap)                              │
│     └── Entry Date Filter (EarningsTradeTiming.entry_date == session)   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  3. For Each Event: Process                                             │
│     ├── Get Spot Price at entry_time                                    │
│     ├── Get Option Chain DataFrame                                      │
│     ├── Build IV Surface (strike → IV)                                  │
│     ├── Get Available Expirations & Strikes                             │
│     └── Build OptionChainData                                           │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  4. Strategy Selection                                                  │
│     ├── ATMStrategy: min(|strike - spot|)                               │
│     ├── DeltaStrategy::Fixed: delta_to_strike(target_delta)             │
│     └── DeltaStrategy::Scan: OpportunityAnalyzer → best delta → strike  │
│                                                                         │
│     Common: select_expirations(min_short_dte..max_short_dte,            │
│                                min_long_dte..max_long_dte)              │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  5. Trade Execution                                                     │
│     ├── TradeExecutor.execute_trade(spread, entry_time, exit_time)      │
│     ├── Price Entry: SpreadPricer.price_spread(entry_chain, spot)       │
│     ├── Validate: entry_cost > 0 && entry_cost >= $0.05                 │
│     ├── Price Exit: SpreadPricer.price_spread(exit_chain, spot)         │
│     ├── Calculate PnL: exit_value - entry_cost                          │
│     └── Calculate Greeks & Attribution                                  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  6. Post-Filter                                                         │
│     └── IV Ratio Filter: iv_ratio >= min_iv_ratio                       │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  7. Collect Results                                                     │
│     ├── Successful trades → BacktestResult.results                      │
│     └── Failed trades → BacktestResult.dropped_events                   │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Configuration Reference

### TradeSelectionCriteria

| Field | Default | Description |
|-------|---------|-------------|
| `min_short_dte` | 3 | Minimum short leg DTE (avoid gamma risk) |
| `max_short_dte` | 45 | Maximum short leg DTE |
| `min_long_dte` | 14 | Minimum long leg DTE (ensure time value) |
| `max_long_dte` | 90 | Maximum long leg DTE |
| `target_delta` | None | Target delta for delta strategies |
| `min_iv_ratio` | None | Minimum IV ratio filter (short/long) |
| `max_bid_ask_spread_pct` | None | **UNUSED** - max spread % |

### BacktestConfig

| Field | Default | Description |
|-------|---------|-------------|
| `strategy` | ATM | Strategy type |
| `iv_model` | StickyStrike | IV interpolation model |
| `vol_model` | Linear | Vol slice interpolation |
| `target_delta` | 0.50 | For Delta strategy |
| `delta_range` | (0.25, 0.75) | For DeltaScan |
| `delta_scan_steps` | 5 | Scan granularity |
| `parallel` | true | Parallel event processing |

---

## Recommendations

### Immediate Fixes

1. **Move IV ratio filter before execution:**
   ```rust
   // In process_event(), after building IV surface:
   let iv_ratio = compute_iv_ratio(&iv_surface, short_exp, long_exp, target_delta);
   if let Some(min) = self.config.selection.min_iv_ratio {
       if iv_ratio < min {
           return Err(TradeGenerationError { reason: "IV_RATIO_FILTER", ... });
       }
   }
   ```

2. **Extract `select_expirations` to shared module:**
   ```rust
   // cs-domain/src/strategies/expiration_selector.rs
   pub fn select_expirations(...) -> Result<(NaiveDate, NaiveDate), StrategyError>
   ```

3. **Use entry_date for DTE calculation:**
   ```rust
   let reference_date = self.earnings_timing.entry_date(event);
   let (short_exp, long_exp) = select_expirations(&expirations, reference_date, ...);
   ```

### Future Enhancements

1. **Implement bid-ask spread filtering**
2. **Add wing extrapolation for VolSlice**
3. **Add theta-weighted scoring to OpportunityAnalyzer**
4. **Make risk-free rate configurable**
5. **Add configurable position sizing**

---

## Conceptual: Filters vs Strategy

### The Boundary Problem

The current architecture distinguishes between "filters" and "strategy," but this boundary is blurry:

| Current "Filter" | Current "Strategy" |
|------------------|-------------------|
| Market cap | Strike selection (ATM/Delta) |
| Entry date matching | Expiration selection |
| IV ratio threshold | Delta scanning |
| (unused) Bid-ask spread | Opportunity scoring |

**The problem:** Filtering out valid trades IS part of the strategy. Where do you draw the line?

- Is `min_iv_ratio` a filter or a strategy parameter? It defines "what edge do we require."
- Are DTE constraints filters or strategy? They define "what expirations we want."
- Is `min_market_cap` a filter or universe selection?

### Current Parameter Scatter

Parameters that affect trade selection are scattered across multiple structs:

```
TradeSelectionCriteria (cs-domain/strategies/mod.rs)
├── min_short_dte, max_short_dte    ← Expiration constraints
├── min_long_dte, max_long_dte      ← Expiration constraints
├── target_delta                     ← Strike selection
├── min_iv_ratio                     ← Quality threshold
└── max_bid_ask_spread_pct           ← Quality threshold (UNUSED)

TimingConfig (cs-domain/value_objects.rs)
├── entry_hour, entry_minute         ← Timing
└── exit_hour, exit_minute           ← Timing

BacktestConfig (cs-backtest/config.rs)
├── strategy (ATM/Delta/DeltaScan)   ← Strike selection method
├── target_delta                     ← Duplicate!
├── delta_range                      ← Strike selection
├── delta_scan_steps                 ← Strike selection
├── iv_model                         ← Pricing model
├── vol_model                        ← Pricing model
├── min_market_cap                   ← Universe filter
└── selection: TradeSelectionCriteria ← Nested

EarningsTradeTiming (service)
└── BMO/AMC handling                 ← Timing logic
```

**Problems:**
1. `target_delta` appears in both `TradeSelectionCriteria` and `BacktestConfig`
2. Related parameters are split across structs
3. User must understand internal architecture to configure

### Proposed Mental Model

Instead of "filters" vs "strategy," think in terms of **three concerns**:

```
┌─────────────────────────────────────────────────────────────────┐
│  1. UNIVERSE                                                    │
│     "What do we scan?"                                          │
│     ├── Date range (start, end)                                 │
│     ├── Symbols (whitelist/blacklist)                           │
│     └── Market cap threshold                                    │
│                                                                 │
│     These are EXTERNAL constraints, not part of the trade thesis│
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  2. STRATEGY (The Trade Thesis)                                 │
│     "What trade do we want?"                                    │
│                                                                 │
│     Strike & Expiration:                                        │
│     ├── Strike selection method (ATM, Delta, DeltaScan)         │
│     ├── Target delta / delta range                              │
│     ├── Expiration constraints (DTE ranges)                     │
│     └── Option type (Call/Put)                                  │
│                                                                 │
│     Edge Definition:                                            │
│     ├── Min IV ratio ← THIS IS THE EDGE, not a filter           │
│     ├── Opportunity scoring weights                             │
│     └── IV/Vol interpolation models                             │
│                                                                 │
│     Timing:                                                     │
│     ├── Entry time (hour:minute)                                │
│     ├── Exit time (hour:minute)                                 │
│     └── BMO/AMC handling                                        │
│                                                                 │
│     All of this IS the strategy. A strategy requiring 1.20 IV   │
│     ratio is fundamentally different from one accepting 1.05.   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  3. EXECUTION CONSTRAINTS                                       │
│     "Can we execute this trade?"                                │
│     ├── Max bid-ask spread (liquidity)                          │
│     ├── Min entry cost (avoid degenerate spreads)               │
│     ├── Min open interest / volume                              │
│     └── Slippage assumptions                                    │
│                                                                 │
│     These are about PRACTICALITY, not the trade thesis.         │
│     A trade can be strategically valid but unexecutable.        │
└─────────────────────────────────────────────────────────────────┘
```

**Key distinction:**
- **Strategy** = "What trade do I want?" (includes IV ratio as the core edge)
- **Execution** = "Can I actually do it?" (bid-ask, liquidity, cost)

### Recommended: Unified Configuration

```rust
/// Universe - what to scan (external constraints)
pub struct UniverseConfig {
    pub symbols: Option<Vec<String>>,
    pub min_market_cap: Option<f64>,
}

/// Strategy - the trade thesis (what trade do we want)
pub struct StrategyConfig {
    // Strike & Expiration
    pub strike_method: StrikeMethod,
    pub short_dte: DteRange,
    pub long_dte: DteRange,
    pub option_type: OptionType,

    // Edge Definition - THIS IS THE STRATEGY
    pub min_iv_ratio: f64,            // The core edge requirement
    pub scoring_weights: ScoringWeights,
    pub iv_model: IVModel,
    pub vol_model: InterpolationMode,
    pub risk_free_rate: f64,

    // Timing
    pub entry_time: MarketTime,
    pub exit_time: MarketTime,
}

/// Execution - can we actually do it (practicality)
pub struct ExecutionConfig {
    pub max_bid_ask_pct: Option<f64>,
    pub min_entry_cost: Decimal,
    pub min_open_interest: Option<u64>,
}

pub enum StrikeMethod {
    ATM,
    Delta(f64),
    DeltaScan { range: (f64, f64), steps: usize },
}

pub struct DteRange {
    pub min: i32,
    pub max: i32,
}

pub struct ScoringWeights {
    pub iv_ratio: f64,      // default 10.0
    pub absolute_iv: f64,   // default 2.0
    pub atm_proximity: f64, // default 1.0
}
```

### Benefits

1. **Single location** for all strategy parameters
2. **Clear mental model** — users configure one thing
3. **No duplication** — `target_delta` lives in one place
4. **Explicit stages** — universe, construction, timing, quality
5. **Easier CLI** — can map directly to struct fields

### CLI Mapping

```bash
# Current (scattered across concepts)
./cs backtest \
  --strategy delta-scan \
  --target-delta 0.40 \
  --delta-range "0.25,0.75" \
  --min-short-dte 3 \
  --max-short-dte 45 \
  --entry-hour 9 --entry-minute 35 \
  --min-iv-ratio 1.10 \
  --min-market-cap 1000000000

# Proposed (config file with clear sections)
./cs backtest --config strategy.toml

# strategy.toml
[universe]
min_market_cap = 1_000_000_000
# symbols = ["AAPL", "MSFT"]  # optional whitelist

[strategy]
# Strike & Expiration
method = "delta-scan"
delta_range = [0.25, 0.75]
delta_steps = 5
short_dte = { min = 3, max = 45 }
long_dte = { min = 14, max = 90 }
option_type = "call"

# Edge Definition
min_iv_ratio = 1.10  # THE CORE EDGE
iv_model = "sticky-delta"
vol_model = "linear"

# Timing
entry_time = "09:35"
exit_time = "15:45"

[execution]
max_bid_ask_pct = 0.10  # 10% max spread
min_entry_cost = 0.10   # $0.10 minimum
```

### Summary: Three Concerns

| Concern | Question | Parameters |
|---------|----------|------------|
| **Universe** | "What to scan?" | symbols, market_cap, dates |
| **Strategy** | "What trade do I want?" | strike method, DTE, delta, **IV ratio**, timing |
| **Execution** | "Can I do it?" | bid-ask, min cost, liquidity |

**Key insight:** IV ratio is the **core edge** of the calendar spread strategy. It belongs in Strategy, not as a post-hoc filter. A strategy with `min_iv_ratio = 1.20` is a different strategy than one with `min_iv_ratio = 1.05` — they should be configured together, not in separate code paths.
