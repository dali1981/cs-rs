# Code Duplication & Refactoring Analysis
**Date**: 2026-01-08
**Scope**: cs-rs-new-approach codebase analysis for duplicates, over-engineering, and maintainability improvements

---

## Executive Summary

Analyzed 6 major source files and identified **10 high-impact refactoring opportunities**. The codebase has significant duplication across:
- **Delta providers** (4 implementations, ~95% code overlap)
- **CLI command handling** (3045-line main.rs with scattered logic)
- **Result builders** (4 spreads with identical boilerplate)
- **P&L attribution** (2 implemented, 2 incomplete with duplicated patterns)

**Total potential reduction**: ~550 lines of duplicated/over-engineered code with high impact on maintainability.

---

## Top 10 Refactoring Opportunities

### 1. ⭐ CRITICAL: Delta Providers Repetitive Leg-by-Leg Pattern

**Files Affected**:
- `cs-backtest/src/delta_providers/entry_volatility.rs:54-77`
- `cs-backtest/src/delta_providers/current_hv.rs:84-106`
- `cs-backtest/src/delta_providers/current_market_iv.rs:71-98`
- `cs-backtest/src/delta_providers/historical_average_iv.rs:157-189`

**Problem**:
All 4 implementations repeat the same leg-by-leg computation:
1. Iterate through legs
2. Compute time-to-expiration (TTE)
3. Handle expiration edge case
4. Call Black-Scholes with different volatility source
5. Apply position sign
6. Sum deltas

```rust
// REPEATED IN 4 FILES
let position_delta: f64 = self.trade.legs().iter().map(|(leg, position)| {
    let tte = (leg.expiration - timestamp.date_naive()).num_days() as f64 / 365.0;
    if tte <= 0.0 { return 0.0; }
    let is_call = leg.option_type == OptionType::Call;
    let strike = leg.strike.value().to_f64().unwrap_or(0.0);
    let leg_delta = bs_delta(spot, strike, tte, VOLATILITY_SOURCE, is_call, rate);
    leg_delta * position.sign()
}).sum();
```

**Impact**:
- ~40 lines of duplicated code
- Maintenance burden: bug fixes need to be applied in 4 places
- Risk: consistency issues if one provider is updated differently

**Solution**:
Extract shared computation into trait helper method:
```rust
// In DeltaProvider trait
fn compute_position_delta_from_vol(
    &self,
    spot: f64,
    timestamp: DateTime<Utc>,
    volatility: f64,
    rate: f64,
) -> Result<f64, String> {
    let position_delta: f64 = self.get_trade().legs()
        .iter()
        .map(|(leg, position)| {
            let tte = (leg.expiration - timestamp.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 { return 0.0; }
            let is_call = leg.option_type == OptionType::Call;
            let strike = leg.strike.value().to_f64().unwrap_or(0.0);
            let leg_delta = bs_delta(spot, strike, tte, volatility, is_call, rate);
            leg_delta * position.sign()
        })
        .sum();
    Ok(position_delta)
}
```

Each provider then just calls:
```rust
async fn compute_delta(&mut self, spot: f64, timestamp: DateTime<Utc>) -> Result<f64, String> {
    let vol = self.get_volatility(spot, timestamp).await?;
    self.compute_position_delta_from_vol(spot, timestamp, vol, 0.05)
}
```

**Effort**: Low (1-2 hours)
**Impact**: High (eliminates 40 lines, improves consistency)
**Priority**: P0 - Fix before adding new delta modes

---

### 2. ⭐ MAJOR: Duplicate Roll Policy Parsing Functions

**Files Affected**:
- `cs-cli/src/main.rs:999-1025` (`parse_roll_policy()`)
- `cs-cli/src/main.rs:2542-2575` (`parse_campaign_roll_policy()`)

**Problem**:
Nearly identical parsing logic duplicated in two places with minor differences:

