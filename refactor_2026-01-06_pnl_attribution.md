# P&L Attribution Implementation Plan

**Date**: 2026-01-06
**Status**: Implementation Plan
**Prerequisite**: Hedge State Strategy Pattern Refactoring (Phase 1-5 complete)
**Goal**: Implement daily P&L attribution with Greeks decomposition

---

## Executive Summary

P&L Attribution breaks down total P&L into components:
- **Delta P&L**: Gross, hedge, and net directional exposure
- **Gamma P&L**: Convexity gains/losses from spot moves
- **Theta P&L**: Time decay
- **Vega P&L**: IV change impact
- **Unexplained**: Actual P&L minus sum of attributed components

---

## Current State

### What Exists (Complete Domain Model)

**`cs-domain/src/position/position_attribution.rs`**:
- `DailyAttribution` - Single day P&L breakdown
- `PositionAttribution` - Aggregated attribution with totals
- `PositionAttribution::from_snapshots(Vec<(PositionSnapshot, PositionSnapshot)>, actual_pnl)`

**`cs-domain/src/position/daily_snapshot.rs`**:
- `PositionGreeks` - Position-level Greeks (scaled by multiplier)
- `PositionSnapshot` - Point-in-time state (timestamp, spot, iv, greeks, hedge_shares)

### What's Missing (Orchestration Layer)

1. **Snapshot Collection**: No code collects (open, close) snapshots during holding period
2. **Greeks Recomputation**: No code builds IV surface and computes fresh Greeks daily
3. **Hedge Timeline Integration**: No code maps hedge_shares at each snapshot time
4. **Integration with HedgeState**: Attribution not wired into finalize()

### Current Placeholder Code

All result types set `position_attribution: None`:
```
cs-backtest/src/execution/straddle_impl.rs:140
cs-backtest/src/execution/straddle_impl.rs:197
cs-backtest/src/execution/calendar_spread_impl.rs:183
cs-backtest/src/execution/calendar_spread_impl.rs:250
cs-backtest/src/execution/iron_butterfly_impl.rs:251
cs-backtest/src/execution/iron_butterfly_impl.rs:317
cs-backtest/src/execution/calendar_straddle_impl.rs:240
cs-backtest/src/execution/calendar_straddle_impl.rs:301
cs-backtest/src/trade_executor.rs:481
```

---

## Design

### Integration with Strategy Pattern Architecture

Attribution fits naturally into the refactored `HedgeState<P>`:

```
HedgeState<DeltaProvider>
├── delta_provider: P
├── spot_history: Vec<(DateTime, f64)>  // For RV (already exists)
├── snapshot_collector: Option<SnapshotCollector>  // NEW: For attribution
└── finalize() → HedgePosition with attribution
```

### Core Components

```
cs-backtest/src/attribution/
├── mod.rs                    # Module exports
├── snapshot_collector.rs     # Collects daily (open, close) snapshots
├── greeks_computer.rs        # Recomputes position Greeks from IV surface
└── config.rs                 # AttributionConfig
```

### Data Flow

```
Entry Time                    Daily Market Open/Close               Exit Time
    │                               │                                   │
    ▼                               ▼                                   ▼
Trade Executed            SnapshotCollector.collect()            Finalize
    │                               │                                   │
    │                    ┌──────────┴──────────┐                        │
    │                    ▼                     ▼                        │
    │              Build IV Surface      Get Hedge Shares               │
    │                    │                     │                        │
    │                    ▼                     ▼                        │
    │              Compute Greeks       PositionSnapshot                │
    │                    │                     │                        │
    │                    └─────────────────────┘                        │
    │                               │                                   │
    │                               ▼                                   │
    │                    DailyAttribution::compute()                    │
    │                               │                                   │
    │                               ▼                                   │
    └───────────────────────────────┴───────────────────────────────────┘
                                    │
                                    ▼
                        PositionAttribution::from_daily()
```

---

## Implementation

### Phase 1: AttributionConfig (`cs-domain/src/hedging.rs`)

