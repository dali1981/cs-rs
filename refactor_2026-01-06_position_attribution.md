# Position Attribution Reintegration Design

**Date**: 2026-01-06
**Status**: Final Implementation Plan
**Goal**: Reintegrate position attribution with configurable IV sources for hedging and P&L attribution

---

## Executive Summary

The position attribution feature was removed in commit 855d867 (Jan 5, 20:17) during the generalization of hedging. This document designs the reintegration with additional configurability:

1. **Hedging Delta Computation**: Delta for rehedging can use different IV/HV sources
2. **P&L Attribution IV**: Greeks recomputation for attribution can use different IV/HV sources
3. **Realized Volatility Tracking**: Compute and report RV during hedging period
4. **Clear Naming**: Distinct concepts for hedging vs attribution volatility

---

## Current State Analysis

### What Exists (Unused)

**Domain Types** (`cs-domain/src/position/`):
- `PositionSnapshot` - Point-in-time position state with Greeks and hedge_shares
- `PositionGreeks` - Position-level Greeks (scaled by multiplier)
- `DailyAttribution` - Single day P&L breakdown by Greek
- `PositionAttribution` - Aggregated attribution over holding period

**Hedging** (`cs-domain/src/hedging.rs`):
- `HedgeState` - State machine using gamma approximation for delta updates
- `HedgeConfig` - Strategy configuration (TimeBased, DeltaThreshold, GammaDollar)
- `HedgePosition` - Cumulative hedge transactions

**IV Interpolation** (`cs-analytics/src/iv_model.rs`):
- `PricingIVProvider` trait - Interface for IV interpolation
- `StickyStrikePricing`, `StickyMoneynessPricing`, `StickyDeltaPricing` - Implementations
- `PricingModel` enum - Configuration selector

**Historical/Realized Volatility** (`cs-analytics/src/realized_volatility.rs`):
- `realized_volatility(prices, window, annualization_factor)` - Computes HV from price history
- Used in `MinuteAlignedIvUseCase` for ATM IV analysis
- `EquityDataRepository::get_bars()` provides daily OHLCV data

### What's Missing

1. **Snapshot Collection**: No code collects daily (open, close) snapshots during holding period
2. **HV at Entry**: No code captures Historical Volatility at trade entry
3. **Realized Volatility Tracking**: No code computes RV during hedging period
4. **Configurable IV/HV Sources**: Hedging uses gamma approximation, not configurable

### Current P&L Tracking Architecture (Redundancy Analysis)

| Component | Purpose | Location | Fields |
|-----------|---------|----------|--------|
| **TradeResult trait** | Canonical P&L source | `cs-domain/src/trade/rollable.rs:63-149` | `pnl()`, `entry_cost()`, `exit_value()`, `entry_iv()`, `exit_iv()` |
| **SessionPnL** | Session transport type | `cs-backtest/src/session_executor.rs:42-52` | `pnl`, `entry_cost`, `exit_value`, `iv_entry`, `iv_exit` |
| **RollPeriod** | Rolling aggregation | `cs-domain/src/entities/rolling_result.rs:104-145` | `pnl`, `entry_debit`, `exit_credit`, `iv_entry`, `iv_exit`, `position_attribution` |
| **BatchResult** | Batch analytics | `cs-backtest/src/session_executor.rs:93-174` | `total_pnl()`, `avg_pnl()`, `win_rate()` |

**Analysis**: **No true redundancy**. Each serves a different layer:
- `TradeResult` → Canonical source (all data)
- `SessionPnL` → Lightweight transport for session-based execution (subset extraction)
- `RollPeriod` → Persistence/serialization for rolling campaigns (includes `position_attribution`)
- `BatchResult` → Analytics aggregation over multiple sessions

**Recommendation**:
- `SessionPnL` should extract `position_attribution` from `TradeResult` when available
- `RollPeriod.position_attribution` is the right place for detailed attribution
- No changes needed to existing structures

---

## Design Proposal

### Core Concept: Volatility Source Strategy

Introduce a unified concept for "where to get volatility" that applies to both hedging and attribution.

