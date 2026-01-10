# Implementation Plan: Unified Entry Rules System

## Overview

This document describes the implementation of a modular, configurable entry rules system for the backtesting engine. Entry rules act as gates that determine whether a trade should be executed, evaluated at different stages of the backtest pipeline.

## Problem Statement

### Current State

The codebase has **fragmented filtering logic** across multiple locations:

| Filter | Location | When Evaluated | Issue |
|--------|----------|----------------|-------|
| `symbols` | Event-level | Before execution | OK |
| `min_market_cap` | Event-level | Before execution | OK |
| `max_entry_iv` | `validate_entry()` | During execution | OK |
| `min_iv_ratio` | `apply_filter()` | **After execution** | Wasteful |
| `min_notional` | Defined but not wired | - | Unused |
| `min_entry_price` | Defined but not wired | - | Unused |
| `max_entry_price` | Defined but not wired | - | Unused |

**Key Problem**: `min_iv_ratio` is evaluated **after** trade execution via `apply_filter()`. This means:
1. Expensive operations (chain loading, IV surface building, pricing) happen first
2. Then the trade is discarded if IV ratio doesn't pass
3. Wasteful computation that could be avoided

### Desired State

All entry rules evaluated at the **earliest possible stage**:

```
1. Event-level rules (no market data needed)
   ├── symbols
   ├── min_market_cap
   └── [future: earnings_time, sector, etc.]

2. Market-level rules (need PreparedData: IV surface, spot, chain)
   ├── iv_slope (iv_7 > iv_20 + threshold)
   ├── max_entry_iv (atm_iv < threshold)
   ├── min_iv_ratio (short_iv / long_iv >= threshold)
   ├── iv_vs_hv (iv >= hv * factor)
   └── min_notional (volume * 100 * spot >= threshold)

3. Trade-level rules (need pricing result)
   ├── min_entry_price
   └── max_entry_price
```

## Design Principles

### 1. Config Pattern: Partial File + Code Defaults

Following the Rust-idiomatic pattern from CLAUDE.md:

```rust
// RUNTIME CONFIG - has real defaults (source of truth)
#[derive(Debug, Clone)]
pub struct RulesConfig {
    pub event_rules: Vec<EventRule>,
    pub market_rules: Vec<MarketRule>,
    pub trade_rules: Vec<TradeRule>,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            event_rules: vec![],  // No rules by default
            market_rules: vec![],
            trade_rules: vec![],
        }
    }
}

// FILE CONFIG - partial, for TOML deserialization
#[derive(Debug, Deserialize)]
pub struct FileRulesConfig {
    pub event: Option<Vec<FileEventRule>>,
    pub market: Option<Vec<FileMarketRule>>,
    pub trade: Option<Vec<FileTradeRule>>,
}

// MERGE - file overrides defaults
impl RulesConfig {
    pub fn apply_file(mut self, file: FileRulesConfig) -> Self {
        if let Some(event_rules) = file.event {
            self.event_rules = event_rules.into_iter().map(Into::into).collect();
        }
        if let Some(market_rules) = file.market {
            self.market_rules = market_rules.into_iter().map(Into::into).collect();
        }
        if let Some(trade_rules) = file.trade {
            self.trade_rules = trade_rules.into_iter().map(Into::into).collect();
        }
        self
    }
}
```

### 2. Rule Trait Design

```rust
/// Core trait for all entry rules
pub trait EntryRule: Send + Sync + std::fmt::Debug {
    /// Human-readable name for logging/debugging
    fn name(&self) -> &'static str;

    /// What context level this rule requires
    fn level(&self) -> RuleLevel;
}

/// Evaluation context levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleLevel {
    Event,   // Only needs EarningsEvent
    Market,  // Needs PreparedData (IV surface, spot, chain)
    Trade,   // Needs trade execution result
}
```

### 3. Separate Evaluator Traits by Level