Add configuration to control attribution behavior:

```rust
/// Configuration for P&L attribution computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionConfig {
    /// Whether to compute attribution (adds overhead)
    pub enabled: bool,

    /// Volatility source for Greeks recomputation
    /// Note: Uses same VolatilitySource as hedging delta computation
    pub vol_source: VolatilitySource,

    /// IV interpolation model (for CurrentMarketIV source)
    #[serde(default)]
    pub pricing_model: PricingModel,

    /// Snapshot times configuration
    #[serde(default)]
    pub snapshot_times: SnapshotTimes,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum SnapshotTimes {
    /// Market open (9:30 ET) and close (16:00 ET)
    #[default]
    OpenClose,
    /// Only end of day (cheaper, less accurate)
    CloseOnly,
    /// Custom times (hour, minute) in Eastern Time
    Custom {
        open_hour: u32,
        open_minute: u32,
        close_hour: u32,
        close_minute: u32,
    },
}

impl Default for AttributionConfig {
    fn default() -> Self {
        Self {
            enabled: false,  // Opt-in
            vol_source: VolatilitySource::CurrentMarketIV,  // Most accurate
            pricing_model: PricingModel::default(),
            snapshot_times: SnapshotTimes::OpenClose,
        }
    }
}
```

### Phase 2: GreeksComputer (`cs-backtest/src/attribution/greeks_computer.rs`)

Recompute position-level Greeks from IV surface:

```rust
use cs_domain::trade::CompositeTrade;
use cs_domain::position::PositionGreeks;
use cs_analytics::{bs_greeks, Greeks, IVSurface, PricingIVProvider};
use finq_core::OptionType;

/// Computes position-level Greeks for attribution
pub struct GreeksComputer<'a, T: CompositeTrade> {
    trade: &'a T,
    contract_multiplier: i32,
    risk_free_rate: f64,
}

impl<'a, T: CompositeTrade> GreeksComputer<'a, T> {
    pub fn new(trade: &'a T, contract_multiplier: i32, risk_free_rate: f64) -> Self {
        Self { trade, contract_multiplier, risk_free_rate }
    }

    /// Compute position Greeks from a single volatility value
    ///
    /// Used for EntryHV, EntryIV, CurrentHV modes where we have one vol for all legs.
    pub fn compute_with_flat_vol(
        &self,
        spot: f64,
        volatility: f64,
        at_time: DateTime<Utc>,
    ) -> PositionGreeks {
        let mut total = Greeks::default();

        for (leg, position) in self.trade.legs() {
            let tte = (leg.expiration - at_time.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 { continue; }

            let is_call = leg.option_type == OptionType::Call;
            let strike = leg.strike.value().to_f64().unwrap_or(0.0);

            let leg_greeks = bs_greeks(
                spot,
                strike,
                tte,
                volatility,
                is_call,
                self.risk_free_rate,
            );

            // Apply position sign (long = +1, short = -1)
            let sign = position.sign();
            total.delta += leg_greeks.delta * sign;
            total.gamma += leg_greeks.gamma * sign;
            total.theta += leg_greeks.theta * sign;
            total.vega += leg_greeks.vega * sign;
        }

        // Scale to position level
        PositionGreeks::from_per_share(&total, self.contract_multiplier)
    }

    /// Compute position Greeks from IV surface (per-leg IV)
    ///
    /// Used for CurrentMarketIV mode where each leg can have different IV.
    pub fn compute_with_surface(
        &self,
        spot: f64,
        surface: &IVSurface,
        provider: &dyn PricingIVProvider,
        at_time: DateTime<Utc>,
    ) -> PositionGreeks {
        let mut total = Greeks::default();

        for (leg, position) in self.trade.legs() {
            let tte = (leg.expiration - at_time.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 { continue; }

            let is_call = leg.option_type == OptionType::Call;
            let strike = leg.strike.value();
            let strike_f64 = strike.to_f64().unwrap_or(0.0);

            // Get IV from surface for this specific leg
            let iv = provider.get_iv(surface, strike, leg.expiration, is_call)
                .unwrap_or(0.30);  // Fallback

            let leg_greeks = bs_greeks(
                spot,
                strike_f64,
                tte,
                iv,
                is_call,
                self.risk_free_rate,
            );

            let sign = position.sign();
            total.delta += leg_greeks.delta * sign;
            total.gamma += leg_greeks.gamma * sign;
            total.theta += leg_greeks.theta * sign;
            total.vega += leg_greeks.vega * sign;
        }

        PositionGreeks::from_per_share(&total, self.contract_multiplier)
    }
}
```