```rust
/// Strategy for obtaining volatility during position lifetime
///
/// This determines how we get volatility values when:
/// 1. Recomputing Greeks for hedging decisions
/// 2. Recomputing Greeks for P&L attribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VolatilitySource {
    /// Use IV from the trade entry (captured once, never changes)
    /// Fastest, but ignores market IV evolution
    EntryIV,

    /// Build fresh IV surface at each observation time
    /// Most accurate, but requires market data lookups
    CurrentMarketIV,

    /// Use historical average IV over a lookback period
    /// Smooths out IV noise
    HistoricalAverageIV {
        lookback_days: u32,
    },

    /// Use Historical Volatility (HV) of the underlying at trade entry
    /// Based on realized price moves, not options market
    EntryHV {
        window: u32,  // e.g., 20, 30, 60 days
    },

    /// Use Historical Volatility recomputed at each observation
    /// Tracks actual underlying volatility evolution
    CurrentHV {
        window: u32,
    },
}

impl Default for VolatilitySource {
    fn default() -> Self {
        VolatilitySource::EntryIV  // Matches current gamma-only behavior semantics
    }
}
```

### Naming Convention

| Concept | Purpose | Config Field |
|---------|---------|--------------|
| `hedging_vol_source` | Volatility for delta computation in rehedge decisions | `HedgeConfig` |
| `attribution_vol_source` | Volatility for Greeks recomputation in P&L attribution | `AttributionConfig` |
| `pricing_model` | IV interpolation method (sticky-strike, etc.) | Both configs (only for IV sources) |

### Realized Volatility Tracking

Add tracking of actual realized volatility during the hedging period:

```rust
/// Realized volatility metrics computed during hedging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealizedVolatilityMetrics {
    /// Entry HV (Historical Volatility at trade entry)
    pub entry_hv: Option<f64>,

    /// Realized volatility during holding period
    /// Computed from actual spot price moves during hedging
    pub realized_vol: f64,

    /// IV at entry (for comparison)
    pub entry_iv: Option<f64>,

    /// IV at exit
    pub exit_iv: Option<f64>,

    /// Vol of vol (volatility of the volatility - optional)
    pub vol_of_vol: Option<f64>,

    /// Number of observations used
    pub num_observations: usize,

    /// IV premium/discount: (entry_iv - entry_hv) / entry_hv × 100
    pub iv_premium_at_entry: Option<f64>,

    /// Realized vs Entry IV: (realized_vol - entry_iv) / entry_iv × 100
    pub realized_vs_implied: Option<f64>,
}

impl RealizedVolatilityMetrics {
    /// Compute from spot observations during hedging
    pub fn from_spot_history(
        spots: &[(DateTime<Utc>, f64)],
        entry_hv: Option<f64>,
        entry_iv: Option<f64>,
        exit_iv: Option<f64>,
    ) -> Self {
        // Extract prices in order
        let prices: Vec<f64> = spots.iter().map(|(_, p)| *p).collect();

        // Use full period for realized vol (annualized)
        let realized_vol = realized_volatility(&prices, prices.len().saturating_sub(1), 252.0)
            .unwrap_or(0.0);

        let iv_premium_at_entry = match (entry_iv, entry_hv) {
            (Some(iv), Some(hv)) if hv > 0.0 => Some((iv - hv) / hv * 100.0),
            _ => None,
        };

        let realized_vs_implied = match entry_iv {
            Some(iv) if iv > 0.0 => Some((realized_vol - iv) / iv * 100.0),
            _ => None,
        };

        Self {
            entry_hv,
            realized_vol,
            entry_iv,
            exit_iv,
            vol_of_vol: None,  // Can be computed if needed
            num_observations: prices.len(),
            iv_premium_at_entry,
            realized_vs_implied,
        }
    }
}
```

### Configuration Structures