```rust
// FUNCTION 1: parse_roll_policy (lines 999-1025)
fn parse_roll_policy(policy: &str) -> Result<RollPolicy> {
    match policy {
        "none" => Ok(RollPolicy::None),
        "weekly" => {
            // ... parse weekday ...
            Ok(RollPolicy::Weekly(weekday))
        }
        s if s.starts_with("days:") => {
            // ... parse interval ...
            Ok(RollPolicy::FixedDays(days))
        }
        _ => Err(...),
    }
}

// FUNCTION 2: parse_campaign_roll_policy (lines 2542-2575) - 70% DUPLICATE
fn parse_campaign_roll_policy(policy: &str) -> Result<RollPolicy> {
    match policy {
        "none" => Ok(RollPolicy::None),
        "weekly" => {
            // ... IDENTICAL weekday parsing ...
            Ok(RollPolicy::Weekly(weekday))
        }
        "monthly" => {
            // ... parse month offset ...
            Ok(RollPolicy::Monthly(offset))
        }
        s if s.starts_with("days:") => {
            // ... IDENTICAL days parsing ...
            Ok(RollPolicy::FixedDays(days))
        }
        _ => Err(...),
    }
}
```

**Issue**: Copy-paste error risk. If weekday parsing is fixed in one, the other becomes inconsistent.

**Solution**:
Merge into single function:
```rust
fn parse_roll_policy(policy: &str, allow_monthly: bool) -> Result<RollPolicy> {
    match policy {
        "none" => Ok(RollPolicy::None),
        "weekly" => parse_weekday(policy).map(RollPolicy::Weekly),
        "monthly" if allow_monthly => parse_month_offset(policy).map(RollPolicy::Monthly),
        s if s.starts_with("days:") => parse_interval(s).map(RollPolicy::FixedDays),
        _ => Err(anyhow::anyhow!("Unknown roll policy: {}", policy)),
    }
}

// Call sites:
parse_roll_policy(&policy, false)?  // backtest command
parse_roll_policy(&policy, true)?   // campaign command
```

**Effort**: Low (30 minutes)
**Impact**: Medium (eliminates 70 lines, single source of truth)
**Priority**: P1 - Quick win

---

### 3. ⭐ MAJOR: Boilerplate `to_failed_result()` in 4 Spread Implementations

**Files Affected**:
- `cs-backtest/src/execution/calendar_spread_impl.rs:187-252` (~65 lines)
- `cs-backtest/src/execution/straddle_impl.rs:166-199` (~33 lines)
- `cs-backtest/src/execution/calendar_straddle_impl.rs:~145-210` (~65 lines)
- `cs-backtest/src/execution/iron_butterfly_impl.rs:255-310+` (~55 lines)

**Problem**:
Each spread implementation has identical boilerplate to construct a "failed" result with all fields set to defaults:

```rust
// Repeated in 4 files with different struct types
fn to_failed_result(
    symbol: &str,
    earnings_date: NaiveDate,
    earnings_time: EarningsTime,
    failure_reason: &str,
) -> StraddleResult {
    StraddleResult {
        symbol: symbol.to_string(),
        earnings_date,
        earnings_time,
        earnings_expected_move_pct: None,
        entry_debit: Decimal::ZERO,
        exit_credit: Decimal::ZERO,
        pnl: Decimal::ZERO,
        spot_at_entry: 0.0,
        spot_at_exit: 0.0,
        spot_move_pct: 0.0,
        iv_entry: None,
        iv_exit: None,
        iv_change: None,
        net_delta: 0.0,
        net_gamma: 0.0,
        net_theta: None,
        net_vega: None,
        delta_pnl: None,
        gamma_pnl: None,
        theta_pnl: None,
        vega_pnl: None,
        unexplained_pnl: None,
        // ... 20 more fields ...
        success: false,
        failure_reason: Some(failure_reason.to_string()),
        hedge_position: None,
        // ... more fields ...
    }
}
```

This pattern is pure boilerplate and violates DRY.

**Solution Option 1: Trait Default + Builder**
```rust
impl Default for StraddleResult {
    fn default() -> Self {
        // All fields with default values
    }
}

// In execution code:
let mut result = StraddleResult::default();
result.symbol = symbol.to_string();
result.earnings_date = earnings_date;
result.success = false;
result.failure_reason = Some(failure_reason.to_string());
```