```rust
/// Event-level rule evaluation
pub trait EventRuleEval: EntryRule {
    fn eval(&self, event: &EarningsEvent) -> Result<bool, RuleError>;
}

/// Market-level rule evaluation
pub trait MarketRuleEval: EntryRule {
    fn eval(&self, event: &EarningsEvent, data: &PreparedData) -> Result<bool, RuleError>;
}

/// Trade-level rule evaluation
pub trait TradeRuleEval: EntryRule {
    fn eval(&self, event: &EarningsEvent, data: &PreparedData, result: &dyn TradeResultMethods) -> Result<bool, RuleError>;
}
```

## Implementation Steps

### Phase 1: Domain Layer (`cs-domain`)

#### Step 1.1: Create Rules Module Structure

```
cs-domain/src/
├── rules/
│   ├── mod.rs           # Module exports, RuleLevel enum
│   ├── error.rs         # RuleError type
│   ├── event/
│   │   ├── mod.rs
│   │   ├── market_cap.rs
│   │   └── symbols.rs
│   ├── market/
│   │   ├── mod.rs
│   │   ├── iv_slope.rs
│   │   ├── max_entry_iv.rs
│   │   ├── min_iv_ratio.rs
│   │   ├── iv_vs_hv.rs
│   │   └── min_notional.rs
│   └── trade/
│       ├── mod.rs
│       └── entry_price_range.rs
```

#### Step 1.2: Define Rule Enums (Runtime Config)

```rust
// cs-domain/src/rules/event/mod.rs

/// Event-level rules (no market data needed)
#[derive(Debug, Clone)]
pub enum EventRule {
    MinMarketCap { threshold: u64 },
    Symbols { include: Vec<String> },
}

impl EventRule {
    pub fn name(&self) -> &'static str {
        match self {
            Self::MinMarketCap { .. } => "min_market_cap",
            Self::Symbols { .. } => "symbols",
        }
    }

    pub fn eval(&self, event: &EarningsEvent) -> bool {
        match self {
            Self::MinMarketCap { threshold } => {
                event.market_cap.map_or(false, |cap| cap >= *threshold)
            }
            Self::Symbols { include } => {
                include.iter().any(|s| s.eq_ignore_ascii_case(&event.symbol))
            }
        }
    }
}
```

```rust
// cs-domain/src/rules/market/mod.rs

/// Market-level rules (need IV surface, spot, chain)
#[derive(Debug, Clone)]
pub enum MarketRule {
    /// IV term structure slope: iv_short > iv_long + threshold_pp
    IvSlope {
        short_dte: u16,
        long_dte: u16,
        threshold_pp: f64,
    },
    /// Maximum ATM IV at entry
    MaxEntryIv {
        threshold: f64,
    },
    /// Minimum IV ratio (short/long)
    MinIvRatio {
        short_dte: u16,
        long_dte: u16,
        threshold: f64,
    },
    /// IV vs Historical Volatility comparison
    IvVsHv {
        hv_window_days: u16,
        min_ratio: f64,  // iv >= hv * min_ratio
    },
    /// Minimum daily option notional
    MinNotional {
        threshold: f64,
    },
}
```

```rust
// cs-domain/src/rules/trade/mod.rs

/// Trade-level rules (need execution result)
#[derive(Debug, Clone)]
pub enum TradeRule {
    /// Entry price must be within range
    EntryPriceRange {
        min: Option<f64>,
        max: Option<f64>,
    },
}
```

#### Step 1.3: Define Runtime Config

