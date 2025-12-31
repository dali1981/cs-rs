# Calendar Spread Backtest - Rust Rewrite Plan

## Overview

Full rewrite of `calendar-spread-backtest` Python codebase to Rust as a **separate workspace** (`cs-rs`), consuming `finq-rs` as an external dependency.

**Timeline**: 18-25 weeks
**Target**: 5x backtest performance, single-language codebase
**Dependency**: finq-rs SDK completion (~2-4 weeks)

---

## Architecture: Separate Workspaces

```
~/finq-rs/                        # Data layer (reusable)
├── Cargo.toml (workspace)
├── crates/
│   ├── finq-core/                # ✅ Bar, Quote, Trade, IVPoint
│   ├── finq-flatfiles/           # ✅ Parquet readers
│   ├── finq-rest/                # ✅ REST snapshots
│   ├── finq-websocket/           # ✅ Streaming
│   ├── finq-sdk/                 # 🚧 Unified client
│   └── finq-cli/                 # 🚧 CLI

~/cs-rs/                          # Calendar spread backtest (NEW)
├── Cargo.toml                    # workspace
├── cs-analytics/                 # Black-Scholes, Greeks, IV surface
│   ├── Cargo.toml
│   └── src/
├── cs-domain/                    # CalendarSpread, Strategies, Repositories
│   ├── Cargo.toml
│   └── src/
├── cs-backtest/                  # BacktestUseCase, TradeExecutor
│   ├── Cargo.toml
│   └── src/
├── cs-python/                    # PyO3 Python bindings
│   ├── Cargo.toml
│   └── src/
└── cs-cli/                       # CLI application
    ├── Cargo.toml
    └── src/
```

**Why separate?**
- `finq-rs` = pure data layer, reusable for any trading strategy
- `cs-rs` = specific calendar spread strategy implementation
- Different release cycles and versioning
- Clean dependency graph (cs-rs → finq-rs, not vice versa)
- finq-rs can be published to crates.io independently

---

## finq-rs Status (External Dependency)

| Crate | Status | Provides |
|-------|--------|----------|
| `finq-core` | ✅ 100% | Bar, Quote, Trade, IVPoint, HVPoint, OptionType |
| `finq-flatfiles` | ✅ 100% | StockBarRepository, OptionBarRepository, IVSurfaceRepository |
| `finq-rest` | ✅ 100% | Stock/Options/Forex/Crypto snapshots, option chain |
| `finq-websocket` | ✅ 100% | Trade/Quote/Aggregate streaming, auto-reconnect |
| `finq-sdk` | 🚧 0% | Unified client (ETA: 2-3 weeks) |
| `finq-cli` | 🚧 0% | CLI (ETA: 1-2 weeks) |

---

## cs-rs Workspace Setup

### Root Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "cs-analytics",
    "cs-domain",
    "cs-backtest",
    "cs-python",
    "cs-cli",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.75"
license = "MIT"
authors = ["Mohamed Ali"]

[workspace.dependencies]
# Internal crates
cs-analytics = { path = "cs-analytics" }
cs-domain = { path = "cs-domain" }
cs-backtest = { path = "cs-backtest" }

# External: finq-rs (path for dev, version for release)
finq-core = { path = "../finq-rs/crates/finq-core" }
finq-flatfiles = { path = "../finq-rs/crates/finq-flatfiles" }
finq-rest = { path = "../finq-rs/crates/finq-rest" }
finq-websocket = { path = "../finq-rs/crates/finq-websocket" }
finq-sdk = { path = "../finq-rs/crates/finq-sdk" }

# Async runtime
tokio = { version = "1.35", features = ["rt-multi-thread", "macros", "sync", "time"] }
tokio-stream = "0.1"
futures = "0.3"
async-trait = "0.1"

# Data processing (match finq-rs versions)
polars = { version = "0.41", features = ["parquet", "lazy", "temporal"] }
arrow = "53"
parquet = { version = "53", features = ["async"] }

# Math & numerics
rust_decimal = { version = "1.33", features = ["serde", "serde-with-str"] }
statrs = "0.16"
roots = "0.0.8"

# Time handling
chrono = { version = "0.4", features = ["serde"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Error handling
thiserror = "1.0"
anyhow = "1.0"

# Parallelism
rayon = "1.8"

# CLI
clap = { version = "4.4", features = ["derive", "env"] }
tabled = "0.15"
indicatif = "0.17"
console = "0.15"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# Python bindings
pyo3 = { version = "0.20", features = ["extension-module"] }

# Testing
tokio-test = "0.4"
criterion = "0.5"
approx = "0.5"

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = true
```

---

## Phase 1: Analytics Core (Weeks 1-3)

### Crate: `cs-analytics`

Pure computational functions with no I/O. No dependency on finq-rs.

**Directory structure:**
```
cs-analytics/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── black_scholes.rs
│   ├── greeks.rs
│   ├── iv_surface.rs
│   ├── historical_iv.rs
│   └── price_interpolation.rs
└── benches/
    └── black_scholes.rs
```

#### 1.1 Cargo.toml

```toml
[package]
name = "cs-analytics"
version.workspace = true
edition.workspace = true

[dependencies]
rust_decimal = { workspace = true }
chrono = { workspace = true }
polars = { workspace = true }
statrs = { workspace = true }
roots = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
criterion = { workspace = true }
approx = { workspace = true }

[[bench]]
name = "black_scholes"
harness = false
```

#### 1.2 Black-Scholes Module

**File**: `src/black_scholes.rs`

```rust
use statrs::distribution::{ContinuousCDF, Normal};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BSError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("IV solver failed to converge")]
    ConvergenceFailure,
}

/// Configuration for Black-Scholes calculations
#[derive(Debug, Clone, Copy)]
pub struct BSConfig {
    pub risk_free_rate: f64,
    pub min_iv: f64,
    pub max_iv: f64,
    pub tolerance: f64,
    pub max_iterations: usize,
}

impl Default for BSConfig {
    fn default() -> Self {
        Self {
            risk_free_rate: 0.05,
            min_iv: 0.0001,
            max_iv: 5.0,
            tolerance: 1e-6,
            max_iterations: 100,
        }
    }
}

/// Calculate option price using Black-Scholes formula
#[inline]
pub fn bs_price(
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    volatility: f64,
    is_call: bool,
    risk_free_rate: f64,
) -> f64 {
    if time_to_expiry <= 0.0 || volatility <= 0.0 {
        return if is_call {
            (spot - strike).max(0.0)
        } else {
            (strike - spot).max(0.0)
        };
    }

    let sqrt_t = time_to_expiry.sqrt();
    let d1 = ((spot / strike).ln() + (risk_free_rate + 0.5 * volatility.powi(2)) * time_to_expiry)
        / (volatility * sqrt_t);
    let d2 = d1 - volatility * sqrt_t;

    let norm = Normal::new(0.0, 1.0).unwrap();
    let discount = (-risk_free_rate * time_to_expiry).exp();

    if is_call {
        spot * norm.cdf(d1) - strike * discount * norm.cdf(d2)
    } else {
        strike * discount * norm.cdf(-d2) - spot * norm.cdf(-d1)
    }
}

/// Calculate implied volatility using Brent's method
pub fn bs_implied_volatility(
    option_price: f64,
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    is_call: bool,
    config: &BSConfig,
) -> Option<f64> {
    if option_price <= 0.0 || spot <= 0.0 || strike <= 0.0 || time_to_expiry <= 0.0 {
        return None;
    }

    // Check arbitrage bounds
    let discount = (-config.risk_free_rate * time_to_expiry).exp();
    let (intrinsic, max_price) = if is_call {
        ((spot - strike * discount).max(0.0), spot)
    } else {
        ((strike * discount - spot).max(0.0), strike * discount)
    };

    if option_price < intrinsic || option_price > max_price {
        return None;
    }

    // Objective function for root finding
    let objective = |sigma: f64| -> f64 {
        bs_price(spot, strike, time_to_expiry, sigma, is_call, config.risk_free_rate) - option_price
    };

    // Brent's method
    match roots::find_root_brent(config.min_iv, config.max_iv, objective, &mut config.tolerance.clone()) {
        Ok(iv) => Some(iv),
        Err(_) => None,
    }
}