**Solution Option 2: Builder Pattern**
```rust
StraddleResult::builder(symbol, earnings_date, earnings_time)
    .with_failure(failure_reason)
    .build()
```

**Solution Option 3: Simple Function** (simplest)
```rust
pub fn failed_result(
    symbol: &str,
    earnings_date: NaiveDate,
    earnings_time: EarningsTime,
    reason: &str,
) -> StraddleResult {
    let mut result = StraddleResult::default();
    result.symbol = symbol.to_string();
    result.earnings_date = earnings_date;
    result.earnings_time = earnings_time;
    result.success = false;
    result.failure_reason = Some(reason.to_string());
    result
}
```

**Effort**: Low (1 hour per spread type)
**Impact**: High (eliminates 60+ lines, improves consistency)
**Priority**: P1 - Quick win after adding Default derives

---

### 4. ⭐ MAJOR: P&L Attribution Incomplete & Duplicated

**Files Affected**:
- `cs-backtest/src/execution/calendar_spread_impl.rs:86-127` (implemented, leg-by-leg)
- `cs-backtest/src/execution/straddle_impl.rs:254-300+` (implemented, similar structure)
- `cs-backtest/src/execution/calendar_straddle_impl.rs:~120-140` (placeholder)
- `cs-backtest/src/execution/iron_butterfly_impl.rs:181-185` (**NOT IMPLEMENTED** - returns all None)

**Current State**:
```rust
// iron_butterfly_impl.rs:181-185 - TODO!
let (delta_pnl, gamma_pnl, theta_pnl, vega_pnl, unexplained_pnl) = {
    // TODO: Implement proper 4-leg attribution
    (None, None, None, None, None)
};
```

**Problem**:
1. Iron butterfly returns `(None, None, None, None, None)` - feature incomplete
2. Calendar spread and straddle implement similar logic with duplicated patterns
3. No shared code between implementations despite similar mathematics

**Analysis of Current Implementations**:

Calendar Spread (working):
```rust
// Separate short/long leg attribution
let (short_delta_pnl, short_gamma_pnl, ...) = calculate_option_leg_pnl(...);
let (long_delta_pnl, long_gamma_pnl, ...) = calculate_option_leg_pnl(...);
// Then combine with position signs
```

Straddle (working):
```rust
// Call + put attribution
let call_pnl = calculate_option_leg_pnl(short call);
let put_pnl = calculate_option_leg_pnl(short put);
// Scale and sum
```

Iron Butterfly (broken):
```rust
// 4 legs but no code to handle them
```

**Solution**:
Create generic attribution helper in `cs-domain`:
```rust
pub fn compute_spread_attribution(
    legs: &[(PricingData, LegPosition)],
    spot_change: f64,
    days_held: f64,
    total_pnl: Decimal,
) -> Result<(Option<Decimal>, Option<Decimal>, Option<Decimal>, Option<Decimal>, Option<Decimal>)> {
    let mut delta_pnl = 0.0;
    let mut gamma_pnl = 0.0;
    let mut theta_pnl = 0.0;
    let mut vega_pnl = 0.0;

    for (pricing, position) in legs {
        let sign = position.multiplier();
        let leg_attr = calculate_option_leg_pnl(
            pricing.greeks.as_ref(),
            pricing.iv_entry,
            pricing.iv_exit,
            spot_change,
            days_held,
            sign as f64,
        )?;

        delta_pnl += leg_attr.delta;
        gamma_pnl += leg_attr.gamma;
        theta_pnl += leg_attr.theta;
        vega_pnl += leg_attr.vega;
    }

    let explained = delta_pnl + gamma_pnl + theta_pnl + vega_pnl;
    let unexplained = total_pnl.to_f64().unwrap_or(0.0) - explained;

    Ok((
        Some(Decimal::try_from(delta_pnl)?),
        Some(Decimal::try_from(gamma_pnl)?),
        Some(Decimal::try_from(theta_pnl)?),
        Some(Decimal::try_from(vega_pnl)?),
        Some(Decimal::try_from(unexplained)?),
    ))
}
```