```rust
/// Configuration for P&L attribution computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionConfig {
    /// Whether to compute attribution (can be expensive)
    pub enabled: bool,

    /// How to obtain volatility for Greeks recomputation
    pub vol_source: VolatilitySource,

    /// IV interpolation model for surface lookups (when using IV sources)
    pub pricing_model: PricingModel,

    /// Times to snapshot each day (default: market open + close)
    pub snapshot_times: SnapshotTimes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SnapshotTimes {
    /// Market open (9:30 ET) and close (16:00 ET)
    OpenClose,
    /// Custom times
    Custom { open_time: NaiveTime, close_time: NaiveTime },
    /// Only end of day
    CloseOnly,
}

impl Default for AttributionConfig {
    fn default() -> Self {
        Self {
            enabled: false,  // Opt-in, as it adds cost
            vol_source: VolatilitySource::CurrentMarketIV,  // Most accurate for attribution
            pricing_model: PricingModel::StickyMoneyness,
            snapshot_times: SnapshotTimes::OpenClose,
        }
    }
}
```

### Extended HedgeConfig

```rust
/// Configuration for delta hedging (EXTENDED)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HedgeConfig {
    // Existing fields
    pub strategy: HedgeStrategy,
    pub max_rehedges: Option<usize>,
    pub min_hedge_size: i32,
    pub transaction_cost_per_share: Decimal,
    pub contract_multiplier: i32,

    // NEW: Volatility source for delta computation
    /// How to compute delta for rehedge decisions
    /// Default: GammaApproximation (current behavior)
    pub delta_computation: DeltaComputation,

    // NEW: Track realized volatility during hedging
    /// Whether to compute and report realized volatility metrics
    pub track_realized_vol: bool,
}

/// How to compute delta for hedging decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeltaComputation {
    /// Use gamma × ΔS approximation (fast, current behavior)
    /// Delta evolves: δ' = δ + γ × (S' - S)
    GammaApproximation,

    /// Recompute from IV at trade entry
    /// Uses Black-Scholes with entry IV, current spot, remaining DTE
    EntryIV {
        pricing_model: PricingModel,
    },

    /// Recompute from current market IV surface
    /// Most accurate, requires IV surface build at each rehedge
    CurrentMarketIV {
        pricing_model: PricingModel,
    },

    /// Use Historical Volatility at trade entry for delta computation
    /// HV is computed from underlying price history, not options
    EntryHV {
        window: u32,  // e.g., 20 for 20-day HV
    },

    /// Recompute HV at each rehedge from recent underlying prices
    CurrentHV {
        window: u32,
    },

    /// Use historical average IV over lookback period
    HistoricalAverageIV {
        lookback_days: u32,
        pricing_model: PricingModel,
    },
}

impl Default for DeltaComputation {
    fn default() -> Self {
        DeltaComputation::GammaApproximation
    }
}
```

### Extended HedgePosition (for RV tracking)

```rust
/// Cumulative hedge position state (EXTENDED)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HedgePosition {
    // Existing fields
    pub cumulative_shares: i32,
    pub hedges: Vec<HedgeAction>,
    pub realized_pnl: Decimal,
    pub unrealized_pnl: Decimal,
    pub total_cost: Decimal,

    // NEW: Realized volatility tracking
    /// Spot observations during hedging (for RV computation)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spot_history: Vec<(DateTime<Utc>, f64)>,

    /// Realized volatility metrics (computed at finalize)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realized_vol_metrics: Option<RealizedVolatilityMetrics>,
}
```

---

## Architecture Changes

### New Module: `cs-backtest/src/attribution/`

```
cs-backtest/src/attribution/
├── mod.rs                    # Module exports
├── snapshot_collector.rs     # Collects daily snapshots
├── vol_source_provider.rs    # Resolves VolatilitySource to actual values
├── attribution_computer.rs   # Computes attribution from snapshots
└── realized_vol_tracker.rs   # Tracks RV during hedging
```

### Component: VolatilitySourceProvider