### Phase 3: SnapshotCollector (`cs-backtest/src/attribution/snapshot_collector.rs`)

Collects daily (open, close) snapshot pairs:

```rust
use std::sync::Arc;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use cs_domain::{
    EquityDataRepository, OptionsDataRepository,
    TradingCalendar,
    position::{PositionSnapshot, PositionGreeks},
    trade::CompositeTrade,
};
use cs_analytics::PricingModel;
use crate::iv_surface_builder::build_iv_surface_minute_aligned;
use super::greeks_computer::GreeksComputer;

/// Collects position snapshots for P&L attribution
pub struct SnapshotCollector<T: CompositeTrade> {
    trade: T,
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    symbol: String,
    config: AttributionConfig,
    greeks_computer: GreeksComputer<T>,

    /// Collected snapshot pairs: (open, close) for each trading day
    snapshots: Vec<(PositionSnapshot, PositionSnapshot)>,

    /// Hedge shares timeline from HedgePosition
    /// Populated after hedging completes
    hedge_timeline: Vec<(DateTime<Utc>, i32)>,
}

impl<T: CompositeTrade + Clone> SnapshotCollector<T> {
    pub fn new(
        trade: T,
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        symbol: String,
        config: AttributionConfig,
        contract_multiplier: i32,
        risk_free_rate: f64,
    ) -> Self {
        let greeks_computer = GreeksComputer::new(&trade, contract_multiplier, risk_free_rate);
        Self {
            trade,
            options_repo,
            equity_repo,
            symbol,
            config,
            greeks_computer,
            snapshots: Vec::new(),
            hedge_timeline: Vec::new(),
        }
    }

    /// Set hedge timeline after hedging phase completes
    pub fn set_hedge_timeline(&mut self, hedges: &[HedgeAction]) {
        let mut cumulative = 0i32;
        self.hedge_timeline.clear();

        for hedge in hedges {
            cumulative += hedge.shares;
            self.hedge_timeline.push((hedge.timestamp, cumulative));
        }
    }

    /// Get hedge shares at a specific time
    fn hedge_shares_at(&self, timestamp: DateTime<Utc>) -> i32 {
        // Find most recent hedge before or at timestamp
        self.hedge_timeline
            .iter()
            .rev()
            .find(|(t, _)| *t <= timestamp)
            .map(|(_, shares)| *shares)
            .unwrap_or(0)
    }

    /// Collect snapshots for all trading days in range
    pub async fn collect(
        &mut self,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Result<(), String> {
        let trading_days = TradingCalendar::trading_days_between(
            entry_time.date_naive(),
            exit_time.date_naive(),
        );

        for date in trading_days {
            match self.collect_day(date).await {
                Ok(Some((open, close))) => {
                    self.snapshots.push((open, close));
                }
                Ok(None) => {
                    // Skip days with missing data
                    tracing::debug!("Skipping attribution for {}: missing data", date);
                }
                Err(e) => {
                    tracing::warn!("Attribution snapshot failed for {}: {}", date, e);
                }
            }
        }

        Ok(())
    }

    /// Collect (open, close) snapshots for a single day
    async fn collect_day(
        &self,
        date: NaiveDate,
    ) -> Result<Option<(PositionSnapshot, PositionSnapshot)>, String> {
        let (open_time, close_time) = self.snapshot_times(date);

        // Collect open snapshot
        let open_snapshot = match self.create_snapshot(open_time).await {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };

        // Collect close snapshot
        let close_snapshot = match self.create_snapshot(close_time).await {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };

        Ok(Some((open_snapshot, close_snapshot)))
    }

    /// Create a single snapshot at a specific time
    async fn create_snapshot(&self, timestamp: DateTime<Utc>) -> Result<PositionSnapshot, String> {
        // 1. Get spot price
        let spot = self.equity_repo
            .get_spot_price(&self.symbol, timestamp)
            .await
            .map_err(|e| e.to_string())?
            .to_f64();

        // 2. Get volatility and compute Greeks based on configured source
        let (iv, greeks) = match &self.config.vol_source {
            VolatilitySource::EntryIV | VolatilitySource::EntryHV { .. } => {
                // For entry-based sources, we'd need the entry vol passed in
                // This is a limitation - might need to store entry vol in collector
                return Err("EntryIV/EntryHV not yet supported for attribution".to_string());
            }
            VolatilitySource::CurrentMarketIV => {
                self.compute_with_current_market_iv(spot, timestamp).await?
            }
            VolatilitySource::CurrentHV { window } => {
                self.compute_with_current_hv(spot, timestamp, *window).await?
            }
            VolatilitySource::HistoricalAverageIV { .. } => {
                return Err("HistoricalAverageIV not yet supported for attribution".to_string());
            }
        };

        // 3. Get hedge shares at this time
        let hedge_shares = self.hedge_shares_at(timestamp);

        Ok(PositionSnapshot::new(timestamp, spot, iv, greeks, hedge_shares))
    }

    /// Compute Greeks using current market IV surface
    async fn compute_with_current_market_iv(
        &self,
        spot: f64,
        timestamp: DateTime<Utc>,
    ) -> Result<(f64, PositionGreeks), String> {
        // Build IV surface
        let chain = self.options_repo
            .get_option_bars_at_time(&self.symbol, timestamp)
            .await
            .map_err(|e| e.to_string())?;

        let surface = build_iv_surface_minute_aligned(&chain, self.equity_repo.as_ref(), &self.symbol)
            .await
            .ok_or("Failed to build IV surface")?;

        // Get average IV for the position (for vega attribution)
        let provider = self.config.pricing_model.to_provider();
        let iv = self.compute_position_avg_iv(&surface, provider.as_ref(), timestamp);

        // Compute Greeks from surface
        let greeks = self.greeks_computer.compute_with_surface(
            spot,
            &surface,
            provider.as_ref(),
            timestamp,
        );

        Ok((iv, greeks))
    }

    /// Compute Greeks using current HV
    async fn compute_with_current_hv(
        &self,
        spot: f64,
        timestamp: DateTime<Utc>,
        window: u32,
    ) -> Result<(f64, PositionGreeks), String> {
        // Get HV from price history
        let end_date = timestamp.date_naive();
        let start_date = end_date - chrono::Duration::days(window as i64 + 10);

        let bars = self.equity_repo
            .get_bars(&self.symbol, start_date, end_date)
            .await
            .map_err(|e| e.to_string())?;

        let closes: Vec<f64> = bars.column("close")
            .map_err(|_| "No close column")?
            .f64()
            .map_err(|_| "Invalid type")?
            .into_no_null_iter()
            .collect();

        let hv = cs_analytics::realized_volatility(&closes, window as usize, 252.0)
            .ok_or("Insufficient data for HV")?;

        // Compute Greeks with flat HV
        let greeks = self.greeks_computer.compute_with_flat_vol(spot, hv, timestamp);

        Ok((hv, greeks))
    }

    /// Compute average IV across position legs
    fn compute_position_avg_iv(
        &self,
        surface: &IVSurface,
        provider: &dyn PricingIVProvider,
        timestamp: DateTime<Utc>,
    ) -> f64 {
        let ivs: Vec<f64> = self.trade.legs().iter().filter_map(|(leg, _)| {
            let is_call = leg.option_type == OptionType::Call;
            provider.get_iv(surface, leg.strike.value(), leg.expiration, is_call)
        }).collect();

        if ivs.is_empty() {
            0.30  // Fallback
        } else {
            ivs.iter().sum::<f64>() / ivs.len() as f64
        }
    }

    /// Convert snapshot times config to DateTime
    fn snapshot_times(&self, date: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
        use cs_domain::datetime::eastern_to_utc;

        let (open_h, open_m, close_h, close_m) = match &self.config.snapshot_times {
            SnapshotTimes::OpenClose => (9, 30, 16, 0),
            SnapshotTimes::CloseOnly => (16, 0, 16, 0),  // Same time - will skip open
            SnapshotTimes::Custom { open_hour, open_minute, close_hour, close_minute } => {
                (*open_hour, *open_minute, *close_hour, *close_minute)
            }
        };

        let open_time = eastern_to_utc(date, NaiveTime::from_hms_opt(open_h, open_m, 0).unwrap());
        let close_time = eastern_to_utc(date, NaiveTime::from_hms_opt(close_h, close_m, 0).unwrap());

        (open_time, close_time)
    }

    /// Build PositionAttribution from collected snapshots
    pub fn build_attribution(&self, actual_pnl: Decimal) -> Option<PositionAttribution> {
        if self.snapshots.is_empty() {
            return None;
        }

        Some(PositionAttribution::from_snapshots(
            self.snapshots.clone(),
            actual_pnl,
        ))
    }
}
```

