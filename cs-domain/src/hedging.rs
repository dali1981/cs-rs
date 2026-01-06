use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use crate::CONTRACT_MULTIPLIER;

/// A single hedge transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HedgeAction {
    pub timestamp: DateTime<Utc>,
    pub shares: i32,          // Positive = buy, negative = sell
    pub spot_price: f64,
    pub delta_before: f64,    // Position delta before hedge
    pub delta_after: f64,     // Position delta after hedge
    pub cost: Decimal,        // Transaction cost
}

/// Cumulative hedge position state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HedgePosition {
    pub cumulative_shares: i32,   // Net shares held
    pub hedges: Vec<HedgeAction>, // All hedge transactions
    pub realized_pnl: Decimal,    // P&L from closed hedge portions
    pub unrealized_pnl: Decimal,  // P&L from open hedge position
    pub total_cost: Decimal,      // Sum of transaction costs

    /// Spot observations during hedging (for RV computation)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub spot_history: Vec<(DateTime<Utc>, f64)>,

    /// Realized volatility metrics (computed at finalize)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub realized_vol_metrics: Option<RealizedVolatilityMetrics>,
}

impl HedgePosition {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new hedge action
    pub fn add_hedge(&mut self, action: HedgeAction) {
        self.cumulative_shares += action.shares;
        self.total_cost += action.cost;
        self.hedges.push(action);
    }

    /// Calculate hedge P&L at exit
    pub fn calculate_pnl(&self, exit_spot: f64) -> Decimal {
        // Sum of (shares × (exit_spot - hedge_spot)) for each hedge
        self.hedges
            .iter()
            .map(|h| {
                let pnl_per_share = exit_spot - h.spot_price;
                Decimal::try_from(h.shares as f64 * pnl_per_share).unwrap_or_default()
            })
            .sum()
    }

    /// Number of rehedges performed
    pub fn rehedge_count(&self) -> usize {
        self.hedges.len()
    }

    /// Average hedge price (for reporting)
    pub fn average_hedge_price(&self) -> Option<f64> {
        if self.hedges.is_empty() {
            return None;
        }
        let total_value: f64 = self.hedges.iter().map(|h| h.shares as f64 * h.spot_price).sum();
        let total_shares: i32 = self.hedges.iter().map(|h| h.shares).sum();
        if total_shares == 0 {
            None
        } else {
            Some(total_value / total_shares as f64)
        }
    }
}

/// Hedging strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HedgeStrategy {
    /// Rehedge at fixed time intervals
    TimeBased { interval: Duration },
    /// Rehedge when absolute delta exceeds threshold
    DeltaThreshold {
        threshold: f64, // e.g., 0.10 = rehedge when |delta| > 0.10
    },
    /// Rehedge based on dollar gamma exposure
    GammaDollar {
        threshold: f64, // Rehedge when |gamma × spot² × 0.01| > threshold
    },
    /// No hedging (baseline)
    None,
}

impl Default for HedgeStrategy {
    fn default() -> Self {
        HedgeStrategy::None
    }
}

/// Configuration for delta hedging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HedgeConfig {
    pub strategy: HedgeStrategy,
    pub max_rehedges: Option<usize>, // Limit number of rehedges
    pub min_hedge_size: i32,          // Minimum shares to trade
    pub transaction_cost_per_share: Decimal, // Cost per share traded
    pub contract_multiplier: i32,     // Usually 100 for options

    /// How to compute delta for rehedge decisions (default: GammaApproximation)
    #[serde(default)]
    pub delta_computation: DeltaComputation,

    /// Whether to compute and report realized volatility metrics
    #[serde(default)]
    pub track_realized_vol: bool,
}

impl Default for HedgeConfig {
    fn default() -> Self {
        Self {
            strategy: HedgeStrategy::None,
            max_rehedges: None,
            min_hedge_size: 1,
            transaction_cost_per_share: Decimal::ZERO,
            contract_multiplier: CONTRACT_MULTIPLIER,
            delta_computation: DeltaComputation::default(),
            track_realized_vol: false,
        }
    }
}

impl HedgeConfig {
    /// Check if hedging is enabled
    pub fn is_enabled(&self) -> bool {
        !matches!(self.strategy, HedgeStrategy::None)
    }

    /// Determine if rehedge is needed based on strategy
    pub fn should_rehedge(&self, position_delta: f64, spot: f64, gamma: f64) -> bool {
        match &self.strategy {
            HedgeStrategy::None => false,
            HedgeStrategy::TimeBased { .. } => true, // Always rehedge at scheduled times
            HedgeStrategy::DeltaThreshold { threshold } => position_delta.abs() > *threshold,
            HedgeStrategy::GammaDollar { threshold } => {
                // Dollar gamma = gamma × spot² × 0.01 (for 1% move)
                let dollar_gamma = gamma.abs() * spot * spot * 0.01;
                dollar_gamma > *threshold
            }
        }
    }