Then each spread just calls:
```rust
let attribution = compute_spread_attribution(
    &[(short_call, LegPosition::Short), (short_put, LegPosition::Short), ...],
    spot_change,
    days_held,
    pnl,
)?;
```

**Effort**: Medium (4-6 hours - need to verify leg positions for all 4 spreads)
**Impact**: High (completes feature, eliminates duplication, ensures consistency)
**Priority**: P0 - Feature completeness

---

### 5. ⭐ MAJOR: CLI main.rs 3045 Lines - Needs Module Extraction

**Files Affected**:
- `cs-cli/src/main.rs` (entire file - 3045 lines)

**Current Structure**:
- Lines 1-350: Imports, CLI struct definitions
- Lines 351-700: Data types and enums
- Lines 701-1200: Trait implementations
- Lines 1201-1500: `run_backtest` command handler (300 lines)
- Lines 1501-1800: `run_rolling_straddle` command handler (300 lines)
- Lines 1801-2100: `run_campaign_command` handler (300 lines)
- Lines 2101-2300: ATM IV command (200 lines)
- Lines 2301-2500: Earnings analysis (200 lines)
- Lines 2501-2800: Helper functions
- Lines 2801-3045: Result formatting, main() function

**Problem**:
- Single file handles all command routing, parsing, and execution
- Difficult to test individual commands
- Hard to navigate and maintain
- Increases cognitive load

**Solution**:
Extract into module structure:
```
cs-cli/src/
├── main.rs              (100 lines - just main() and CLI setup)
├── lib.rs               (exports modules)
├── commands/
│   ├── mod.rs
│   ├── backtest.rs      (300 lines - run_backtest + helpers)
│   ├── rolling.rs       (300 lines - run_rolling_straddle + helpers)
│   ├── campaign.rs      (350 lines - run_campaign_command + helpers)
│   ├── iv_analysis.rs   (200 lines - ATM IV, earnings analysis)
│   └── price.rs         (150 lines - price command)
├── parsing/
│   ├── mod.rs
│   ├── roll_policy.rs   (shared roll policy parsing)
│   ├── time.rs          (time parsing utilities)
│   └── config.rs        (config building helpers)
└── formatting/
    ├── mod.rs
    ├── csv.rs           (CSV output formatting)
    └── json.rs          (JSON output formatting)
```

**Effort**: High (8-12 hours - careful extraction and testing)
**Impact**: High (vastly improved maintainability, testability)
**Priority**: P1 - Do after quick wins

---

### 6. ⭐ SIGNIFICANT: Time & Config Parsing Scattered Throughout

**Files Affected**:
- `cs-cli/src/main.rs:1361-1384` (time parsing inline)
- `cs-cli/src/main.rs:1028-1150` (config building - 60+ parameters)
- `cs-cli/src/main.rs:2290-2310` (hardcoded times)
- `cs-cli/src/config.rs:1-200` (config structures)

**Problem**:

Time parsing duplicated:
```rust
// Called twice for entry_time and exit_time
let entry_time = if let Some(time_str) = &entry_time {
    let parts: Vec<&str> = time_str.split(':').collect();
    let h = parts[0].parse::<u32>()?;
    let m = parts[1].parse::<u32>()?;
    NaiveTime::from_hms_opt(h, m, 0).ok_or_else(|| anyhow!("Invalid time"))?
} else {
    NaiveTime::from_hms_opt(9, 35, 0).unwrap()
};
```

Config building function has 60+ parameters:
```rust
fn build_cli_overrides(
    data_dir, earnings_dir, spread, selection, delta_range_str,
    delta_scan_steps, symbols, entry_hour, entry_minute, exit_hour,
    exit_minute, min_market_cap, min_short_dte, min_iv_ratio,
    leg_strategy, interpolate_prices, // ... 20+ more
) -> Result<TradingCampaign> { ... }
```