### Phase 4: Integration with HedgeState (`cs-domain/src/hedging.rs`)

Extend `HedgeState<P>` to support optional attribution:

```rust
/// Stateful delta hedge manager with pluggable delta computation
pub struct HedgeState<P: DeltaProvider> {
    config: HedgeConfig,
    delta_provider: P,
    stock_shares: i32,
    last_delta: f64,
    last_gamma: Option<f64>,
    position: HedgePosition,

    // RV tracking
    spot_history: Vec<(DateTime<Utc>, f64)>,
    track_rv: bool,

    // NEW: Attribution support
    attribution_enabled: bool,
}

impl<P: DeltaProvider> HedgeState<P> {
    pub fn new(
        config: HedgeConfig,
        delta_provider: P,
        initial_spot: f64,
        attribution_enabled: bool,  // NEW parameter
    ) -> Self {
        let track_rv = config.track_realized_vol;
        Self {
            config,
            delta_provider,
            stock_shares: 0,
            last_delta: 0.0,
            last_gamma: None,
            position: HedgePosition::new(),
            spot_history: if track_rv { vec![(Utc::now(), initial_spot)] } else { vec![] },
            track_rv,
            attribution_enabled,
        }
    }

    /// Get hedge actions for attribution timeline
    pub fn hedge_actions(&self) -> &[HedgeAction] {
        &self.position.hedges
    }

    /// Check if attribution is enabled
    pub fn attribution_enabled(&self) -> bool {
        self.attribution_enabled
    }

    // ... rest unchanged
}
```