/// Calculate all Greeks efficiently in one pass
pub fn bs_greeks(
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    volatility: f64,
    is_call: bool,
    risk_free_rate: f64,
) -> Greeks {
    if time_to_expiry <= 0.0 || volatility <= 0.0 {
        return Greeks::at_expiry(spot, strike, is_call);
    }

    let sqrt_t = time_to_expiry.sqrt();
    let d1 = ((spot / strike).ln() + (risk_free_rate + 0.5 * volatility.powi(2)) * time_to_expiry)
        / (volatility * sqrt_t);
    let d2 = d1 - volatility * sqrt_t;

    let norm = Normal::new(0.0, 1.0).unwrap();
    let n_d1 = norm.cdf(d1);
    let n_d2 = norm.cdf(d2);
    let n_prime_d1 = (-0.5 * d1.powi(2)).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let discount = (-risk_free_rate * time_to_expiry).exp();

    let delta = if is_call { n_d1 } else { n_d1 - 1.0 };
    let gamma = n_prime_d1 / (spot * volatility * sqrt_t);
    let vega = spot * n_prime_d1 * sqrt_t * 0.01; // Per 1% vol change

    let theta = if is_call {
        (-spot * n_prime_d1 * volatility / (2.0 * sqrt_t)
            - risk_free_rate * strike * discount * n_d2)
            / 365.0
    } else {
        (-spot * n_prime_d1 * volatility / (2.0 * sqrt_t)
            + risk_free_rate * strike * discount * norm.cdf(-d2))
            / 365.0
    };

    let rho = if is_call {
        strike * time_to_expiry * discount * n_d2 * 0.01
    } else {
        -strike * time_to_expiry * discount * norm.cdf(-d2) * 0.01
    };

    Greeks { delta, gamma, theta, vega, rho }
}
```

#### 1.3 Greeks Value Object

**File**: `src/greeks.rs`

```rust
use std::ops::{Add, Sub, Mul, Neg};

/// Option Greeks - immutable value object
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Greeks {
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,  // Per day
    pub vega: f64,   // Per 1% vol change
    pub rho: f64,    // Per 1% rate change
}

impl Greeks {
    pub const ZERO: Greeks = Greeks {
        delta: 0.0,
        gamma: 0.0,
        theta: 0.0,
        vega: 0.0,
        rho: 0.0,
    };

    /// Greeks at expiry (delta only)
    pub fn at_expiry(spot: f64, strike: f64, is_call: bool) -> Self {
        let delta = if is_call {
            if spot > strike { 1.0 } else { 0.0 }
        } else {
            if spot < strike { -1.0 } else { 0.0 }
        };
        Self { delta, ..Self::ZERO }
    }

    /// Spread Greeks = long - short
    pub fn spread(long: &Greeks, short: &Greeks) -> Greeks {
        *long - *short
    }

    /// Position Greeks = greeks * signed_quantity
    pub fn position(&self, quantity: i32) -> Greeks {
        *self * (quantity as f64)
    }
}

impl Add for Greeks {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            delta: self.delta + other.delta,
            gamma: self.gamma + other.gamma,
            theta: self.theta + other.theta,
            vega: self.vega + other.vega,
            rho: self.rho + other.rho,
        }
    }
}

impl Sub for Greeks {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self {
            delta: self.delta - other.delta,
            gamma: self.gamma - other.gamma,
            theta: self.theta - other.theta,
            vega: self.vega - other.vega,
            rho: self.rho - other.rho,
        }
    }
}

impl Mul<f64> for Greeks {
    type Output = Self;
    fn mul(self, scalar: f64) -> Self {
        Self {
            delta: self.delta * scalar,
            gamma: self.gamma * scalar,
            theta: self.theta * scalar,
            vega: self.vega * scalar,
            rho: self.rho * scalar,
        }
    }
}

impl Neg for Greeks {
    type Output = Self;
    fn neg(self) -> Self {
        self * -1.0
    }
}
```

#### 1.4 IV Surface Module

**File**: `src/iv_surface.rs`

```rust
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

/// Single IV observation
#[derive(Debug, Clone)]
pub struct IVPoint {
    pub strike: Decimal,
    pub expiration: NaiveDate,
    pub iv: f64,
    pub timestamp: DateTime<Utc>,
    pub underlying_price: Decimal,
    pub is_call: bool,
    pub contract_ticker: String,
}

impl IVPoint {
    pub fn moneyness(&self) -> f64 {
        if self.underlying_price.is_zero() {
            return 1.0;
        }
        (self.strike / self.underlying_price).try_into().unwrap_or(1.0)
    }

    pub fn is_atm(&self, tolerance: f64) -> bool {
        (self.moneyness() - 1.0).abs() <= tolerance
    }
}

/// Implied volatility surface: σ(K, T)
#[derive(Debug, Clone)]
pub struct IVSurface {
    points: Vec<IVPoint>,
    underlying: String,
    as_of_time: DateTime<Utc>,
    spot_price: Decimal,
}

impl IVSurface {
    pub fn new(
        points: Vec<IVPoint>,
        underlying: String,
        as_of_time: DateTime<Utc>,
        spot_price: Decimal,
    ) -> Self {
        Self { points, underlying, as_of_time, spot_price }
    }

    pub fn underlying(&self) -> &str { &self.underlying }
    pub fn as_of_time(&self) -> DateTime<Utc> { self.as_of_time }
    pub fn spot_price(&self) -> Decimal { self.spot_price }
    pub fn points(&self) -> &[IVPoint] { &self.points }

    /// Interpolate IV for given strike/expiration
    pub fn get_iv(
        &self,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        let matching: Vec<_> = self.points.iter()
            .filter(|p| p.is_call == is_call)
            .collect();

        if matching.is_empty() {
            return None;
        }

        // Group by expiration
        let mut by_expiry: BTreeMap<NaiveDate, Vec<&IVPoint>> = BTreeMap::new();
        for p in &matching {
            by_expiry.entry(p.expiration).or_default().push(p);
        }

        // Try exact expiration first
        if let Some(points) = by_expiry.get(&expiration) {
            if let Some(iv) = self.interpolate_strike(points, strike) {
                return Some(iv);
            }
        }

        // Interpolate across expirations
        self.interpolate_expiration(&by_expiry, strike, expiration)
    }

    /// Get IV at moneyness and TTM
    pub fn get_iv_by_moneyness_ttm(
        &self,
        moneyness: f64,
        ttm_days: i32,
        is_call: bool,
    ) -> Option<f64> {
        let strike_f64: f64 = self.spot_price.try_into().unwrap_or(0.0);
        let strike = Decimal::try_from(strike_f64 * moneyness).ok()?;
        let target_expiry = self.as_of_time.date_naive() + chrono::Duration::days(ttm_days as i64);
        self.get_iv(strike, target_expiry, is_call)
    }

    /// Get ATM term structure
    pub fn get_atm_term_structure(&self, is_call: bool) -> BTreeMap<NaiveDate, f64> {
        let mut result = BTreeMap::new();

        let matching: Vec<_> = self.points.iter()
            .filter(|p| p.is_call == is_call)
            .collect();

        let mut by_expiry: BTreeMap<NaiveDate, Vec<&IVPoint>> = BTreeMap::new();
        for p in &matching {
            by_expiry.entry(p.expiration).or_default().push(p);
        }

        for (exp, points) in by_expiry {
            if let Some(iv) = self.interpolate_strike(&points, self.spot_price) {
                result.insert(exp, iv);
            }
        }

        result
    }