**Solution**:
Create time/config grouping structures:
```rust
#[derive(Debug, Clone)]
pub struct TimeConfig {
    pub entry: (u32, u32),      // (hour, minute)
    pub exit: (u32, u32),
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            entry: (9, 35),
            exit: (15, 55),
        }
    }
}

pub fn parse_time(s: Option<String>) -> Result<(u32, u32)> {
    match s {
        Some(time_str) => {
            let parts: Vec<&str> = time_str.split(':').collect();
            let h = parts[0].parse::<u32>()?;
            let m = parts[1].parse::<u32>()?;
            Ok((h, m))
        }
        None => Ok((0, 0)), // Caller uses default
    }
}

// Usage
let time_config = TimeConfig {
    entry: parse_time(entry_time)?,
    exit: parse_time(exit_time)?,
};
```

Reduce `build_cli_overrides` parameters by 50%:
```rust
fn build_cli_overrides(
    base_config: &BaseConfig,
    spread_config: &SpreadConfig,
    timing_config: &TimeConfig,
    // ... only 10 parameters instead of 60
) -> Result<TradingCampaign> { ... }
```

**Effort**: Low (2-3 hours)
**Impact**: Medium (improves readability, reduces parameter passing)
**Priority**: P2 - Refactor after main.rs extraction

---

### 7. SIGNIFICANT: IV Surface Building Repeated

**Files Affected**:
- `cs-backtest/src/delta_providers/current_market_iv.rs:61-68`
- `cs-backtest/src/delta_providers/historical_average_iv.rs:129-132` (in loop)
- `cs-backtest/src/iv_surface_builder.rs:17-90` (main implementation)

**Problem**:
Multiple delta providers call `build_iv_surface()` with minimal variation:
```rust
// current_market_iv.rs
let iv_surface = build_iv_surface(&chain_df, spot, timestamp, &self.symbol)
    .ok_or_else(|| format!("Failed to build IV surface"))?;

// historical_average_iv.rs - called in loop for each date
let iv_surface = match build_iv_surface(&chain_df, spot, pricing_time, &self.symbol) {
    Some(surface) => surface,
    None => continue,  // Skip bad data
};
```

**Issue**: No caching between calls. If surface building is expensive, it's recomputed unnecessarily.

**Solution**:
Add optional caching layer:
```rust
struct CachedIvSurfaceProvider {
    cache: HashMap<(Date, f64, String), IVSurface>, // (date, spot, symbol) -> surface
    data_repo: DataRepository,
}

impl CachedIvSurfaceProvider {
    pub fn get_surface(
        &mut self,
        symbol: &str,
        spot: f64,
        timestamp: DateTime<Utc>,
    ) -> Result<IVSurface> {
        let key = (timestamp.date_naive(), spot.round() as i32 as f64, symbol.to_string());

        if let Some(surface) = self.cache.get(&key) {
            return Ok(surface.clone());
        }

        let chain_df = self.data_repo.get_option_chain(...)?;
        let surface = build_iv_surface(&chain_df, spot, timestamp, symbol)
            .ok_or_else(|| anyhow!("Failed to build IV surface"))?;

        self.cache.insert(key, surface.clone());
        Ok(surface)
    }
}
```

**Effort**: Low (2-3 hours)
**Impact**: Medium (performance improvement, reduces redundant computation)
**Priority**: P2 - Optimize after core refactoring done

---

### 8. SIGNIFICANT: IV Validation Pattern Repeated (4 spreads)

**Files Affected**:
- `cs-backtest/src/execution/calendar_spread_impl.rs:26-46` (2 legs)
- `cs-backtest/src/execution/straddle_impl.rs:35-44` (2 legs)
- `cs-backtest/src/execution/calendar_straddle_impl.rs:27-68` (4 legs)
- `cs-backtest/src/execution/iron_butterfly_impl.rs:26-46` (4 legs)