```rust
/// Resolves VolatilitySource configuration to actual volatility values
///
/// This is the "adapter" between abstract config and concrete data access.
pub struct VolatilitySourceProvider<'a> {
    options_repo: &'a dyn OptionsDataRepository,
    equity_repo: &'a dyn EquityDataRepository,
    symbol: &'a str,
    strike: Decimal,
    expiration: NaiveDate,
    is_call: bool,

    // Cached values
    entry_iv: Option<f64>,
    entry_hv: Option<f64>,
    entry_surface: Option<IVSurface>,
}

impl<'a> VolatilitySourceProvider<'a> {
    /// Initialize with entry-time data
    pub async fn new(
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        symbol: &'a str,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
        entry_time: DateTime<Utc>,
        hv_window: Option<u32>,
    ) -> Result<Self, AttributionError> {
        // Pre-compute entry HV if needed
        let entry_hv = if let Some(window) = hv_window {
            Self::compute_hv(equity_repo, symbol, entry_time, window).await?
        } else {
            None
        };

        Ok(Self {
            options_repo,
            equity_repo,
            symbol,
            strike,
            expiration,
            is_call,
            entry_iv: None,
            entry_hv,
            entry_surface: None,
        })
    }

    /// Get volatility at a specific time based on configured source
    pub async fn get_volatility(
        &self,
        source: &VolatilitySource,
        at_time: DateTime<Utc>,
        pricing_model: Option<PricingModel>,
    ) -> Result<f64, AttributionError> {
        match source {
            VolatilitySource::EntryIV => {
                self.entry_iv.ok_or(AttributionError::NoEntryIV)
            }

            VolatilitySource::CurrentMarketIV => {
                let chain = self.options_repo
                    .get_option_chain(self.symbol, at_time)
                    .await?;
                let surface = build_iv_surface_minute_aligned(
                    &chain, self.equity_repo, self.symbol
                ).await?;
                let provider = pricing_model.unwrap_or_default().to_provider();
                provider.get_iv(&surface, self.strike, self.expiration, self.is_call)
                    .ok_or(AttributionError::IVInterpolationFailed)
            }

            VolatilitySource::EntryHV { .. } => {
                self.entry_hv.ok_or(AttributionError::NoEntryHV)
            }

            VolatilitySource::CurrentHV { window } => {
                Self::compute_hv(self.equity_repo, self.symbol, at_time, *window).await
            }

            VolatilitySource::HistoricalAverageIV { lookback_days } => {
                self.compute_historical_average_iv(at_time, *lookback_days, pricing_model).await
            }
        }
    }

    /// Compute Historical Volatility from underlying prices
    async fn compute_hv(
        equity_repo: &dyn EquityDataRepository,
        symbol: &str,
        at_time: DateTime<Utc>,
        window: u32,
    ) -> Result<f64, AttributionError> {
        let end_date = at_time.date_naive();
        let start_date = end_date - chrono::Duration::days(window as i64 + 10);  // Buffer

        let bars = equity_repo.get_bars(symbol, start_date, end_date).await
            .map_err(|e| AttributionError::DataError(e.to_string()))?;

        let closes: Vec<f64> = bars.column("close")
            .map_err(|_| AttributionError::DataError("No close column".into()))?
            .f64()
            .map_err(|_| AttributionError::DataError("Invalid close type".into()))?
            .into_no_null_iter()
            .collect();

        realized_volatility(&closes, window as usize, 252.0)
            .ok_or(AttributionError::InsufficientData)
    }
}
```

### Component: RealizedVolatilityTracker

```rust
/// Tracks spot prices during hedging for realized volatility computation
pub struct RealizedVolatilityTracker {
    spot_history: Vec<(DateTime<Utc>, f64)>,
    entry_hv: Option<f64>,
    entry_iv: Option<f64>,
}

impl RealizedVolatilityTracker {
    pub fn new(entry_hv: Option<f64>, entry_iv: Option<f64>) -> Self {
        Self {
            spot_history: Vec::new(),
            entry_hv,
            entry_iv,
        }
    }

    /// Record a spot observation
    pub fn record(&mut self, timestamp: DateTime<Utc>, spot: f64) {
        self.spot_history.push((timestamp, spot));
    }

    /// Compute final metrics
    pub fn finalize(self, exit_iv: Option<f64>) -> RealizedVolatilityMetrics {
        RealizedVolatilityMetrics::from_spot_history(
            &self.spot_history,
            self.entry_hv,
            self.entry_iv,
            exit_iv,
        )
    }
}
```