    fn interpolate_strike(&self, points: &[&IVPoint], target_strike: Decimal) -> Option<f64> {
        if points.is_empty() {
            return None;
        }

        let mut sorted: Vec<_> = points.iter().collect();
        sorted.sort_by_key(|p| p.strike);

        // Exact match
        if let Some(p) = sorted.iter().find(|p| p.strike == target_strike) {
            return Some(p.iv);
        }

        // Find bracketing strikes
        let mut lower: Option<&IVPoint> = None;
        let mut upper: Option<&IVPoint> = None;

        for p in sorted {
            if p.strike < target_strike {
                lower = Some(p);
            } else if p.strike > target_strike && upper.is_none() {
                upper = Some(p);
                break;
            }
        }

        match (lower, upper) {
            (Some(l), Some(u)) => {
                let range: f64 = (u.strike - l.strike).try_into().unwrap_or(1.0);
                if range == 0.0 { return Some(l.iv); }
                let weight: f64 = ((target_strike - l.strike) / (u.strike - l.strike))
                    .try_into().unwrap_or(0.5);
                Some(l.iv + weight * (u.iv - l.iv))
            }
            (Some(l), None) => Some(l.iv),
            (None, Some(u)) => Some(u.iv),
            (None, None) => None,
        }
    }

    fn interpolate_expiration(
        &self,
        by_expiry: &BTreeMap<NaiveDate, Vec<&IVPoint>>,
        target_strike: Decimal,
        target_expiration: NaiveDate,
    ) -> Option<f64> {
        // Get IV at target strike for each expiration
        let mut expiry_ivs: Vec<(NaiveDate, f64)> = Vec::new();
        for (exp, points) in by_expiry {
            if let Some(iv) = self.interpolate_strike(points, target_strike) {
                expiry_ivs.push((*exp, iv));
            }
        }

        if expiry_ivs.is_empty() {
            return None;
        }

        expiry_ivs.sort_by_key(|(exp, _)| *exp);

        // Find bracketing expirations
        let mut lower: Option<(NaiveDate, f64)> = None;
        let mut upper: Option<(NaiveDate, f64)> = None;

        for (exp, iv) in &expiry_ivs {
            if *exp < target_expiration {
                lower = Some((*exp, *iv));
            } else if *exp > target_expiration && upper.is_none() {
                upper = Some((*exp, *iv));
                break;
            } else if *exp == target_expiration {
                return Some(*iv);
            }
        }

        match (lower, upper) {
            (Some((l_exp, l_iv)), Some((u_exp, u_iv))) => {
                // sqrt(time) weighted interpolation
                let as_of = self.as_of_time.date_naive();
                let sqrt_time = |exp: NaiveDate| -> f64 {
                    ((exp - as_of).num_days().max(1) as f64 / 365.0).sqrt()
                };

                let sqrt_lower = sqrt_time(l_exp);
                let sqrt_upper = sqrt_time(u_exp);
                let sqrt_target = sqrt_time(target_expiration);

                let range = sqrt_upper - sqrt_lower;
                if range == 0.0 { return Some(l_iv); }

                let weight = (sqrt_target - sqrt_lower) / range;
                Some(l_iv + weight * (u_iv - l_iv))
            }
            (Some((_, iv)), None) => Some(iv),
            (None, Some((_, iv))) => Some(iv),
            (None, None) => None,
        }
    }
}
```

#### 1.5 Historical IV Module

**File**: `src/historical_iv.rs`

```rust
/// Calculate IV percentile over lookback period
pub fn iv_percentile(current_iv: f64, historical_ivs: &[f64]) -> f64 {
    if historical_ivs.is_empty() {
        return 50.0;
    }

    let count_below = historical_ivs.iter().filter(|&&iv| iv < current_iv).count();
    (count_below as f64 / historical_ivs.len() as f64) * 100.0
}

/// Calculate IV rank (position in range)
pub fn iv_rank(current_iv: f64, historical_ivs: &[f64]) -> f64 {
    if historical_ivs.is_empty() {
        return 50.0;
    }

    let min = historical_ivs.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = historical_ivs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    if (max - min).abs() < 1e-10 {
        return 50.0;
    }

    ((current_iv - min) / (max - min)) * 100.0
}

/// Calculate realized volatility from price returns
pub fn realized_volatility(
    prices: &[f64],
    window: usize,
    annualization_factor: f64,
) -> Option<f64> {
    if prices.len() < window + 1 {
        return None;
    }

    // Calculate log returns
    let returns: Vec<f64> = prices.windows(2)
        .map(|w| (w[1] / w[0]).ln())
        .collect();

    if returns.len() < window {
        return None;
    }

    // Take last `window` returns
    let recent_returns = &returns[returns.len() - window..];

    // Calculate standard deviation
    let mean = recent_returns.iter().sum::<f64>() / window as f64;
    let variance = recent_returns.iter()
        .map(|r| (r - mean).powi(2))
        .sum::<f64>() / (window - 1) as f64;

    Some(variance.sqrt() * annualization_factor.sqrt())
}
```

#### 1.6 lib.rs

**File**: `src/lib.rs`

```rust
pub mod black_scholes;
pub mod greeks;
pub mod iv_surface;
pub mod historical_iv;
pub mod price_interpolation;

pub use black_scholes::{bs_price, bs_implied_volatility, bs_greeks, BSConfig, BSError};
pub use greeks::Greeks;
pub use iv_surface::{IVSurface, IVPoint};
pub use historical_iv::{iv_percentile, iv_rank, realized_volatility};
```

#### Phase 1 Deliverables

- [ ] `bs_price()` - Option pricing
- [ ] `bs_implied_volatility()` - IV solver (Brent)
- [ ] `bs_greeks()` - All Greeks in one pass
- [ ] `Greeks` value object with arithmetic
- [ ] `IVSurface` with strike/expiry interpolation
- [ ] `iv_percentile()`, `iv_rank()`, `realized_volatility()`
- [ ] Benchmark suite (criterion)
- [ ] Unit tests with known values

---

## Phase 2: Domain Models (Weeks 4-7)

### Crate: `cs-domain`

Core business logic. Depends on `cs-analytics` and `finq-core`.

**Directory structure:**
```
cs-domain/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── value_objects.rs
│   ├── entities.rs
│   ├── trading_session.rs
│   ├── strategies/
│   │   ├── mod.rs
│   │   ├── atm.rs
│   │   ├── delta.rs
│   │   ├── liquidity.rs
│   │   └── iv_ratio.rs
│   ├── repositories.rs
│   └── services/
│       ├── mod.rs
│       ├── spread_pricer.rs
│       ├── pnl_calculator.rs
│       └── trading_calendar.rs
```

#### 2.1 Cargo.toml

```toml
[package]
name = "cs-domain"
version.workspace = true
edition.workspace = true

[dependencies]
cs-analytics = { workspace = true }
finq-core = { workspace = true }
rust_decimal = { workspace = true }
chrono = { workspace = true }
polars = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
```

#### 2.2 Value Objects

**File**: `src/value_objects.rs`

```rust
use chrono::{NaiveDate, NaiveTime, DateTime, Utc};
use rust_decimal::Decimal;
use finq_core::OptionType;
use serde::{Serialize, Deserialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Strike must be positive, got {0}")]
    InvalidStrike(Decimal),
    #[error("Expiration mismatch: short {short} must be before long {long}")]
    ExpirationMismatch { short: NaiveDate, long: NaiveDate },
    #[error("Symbol mismatch: {0} != {1}")]
    SymbolMismatch(String, String),
}

/// Strike price (validated positive)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Strike(Decimal);