```rust
// cs-domain/src/rules/config.rs

/// Runtime rules configuration (source of truth for defaults)
#[derive(Debug, Clone)]
pub struct RulesConfig {
    pub event: Vec<EventRule>,
    pub market: Vec<MarketRule>,
    pub trade: Vec<TradeRule>,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            event: vec![],   // No rules by default - explicit opt-in
            market: vec![],
            trade: vec![],
        }
    }
}

impl RulesConfig {
    /// Check if any rules are configured
    pub fn has_rules(&self) -> bool {
        !self.event.is_empty() || !self.market.is_empty() || !self.trade.is_empty()
    }

    /// Create from common filter patterns (for migration)
    pub fn from_filter_criteria(criteria: &FilterCriteria) -> Self {
        let mut config = Self::default();

        if let Some(symbols) = &criteria.symbols {
            config.event.push(EventRule::Symbols {
                include: symbols.clone()
            });
        }

        if let Some(min_cap) = criteria.min_market_cap {
            config.event.push(EventRule::MinMarketCap {
                threshold: min_cap
            });
        }

        if let Some(max_iv) = criteria.max_entry_iv {
            config.market.push(MarketRule::MaxEntryIv {
                threshold: max_iv
            });
        }

        if let Some(min_ratio) = criteria.min_iv_ratio {
            // Default DTEs for backward compatibility
            config.market.push(MarketRule::MinIvRatio {
                short_dte: 7,
                long_dte: 30,
                threshold: min_ratio,
            });
        }

        if let Some(min_price) = criteria.min_entry_price {
            config.trade.push(TradeRule::EntryPriceRange {
                min: Some(min_price),
                max: criteria.max_entry_price,
            });
        } else if let Some(max_price) = criteria.max_entry_price {
            config.trade.push(TradeRule::EntryPriceRange {
                min: None,
                max: Some(max_price),
            });
        }

        config
    }
}
```

### Phase 2: Config Layer (`cs-backtest/src/config`)

#### Step 2.1: File Config (Partial, for TOML)

```rust
// cs-backtest/src/config/rules.rs

use serde::Deserialize;

/// File config for rules - all fields optional (partial)
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileRulesConfig {
    /// Event-level rules
    pub event: Option<Vec<FileEventRule>>,
    /// Market-level rules
    pub market: Option<Vec<FileMarketRule>>,
    /// Trade-level rules
    pub trade: Option<Vec<FileTradeRule>>,
}

/// Event rule from TOML (tagged enum)
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileEventRule {
    MinMarketCap { threshold: u64 },
    Symbols { include: Vec<String> },
}

/// Market rule from TOML (tagged enum)
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileMarketRule {
    IvSlope {
        short_dte: u16,
        long_dte: u16,
        threshold_pp: f64,
    },
    MaxEntryIv {
        threshold: f64,
    },
    MinIvRatio {
        short_dte: Option<u16>,  // Optional, defaults to 7
        long_dte: Option<u16>,   // Optional, defaults to 30
        threshold: f64,
    },
    IvVsHv {
        hv_window_days: Option<u16>,  // Optional, defaults to 20
        min_ratio: f64,
    },
    MinNotional {
        threshold: f64,
    },
}

/// Trade rule from TOML (tagged enum)
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileTradeRule {
    EntryPriceRange {
        min: Option<f64>,
        max: Option<f64>,
    },
}
```

#### Step 2.2: Conversion from File to Runtime

```rust
// cs-backtest/src/config/rules.rs (continued)

impl From<FileEventRule> for EventRule {
    fn from(file: FileEventRule) -> Self {
        match file {
            FileEventRule::MinMarketCap { threshold } => {
                EventRule::MinMarketCap { threshold }
            }
            FileEventRule::Symbols { include } => {
                EventRule::Symbols { include }
            }
        }
    }
}

impl From<FileMarketRule> for MarketRule {
    fn from(file: FileMarketRule) -> Self {
        match file {
            FileMarketRule::IvSlope { short_dte, long_dte, threshold_pp } => {
                MarketRule::IvSlope { short_dte, long_dte, threshold_pp }
            }
            FileMarketRule::MaxEntryIv { threshold } => {
                MarketRule::MaxEntryIv { threshold }
            }
            FileMarketRule::MinIvRatio { short_dte, long_dte, threshold } => {
                MarketRule::MinIvRatio {
                    short_dte: short_dte.unwrap_or(7),
                    long_dte: long_dte.unwrap_or(30),
                    threshold,
                }
            }
            FileMarketRule::IvVsHv { hv_window_days, min_ratio } => {
                MarketRule::IvVsHv {
                    hv_window_days: hv_window_days.unwrap_or(20),
                    min_ratio,
                }
            }
            FileMarketRule::MinNotional { threshold } => {
                MarketRule::MinNotional { threshold }
            }
        }
    }
}

impl From<FileTradeRule> for TradeRule {
    fn from(file: FileTradeRule) -> Self {
        match file {
            FileTradeRule::EntryPriceRange { min, max } => {
                TradeRule::EntryPriceRange { min, max }
            }
        }
    }
}

impl RulesConfig {
    /// Apply file config overrides
    pub fn apply_file(mut self, file: FileRulesConfig) -> Self {
        if let Some(event_rules) = file.event {
            self.event = event_rules.into_iter().map(Into::into).collect();
        }
        if let Some(market_rules) = file.market {
            self.market = market_rules.into_iter().map(Into::into).collect();
        }
        if let Some(trade_rules) = file.trade {
            self.trade = trade_rules.into_iter().map(Into::into).collect();
        }
        self
    }
}
```