### Phase 5: Integration with TradeExecutor (`cs-backtest/src/trade_executor.rs`)

Wire attribution into the unified hedging flow:

```rust
impl<T> TradeExecutor<T>
where
    T: RollableTrade + ExecutableTrade + CompositeTrade + Clone,
{
    /// Apply hedging with optional attribution
    async fn apply_hedging(
        &self,
        trade: &T,
        result: &mut <T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
    ) -> Result<(), String> {
        let hedge_config = self.hedge_config.as_ref()
            .ok_or("Hedge config not set")?;

        let symbol = result.symbol().to_string();
        let entry_spot = result.spot_at_entry();

        // Create delta provider based on mode
        let delta_provider = self.create_delta_provider(trade, result, entry_time).await?;

        // Check if attribution is enabled
        let attribution_enabled = self.attribution_config
            .as_ref()
            .map(|c| c.enabled)
            .unwrap_or(false);

        // Create hedge state
        let mut hedge_state = HedgeState::new(
            hedge_config.clone(),
            delta_provider,
            entry_spot,
            attribution_enabled,
        );

        // === UNIFIED HEDGING LOOP ===
        for rehedge_time in rehedge_times {
            if hedge_state.at_max_rehedges() {
                break;
            }

            let spot = self.equity_repo
                .get_spot_price(&symbol, rehedge_time)
                .await
                .map_err(|e| e.to_string())?
                .to_f64();

            hedge_state.update(rehedge_time, spot).await?;
        }

        // === ATTRIBUTION PHASE (after hedging completes) ===
        let attribution = if attribution_enabled {
            self.compute_attribution(
                trade,
                &hedge_state,
                entry_time,
                exit_time,
                result.pnl(),
            ).await.ok()
        } else {
            None
        };

        // Finalize
        let exit_spot = result.spot_at_exit();
        let entry_iv = result.entry_iv().map(|iv| iv.primary);
        let exit_iv = result.exit_iv().map(|iv| iv.primary);
        let hedge_position = hedge_state.finalize(exit_spot, entry_iv, exit_iv);

        // Apply results
        if hedge_position.rehedge_count() > 0 || attribution.is_some() {
            let hedge_pnl = hedge_position.calculate_pnl(exit_spot);
            let total_pnl = result.pnl() + hedge_pnl - hedge_position.total_cost;
            result.apply_hedge_results(hedge_position, hedge_pnl, total_pnl, attribution);
        }

        Ok(())
    }

    /// Compute P&L attribution (after hedging completes)
    async fn compute_attribution<P: DeltaProvider>(
        &self,
        trade: &T,
        hedge_state: &HedgeState<P>,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        actual_pnl: Decimal,
    ) -> Result<PositionAttribution, String> {
        let attr_config = self.attribution_config.as_ref()
            .ok_or("Attribution config not set")?;

        let symbol = trade.symbol().to_string();
        let contract_multiplier = self.hedge_config
            .as_ref()
            .map(|c| c.contract_multiplier)
            .unwrap_or(100);

        // Create snapshot collector
        let mut collector = SnapshotCollector::new(
            trade.clone(),
            self.options_repo.clone(),
            self.equity_repo.clone(),
            symbol,
            attr_config.clone(),
            contract_multiplier,
            0.05,  // risk_free_rate
        );

        // Set hedge timeline from completed hedging
        collector.set_hedge_timeline(hedge_state.hedge_actions());

        // Collect daily snapshots
        collector.collect(entry_time, exit_time).await?;

        // Build attribution
        collector.build_attribution(actual_pnl)
            .ok_or_else(|| "No snapshots collected".to_string())
    }
}
```