impl Strike {
    pub fn new(value: Decimal) -> Result<Self, ValidationError> {
        if value <= Decimal::ZERO {
            return Err(ValidationError::InvalidStrike(value));
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> Decimal { self.0 }
}

impl From<Strike> for f64 {
    fn from(s: Strike) -> f64 {
        s.0.try_into().unwrap_or(0.0)
    }
}

/// Timing configuration for trade entry/exit
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TimingConfig {
    pub entry_hour: u32,
    pub entry_minute: u32,
    pub exit_hour: u32,
    pub exit_minute: u32,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            entry_hour: 9,
            entry_minute: 35,
            exit_hour: 15,
            exit_minute: 55,
        }
    }
}

impl TimingConfig {
    pub fn entry_time(&self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.entry_hour, self.entry_minute, 0).unwrap()
    }

    pub fn exit_time(&self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.exit_hour, self.exit_minute, 0).unwrap()
    }

    pub fn entry_datetime(&self, date: NaiveDate) -> DateTime<Utc> {
        date.and_time(self.entry_time()).and_utc()
    }

    pub fn exit_datetime(&self, date: NaiveDate) -> DateTime<Utc> {
        date.and_time(self.exit_time()).and_utc()
    }
}

/// Earnings announcement timing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EarningsTime {
    BeforeMarketOpen,
    AfterMarketClose,
    Unknown,
}

/// Spot price with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotPrice {
    pub value: Decimal,
    pub timestamp: DateTime<Utc>,
}

/// Trade failure reasons
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureReason {
    NoSpotPrice,
    NoOptionsData,
    DegenerateSpread,
    InsufficientExpirations,
    IVRatioFilter,
    PricingError(String),
}
```

#### 2.3 Entities

**File**: `src/entities.rs`

```rust
use chrono::{NaiveDate, DateTime, Utc};
use rust_decimal::Decimal;
use finq_core::OptionType;
use serde::{Serialize, Deserialize};

use crate::value_objects::*;
use cs_analytics::Greeks;

/// Earnings event for a symbol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarningsEvent {
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub company_name: Option<String>,
    pub eps_forecast: Option<Decimal>,
    pub market_cap: Option<u64>,
}

/// Single option leg
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionLeg {
    pub symbol: String,
    pub strike: Strike,
    pub expiration: NaiveDate,
    pub option_type: OptionType,
}

impl OptionLeg {
    pub fn new(
        symbol: String,
        strike: Strike,
        expiration: NaiveDate,
        option_type: OptionType,
    ) -> Self {
        Self { symbol, strike, expiration, option_type }
    }

    /// Generate OCC ticker (e.g., "O:AAPL250117C00180000")
    pub fn occ_ticker(&self) -> String {
        let opt_char = match self.option_type {
            OptionType::Call => 'C',
            OptionType::Put => 'P',
        };
        let strike_int = (self.strike.value() * Decimal::from(1000))
            .to_string()
            .parse::<u64>()
            .unwrap_or(0);
        format!(
            "O:{}{}{}{}",
            self.symbol,
            self.expiration.format("%y%m%d"),
            opt_char,
            format!("{:08}", strike_int)
        )
    }

    /// Days to expiry from given date
    pub fn dte(&self, from: NaiveDate) -> i32 {
        (self.expiration - from).num_days() as i32
    }
}

/// Calendar spread = short near-term + long far-term
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSpread {
    pub short_leg: OptionLeg,
    pub long_leg: OptionLeg,
}

impl CalendarSpread {
    pub fn new(short: OptionLeg, long: OptionLeg) -> Result<Self, ValidationError> {
        if short.symbol != long.symbol {
            return Err(ValidationError::SymbolMismatch(
                short.symbol.clone(),
                long.symbol.clone(),
            ));
        }
        if short.expiration >= long.expiration {
            return Err(ValidationError::ExpirationMismatch {
                short: short.expiration,
                long: long.expiration,
            });
        }
        Ok(Self { short_leg: short, long_leg: long })
    }

    pub fn symbol(&self) -> &str { &self.short_leg.symbol }
    pub fn strike(&self) -> Strike { self.short_leg.strike }
    pub fn option_type(&self) -> OptionType { self.short_leg.option_type }
    pub fn short_expiry(&self) -> NaiveDate { self.short_leg.expiration }
    pub fn long_expiry(&self) -> NaiveDate { self.long_leg.expiration }
}

/// Trade opportunity generated by strategy
#[derive(Debug, Clone)]
pub struct TradeOpportunity {
    pub spread: CalendarSpread,
    pub earnings_event: EarningsEvent,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub spot_price_at_selection: SpotPrice,
}

/// Completed trade result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSpreadResult {
    // Identification
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub strike: Strike,
    pub option_type: OptionType,
    pub short_expiry: NaiveDate,
    pub long_expiry: NaiveDate,

    // Entry
    pub entry_time: DateTime<Utc>,
    pub short_entry_price: Decimal,
    pub long_entry_price: Decimal,
    pub entry_cost: Decimal,

    // Exit
    pub exit_time: DateTime<Utc>,
    pub short_exit_price: Decimal,
    pub long_exit_price: Decimal,
    pub exit_value: Decimal,

    // P&L
    pub pnl: Decimal,
    pub pnl_per_contract: Decimal,
    pub pnl_pct: Decimal,

    // Greeks at entry
    pub short_delta: Option<f64>,
    pub short_gamma: Option<f64>,
    pub short_theta: Option<f64>,
    pub short_vega: Option<f64>,
    pub long_delta: Option<f64>,
    pub long_gamma: Option<f64>,
    pub long_theta: Option<f64>,
    pub long_vega: Option<f64>,

    // IV at entry/exit
    pub iv_short_entry: Option<f64>,
    pub iv_long_entry: Option<f64>,
    pub iv_short_exit: Option<f64>,
    pub iv_long_exit: Option<f64>,

    // P&L Attribution
    pub delta_pnl: Option<Decimal>,
    pub gamma_pnl: Option<Decimal>,
    pub theta_pnl: Option<Decimal>,
    pub vega_pnl: Option<Decimal>,
    pub unexplained_pnl: Option<Decimal>,

    // Spot prices
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,

    // Status
    pub success: bool,
    pub failure_reason: Option<FailureReason>,
}

impl CalendarSpreadResult {
    pub fn iv_ratio(&self) -> Option<f64> {
        match (self.iv_short_entry, self.iv_long_entry) {
            (Some(short), Some(long)) if long > 0.0 => Some(short / long),
            _ => None,
        }
    }

    pub fn is_winner(&self) -> bool {
        self.success && self.pnl > Decimal::ZERO
    }
}
```

#### 2.4 Trading Strategies

**File**: `src/strategies/mod.rs`

```rust
pub mod atm;
pub mod delta;
pub mod liquidity;
pub mod iv_ratio;

use crate::entities::*;
use crate::value_objects::*;
use thiserror::Error;

pub use atm::ATMStrategy;
pub use delta::DeltaStrategy;
pub use liquidity::LiquidityStrategy;
pub use iv_ratio::IVRatioStrategy;