### Integration with TradeExecutor

```rust
impl<T> TradeExecutor<T>
where
    T: RollableTrade + ExecutableTrade,
{
    /// Apply hedging with optional attribution and RV tracking
    async fn apply_hedging(
        &self,
        result: &mut <T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
    ) -> Result<(), String> {
        let hedge_config = self.hedge_config.as_ref()
            .ok_or("Hedge config not set")?;

        // Initialize RV tracker if enabled
        let mut rv_tracker = if hedge_config.track_realized_vol {
            let entry_hv = self.compute_entry_hv(result.symbol(), entry_time).await.ok();
            let entry_iv = result.entry_iv().map(|iv| iv.primary);
            Some(RealizedVolatilityTracker::new(entry_hv, entry_iv))
        } else {
            None
        };

        // === Hedging Phase ===
        let mut hedge_position = match &hedge_config.delta_computation {
            DeltaComputation::GammaApproximation => {
                self.hedge_with_gamma_approximation(result, rehedge_times, &mut rv_tracker).await?
            }
            DeltaComputation::EntryIV { pricing_model } => {
                self.hedge_with_entry_iv(result, rehedge_times, *pricing_model, &mut rv_tracker).await?
            }
            DeltaComputation::CurrentMarketIV { pricing_model } => {
                self.hedge_with_current_market_iv(result, rehedge_times, *pricing_model, &mut rv_tracker).await?
            }
            DeltaComputation::EntryHV { window } => {
                self.hedge_with_entry_hv(result, rehedge_times, *window, &mut rv_tracker).await?
            }
            DeltaComputation::CurrentHV { window } => {
                self.hedge_with_current_hv(result, rehedge_times, *window, &mut rv_tracker).await?
            }
            DeltaComputation::HistoricalAverageIV { lookback_days, pricing_model } => {
                self.hedge_with_historical_iv(result, rehedge_times, *lookback_days, *pricing_model, &mut rv_tracker).await?
            }
        };

        // Finalize RV tracking
        if let Some(tracker) = rv_tracker {
            let exit_iv = result.exit_iv().map(|iv| iv.primary);
            hedge_position.realized_vol_metrics = Some(tracker.finalize(exit_iv));
        }

        // === Attribution Phase (Optional) ===
        let attribution = if let Some(ref attr_config) = self.attribution_config {
            if attr_config.enabled {
                Some(self.compute_attribution(
                    result,
                    entry_time,
                    exit_time,
                    &hedge_position,
                    attr_config,
                ).await?)
            } else {
                None
            }
        } else {
            None
        };

        // Apply results
        let hedge_pnl = hedge_position.calculate_pnl(result.spot_at_exit());
        let total_pnl = result.pnl() + hedge_pnl - hedge_position.total_cost;
        result.apply_hedge_results(hedge_position, hedge_pnl, total_pnl, attribution);

        Ok(())
    }

    /// Compute entry HV using existing infrastructure
    async fn compute_entry_hv(&self, symbol: &str, entry_time: DateTime<Utc>) -> Result<f64, String> {
        let end_date = entry_time.date_naive();
        let start_date = end_date - chrono::Duration::days(40);

        let bars = self.equity_repo.get_bars(symbol, start_date, end_date).await
            .map_err(|e| e.to_string())?;

        let closes: Vec<f64> = bars.column("close")
            .map_err(|_| "No close column")?
            .f64()
            .map_err(|_| "Invalid type")?
            .into_no_null_iter()
            .collect();

        realized_volatility(&closes, 20, 252.0)
            .ok_or_else(|| "Insufficient data for HV".to_string())
    }
}
```

---

## Detailed Implementation: Delta Computation Modes

### Mode 1: GammaApproximation (Current)