#### Step 2.3: Add to BacktestConfig

```rust
// cs-backtest/src/config/mod.rs

pub struct BacktestConfig {
    // ... existing fields ...

    /// Entry rules configuration
    #[serde(default)]
    pub rules: FileRulesConfig,
}
```

### Phase 3: CLI Layer (`cs-cli`)

#### Step 3.1: CLI Overrides (Partial)

```rust
// cs-cli/src/cli_args.rs

/// CLI overrides for entry rules
#[derive(Debug, Clone, Serialize, Default)]
#[serde(default)]
pub struct CliRulesOverrides {
    // IV Slope rule
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iv_slope_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iv_slope_short_dte: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iv_slope_long_dte: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iv_slope_threshold: Option<f64>,

    // Max entry IV (already exists, migrate here)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_entry_iv: Option<f64>,

    // Min IV ratio (already exists, migrate here)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_iv_ratio: Option<f64>,

    // IV vs HV rule
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iv_vs_hv_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iv_vs_hv_window: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iv_vs_hv_min_ratio: Option<f64>,

    // Min notional
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_notional: Option<f64>,
}
```

#### Step 3.2: CLI Args (Wrapper Types)

```rust
// cs-cli/src/args/rules.rs

use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct RulesArgs {
    // IV Slope entry rule
    /// Enable IV slope entry rule (iv_short > iv_long + threshold)
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub entry_iv_slope: bool,

    /// IV slope: short-term DTE window (default: 7)
    #[arg(long)]
    pub iv_slope_short_dte: Option<u16>,

    /// IV slope: long-term DTE window (default: 20)
    #[arg(long)]
    pub iv_slope_long_dte: Option<u16>,

    /// IV slope: threshold in percentage points (default: 0.05 = 5pp)
    #[arg(long)]
    pub iv_slope_threshold: Option<f64>,

    // Max entry IV (migrated from existing)
    /// Maximum ATM IV at entry (e.g., 1.5 = 150%)
    #[arg(long)]
    pub max_entry_iv: Option<f64>,

    // Min IV ratio (migrated from existing)
    /// Minimum IV ratio (short_iv / long_iv)
    #[arg(long)]
    pub min_iv_ratio: Option<f64>,

    // IV vs HV rule
    /// Enable IV vs HV comparison rule
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub entry_iv_vs_hv: bool,

    /// IV vs HV: historical volatility window in days (default: 20)
    #[arg(long)]
    pub hv_window: Option<u16>,

    /// IV vs HV: minimum ratio (iv >= hv * ratio, default: 1.0)
    #[arg(long)]
    pub iv_vs_hv_ratio: Option<f64>,

    // Min notional
    /// Minimum daily option notional ($)
    #[arg(long)]
    pub min_notional: Option<f64>,
}
```

### Phase 4: Rule Evaluation (`cs-backtest`)

#### Step 4.1: Rule Evaluator