#[derive(Error, Debug)]
pub enum StrategyError {
    #[error("No strikes available")]
    NoStrikes,
    #[error("No expirations available")]
    NoExpirations,
    #[error("Insufficient expirations: need {needed}, have {available}")]
    InsufficientExpirations { needed: usize, available: usize },
    #[error("No delta data available")]
    NoDeltaData,
    #[error("No liquidity data available")]
    NoLiquidityData,
    #[error("Spread creation failed: {0}")]
    SpreadCreation(#[from] ValidationError),
}

/// Trade selection criteria
#[derive(Debug, Clone, Default)]
pub struct TradeSelectionCriteria {
    pub min_short_dte: i32,
    pub min_long_dte: i32,
    pub target_delta: Option<f64>,
    pub min_iv_ratio: Option<f64>,
    pub max_bid_ask_spread_pct: Option<f64>,
}

/// Option chain data for strategy selection
pub struct OptionChainData {
    pub expirations: Vec<NaiveDate>,
    pub strikes: Vec<Strike>,
    pub deltas: Option<Vec<(Strike, f64)>>,
    pub volumes: Option<Vec<(Strike, u64)>>,
    pub iv_ratios: Option<Vec<(Strike, f64)>>,
}

/// Strategy trait for strike selection
pub trait TradingStrategy: Send + Sync {
    fn select(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
        option_type: finq_core::OptionType,
    ) -> Result<CalendarSpread, StrategyError>;
}
```

**File**: `src/strategies/atm.rs`

```rust
use super::*;
use chrono::NaiveDate;

/// ATM strategy - select strike closest to spot
pub struct ATMStrategy {
    pub criteria: TradeSelectionCriteria,
}

impl Default for ATMStrategy {
    fn default() -> Self {
        Self {
            criteria: TradeSelectionCriteria {
                min_short_dte: 1,
                min_long_dte: 7,
                ..Default::default()
            },
        }
    }
}

impl TradingStrategy for ATMStrategy {
    fn select(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
        option_type: finq_core::OptionType,
    ) -> Result<CalendarSpread, StrategyError> {
        if chain_data.strikes.is_empty() {
            return Err(StrategyError::NoStrikes);
        }

        // Find ATM strike
        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let atm_strike = chain_data.strikes
            .iter()
            .min_by(|a, b| {
                let a_diff = (f64::from(**a) - spot_f64).abs();
                let b_diff = (f64::from(**b) - spot_f64).abs();
                a_diff.partial_cmp(&b_diff).unwrap()
            })
            .ok_or(StrategyError::NoStrikes)?;

        // Select expirations
        let (short_exp, long_exp) = select_expirations(
            &chain_data.expirations,
            event.earnings_date,
            self.criteria.min_short_dte,
            self.criteria.min_long_dte,
        )?;

        let short_leg = OptionLeg::new(
            event.symbol.clone(),
            *atm_strike,
            short_exp,
            option_type,
        );
        let long_leg = OptionLeg::new(
            event.symbol.clone(),
            *atm_strike,
            long_exp,
            option_type,
        );

        CalendarSpread::new(short_leg, long_leg).map_err(Into::into)
    }
}

fn select_expirations(
    expirations: &[NaiveDate],
    reference_date: NaiveDate,
    min_short_dte: i32,
    min_long_dte: i32,
) -> Result<(NaiveDate, NaiveDate), StrategyError> {
    if expirations.len() < 2 {
        return Err(StrategyError::InsufficientExpirations {
            needed: 2,
            available: expirations.len(),
        });
    }

    let mut sorted: Vec<_> = expirations.iter().collect();
    sorted.sort();

    // Find short expiry (first one meeting min DTE)
    let short_exp = sorted
        .iter()
        .find(|&&exp| (*exp - reference_date).num_days() >= min_short_dte as i64)
        .ok_or(StrategyError::NoExpirations)?;

    // Find long expiry (first one after short meeting min DTE gap)
    let long_exp = sorted
        .iter()
        .find(|&&exp| {
            exp > short_exp && (*exp - reference_date).num_days() >= min_long_dte as i64
        })
        .ok_or(StrategyError::InsufficientExpirations {
            needed: 2,
            available: 1,
        })?;

    Ok((**short_exp, **long_exp))
}
```

#### 2.5 Repositories (Traits)

**File**: `src/repositories.rs`

```rust
use async_trait::async_trait;
use chrono::{NaiveDate, DateTime, Utc};
use thiserror::Error;

use crate::entities::*;
use crate::value_objects::*;

#[derive(Error, Debug)]
pub enum RepositoryError {
    #[error("Data not found: {0}")]
    NotFound(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Polars error: {0}")]
    Polars(String),
}

/// Earnings data repository
#[async_trait]
pub trait EarningsRepository: Send + Sync {
    async fn load_earnings(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        symbols: Option<&[String]>,
    ) -> Result<Vec<EarningsEvent>, RepositoryError>;
}

/// Options data repository
#[async_trait]
pub trait OptionsDataRepository: Send + Sync {
    async fn get_option_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<polars::frame::DataFrame, RepositoryError>;

    async fn get_available_expirations(
        &self,
        underlying: &str,
        as_of_date: NaiveDate,
    ) -> Result<Vec<NaiveDate>, RepositoryError>;

    async fn get_available_strikes(
        &self,
        underlying: &str,
        expiration: NaiveDate,
        as_of_date: NaiveDate,
    ) -> Result<Vec<Strike>, RepositoryError>;
}

/// Equity data repository
#[async_trait]
pub trait EquityDataRepository: Send + Sync {
    async fn get_spot_price(
        &self,
        symbol: &str,
        target_time: DateTime<Utc>,
    ) -> Result<SpotPrice, RepositoryError>;

    async fn get_bars(
        &self,
        symbol: &str,
        date: NaiveDate,
    ) -> Result<polars::frame::DataFrame, RepositoryError>;
}

/// Results persistence repository
#[async_trait]
pub trait ResultsRepository: Send + Sync {
    async fn save_results(
        &self,
        results: &[CalendarSpreadResult],
        run_id: &str,
    ) -> Result<(), RepositoryError>;

    async fn load_results(
        &self,
        run_id: &str,
    ) -> Result<Vec<CalendarSpreadResult>, RepositoryError>;
}
```

#### 2.6 Domain Services

**File**: `src/services/pnl_calculator.rs`

```rust
use rust_decimal::Decimal;
use cs_analytics::Greeks;

/// P&L attribution breakdown
#[derive(Debug, Clone, Default)]
pub struct PnLAttribution {
    pub total: Decimal,
    pub delta: Decimal,
    pub gamma: Decimal,
    pub theta: Decimal,
    pub vega: Decimal,
    pub unexplained: Decimal,
}

/// Calculate P&L attribution from Greeks
pub fn calculate_pnl_attribution(
    entry_greeks: &Greeks,
    spot_change: f64,
    iv_change: f64,
    days_held: f64,
    total_pnl: Decimal,
) -> PnLAttribution {
    let delta_pnl = Decimal::try_from(entry_greeks.delta * spot_change).unwrap_or_default();
    let gamma_pnl = Decimal::try_from(0.5 * entry_greeks.gamma * spot_change.powi(2)).unwrap_or_default();
    let theta_pnl = Decimal::try_from(entry_greeks.theta * days_held).unwrap_or_default();
    let vega_pnl = Decimal::try_from(entry_greeks.vega * iv_change * 100.0).unwrap_or_default();

    let explained = delta_pnl + gamma_pnl + theta_pnl + vega_pnl;
    let unexplained = total_pnl - explained;

    PnLAttribution {
        total: total_pnl,
        delta: delta_pnl,
        gamma: gamma_pnl,
        theta: theta_pnl,
        vega: vega_pnl,
        unexplained,
    }
}
```

**File**: `src/services/trading_calendar.rs`

```rust
use chrono::{NaiveDate, Weekday, Datelike};

/// Trading calendar utilities
pub struct TradingCalendar;

impl TradingCalendar {
    /// Check if date is a trading day (excludes weekends, not holidays)
    pub fn is_trading_day(date: NaiveDate) -> bool {
        !matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
    }

    /// Get next trading day
    pub fn next_trading_day(date: NaiveDate) -> NaiveDate {
        let mut next = date + chrono::Duration::days(1);
        while !Self::is_trading_day(next) {
            next += chrono::Duration::days(1);
        }
        next
    }