```rust
async fn hedge_with_gamma_approximation(
    &self,
    result: &<T as ExecutableTrade>::Result,
    rehedge_times: Vec<DateTime<Utc>>,
    rv_tracker: &mut Option<RealizedVolatilityTracker>,
) -> Result<HedgePosition, String> {
    let mut hedge_state = HedgeState::new(
        self.hedge_config.as_ref().unwrap().clone(),
        result.net_delta().unwrap_or(0.0),
        result.net_gamma().unwrap_or(0.0),
        result.spot_at_entry(),
    );

    // Record entry spot for RV tracking
    if let Some(ref mut tracker) = rv_tracker {
        tracker.record(result.entry_time(), result.spot_at_entry());
    }

    for rehedge_time in rehedge_times {
        if hedge_state.at_max_rehedges() { break; }
        let spot = self.equity_repo.get_spot_price(result.symbol(), rehedge_time).await?;
        let spot_f64 = spot.to_f64();

        hedge_state.update(rehedge_time, spot_f64);

        // Track for RV computation
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(rehedge_time, spot_f64);
        }
    }

    // Record exit spot
    if let Some(ref mut tracker) = rv_tracker {
        tracker.record(result.exit_time(), result.spot_at_exit());
    }

    Ok(hedge_state.finalize(result.spot_at_exit()))
}
```

### Mode 2: EntryHV (NEW)

```rust
async fn hedge_with_entry_hv(
    &self,
    result: &<T as ExecutableTrade>::Result,
    rehedge_times: Vec<DateTime<Utc>>,
    window: u32,
    rv_tracker: &mut Option<RealizedVolatilityTracker>,
) -> Result<HedgePosition, String> {
    // Compute HV at entry
    let entry_hv = self.compute_entry_hv(result.symbol(), result.entry_time()).await?;

    let mut position = HedgePosition::new();
    let symbol = result.symbol();
    let strike = result.strike();
    let expiration = result.expiration();
    let is_call = result.is_call();

    let mut current_shares = 0i32;
    let mut last_delta = result.net_delta().unwrap_or(0.0);

    // Track entry
    if let Some(ref mut tracker) = rv_tracker {
        tracker.record(result.entry_time(), result.spot_at_entry());
    }

    for rehedge_time in rehedge_times {
        let spot = self.equity_repo.get_spot_price(symbol, rehedge_time).await?.to_f64();

        // Track for RV
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(rehedge_time, spot);
        }

        // Recompute delta from ENTRY HV with CURRENT spot and remaining DTE
        let dte = (expiration - rehedge_time.date_naive()).num_days() as f64 / 365.0;
        if dte <= 0.0 { continue; }

        // Use entry_hv as volatility for Black-Scholes delta
        let new_delta = bs_delta(spot, strike.to_f64(), dte, entry_hv, is_call, 0.05);

        // Check rehedge trigger
        if self.should_rehedge(new_delta, current_shares, spot) {
            let shares = self.compute_hedge_shares(new_delta, current_shares);
            current_shares += shares;

            position.add_hedge(HedgeAction {
                timestamp: rehedge_time,
                shares,
                spot_price: spot,
                delta_before: last_delta,
                delta_after: new_delta,
                cost: self.transaction_cost(shares),
            });

            last_delta = new_delta;
        }
    }

    // Track exit
    if let Some(ref mut tracker) = rv_tracker {
        tracker.record(result.exit_time(), result.spot_at_exit());
    }

    position.unrealized_pnl = position.calculate_pnl(result.spot_at_exit());
    Ok(position)
}
```

### Mode 3: CurrentHV (NEW)

```rust
async fn hedge_with_current_hv(
    &self,
    result: &<T as ExecutableTrade>::Result,
    rehedge_times: Vec<DateTime<Utc>>,
    window: u32,
    rv_tracker: &mut Option<RealizedVolatilityTracker>,
) -> Result<HedgePosition, String> {
    // Similar to EntryHV, but recompute HV at each rehedge time
    // using the most recent `window` days of price history

    for rehedge_time in rehedge_times {
        // Compute current HV
        let current_hv = self.compute_hv_at(result.symbol(), rehedge_time, window).await?;

        // Use current_hv for delta computation
        let new_delta = bs_delta(spot, strike.to_f64(), dte, current_hv, is_call, 0.05);

        // ... rest same as EntryHV
    }
}
```

---

## CLI Integration

### New CLI Flags