    /// Calculate shares needed to delta-neutralize
    pub fn shares_to_hedge(&self, position_delta: f64) -> i32 {
        // Shares = -delta × multiplier
        let raw_shares = (-position_delta * self.contract_multiplier as f64).round() as i32;

        // Apply minimum size filter
        if raw_shares.abs() < self.min_hedge_size {
            0
        } else {
            raw_shares
        }
    }
}

/// Stateful delta hedge manager
///
/// Tracks both option greeks and stock position to compute net exposure.
/// Call `update()` with each new spot observation; it returns a HedgeAction
/// if rebalancing is needed.
///
/// # Key Features
/// - Incremental delta updates using gamma approximation
/// - Tracks net position delta (options + stock)
/// - Only hedges incremental changes, not full position
/// - Same interface works for real-time and historical backtesting
#[derive(Debug, Clone)]
pub struct HedgeState {
    // Configuration (immutable after creation)
    config: HedgeConfig,

    // Option position greeks (per-share, updated incrementally)
    option_delta: f64,
    option_gamma: f64,

    // Stock hedge position
    stock_shares: i32,

    // Reference point for incremental delta updates
    last_spot: f64,

    // Transaction history
    position: HedgePosition,
}

impl HedgeState {
    /// Create new hedge state from initial option position
    pub fn new(
        config: HedgeConfig,
        initial_delta: f64,    // Option delta at entry (per-share)
        initial_gamma: f64,    // Option gamma at entry (per-share)
        initial_spot: f64,     // Spot price at entry
    ) -> Self {
        Self {
            config,
            option_delta: initial_delta,
            option_gamma: initial_gamma,
            stock_shares: 0,
            last_spot: initial_spot,
            position: HedgePosition::new(),
        }
    }

    /// Net position delta (options + stock)
    pub fn net_delta(&self) -> f64 {
        let stock_delta = self.stock_shares as f64 / self.config.contract_multiplier as f64;
        self.option_delta + stock_delta
    }

    /// Current stock position
    pub fn stock_shares(&self) -> i32 {
        self.stock_shares
    }

    /// Number of rehedges executed
    pub fn rehedge_count(&self) -> usize {
        self.position.rehedge_count()
    }

    /// Check if max rehedges reached
    pub fn at_max_rehedges(&self) -> bool {
        if let Some(max) = self.config.max_rehedges {
            self.rehedge_count() >= max
        } else {
            false
        }
    }

    /// Process a new spot price observation
    ///
    /// Returns Some(HedgeAction) if a rebalance was executed, None otherwise.
    ///
    /// # State Transitions
    /// 1. Update option_delta using gamma approximation
    /// 2. Check if net_delta exceeds threshold
    /// 3. If yes, compute shares to trade and execute
    /// 4. Update stock_shares and record transaction
    pub fn update(
        &mut self,
        timestamp: DateTime<Utc>,
        new_spot: f64,
    ) -> Option<HedgeAction> {
        // 1. Update option delta using gamma approximation
        let spot_change = new_spot - self.last_spot;
        self.option_delta += self.option_gamma * spot_change;
        self.last_spot = new_spot;

        // 2. Check if rehedge needed based on NET delta
        let net_delta = self.net_delta();
        if !self.config.should_rehedge(net_delta, new_spot, self.option_gamma) {
            return None;
        }

        // 3. Calculate INCREMENTAL shares needed (to neutralize net_delta)
        let shares = self.config.shares_to_hedge(net_delta);
        if shares == 0 {
            return None;
        }

        // 4. Execute hedge and update state
        let delta_before = net_delta;
        self.stock_shares += shares;
        let delta_after = self.net_delta();

        let action = HedgeAction {
            timestamp,
            shares,
            spot_price: new_spot,
            delta_before,
            delta_after,
            cost: self.config.transaction_cost_per_share * Decimal::from(shares.abs()),
        };

        self.position.add_hedge(action.clone());

        Some(action)
    }

    /// Finalize position and compute P&L at exit
    pub fn finalize(mut self, exit_spot: f64) -> HedgePosition {
        // Calculate unrealized P&L
        self.position.unrealized_pnl = self.position.calculate_pnl(exit_spot);
        self.position
    }
}

// =============================================================================
// Delta Computation Modes
// =============================================================================