    /// Get previous trading day
    pub fn previous_trading_day(date: NaiveDate) -> NaiveDate {
        let mut prev = date - chrono::Duration::days(1);
        while !Self::is_trading_day(prev) {
            prev -= chrono::Duration::days(1);
        }
        prev
    }

    /// Iterate over trading days in range (inclusive)
    pub fn trading_days_between(
        start: NaiveDate,
        end: NaiveDate,
    ) -> impl Iterator<Item = NaiveDate> {
        let mut current = start;
        std::iter::from_fn(move || {
            while current <= end && !Self::is_trading_day(current) {
                current += chrono::Duration::days(1);
            }
            if current <= end {
                let result = current;
                current += chrono::Duration::days(1);
                Some(result)
            } else {
                None
            }
        })
    }
}
```

#### Phase 2 Deliverables

- [ ] Value objects: `Strike`, `TimingConfig`, `EarningsTime`, `SpotPrice`, `FailureReason`
- [ ] Entities: `EarningsEvent`, `OptionLeg`, `CalendarSpread`, `TradeOpportunity`, `CalendarSpreadResult`
- [ ] Strategy trait + implementations (ATM, Delta, Liquidity, IVRatio)
- [ ] Repository traits
- [ ] Domain services: `PnLCalculator`, `TradingCalendar`
- [ ] Comprehensive tests

---

## Phase 3: finq-rs Integration (Weeks 8-10)

### Repository Implementations

Bridge `cs-domain` repository traits to `finq-rs` data layer.

**File**: `cs-domain/src/infrastructure/mod.rs`

```rust
pub mod finq_options_repo;
pub mod finq_equity_repo;
pub mod earnings_client;
```

**File**: `cs-domain/src/infrastructure/finq_options_repo.rs`

```rust
use async_trait::async_trait;
use chrono::{NaiveDate, DateTime, Utc};
use finq_flatfiles::{OptionBarRepository, OptionBarReader};

use crate::repositories::{OptionsDataRepository, RepositoryError};
use crate::value_objects::Strike;

pub struct FinqOptionsRepository {
    flatfiles: OptionBarRepository,
}

impl FinqOptionsRepository {
    pub fn new(data_dir: &std::path::Path) -> Self {
        let config = finq_flatfiles::FlatfileConfig::new(data_dir);
        Self {
            flatfiles: OptionBarRepository::new(config),
        }
    }
}

#[async_trait]
impl OptionsDataRepository for FinqOptionsRepository {
    async fn get_option_bars(
        &self,
        underlying: &str,
        date: NaiveDate,
    ) -> Result<polars::frame::DataFrame, RepositoryError> {
        self.flatfiles
            .get_chain_bars(underlying, date)
            .await
            .map_err(|e| RepositoryError::NotFound(e.to_string()))
    }

    async fn get_available_expirations(
        &self,
        underlying: &str,
        as_of_date: NaiveDate,
    ) -> Result<Vec<NaiveDate>, RepositoryError> {
        let bars = self.get_option_bars(underlying, as_of_date).await?;

        // Extract unique expirations from DataFrame
        let expirations = bars
            .column("expiration")
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .date()
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .unique()
            .map_err(|e| RepositoryError::Parse(e.to_string()))?;

        let mut result: Vec<NaiveDate> = expirations
            .into_iter()
            .filter_map(|opt| opt.map(|d| NaiveDate::from_num_days_from_ce_opt(d).unwrap()))
            .filter(|&exp| exp > as_of_date)
            .collect();

        result.sort();
        Ok(result)
    }

    async fn get_available_strikes(
        &self,
        underlying: &str,
        _expiration: NaiveDate,
        as_of_date: NaiveDate,
    ) -> Result<Vec<Strike>, RepositoryError> {
        let bars = self.get_option_bars(underlying, as_of_date).await?;

        let strikes = bars
            .column("strike")
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .f64()
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .unique()
            .map_err(|e| RepositoryError::Parse(e.to_string()))?;

        let mut result: Vec<Strike> = strikes
            .into_iter()
            .filter_map(|opt| {
                opt.and_then(|v| {
                    rust_decimal::Decimal::try_from(v)
                        .ok()
                        .and_then(|d| Strike::new(d).ok())
                })
            })
            .collect();

        result.sort();
        Ok(result)
    }
}
```

**File**: `cs-domain/src/infrastructure/finq_equity_repo.rs`

```rust
use async_trait::async_trait;
use chrono::{NaiveDate, DateTime, Utc, Timelike};
use finq_flatfiles::{StockBarRepository, StockBarReader};
use rust_decimal::Decimal;

use crate::repositories::{EquityDataRepository, RepositoryError};
use crate::value_objects::SpotPrice;

pub struct FinqEquityRepository {
    flatfiles: StockBarRepository,
}

impl FinqEquityRepository {
    pub fn new(data_dir: &std::path::Path) -> Self {
        let config = finq_flatfiles::FlatfileConfig::new(data_dir);
        Self {
            flatfiles: StockBarRepository::new(config),
        }
    }
}

#[async_trait]
impl EquityDataRepository for FinqEquityRepository {
    async fn get_spot_price(
        &self,
        symbol: &str,
        target_time: DateTime<Utc>,
    ) -> Result<SpotPrice, RepositoryError> {
        let date = target_time.date_naive();
        let df = self.flatfiles
            .get_bars_dataframe(symbol, date, date)
            .await
            .map_err(|e| RepositoryError::NotFound(e.to_string()))?;

        // Filter to bars at or before target time
        let filtered = df
            .lazy()
            .filter(polars::prelude::col("timestamp").lt_eq(polars::prelude::lit(target_time.timestamp_nanos_opt().unwrap_or(0))))
            .sort(["timestamp"], polars::prelude::SortMultipleOptions::default().with_order_descending(true))
            .limit(1)
            .collect()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        if filtered.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No spot price for {} at {}",
                symbol, target_time
            )));
        }

        let close = filtered
            .column("close")
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .f64()
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .get(0)
            .ok_or_else(|| RepositoryError::NotFound("Empty close column".into()))?;

        let timestamp = filtered
            .column("timestamp")
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .datetime()
            .map_err(|e| RepositoryError::Parse(e.to_string()))?
            .get(0)
            .ok_or_else(|| RepositoryError::NotFound("Empty timestamp column".into()))?;

        Ok(SpotPrice {
            value: Decimal::try_from(close).unwrap_or_default(),
            timestamp: DateTime::from_timestamp_nanos(timestamp),
        })
    }

    async fn get_bars(
        &self,
        symbol: &str,
        date: NaiveDate,
    ) -> Result<polars::frame::DataFrame, RepositoryError> {
        self.flatfiles
            .get_bars_dataframe(symbol, date, date)
            .await
            .map_err(|e| RepositoryError::NotFound(e.to_string()))
    }
}
```

#### Phase 3 Deliverables

- [ ] `FinqOptionsRepository` implementation
- [ ] `FinqEquityRepository` implementation
- [ ] `EarningsClient` implementation
- [ ] Integration tests with real finq data
- [ ] Error mapping from finq-rs errors

---

## Phase 4: Backtest Engine (Weeks 11-15)

### Crate: `cs-backtest`

Core backtest execution engine with parallel processing.

**Directory structure:**
```
cs-backtest/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── config.rs
│   ├── backtest_use_case.rs
│   ├── trade_executor.rs
│   ├── session_processor.rs
│   └── parallel.rs
```

#### 4.1 Cargo.toml

```toml
[package]
name = "cs-backtest"
version.workspace = true
edition.workspace = true

[dependencies]
cs-analytics = { workspace = true }
cs-domain = { workspace = true }
finq-core = { workspace = true }
finq-flatfiles = { workspace = true }
tokio = { workspace = true }
futures = { workspace = true }
rayon = { workspace = true }
polars = { workspace = true }
rust_decimal = { workspace = true }
chrono = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
parquet = { workspace = true }