```bash
# Hedging delta computation mode
--hedge-delta-mode gamma            # Current behavior (default)
--hedge-delta-mode entry-iv         # Recompute from entry IV
--hedge-delta-mode current-iv       # Build IV surface each rehedge
--hedge-delta-mode entry-hv         # Use Historical Volatility at entry
--hedge-delta-mode current-hv       # Recompute HV each rehedge
--hedge-delta-mode historical-iv    # Use historical average IV

# HV/IV windows and lookbacks
--hv-window 20                      # Window for HV computation (default: 20 days)
--iv-lookback 30                    # Lookback for historical average IV

# Realized volatility tracking
--track-realized-vol                # Enable RV tracking and reporting

# Attribution
--attribution                       # Enable P&L attribution
--attribution-vol-source entry-iv   # Use entry IV for attribution
--attribution-vol-source current-iv # Use current market IV (default when enabled)
--attribution-vol-source entry-hv   # Use entry HV
--attribution-vol-source current-hv # Recompute HV daily
```

### Configuration File Example

```toml
[hedging]
strategy = "time_based"
interval_hours = 4
max_rehedges = 10
transaction_cost = 0.005
track_realized_vol = true

[hedging.delta_computation]
mode = "entry_hv"           # or "gamma", "entry_iv", "current_iv", "current_hv", "historical_iv"
hv_window = 20              # for HV modes
pricing_model = "sticky_moneyness"  # for IV modes

[attribution]
enabled = true
vol_source = "current_iv"   # or "entry_iv", "entry_hv", "current_hv"
hv_window = 20
pricing_model = "sticky_moneyness"
snapshot_times = "open_close"
```

---

## Output Enhancement

### RV Metrics in JSON Output

```json
{
  "hedge_position": {
    "cumulative_shares": -30,
    "rehedge_count": 5,
    "total_cost": 0.75,
    "realized_vol_metrics": {
      "entry_hv": 0.22,
      "realized_vol": 0.28,
      "entry_iv": 0.35,
      "exit_iv": 0.25,
      "num_observations": 12,
      "iv_premium_at_entry": 59.1,
      "realized_vs_implied": -20.0
    }
  }
}
```

### CLI Summary Output

```
=== Realized Volatility Metrics ===
Entry HV (20-day):    22.0%
Entry IV:             35.0%
IV Premium at Entry:  +59.1%  (IV was rich vs HV)

Realized Vol:         28.0%
Exit IV:              25.0%

Realized vs Entry IV: -20.0%  (actual moves lower than implied)
```

---

## Performance Considerations

| Mode | IV Builds | HV Computes | Speed | Accuracy |
|------|-----------|-------------|-------|----------|
| GammaApproximation | 0 | 0 | ★★★★ | ★ |
| EntryIV | 0 | 0 | ★★★★ | ★★ |
| EntryHV | 0 | 1 | ★★★★ | ★★ |
| HistoricalAverageIV | N | 0 | ★★ | ★★★ |
| CurrentHV | 0 | N | ★★★ | ★★★ |
| CurrentMarketIV | N | 0 | ★ | ★★★★ |

**Recommendations**:
- **Hedging**: Default to `EntryHV` for balance of speed and accuracy
- **Attribution**: Default to `CurrentMarketIV` for accurate P&L decomposition
- **RV Tracking**: Always enable (minimal overhead, valuable insight)

---

## Implementation Plan

### Phase 1: Foundation (No Behavior Change)
**Files**: `cs-domain/src/hedging.rs`

1. Add `DeltaComputation` enum with `GammaApproximation` as default
2. Add `track_realized_vol: bool` to `HedgeConfig`
3. Add `spot_history` and `realized_vol_metrics` to `HedgePosition`
4. Add `RealizedVolatilityMetrics` struct
5. **Test**: Existing tests pass, no behavior change

### Phase 2: RV Tracking Infrastructure
**Files**: `cs-backtest/src/trade_executor.rs`, `cs-analytics/src/realized_volatility.rs`

1. Create `RealizedVolatilityTracker`
2. Integrate spot recording in `apply_hedging()`
3. Compute and store `RealizedVolatilityMetrics` on finalize
4. **Test**: RV metrics populated in output