```rust
// cs-backtest/src/rules/evaluator.rs

use cs_domain::rules::{RulesConfig, EventRule, MarketRule, TradeRule};
use crate::backtest_use_case_helpers::PreparedData;

/// Evaluates rules at each stage
pub struct RuleEvaluator {
    config: RulesConfig,
}

impl RuleEvaluator {
    pub fn new(config: RulesConfig) -> Self {
        Self { config }
    }

    /// Evaluate all event-level rules (AND logic)
    pub fn eval_event_rules(&self, event: &EarningsEvent) -> bool {
        if self.config.event.is_empty() {
            return true; // No rules = pass
        }

        for rule in &self.config.event {
            if !rule.eval(event) {
                tracing::debug!(
                    symbol = %event.symbol,
                    rule = rule.name(),
                    "Event rule failed"
                );
                return false;
            }
        }
        true
    }

    /// Evaluate all market-level rules (AND logic)
    pub fn eval_market_rules(
        &self,
        event: &EarningsEvent,
        data: &PreparedData,
        hv_provider: Option<&dyn HvProvider>,
    ) -> Result<bool, RuleError> {
        if self.config.market.is_empty() {
            return Ok(true); // No rules = pass
        }

        for rule in &self.config.market {
            if !rule.eval(event, data, hv_provider)? {
                tracing::debug!(
                    symbol = %event.symbol,
                    rule = rule.name(),
                    "Market rule failed"
                );
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Evaluate all trade-level rules (AND logic)
    pub fn eval_trade_rules<R: TradeResultMethods>(
        &self,
        event: &EarningsEvent,
        result: &R,
    ) -> bool {
        if self.config.trade.is_empty() {
            return true; // No rules = pass
        }

        for rule in &self.config.trade {
            if !rule.eval(event, result) {
                tracing::debug!(
                    symbol = %event.symbol,
                    rule = rule.name(),
                    "Trade rule failed"
                );
                return false;
            }
        }
        true
    }
}
```

#### Step 4.2: Market Rule Implementations

```rust
// cs-backtest/src/rules/market_eval.rs

impl MarketRule {
    pub fn name(&self) -> &'static str {
        match self {
            Self::IvSlope { .. } => "iv_slope",
            Self::MaxEntryIv { .. } => "max_entry_iv",
            Self::MinIvRatio { .. } => "min_iv_ratio",
            Self::IvVsHv { .. } => "iv_vs_hv",
            Self::MinNotional { .. } => "min_notional",
        }
    }

    pub fn eval(
        &self,
        event: &EarningsEvent,
        data: &PreparedData,
        hv_provider: Option<&dyn HvProvider>,
    ) -> Result<bool, RuleError> {
        match self {
            Self::IvSlope { short_dte, long_dte, threshold_pp } => {
                let iv_short = data.surface.atm_iv_at_dte(*short_dte)
                    .ok_or(RuleError::MissingData("short DTE IV"))?;
                let iv_long = data.surface.atm_iv_at_dte(*long_dte)
                    .ok_or(RuleError::MissingData("long DTE IV"))?;
                Ok(iv_short > iv_long + threshold_pp)
            }

            Self::MaxEntryIv { threshold } => {
                let atm_iv = data.surface.atm_iv()
                    .ok_or(RuleError::MissingData("ATM IV"))?;
                Ok(atm_iv <= *threshold)
            }

            Self::MinIvRatio { short_dte, long_dte, threshold } => {
                let iv_short = data.surface.atm_iv_at_dte(*short_dte)
                    .ok_or(RuleError::MissingData("short DTE IV"))?;
                let iv_long = data.surface.atm_iv_at_dte(*long_dte)
                    .ok_or(RuleError::MissingData("long DTE IV"))?;
                let ratio = iv_short / iv_long;
                Ok(ratio >= *threshold)
            }

            Self::IvVsHv { hv_window_days, min_ratio } => {
                let provider = hv_provider
                    .ok_or(RuleError::MissingData("HV provider"))?;
                let atm_iv = data.surface.atm_iv()
                    .ok_or(RuleError::MissingData("ATM IV"))?;
                let hv = provider.get_hv(&event.symbol, *hv_window_days)
                    .ok_or(RuleError::MissingData("historical volatility"))?;
                Ok(atm_iv >= hv * min_ratio)
            }

            Self::MinNotional { threshold } => {
                let notional = data.compute_daily_notional();
                Ok(notional >= *threshold)
            }
        }
    }
}
```

#### Step 4.3: Integration in Backtest Loop