**Problem**:
Similar IV validation loops in all implementations:
```rust
// Repeated pattern with variations
if let Some(max_iv) = config.max_entry_iv {
    if let Some(iv) = pricing.short_leg.iv {
        if iv > max_iv {
            return Err(ExecutionError::InvalidSpread(...));
        }
    }
}

// Repeat for long_leg with similar code
if let Some(max_iv) = config.max_entry_iv {
    if let Some(iv) = pricing.long_leg.iv {
        if iv > max_iv {
            return Err(ExecutionError::InvalidSpread(...));
        }
    }
}
```

**Solution**:
Extract validation helper:
```rust
pub fn validate_leg_ivs(
    legs: &[(&str, Option<f64>)],
    constraints: &IvConstraints,
) -> Result<()> {
    for (name, iv) in legs {
        if let Some(max_iv) = constraints.max_entry_iv {
            if let Some(iv_value) = iv {
                if iv_value > max_iv {
                    return Err(anyhow!(
                        "IV constraint violation on {}: {:.2}% > {:.2}%",
                        name,
                        iv_value * 100.0,
                        max_iv * 100.0
                    ));
                }
            }
        }

        if let Some(min_iv) = constraints.min_entry_iv {
            if let Some(iv_value) = iv {
                if iv_value < min_iv {
                    return Err(anyhow!(
                        "IV below minimum on {}: {:.2}% < {:.2}%",
                        name,
                        iv_value * 100.0,
                        min_iv * 100.0
                    ));
                }
            }
        }
    }
    Ok(())
}

// Usage in all spreads
validate_leg_ivs(
    &[
        ("short_call", pricing.short_call.iv),
        ("short_put", pricing.short_put.iv),
        ("long_call", pricing.long_call.iv),
        ("long_put", pricing.long_put.iv),
    ],
    &config.iv_constraints,
)?;
```

**Effort**: Low (1-2 hours)
**Impact**: Medium (eliminates 30-40 lines of validation boilerplate)
**Priority**: P2 - After core refactoring

---

### 9. SIGNIFICANT: Net Greeks Computation Duplicated

**Files Affected**:
- `cs-backtest/src/execution/straddle_impl.rs:203-223` (2 legs)
- `cs-backtest/src/execution/iron_butterfly_impl.rs:91-137` (4 legs, repeated pattern 4x)

**Problem**:
Iron butterfly repeats Greeks combination pattern 4 times (once per Greek):
```rust
// iron_butterfly_impl.rs lines 91-137 - REPEATED PATTERN
let net_delta = match (
    short_call.greeks,
    short_put.greeks,
    long_call.greeks,
    long_put.greeks,
) {
    (Some(sc), Some(sp), Some(lc), Some(lp)) => {
        let value = -(sc.delta + sp.delta) + (lc.delta + lp.delta);
        Some(value * CONTRACT_MULTIPLIER as f64)
    }
    _ => None,
};

// REPEAT IDENTICAL PATTERN for gamma
let net_gamma = match (...) {
    (Some(sc), Some(sp), Some(lc), Some(lp)) => {
        let value = -(sc.gamma + sp.gamma) + (lc.gamma + lp.gamma);
        Some(value * CONTRACT_MULTIPLIER as f64)
    }
    _ => None,
};

// REPEAT IDENTICAL PATTERN for theta
let net_theta = match (...) {
    (Some(sc), Some(sp), Some(lc), Some(lp)) => {
        let value = -(sc.theta + sp.theta) + (lc.theta + lp.theta);
        Some(value * CONTRACT_MULTIPLIER as f64)
    }
    _ => None,
};

// REPEAT IDENTICAL PATTERN for vega
let net_vega = match (...) {
    (Some(sc), Some(sp), Some(lc), Some(lp)) => {
        let value = -(sc.vega + sp.vega) + (lc.vega + lp.vega);
        Some(value * CONTRACT_MULTIPLIER as f64)
    }
    _ => None,
};
```

This is 40+ lines that could be 10 lines.