### Phase 6: CLI Integration

Add attribution flags to CLI:

```rust
// In cs-cli/src/main.rs

#[derive(Parser)]
struct BacktestArgs {
    // ... existing args ...

    /// Enable P&L attribution (requires more computation)
    #[arg(long)]
    attribution: bool,

    /// Volatility source for attribution Greeks
    /// Options: current-iv, current-hv
    #[arg(long, default_value = "current-iv")]
    attribution_vol_source: String,

    /// Snapshot times: open-close or close-only
    #[arg(long, default_value = "open-close")]
    attribution_snapshots: String,
}

// Build AttributionConfig from args
fn build_attribution_config(args: &BacktestArgs) -> Option<AttributionConfig> {
    if !args.attribution {
        return None;
    }

    Some(AttributionConfig {
        enabled: true,
        vol_source: match args.attribution_vol_source.as_str() {
            "current-hv" => VolatilitySource::CurrentHV { window: 20 },
            _ => VolatilitySource::CurrentMarketIV,
        },
        pricing_model: PricingModel::StickyMoneyness,
        snapshot_times: match args.attribution_snapshots.as_str() {
            "close-only" => SnapshotTimes::CloseOnly,
            _ => SnapshotTimes::OpenClose,
        },
    })
}
```

---

## Output Format

### JSON Output