```rust
// cs-backtest/src/backtest_use_case.rs (modified)

async fn execute_with_strategy<S, R>(...) {
    // 1. Build rule evaluator from config
    let rules = RuleEvaluator::new(self.build_rules_config());

    // 2. Load events
    let events = earnings_repo.load_events(...).await?;

    // 3. Apply EVENT rules (cheap - no data needed)
    let events_after_event_rules: Vec<_> = events
        .into_iter()
        .filter(|e| rules.eval_event_rules(e))
        .collect();

    debug!(
        total = events_after_event_rules.len(),
        "Events after event-level rules"
    );

    // 4. Discover tradable events
    let tradable_events = trading_range.discover_tradable_events(...);

    // 5. For events with market rules, prepare data first
    let has_market_rules = !self.config.rules.market.is_empty();

    let mut events_for_execution = Vec::new();

    for te in tradable_events {
        if has_market_rules {
            // Prepare market data
            let simulator = TradeSimulator::new(...);
            if let Some(data) = simulator.prepare().await {
                // Evaluate market rules
                match rules.eval_market_rules(&te.event, &data, hv_provider.as_deref()) {
                    Ok(true) => events_for_execution.push((te, Some(data))),
                    Ok(false) => {
                        // Log filtered event
                        dropped_events.push(TradeGenerationError::RuleFiltered {
                            symbol: te.event.symbol.clone(),
                            rule: "market_rules",
                        });
                    }
                    Err(e) => {
                        tracing::warn!(symbol = %te.event.symbol, error = %e, "Rule evaluation error");
                    }
                }
            }
        } else {
            // No market rules - pass through
            events_for_execution.push((te, None));
        }
    }

    // 6. Execute trades (can reuse PreparedData if available)
    for (te, cached_data) in events_for_execution {
        let result = if let Some(data) = cached_data {
            strategy.execute_with_prepared_data(&te, &data).await
        } else {
            strategy.execute_trade(&te, ...).await
        };

        if let Some(result) = result {
            // 7. Apply TRADE rules
            if rules.eval_trade_rules(&te.event, &result) {
                all_results.push(result);
            } else {
                dropped_events.push(TradeGenerationError::RuleFiltered {
                    symbol: te.event.symbol.clone(),
                    rule: "trade_rules",
                });
            }
        }
    }
}
```

### Phase 5: Analytics Support (`cs-analytics`)

#### Step 5.1: Historical Volatility Computation

```rust
// cs-analytics/src/historical_vol.rs

/// Compute annualized historical (realized) volatility from price returns
pub fn compute_hv(prices: &[f64], window: usize) -> Option<f64> {
    if prices.len() < window + 1 {
        return None;
    }

    // Log returns
    let returns: Vec<f64> = prices.windows(2)
        .map(|w| (w[1] / w[0]).ln())
        .collect();

    // Take last `window` returns
    let start = returns.len().saturating_sub(window);
    let recent = &returns[start..];

    if recent.is_empty() {
        return None;
    }

    // Mean
    let mean = recent.iter().sum::<f64>() / recent.len() as f64;

    // Variance (sample variance with n-1)
    let variance = recent.iter()
        .map(|r| (r - mean).powi(2))
        .sum::<f64>() / (recent.len() - 1).max(1) as f64;

    // Annualize: sqrt(variance) * sqrt(252)
    Some(variance.sqrt() * (252.0_f64).sqrt())
}

/// HV provider trait for rule evaluation
pub trait HvProvider: Send + Sync {
    fn get_hv(&self, symbol: &str, window_days: u16) -> Option<f64>;
}
```

#### Step 5.2: IV Surface Extensions

```rust
// cs-analytics/src/iv_surface.rs (extensions)

impl IVSurface {
    /// Get ATM IV for a specific DTE
    pub fn atm_iv_at_dte(&self, target_dte: u16) -> Option<f64> {
        // Find closest expiration to target DTE
        let target = target_dte as i32;

        self.expirations()
            .filter_map(|exp| {
                let dte = exp.dte();
                let atm_iv = self.atm_iv_for_expiration(exp)?;
                Some((dte, atm_iv, (dte - target).abs()))
            })
            .min_by_key(|(_, _, diff)| *diff)
            .map(|(_, iv, _)| iv)
    }

    /// Get ATM IV (closest to spot)
    pub fn atm_iv(&self) -> Option<f64> {
        // Use front-month or weighted average
        self.front_month_atm_iv()
    }
}
```