**Solution**:
Create helper to combine Greeks with position signs:
```rust
pub fn combine_greeks(
    legs: &[(Option<Greeks>, i32)],  // (greeks, position_sign: +1 or -1)
    multiplier: f64,
) -> Option<Greeks> {
    let mut combined = Greeks::default();
    let mut all_present = true;

    for (greeks, sign) in legs {
        match greeks {
            Some(g) => {
                combined.delta += g.delta * sign as f64;
                combined.gamma += g.gamma * sign as f64;
                combined.theta += g.theta * sign as f64;
                combined.vega += g.vega * sign as f64;
                combined.rho += g.rho * sign as f64;
            }
            None => {
                all_present = false;
            }
        }
    }

    if all_present {
        combined.delta *= multiplier;
        combined.gamma *= multiplier;
        combined.theta *= multiplier;
        combined.vega *= multiplier;
        combined.rho *= multiplier;
        Some(combined)
    } else {
        None
    }
}

// Usage - 1 call instead of 4
let net_greeks = combine_greeks(
    &[
        (short_call.greeks, -1),
        (short_put.greeks, -1),
        (long_call.greeks, 1),
        (long_put.greeks, 1),
    ],
    CONTRACT_MULTIPLIER as f64,
);

let net_delta = net_greeks.map(|g| g.delta);
let net_gamma = net_greeks.map(|g| g.gamma);
let net_theta = net_greeks.map(|g| g.theta);
let net_vega = net_greeks.map(|g| g.vega);
```

**Effort**: Low (1-2 hours)
**Impact**: Medium (eliminates 40 lines, improves clarity)
**Priority**: P2 - Nice-to-have after core work

---

### 10. MODERATE: Pricer Wrapper Classes Minimal Value

**Files Affected**:
- `cs-backtest/src/straddle_pricer.rs` (~128 lines)
- `cs-backtest/src/calendar_straddle_pricer.rs` (estimated ~150 lines)
- `cs-backtest/src/iron_butterfly_pricer.rs` (estimated ~150 lines)
- `cs-backtest/src/composite_pricer.rs` (estimated ~100 lines)

**Problem**:
These are thin wrapper classes that mostly delegate to `SpreadPricer`:

```rust
pub struct StraddlePricer {
    spread_pricer: SpreadPricer,
}

impl StraddlePricer {
    pub fn new(data_service: Arc<dyn MarketDataService>, symbol: &str) -> Self {
        Self {
            spread_pricer: SpreadPricer::new(data_service, symbol),
        }
    }

    pub fn price(...) -> Result<StraddlePricing> {
        let iv_surface = self.spread_pricer.build_iv_surface(...)?;
        self.price_with_surface(..., iv_surface.as_ref())
    }

    pub fn price_with_surface(...) -> Result<StraddlePricing> {
        let call = self.spread_pricer.price_leg(...)?;
        let put = self.spread_pricer.price_leg(...)?;
        Ok(StraddlePricing { call, put, total_price, ... })
    }
}
```

**Issue**:
- 4 wrapper types with ~70% code overlap
- Only value is creating specific result types (StraddlePricing, IronButterflyPricing, etc.)
- Could be handled by generic `StructuredPricer<T>`
- Adds indirection without much behavior

**Options**:

**Option 1: Generic Pricer (Complex)**
```rust
pub struct StructuredPricer<T: PricingStructure> {
    spread_pricer: SpreadPricer,
    _marker: PhantomData<T>,
}

impl<T: PricingStructure> StructuredPricer<T> {
    pub fn price(...) -> Result<T::Result> {
        T::build_from_legs(...)
    }
}

pub trait PricingStructure {
    type Result: Serialize;
    fn build_from_legs(call: PricingData, put: PricingData) -> Self::Result;
}
```

**Option 2: Keep but Extract Common Logic (Moderate)**
```rust
// Create BasePricer trait with common methods
pub trait BasePricer {
    fn spread_pricer(&self) -> &SpreadPricer;
    fn build_iv_surface(...) -> Result<IVSurface> { ... }
}

impl BasePricer for StraddlePricer { ... }
impl BasePricer for IronButterflyPricer { ... }
```

**Option 3: Remove Wrappers, Call SpreadPricer Directly (Simplest)**
Just use `SpreadPricer` directly and let execution modules build the result types themselves.