/// How to compute delta for hedging decisions
///
/// This determines the method used to calculate the position delta when deciding
/// whether to rehedge and how many shares to trade.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum DeltaComputation {
    /// Use gamma × ΔS approximation (fast, current behavior)
    ///
    /// Delta evolves incrementally: δ' = δ + γ × (S' - S)
    /// This is the fastest method but assumes constant gamma.
    GammaApproximation,

    /// Recompute from IV at trade entry
    ///
    /// Uses Black-Scholes with entry IV, current spot, and remaining DTE.
    /// More accurate than gamma approximation but still uses stale IV.
    EntryIV {
        /// IV interpolation model (not stored in enum for simplicity - will be passed separately)
        /// We mark this field as skipped and use PricingModel from analytics
        #[serde(skip)]
        _marker: (),
    },

    /// Recompute from current market IV surface
    ///
    /// Most accurate method - builds fresh IV surface at each rehedge.
    /// Expensive as it requires market data lookups.
    CurrentMarketIV {
        #[serde(skip)]
        _marker: (),
    },

    /// Use Historical Volatility at trade entry for delta computation
    ///
    /// HV is computed from underlying price history, not options market.
    EntryHV {
        /// Lookback window in days (e.g., 20 for 20-day HV)
        window: u32,
    },

    /// Recompute HV at each rehedge from recent underlying prices
    ///
    /// Tracks actual underlying volatility evolution.
    CurrentHV {
        /// Lookback window in days
        window: u32,
    },

    /// Use historical average IV over lookback period
    ///
    /// Smooths out IV noise by averaging recent market IV values.
    HistoricalAverageIV {
        /// Lookback period in days
        lookback_days: u32,
        #[serde(skip)]
        _marker: (),
    },
}

impl Default for DeltaComputation {
    fn default() -> Self {
        // Match current behavior: gamma approximation
        DeltaComputation::GammaApproximation
    }
}

// =============================================================================
// Realized Volatility Metrics
// =============================================================================

/// Realized volatility metrics computed during hedging
///
/// Provides comprehensive volatility analysis comparing implied vs realized volatility
/// over the hedging period.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RealizedVolatilityMetrics {
    /// Entry HV (Historical Volatility at trade entry)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_hv: Option<f64>,

    /// Realized volatility during holding period
    ///
    /// Computed from actual spot price moves during hedging.
    /// This is the "actual" volatility that occurred.
    pub realized_vol: f64,

    /// IV at entry (for comparison)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_iv: Option<f64>,

    /// IV at exit
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_iv: Option<f64>,

    /// Volatility of volatility (optional advanced metric)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vol_of_vol: Option<f64>,

    /// Number of observations used for computation
    pub num_observations: usize,

    /// IV premium/discount at entry: (entry_iv - entry_hv) / entry_hv × 100
    ///
    /// Positive = IV was rich (premium), Negative = IV was cheap (discount)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iv_premium_at_entry: Option<f64>,

    /// Realized vs Entry IV: (realized_vol - entry_iv) / entry_iv × 100
    ///
    /// Positive = actual moves exceeded implied, Negative = actual moves less than implied
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realized_vs_implied: Option<f64>,
}

impl RealizedVolatilityMetrics {
    /// Create metrics from spot observations
    ///
    /// Uses the realized_volatility function from cs-analytics to compute
    /// the actual volatility from the spot history.
    pub fn from_spot_history(
        spots: &[(DateTime<Utc>, f64)],
        entry_hv: Option<f64>,
        entry_iv: Option<f64>,
        exit_iv: Option<f64>,
    ) -> Self {
        // Extract prices in chronological order
        let prices: Vec<f64> = spots.iter().map(|(_, price)| *price).collect();

        // Compute realized vol over the full period
        // Note: We'll need to import realized_volatility from cs-analytics
        // For now, stub with 0.0 - this will be implemented in Phase 2
        let realized_vol = if prices.len() >= 2 {
            // Simple std dev calculation as placeholder
            let returns: Vec<f64> = prices
                .windows(2)
                .map(|w| (w[1] / w[0]).ln())
                .collect();

            if returns.is_empty() {
                0.0
            } else {
                let mean = returns.iter().sum::<f64>() / returns.len() as f64;
                let variance = returns.iter()
                    .map(|r| (r - mean).powi(2))
                    .sum::<f64>() / returns.len() as f64;
                variance.sqrt() * 252.0_f64.sqrt() // Annualize
            }
        } else {
            0.0
        };

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
            vol_of_vol: None, // Can be computed if needed
            num_observations: prices.len(),
            iv_premium_at_entry,
            realized_vs_implied,
        }
    }
}