### Phase 6: Migration Strategy

#### Step 6.1: Backward Compatibility

Keep existing filter fields working during migration:

```rust
impl BacktestConfig {
    /// Build RulesConfig from both new rules and legacy filters
    pub fn build_rules_config(&self) -> RulesConfig {
        let mut config = RulesConfig::default();

        // Apply file-based rules first
        if let Some(ref file_rules) = self.rules {
            config = config.apply_file(file_rules.clone());
        }

        // Migrate legacy filters (only if no new rules defined)
        if config.event.is_empty() {
            if let Some(symbols) = &self.symbols {
                config.event.push(EventRule::Symbols {
                    include: symbols.clone()
                });
            }
            if let Some(min_cap) = self.min_market_cap {
                config.event.push(EventRule::MinMarketCap {
                    threshold: min_cap
                });
            }
        }

        if config.market.is_empty() {
            if let Some(max_iv) = self.max_entry_iv {
                config.market.push(MarketRule::MaxEntryIv {
                    threshold: max_iv
                });
            }
            // Note: min_iv_ratio migrates to market rules
            if let Some(min_ratio) = self.selection.min_iv_ratio {
                config.market.push(MarketRule::MinIvRatio {
                    short_dte: 7,
                    long_dte: 30,
                    threshold: min_ratio,
                });
            }
        }

        if config.trade.is_empty() {
            if self.min_entry_price.is_some() || self.max_entry_price.is_some() {
                config.trade.push(TradeRule::EntryPriceRange {
                    min: self.min_entry_price,
                    max: self.max_entry_price,
                });
            }
        }

        config
    }
}
```

#### Step 6.2: Deprecation Path

1. **Phase 1**: Add new `[rules]` config section, keep legacy fields working
2. **Phase 2**: Log deprecation warnings when legacy fields are used
3. **Phase 3**: Remove legacy fields in future major version

## TOML Config Examples

### Basic IV Slope Rule

```toml
# Only enter when short-term IV is elevated vs long-term
[[rules.market]]
type = "iv_slope"
short_dte = 7
long_dte = 20
threshold_pp = 0.05  # 5 percentage points
```

### Multiple Rules (AND logic)

```toml
# Event-level filters
[[rules.event]]
type = "min_market_cap"
threshold = 1_000_000_000  # $1B

[[rules.event]]
type = "symbols"
include = ["AAPL", "MSFT", "GOOGL", "AMZN"]

# Market-level filters (require IV data)
[[rules.market]]
type = "max_entry_iv"
threshold = 1.5  # Don't enter if ATM IV > 150%

[[rules.market]]
type = "min_iv_ratio"
threshold = 1.2  # Short IV must be 20% higher than long IV

[[rules.market]]
type = "iv_vs_hv"
hv_window_days = 20
min_ratio = 1.1  # IV must be at least 10% above HV

# Trade-level filters
[[rules.trade]]
type = "entry_price_range"
min = 0.50
max = 50.00
```

### IV Slope + Volume Filter

```toml
[[rules.market]]
type = "iv_slope"
short_dte = 7
long_dte = 20
threshold_pp = 0.05

[[rules.market]]
type = "min_notional"
threshold = 100_000  # $100k minimum daily option activity
```

## CLI Usage Examples