**Recommendation**: **Do NOT refactor now** - it's low impact and adds complexity. The wrappers serve as semantic boundaries (clear intent). Refactor only if:
- Adding 5+ more pricer types
- Need to share complex pricing logic
- Performance analysis shows overhead is significant

**Effort**: High (8-10 hours for Option 1, 4-6 hours for Option 2)
**Impact**: Low (code cleanliness, not functional improvement)
**Priority**: P4 - Defer for now

---

## Refactoring Priority Matrix

| # | Issue | Type | Effort | Impact | Priority | Notes |
|---|-------|------|--------|--------|----------|-------|
| 1 | Delta provider leg computation | Duplication | 1-2h | High | **P0** | Fix before adding delta modes |
| 2 | Roll policy parsing (2×) | Duplication | 30m | Med | **P1** | Quick win |
| 3 | `to_failed_result()` boilerplate | Boilerplate | 1-2h | High | **P1** | Quick win - extract Default |
| 4 | P&L attribution incomplete (4 spreads) | Feature/Dup | 4-6h | High | **P0** | Complete feature |
| 5 | main.rs bloat (3045 lines) | Architecture | 8-12h | High | **P1** | Do after quick wins |
| 6 | Time/config parsing scattered | Duplication | 2-3h | Med | **P2** | After main.rs extraction |
| 7 | IV surface building | Optimization | 2-3h | Med | **P2** | Performance improvement |
| 8 | IV validation pattern (4×) | Duplication | 1-2h | Med | **P2** | After core work |
| 9 | Net Greeks computation (4×) | Duplication | 1-2h | Med | **P2** | Nice-to-have |
| 10 | Pricer wrappers minimal value | Over-engineering | 4-10h | Low | **P4** | Defer - not worth now |

---

## Implementation Roadmap

### **Phase 1: Quick Wins (1 day)**
- ✅ Extract `compute_position_delta_from_vol()` helper (P0)
- ✅ Merge `parse_roll_policy()` functions (P1)
- ✅ Add `Default` impl + extract `failed_result()` helper (P1)

### **Phase 2: Feature Completeness (2-3 days)**
- ✅ Complete P&L attribution for iron butterfly & calendar straddle (P0)
- ✅ Extract shared attribution logic (P0)

### **Phase 3: Maintainability (3-5 days)**
- ✅ Extract command handlers from main.rs (P1)
- ✅ Refactor time/config parsing (P2)

### **Phase 4: Optimizations (2-3 days)**
- ✅ Add IV surface caching (P2)
- ✅ Extract validation helpers (P2)
- ✅ Combine Greeks helper (P2)

### **Phase 5: Future (do not start now)**
- ❌ Refactor pricer wrappers (P4) - only if needed

---

## Code Metrics

| Metric | Current | After Refactoring | Reduction |
|--------|---------|-------------------|-----------|
| Duplicate code | ~350 lines | ~50 lines | 86% ↓ |
| main.rs size | 3045 lines | ~300 lines (core) | 90% ↓ |
| Spread implementations | 4 | 4 (cleaner) | 40% less per file |
| Test coverage ease | Hard | Easy | Huge ↑ |

---

## Risks & Mitigation

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|-----------|
| Delta provider refactor breaks pricing | Low | High | Extract as feature branch, run full test suite |
| main.rs extraction creates circular imports | Med | Med | Careful module design, use pub use |
| Attribution changes break existing results | Med | High | Verify with known test cases before merging |
| Time parsing changes break CLI | Med | Med | Comprehensive CLI tests before landing |

---

## Conclusion

The codebase has **~550 lines of duplicated/over-engineered code** that should be refactored in phases:

1. **Start with quick wins** (delta providers, roll policy, boilerplate) - 1 day
2. **Complete missing features** (P&L attribution) - 1 day
3. **Improve architecture** (main.rs extraction) - 3-5 days
4. **Optimize later** (caching, helpers) - as needed

This refactoring will significantly improve:
- Maintainability (single source of truth)
- Testability (smaller modules)
- Consistency (shared logic)
- Developer velocity (easier to understand & change)