[dev-dependencies]
tokio-test = { workspace = true }
```

#### 4.2 Backtest Configuration

**File**: `src/config.rs`

```rust
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use cs_domain::{TimingConfig, TradeSelectionCriteria};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub data_dir: PathBuf,
    pub timing: TimingConfig,
    pub selection: TradeSelectionCriteria,
    pub strategy: StrategyType,
    pub symbols: Option<Vec<String>>,
    pub min_market_cap: Option<u64>,
    pub interpolate_prices: bool,
    pub parallel: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StrategyType {
    ATM,
    Delta,
    Liquidity,
    IVRatio,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("data"),
            timing: TimingConfig::default(),
            selection: TradeSelectionCriteria::default(),
            strategy: StrategyType::ATM,
            symbols: None,
            min_market_cap: None,
            interpolate_prices: false,
            parallel: true,
        }
    }
}
```

#### 4.3 Backtest Use Case

**File**: `src/backtest_use_case.rs`

```rust
use std::sync::Arc;
use chrono::NaiveDate;
use rayon::prelude::*;
use tracing::{info, debug, warn};

use cs_domain::*;
use cs_analytics::*;
use crate::config::{BacktestConfig, StrategyType};

/// Backtest execution result
#[derive(Debug)]
pub struct BacktestResult {
    pub results: Vec<CalendarSpreadResult>,
    pub sessions_processed: usize,
    pub total_entries: usize,
    pub total_opportunities: usize,
    pub dropped_events: Vec<TradeGenerationError>,
}

impl BacktestResult {
    pub fn win_rate(&self) -> f64 {
        let winners = self.results.iter().filter(|r| r.is_winner()).count();
        if self.results.is_empty() {
            0.0
        } else {
            winners as f64 / self.results.len() as f64
        }
    }

    pub fn total_pnl(&self) -> rust_decimal::Decimal {
        self.results.iter().map(|r| r.pnl).sum()
    }
}

/// Session progress callback
pub struct SessionProgress {
    pub session_date: NaiveDate,
    pub entries_count: usize,
    pub events_found: usize,
}

/// Trade generation error
#[derive(Debug, Clone)]
pub struct TradeGenerationError {
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub reason: String,
    pub details: Option<String>,
    pub phase: String,
}

/// Main backtest use case
pub struct BacktestUseCase<E, O, Q>
where
    E: EarningsRepository,
    O: OptionsDataRepository,
    Q: EquityDataRepository,
{
    earnings_repo: Arc<E>,
    options_repo: Arc<O>,
    equity_repo: Arc<Q>,
    config: BacktestConfig,
}