```bash
# Enable IV slope rule with custom parameters
cs backtest --start 2024-01-01 --end 2024-12-31 \
  --entry-iv-slope \
  --iv-slope-short-dte 7 \
  --iv-slope-long-dte 20 \
  --iv-slope-threshold 0.05

# Combine with existing filters
cs backtest --start 2024-01-01 --end 2024-12-31 \
  --entry-iv-slope \
  --max-entry-iv 1.5 \
  --min-iv-ratio 1.2 \
  --min-market-cap 1000000000

# Add IV vs HV comparison
cs backtest --start 2024-01-01 --end 2024-12-31 \
  --entry-iv-vs-hv \
  --hv-window 20 \
  --iv-vs-hv-ratio 1.1
```

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iv_slope_rule_passes() {
        let rule = MarketRule::IvSlope {
            short_dte: 7,
            long_dte: 20,
            threshold_pp: 0.05,
        };

        let mut surface = MockIVSurface::new();
        surface.set_iv_at_dte(7, 0.35);   // 35%
        surface.set_iv_at_dte(20, 0.28);  // 28%

        let data = PreparedData { surface, .. };

        // 0.35 > 0.28 + 0.05 = 0.33 ✓
        assert!(rule.eval(&mock_event(), &data, None).unwrap());
    }

    #[test]
    fn test_iv_slope_rule_fails() {
        let rule = MarketRule::IvSlope {
            short_dte: 7,
            long_dte: 20,
            threshold_pp: 0.05,
        };

        let mut surface = MockIVSurface::new();
        surface.set_iv_at_dte(7, 0.30);   // 30%
        surface.set_iv_at_dte(20, 0.28);  // 28%

        let data = PreparedData { surface, .. };

        // 0.30 > 0.28 + 0.05 = 0.33 ✗
        assert!(!rule.eval(&mock_event(), &data, None).unwrap());
    }
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_backtest_with_iv_slope_rule() {
    let config = BacktestConfig {
        rules: FileRulesConfig {
            market: Some(vec![
                FileMarketRule::IvSlope {
                    short_dte: 7,
                    long_dte: 20,
                    threshold_pp: 0.05,
                }
            ]),
            ..Default::default()
        },
        ..test_config()
    };

    let result = BacktestUseCase::new(config)
        .execute(&repos)
        .await
        .unwrap();

    // Verify filtered trades
    assert!(result.dropped_events.iter()
        .any(|e| matches!(e, TradeGenerationError::RuleFiltered { rule, .. } if rule == "market_rules")));
}
```

## File Changes Summary

| File | Change Type | Description |
|------|-------------|-------------|
| `cs-domain/src/rules/mod.rs` | New | Module exports, RuleLevel enum |
| `cs-domain/src/rules/error.rs` | New | RuleError type |
| `cs-domain/src/rules/event/mod.rs` | New | EventRule enum + eval |
| `cs-domain/src/rules/market/mod.rs` | New | MarketRule enum + name |
| `cs-domain/src/rules/trade/mod.rs` | New | TradeRule enum + eval |
| `cs-domain/src/rules/config.rs` | New | RulesConfig with Default |
| `cs-backtest/src/config/rules.rs` | New | FileRulesConfig (partial) |
| `cs-backtest/src/config/mod.rs` | Modify | Add `rules` field |
| `cs-backtest/src/rules/mod.rs` | New | Module re-exports |
| `cs-backtest/src/rules/evaluator.rs` | New | RuleEvaluator |
| `cs-backtest/src/rules/market_eval.rs` | New | MarketRule::eval impl |
| `cs-backtest/src/backtest_use_case.rs` | Modify | Integrate rule evaluation |
| `cs-analytics/src/historical_vol.rs` | New | HV computation |
| `cs-analytics/src/iv_surface.rs` | Modify | Add atm_iv_at_dte() |
| `cs-cli/src/cli_args.rs` | Modify | Add CliRulesOverrides |
| `cs-cli/src/args/rules.rs` | New | RulesArgs (CLI wrapper) |
| `cs-cli/src/config/app.rs` | Modify | Add rules to AppConfig |

## Success Criteria

1. **All existing tests pass** - No regression in current behavior
2. **IV slope rule works** - Can filter trades by IV term structure
3. **Rules are configurable** - Via TOML and CLI
4. **Rules are composable** - Multiple rules evaluate as AND
5. **Legacy filters work** - Backward compatibility maintained
6. **Performance improved** - Market rules evaluated before expensive trade execution
7. **Logging complete** - Each rule evaluation logged with symbol and result