### Phase 3: EntryHV Mode
**Files**: `cs-backtest/src/trade_executor.rs`

1. Implement `hedge_with_entry_hv()`
2. Use existing `realized_volatility()` function
3. Add CLI flag `--hedge-delta-mode entry-hv`
4. **Test**: Compare hedge results with EntryHV vs GammaApproximation

### Phase 4: EntryIV Mode
**Files**: `cs-backtest/src/trade_executor.rs`

1. Implement `hedge_with_entry_iv()`
2. Use `CompositeIV` from trade result
3. Add CLI flag `--hedge-delta-mode entry-iv`
4. **Test**: Delta recomputation matches expected values

### Phase 5: CurrentHV and CurrentMarketIV Modes
**Files**: `cs-backtest/src/trade_executor.rs`, new `attribution/vol_source_provider.rs`

1. Create `VolatilitySourceProvider` adapter
2. Implement `hedge_with_current_hv()` and `hedge_with_current_market_iv()`
3. Add CLI flags
4. **Test**: IV surface builds at each rehedge

### Phase 6: Attribution Reintegration
**Files**: `cs-backtest/src/attribution/`

1. Create attribution module
2. Implement `SnapshotCollector`
3. Wire into `apply_hedge_results()` → `RollPeriod.position_attribution`
4. Update CLI output to show attribution breakdown
5. **Test**: Daily attribution matches expected P&L decomposition

### Phase 7: CLI Integration and Documentation
**Files**: `cs-cli/src/main.rs`, `cs-cli/src/config.rs`

1. Add all CLI flags
2. Add TOML config support
3. Update help text
4. Write documentation

---

## Files to Modify

### New Files
1. `cs-backtest/src/attribution/mod.rs`
2. `cs-backtest/src/attribution/snapshot_collector.rs`
3. `cs-backtest/src/attribution/vol_source_provider.rs`
4. `cs-backtest/src/attribution/realized_vol_tracker.rs`

### Modified Files
1. `cs-domain/src/hedging.rs` - Add `DeltaComputation`, `RealizedVolatilityMetrics`, extend `HedgeConfig`, `HedgePosition`
2. `cs-backtest/src/trade_executor.rs` - Add delta computation modes, RV tracking
3. `cs-backtest/src/lib.rs` - Export attribution module
4. `cs-cli/src/main.rs` - Add CLI flags
5. `cs-cli/src/config.rs` - Add attribution config
6. `cs-domain/src/entities/rolling_result.rs` - Ensure `RollPeriod.position_attribution` is used

### No Changes Needed
- `SessionPnL` - Keep as lightweight transport (no attribution needed at this level)
- `BatchResult` - Keep analytics methods as-is
- `TradeResult` trait - Already has `apply_hedge_results()` signature with attribution

---

## Testing Strategy

### Unit Tests
1. `RealizedVolatilityMetrics::from_spot_history()` - verify computation
2. `VolatilitySourceProvider::get_volatility()` - test each source mode
3. `DeltaComputation` modes - verify delta values differ appropriately

### Integration Tests
1. End-to-end with `--track-realized-vol` - verify output format
2. End-to-end with `--attribution` - verify daily breakdown
3. Compare hedge P&L across all delta computation modes

### Parity Tests
1. `GammaApproximation` results match current behavior exactly
2. `EntryIV` vs `GammaApproximation` - delta should diverge with spot moves

---

## Open Questions (Resolved)

| Question | Resolution |
|----------|------------|
| Risk-free rate | Use global default (0.05), configurable later if needed |
| Dividend yield | Not included initially, can add later |
| Multi-leg trades | Attribution uses position-level Greeks (already aggregated) |
| SessionPnL redundancy | No redundancy - different layer, keep as transport type |

---

## References

- Derman (1999): "Regimes of Volatility" - IV model behaviors
- `cs-domain/src/position/position_attribution.rs` - Existing attribution logic
- `cs-analytics/src/iv_model.rs` - PricingIVProvider implementations
- `cs-analytics/src/realized_volatility.rs` - Existing HV computation