```json
{
  "position_attribution": {
    "daily": [
      {
        "date": "2025-01-15",
        "spot_open": 150.0,
        "spot_close": 152.0,
        "spot_change": 2.0,
        "iv_open": 0.35,
        "iv_close": 0.32,
        "iv_change": -0.03,
        "option_delta": 45.0,
        "option_gamma": 5.5,
        "hedge_shares": -40,
        "net_delta": 5.0,
        "gross_delta_pnl": 90.0,
        "hedge_delta_pnl": -80.0,
        "net_delta_pnl": 10.0,
        "gamma_pnl": 11.0,
        "theta_pnl": -18.0,
        "vega_pnl": -165.0
      }
    ],
    "total_gross_delta_pnl": 90.0,
    "total_hedge_delta_pnl": -80.0,
    "total_net_delta_pnl": 10.0,
    "total_gamma_pnl": 11.0,
    "total_theta_pnl": -18.0,
    "total_vega_pnl": -165.0,
    "total_unexplained": 12.0,
    "hedge_efficiency": 88.9
  }
}
```

### CLI Summary Output

```
=== P&L Attribution ===
Period: 2025-01-15 to 2025-01-17 (3 trading days)

Component         Total P&L    % of Total
─────────────────────────────────────────
Gross Delta       $90.00
Hedge Delta      -$80.00
Net Delta         $10.00        6.7%
Gamma             $11.00        7.3%
Theta            -$18.00      -12.0%
Vega            -$165.00     -110.0%
─────────────────────────────────────────
Explained       -$162.00
Unexplained      $12.00        8.0%
─────────────────────────────────────────
Actual P&L      -$150.00

Hedge Efficiency: 88.9% (|hedge_delta| / |gross_delta|)
```

---

## Implementation Phases

| Phase | Deliverable | Files | Lines |
|-------|-------------|-------|-------|
| 1 | `AttributionConfig` | `cs-domain/src/hedging.rs` | +50 |
| 2 | `GreeksComputer` | `cs-backtest/src/attribution/greeks_computer.rs` | +100 |
| 3 | `SnapshotCollector` | `cs-backtest/src/attribution/snapshot_collector.rs` | +250 |
| 4 | HedgeState integration | `cs-domain/src/hedging.rs` | +20 |
| 5 | TradeExecutor integration | `cs-backtest/src/trade_executor.rs` | +80 |
| 6 | CLI flags | `cs-cli/src/main.rs` | +40 |
| 7 | Tests | `cs-backtest/tests/` | +150 |

**Total**: ~700 new lines

---

## Performance Considerations

| Mode | IV Surface Builds | Speed | Accuracy |
|------|-------------------|-------|----------|
| CurrentHV | 0 | Fast | Medium |
| CurrentMarketIV | 2N (N days) | Slow | High |
| CloseOnly | N | Medium | Lower |

**Recommendation**: Default to `CurrentMarketIV` with `OpenClose` for accurate attribution, but offer `CurrentHV` + `CloseOnly` for faster backtests.

---

## Testing Strategy

### Unit Tests
1. `GreeksComputer::compute_with_flat_vol()` - verify position Greeks sum correctly
2. `SnapshotCollector::hedge_shares_at()` - verify timeline lookup
3. `DailyAttribution::compute()` - existing tests pass

### Integration Tests
1. End-to-end with `--attribution` flag
2. Verify unexplained is small (<10% of actual P&L) for well-behaved trades
3. Compare attribution totals to hedge P&L

### Regression Tests
1. Trades without hedging: attribution shows gross delta only
2. Perfect hedge: net_delta_pnl ≈ 0

---

## Dependencies

**Requires completion of**:
- Phase 1-5 of Hedge State Strategy Pattern Refactoring
- `DeltaProvider` trait and implementations
- Refactored `HedgeState<P>`

**Does NOT require**:
- CurrentMarketIV delta provider (but uses same IV surface logic)
- HistoricalAverageIV provider

---

## References

- `cs-domain/src/position/position_attribution.rs` - Domain model
- `cs-domain/src/position/daily_snapshot.rs` - Snapshot types
- `cs-backtest/src/iv_surface_builder.rs` - IV surface construction
- Original attribution (removed in commit 855d867)