impl<E, O, Q> BacktestUseCase<E, O, Q>
where
    E: EarningsRepository + 'static,
    O: OptionsDataRepository + 'static,
    Q: EquityDataRepository + 'static,
{
    pub fn new(
        earnings_repo: E,
        options_repo: O,
        equity_repo: Q,
        config: BacktestConfig,
    ) -> Self {
        Self {
            earnings_repo: Arc::new(earnings_repo),
            options_repo: Arc::new(options_repo),
            equity_repo: Arc::new(equity_repo),
            config,
        }
    }

    pub async fn execute(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        option_type: finq_core::OptionType,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult, BacktestError> {
        let mut all_results = Vec::new();
        let mut dropped_events = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        info!(
            start_date = %start_date,
            end_date = %end_date,
            option_type = ?option_type,
            "Starting backtest"
        );

        let strategy = self.create_strategy();

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings
            let events = self.load_earnings_window(session_date).await?;
            let to_enter = self.filter_for_entry(&events, session_date);

            if to_enter.is_empty() {
                if let Some(ref callback) = on_progress {
                    callback(SessionProgress {
                        session_date,
                        entries_count: 0,
                        events_found: 0,
                    });
                }
                continue;
            }

            debug!(
                session_date = %session_date,
                events_count = to_enter.len(),
                "Processing session"
            );

            // Process events (parallel or sequential)
            let session_results: Vec<_> = if self.config.parallel {
                to_enter
                    .par_iter()
                    .map(|event| {
                        tokio::runtime::Handle::current().block_on(
                            self.process_event(event, session_date, &strategy, option_type)
                        )
                    })
                    .collect()
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    results.push(
                        self.process_event(event, session_date, &strategy, option_type).await
                    );
                }
                results
            };

            // Collect results
            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                match result {
                    Ok(trade_result) => {
                        if self.passes_iv_filter(&trade_result) {
                            all_results.push(trade_result);
                            session_entries += 1;
                        } else {
                            dropped_events.push(TradeGenerationError {
                                symbol: trade_result.symbol.clone(),
                                earnings_date: trade_result.earnings_date,
                                earnings_time: trade_result.earnings_time,
                                reason: "IV_RATIO_FILTER".into(),
                                details: None,
                                phase: "filter".into(),
                            });
                        }
                    }
                    Err(e) => dropped_events.push(e),
                }
            }

            if let Some(ref callback) = on_progress {
                callback(SessionProgress {
                    session_date,
                    entries_count: session_entries,
                    events_found: to_enter.len(),
                });
            }
        }

        info!(
            sessions_processed,
            total_opportunities,
            results_count = all_results.len(),
            dropped_count = dropped_events.len(),
            "Backtest completed"
        );

        Ok(BacktestResult {
            results: all_results,
            sessions_processed,
            total_entries: all_results.len(),
            total_opportunities,
            dropped_events,
        })
    }

    fn create_strategy(&self) -> Box<dyn TradingStrategy> {
        match self.config.strategy {
            StrategyType::ATM => Box::new(ATMStrategy::default()),
            StrategyType::Delta => Box::new(DeltaStrategy {
                criteria: self.config.selection.clone(),
            }),
            StrategyType::Liquidity => Box::new(LiquidityStrategy {
                criteria: self.config.selection.clone(),
            }),
            StrategyType::IVRatio => Box::new(IVRatioStrategy {
                criteria: self.config.selection.clone(),
            }),
        }
    }

    async fn process_event(
        &self,
        event: &EarningsEvent,
        session_date: NaiveDate,
        strategy: &dyn TradingStrategy,
        option_type: finq_core::OptionType,
    ) -> Result<CalendarSpreadResult, TradeGenerationError> {
        // Implementation details...
        // 1. Get spot price
        // 2. Load chain data
        // 3. Select spread via strategy
        // 4. Price entry
        // 5. Price exit
        // 6. Calculate P&L
        // 7. Build result
        todo!("Implement process_event")
    }

    fn passes_iv_filter(&self, result: &CalendarSpreadResult) -> bool {
        match (self.config.selection.min_iv_ratio, result.iv_ratio()) {
            (Some(min), Some(ratio)) => ratio >= min,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    async fn load_earnings_window(&self, session_date: NaiveDate) -> Result<Vec<EarningsEvent>, BacktestError> {
        let start = TradingCalendar::previous_trading_day(session_date);
        let end = TradingCalendar::next_trading_day(session_date);
        self.earnings_repo
            .load_earnings(start, end, self.config.symbols.as_deref())
            .await
            .map_err(|e| BacktestError::Repository(e.to_string()))
    }

    fn filter_for_entry(&self, events: &[EarningsEvent], session_date: NaiveDate) -> Vec<EarningsEvent> {
        events
            .iter()
            .filter(|e| self.should_enter_today(e, session_date))
            .cloned()
            .collect()
    }

    fn should_enter_today(&self, event: &EarningsEvent, session_date: NaiveDate) -> bool {
        match event.earnings_time {
            EarningsTime::AfterMarketClose => event.earnings_date == session_date,
            EarningsTime::BeforeMarketOpen => {
                TradingCalendar::previous_trading_day(event.earnings_date) == session_date
            }
            EarningsTime::Unknown => false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BacktestError {
    #[error("Repository error: {0}")]
    Repository(String),
    #[error("Strategy error: {0}")]
    Strategy(String),
    #[error("Pricing error: {0}")]
    Pricing(String),
}
```

#### Phase 4 Deliverables

- [ ] `BacktestConfig` and `BacktestResult`
- [ ] `BacktestUseCase` with async execution
- [ ] Parallel processing with rayon
- [ ] Progress callback support
- [ ] Error handling and dropped events tracking
- [ ] Integration tests
- [ ] Benchmark: 10K trades < 4 minutes

---

## Phase 5: Python Bindings (Weeks 16-18)

### Crate: `cs-python`

PyO3 bindings for Python interoperability.

#### 5.1 Cargo.toml

```toml
[package]
name = "cs-python"
version.workspace = true
edition.workspace = true

[lib]
name = "cs_rust"
crate-type = ["cdylib"]

[dependencies]
cs-analytics = { workspace = true }
cs-domain = { workspace = true }
cs-backtest = { workspace = true }
pyo3 = { workspace = true }
tokio = { workspace = true }
rust_decimal = { workspace = true }
chrono = { workspace = true }

[build-dependencies]
pyo3-build-config = "0.20"
```

#### 5.2 Module Structure

**File**: `src/lib.rs`

```rust
use pyo3::prelude::*;

mod analytics;
mod domain;
mod backtest;

use analytics::*;
use domain::*;
use backtest::*;

#[pymodule]
fn cs_rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Analytics functions
    m.add_function(wrap_pyfunction!(py_bs_price, m)?)?;
    m.add_function(wrap_pyfunction!(py_bs_implied_volatility, m)?)?;
    m.add_function(wrap_pyfunction!(py_bs_greeks, m)?)?;

    // Domain types
    m.add_class::<PyGreeks>()?;
    m.add_class::<PyCalendarSpreadResult>()?;

    // Backtest
    m.add_class::<PyBacktestConfig>()?;
    m.add_class::<PyBacktestResult>()?;
    m.add_class::<PyBacktestUseCase>()?;

    Ok(())
}
```

**File**: `src/analytics.rs`

```rust
use pyo3::prelude::*;
use cs_analytics::{bs_price, bs_implied_volatility, bs_greeks, BSConfig};

#[pyfunction]
#[pyo3(signature = (spot, strike, time_to_expiry, volatility, is_call, risk_free_rate=0.05))]
pub fn py_bs_price(
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    volatility: f64,
    is_call: bool,
    risk_free_rate: f64,
) -> f64 {
    bs_price(spot, strike, time_to_expiry, volatility, is_call, risk_free_rate)
}

#[pyfunction]
#[pyo3(signature = (option_price, spot, strike, time_to_expiry, is_call))]
pub fn py_bs_implied_volatility(
    option_price: f64,
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    is_call: bool,
) -> Option<f64> {
    bs_implied_volatility(option_price, spot, strike, time_to_expiry, is_call, &BSConfig::default())
}

#[pyfunction]
#[pyo3(signature = (spot, strike, time_to_expiry, volatility, is_call))]
pub fn py_bs_greeks(
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    volatility: f64,
    is_call: bool,
) -> PyGreeks {
    let greeks = bs_greeks(spot, strike, time_to_expiry, volatility, is_call, 0.05);
    PyGreeks::from(greeks)
}

#[pyclass]
#[derive(Clone)]
pub struct PyGreeks {
    #[pyo3(get)]
    pub delta: f64,
    #[pyo3(get)]
    pub gamma: f64,
    #[pyo3(get)]
    pub theta: f64,
    #[pyo3(get)]
    pub vega: f64,
    #[pyo3(get)]
    pub rho: f64,
}

impl From<cs_analytics::Greeks> for PyGreeks {
    fn from(g: cs_analytics::Greeks) -> Self {
        Self {
            delta: g.delta,
            gamma: g.gamma,
            theta: g.theta,
            vega: g.vega,
            rho: g.rho,
        }
    }
}
```

#### 5.3 Python Usage

```python
# After: pip install cs-rust (via maturin)

from cs_rust import (
    py_bs_price as bs_price,
    py_bs_implied_volatility as bs_implied_volatility,
    py_bs_greeks as bs_greeks,
    PyBacktestConfig,
    PyBacktestUseCase,
)

# Use Rust analytics (15-20x faster)
iv = bs_implied_volatility(5.0, 100.0, 100.0, 0.08, True)
greeks = bs_greeks(100.0, 100.0, 0.08, 0.30, True)
print(f"Delta: {greeks.delta}, Vega: {greeks.vega}")

# Run full backtest in Rust
config = PyBacktestConfig(
    data_dir="/path/to/finq_data",
    strategy="delta",
    target_delta=0.50,
)
use_case = PyBacktestUseCase(config)
result = use_case.execute("2025-11-01", "2025-11-30", "call")

print(f"Results: {len(result.results)}")
print(f"Win rate: {result.win_rate():.1%}")
print(f"Total P&L: ${result.total_pnl():.2f}")
```

#### Phase 5 Deliverables

- [ ] PyO3 module setup with maturin
- [ ] Analytics bindings (bs_price, bs_iv, bs_greeks)
- [ ] Domain type bindings
- [ ] `PyBacktestUseCase` with full execution
- [ ] Build scripts and CI
- [ ] Drop-in replacement tests

---

## Phase 6: CLI + Persistence (Weeks 19-21)

### Crate: `cs-cli`

Command-line interface.

#### 6.1 Commands

```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cs")]
#[command(about = "Calendar Spread Backtest CLI")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Data directory
    #[arg(long, env = "FINQ_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run backtest
    Backtest {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long, default_value = "call")]
        option_type: String,
        #[arg(long, default_value = "atm")]
        strategy: String,
        #[arg(long)]
        symbols: Option<Vec<String>>,
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Analyze results from a run
    Analyze {
        #[arg(long)]
        run_dir: PathBuf,
    },

    /// Price a single spread (for debugging)
    Price {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        strike: f64,
        #[arg(long)]
        short_expiry: String,
        #[arg(long)]
        long_expiry: String,
        #[arg(long)]
        date: String,
    },
}
```

#### Phase 6 Deliverables

- [ ] CLI with backtest, analyze, price commands
- [ ] Progress bars (indicatif)
- [ ] Table output (tabled)
- [ ] Parquet persistence
- [ ] JSON config serialization

---

## Phase 7: Testing + Validation (Weeks 22-25)

### Testing Strategy

1. **Unit tests**: All analytics functions, domain models
2. **Integration tests**: With real finq data
3. **Parity tests**: Rust vs Python result comparison
4. **Benchmarks**: criterion suite

#### Phase 7 Deliverables

- [ ] Comprehensive test coverage (>80%)
- [ ] Parity validation suite
- [ ] Benchmark suite
- [ ] Performance validation: 5x speedup
- [ ] Documentation

---

## Timeline Summary

| Phase | Duration | Dependencies |
|-------|----------|--------------|
| 1. Analytics Core | Weeks 1-3 | None |
| 2. Domain Models | Weeks 4-7 | Phase 1, finq-core |
| 3. finq-rs Integration | Weeks 8-10 | finq-sdk |
| 4. Backtest Engine | Weeks 11-15 | Phases 1-3 |
| 5. Python Bindings | Weeks 16-18 | Phase 4 |
| 6. CLI + Persistence | Weeks 19-21 | Phase 4 |
| 7. Testing | Weeks 22-25 | All |

**Total: 18-25 weeks**

---

## Success Criteria

| Metric | Target |
|--------|--------|
| Backtest speed (10K trades) | < 4 minutes |
| BS IV solver speed | < 100ns per call |
| Memory usage | < 50% of Python |
| Result parity | 100% match |
| Test coverage | > 80% |
