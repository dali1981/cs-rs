use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
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

/// Enriched hedge trade with attribution metrics
///
/// Contains all fields from HedgeAction plus computed metrics:
/// - Realized volatility up to this trade
/// - Gamma P&L contribution for this rehedge period
/// - Cumulative hedge P&L
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HedgeTradeDetail {
    // Core trade data (from HedgeAction)
    pub timestamp: DateTime<Utc>,
    pub shares: i32,
    pub spot_price: f64,
    pub delta_before: f64,
    pub delta_after: f64,
    pub cost: Decimal,

    // Expanded metrics
    /// Realized volatility from entry to this trade (annualized)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rv_to_date: Option<f64>,
    /// Gamma P&L for this rehedge period
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gamma_pnl: Option<f64>,
    /// Running total hedge P&L at this point
    pub cumulative_hedge_pnl: Decimal,
    /// Total shares held after this trade
    pub cumulative_shares: i32,
}

impl HedgeTradeDetail {
    /// Create from a HedgeAction with additional computed metrics
    pub fn from_action(
        action: &HedgeAction,
        spot_history: &[(DateTime<Utc>, f64)],
        cumulative_pnl: Decimal,
        cumulative_shares: i32,
        prev_spot: Option<f64>,
        gamma: Option<f64>,
    ) -> Self {
        // Compute RV up to this point
        let rv_to_date = Self::compute_rv_to_date(spot_history, action.timestamp);

        // Compute gamma P&L: 0.5 * gamma * (spot_move)^2
        let gamma_pnl = match (prev_spot, gamma) {
            (Some(prev), Some(g)) => {
                let spot_move = action.spot_price - prev;
                Some(0.5 * g * spot_move.powi(2) * 100.0) // per contract (100 multiplier)
            }
            _ => None,
        };

        Self {
            timestamp: action.timestamp,
            shares: action.shares,
            spot_price: action.spot_price,
            delta_before: action.delta_before,
            delta_after: action.delta_after,
            cost: action.cost,
            rv_to_date,
            gamma_pnl,
            cumulative_hedge_pnl: cumulative_pnl,
            cumulative_shares,
        }
    }

    /// Compute realized volatility from spot history up to a given time
    pub fn compute_rv_to_date(history: &[(DateTime<Utc>, f64)], up_to: DateTime<Utc>) -> Option<f64> {
        let prices: Vec<f64> = history
            .iter()
            .filter(|(t, _)| *t <= up_to)
            .map(|(_, p)| *p)
            .collect();

        if prices.len() < 2 {
            return None;
        }

        let returns: Vec<f64> = prices
            .windows(2)
            .map(|w| (w[1] / w[0]).ln())
            .collect();

        if returns.is_empty() {
            return None;
        }

        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns
            .iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>() / returns.len() as f64;

        Some(variance.sqrt() * 252.0_f64.sqrt())
    }
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

    // Capital tracking (Phase 2a + Issue B fix)
    /// Peak long shares held during hedging
    #[serde(default)]
    pub peak_long_shares: i32,
    /// Peak short shares held during hedging (absolute value)
    #[serde(default)]
    pub peak_short_shares: i32,
    /// Average hedge price (for reporting only - use peak spot for capital)
    #[serde(default)]
    pub avg_hedge_price: f64,
    /// Spot price when peak long shares was reached (for capital calculation)
    #[serde(default)]
    pub peak_long_spot: f64,
    /// Spot price when peak short shares was reached (for margin calculation)
    #[serde(default)]
    pub peak_short_spot: f64,

    // Unwind cost tracking (Issue A fix)
    /// Transaction cost for unwinding the hedge at exit
    /// This is computed during finalize() and added to total_cost
    #[serde(default)]
    pub unwind_cost: Decimal,
}

impl HedgePosition {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new hedge action
    pub fn add_hedge(&mut self, action: HedgeAction) {
        self.cumulative_shares += action.shares;
        self.total_cost += action.cost;

        // Track peak shares and spot at peak (Phase 2a + Issue B fix)
        // Using spot at peak is more robust than average price for capital
        if self.cumulative_shares > self.peak_long_shares {
            self.peak_long_shares = self.cumulative_shares;
            self.peak_long_spot = action.spot_price;
        }
        if self.cumulative_shares < 0 && self.cumulative_shares.abs() > self.peak_short_shares {
            self.peak_short_shares = self.cumulative_shares.abs();
            self.peak_short_spot = action.spot_price;
        }

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

    /// Average hedge price (for reporting and capital calculation)
    ///
    /// Uses absolute value of shares for weighting to avoid sign issues.
    /// Previously used signed shares which caused avg to explode or flip sign
    /// when net position approached zero (Issue B fix).
    pub fn average_hedge_price(&self) -> Option<f64> {
        if self.hedges.is_empty() {
            return None;
        }
        // Use abs(shares) for weighting - prevents division by near-zero
        // and nonsensical averages when buys and sells offset
        let total_value: f64 = self.hedges
            .iter()
            .map(|h| h.shares.abs() as f64 * h.spot_price)
            .sum();
        let total_abs_shares: i32 = self.hedges
            .iter()
            .map(|h| h.shares.abs())
            .sum();
        if total_abs_shares == 0 {
            None
        } else {
            Some(total_value / total_abs_shares as f64)
        }
    }

    /// Compute average hedge price and store it (Phase 2a)
    pub fn compute_avg_hedge_price(&mut self) {
        if let Some(avg) = self.average_hedge_price() {
            self.avg_hedge_price = avg;
        }
    }

    /// Compute capital required for long hedge position (Phase 2a + Issue B fix)
    ///
    /// Uses peak_shares × spot_at_peak instead of average price.
    /// This is more robust when hedge positions flip between long and short.
    pub fn long_hedge_capital(&self) -> Decimal {
        Decimal::from(self.peak_long_shares) * Decimal::try_from(self.peak_long_spot).unwrap_or_default()
    }

    /// Compute margin required for short hedge position (Phase 2a + Issue B fix)
    ///
    /// Uses peak_shares × spot_at_peak instead of average price.
    /// Default margin rate is 0.5 (50%)
    pub fn short_hedge_margin(&self, margin_rate: f64) -> Decimal {
        Decimal::from(self.peak_short_shares)
            * Decimal::try_from(self.peak_short_spot).unwrap_or_default()
            * Decimal::try_from(margin_rate).unwrap_or(Decimal::from_str("0.5").unwrap())
    }

    /// Build enriched trade details with RV and gamma P&L per trade
    ///
    /// # Arguments
    /// * `gamma` - Position gamma at entry (for gamma P&L calculation)
    /// * `entry_spot` - Entry spot price (for cumulative P&L)
    /// * `exit_spot` - Exit spot price (for unwind trade)
    /// * `exit_time` - Exit timestamp (for unwind trade)
    ///
    /// # Returns
    /// Vec of HedgeTradeDetail with computed metrics for each trade, including final unwind
    pub fn build_trade_details(
        &self,
        gamma: Option<f64>,
        entry_spot: f64,
        exit_spot: f64,
        exit_time: DateTime<Utc>,
    ) -> Vec<HedgeTradeDetail> {
        let mut details = Vec::with_capacity(self.hedges.len() + 1); // +1 for unwind
        let mut cumulative_shares = 0i32;
        let mut prev_spot = Some(entry_spot);

        for action in &self.hedges {
            cumulative_shares += action.shares;

            // Compute cumulative P&L up to and including this trade
            // For each prior trade: (current_spot - trade_spot) * shares
            let cumulative_pnl: Decimal = self.hedges
                .iter()
                .take(details.len() + 1)
                .map(|h| {
                    let pnl_per_share = action.spot_price - h.spot_price;
                    Decimal::try_from(h.shares as f64 * pnl_per_share).unwrap_or_default()
                })
                .sum();

            let detail = HedgeTradeDetail::from_action(
                action,
                &self.spot_history,
                cumulative_pnl,
                cumulative_shares,
                prev_spot,
                gamma,
            );

            prev_spot = Some(action.spot_price);
            details.push(detail);
        }

        // Add explicit unwind trade at exit if there are shares to close
        if cumulative_shares != 0 {
            let final_pnl = self.calculate_pnl(exit_spot);
            let rv_to_date = HedgeTradeDetail::compute_rv_to_date(&self.spot_history, exit_time);

            // Gamma P&L for the final period
            let gamma_pnl = match (prev_spot, gamma) {
                (Some(prev), Some(g)) => {
                    let spot_move = exit_spot - prev;
                    Some(0.5 * g * spot_move.powi(2) * 100.0)
                }
                _ => None,
            };

            let unwind_trade = HedgeTradeDetail {
                timestamp: exit_time,
                shares: -cumulative_shares, // Sell all remaining shares
                spot_price: exit_spot,
                delta_before: 0.0, // Delta at exit (option position closing)
                delta_after: 0.0,  // Zero after unwind
                cost: self.unwind_cost, // Transaction cost for unwinding (computed in finalize)
                rv_to_date,
                gamma_pnl,
                cumulative_hedge_pnl: final_pnl,
                cumulative_shares: 0, // Position fully closed
            };
            details.push(unwind_trade);
        }

        details
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

    /// Margin rate for short hedge positions (Issue D fix)
    /// Default is 0.5 (50%). Typical broker requirements range from 25-50%.
    #[serde(default = "default_margin_rate")]
    pub margin_rate: f64,
}

fn default_margin_rate() -> f64 {
    0.5
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
            margin_rate: default_margin_rate(),
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

// =============================================================================
// Delta Provider Trait (Strategy Pattern)
// =============================================================================

/// Strategy for computing position delta at a given point in time
///
/// Implementations provide different methods:
/// - GammaApproximation: δ' = δ + γ × ΔS (fast, incremental)
/// - EntryVolatility: Recompute from Black-Scholes with fixed volatility
/// - CurrentMarketIV: Build IV surface and compute fresh delta
///
/// # Delta Convention
/// All implementations MUST return per-share delta (e.g., 0.5 for ATM call, NOT 50).
/// The contract multiplier (typically 100) is applied separately in HedgeState.
#[async_trait]
pub trait DeltaProvider: Send + Sync {
    /// Compute the current per-share position delta
    ///
    /// # Arguments
    /// * `spot` - Current spot price
    /// * `timestamp` - Current time (for DTE calculation)
    ///
    /// # Returns
    /// Per-share position delta (e.g., 0.5 for ATM call, NOT 50)
    async fn compute_delta(&mut self, spot: f64, timestamp: DateTime<Utc>) -> Result<f64, String>;

    /// Optional: Compute position gamma (for reporting)
    ///
    /// # Returns
    /// Per-share position gamma if available
    fn compute_gamma(&self, _spot: f64, _timestamp: DateTime<Utc>) -> Option<f64> {
        None
    }

    /// Human-readable name for logging
    fn name(&self) -> &'static str;
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

        // Compute unwind cost for closing the hedge at exit (Issue A fix)
        let unwind_shares = self.position.cumulative_shares.abs();
        let unwind_cost = self.config.transaction_cost_per_share * Decimal::from(unwind_shares);
        self.position.unwind_cost = unwind_cost;
        self.position.total_cost += unwind_cost;

        self.position
    }
}

// =============================================================================
// Generic HedgeState with Pluggable Delta Provider
// =============================================================================

/// Generic stateful delta hedge manager with pluggable delta computation
///
/// This is the new version that supports multiple delta computation strategies
/// via the DeltaProvider trait. It replaces the old HedgeState which used
/// gamma approximation exclusively.
///
/// # Key Features
/// - Pluggable delta computation via DeltaProvider trait
/// - Tracks net position delta (options + stock)
/// - Optional realized volatility tracking
/// - Same interface works for real-time and historical backtesting
///
/// # Type Parameters
/// * `P` - Delta provider implementation (GammaApproximation, EntryVolatility, etc.)
pub struct GenericHedgeState<P: DeltaProvider> {
    // Configuration (immutable after creation)
    config: HedgeConfig,
    delta_provider: P,

    // Stock hedge position
    stock_shares: i32,

    // Last known values
    last_delta: f64,
    last_gamma: Option<f64>,

    // Transaction history
    position: HedgePosition,

    // RV tracking (optional)
    spot_history: Vec<(DateTime<Utc>, f64)>,
    track_rv: bool,

    // Attribution tracking (optional)
    attribution_enabled: bool,

    // Entry HV for RV metrics (set when using EntryHV mode)
    entry_hv: Option<f64>,
}

impl<P: DeltaProvider> GenericHedgeState<P> {
    /// Create new hedge state with delta provider
    ///
    /// # Arguments
    /// * `config` - Hedge configuration
    /// * `delta_provider` - Strategy for computing position delta
    /// * `initial_spot` - Spot price at entry (for RV tracking)
    /// * `attribution_enabled` - Whether to enable P&L attribution
    pub fn new(
        config: HedgeConfig,
        delta_provider: P,
        initial_spot: f64,
        attribution_enabled: bool,
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
            entry_hv: None,
        }
    }

    /// Net position delta (options + stock) - ALWAYS per-share
    pub fn net_delta(&self) -> f64 {
        // stock_shares is actual shares, convert to per-share delta
        let stock_delta = self.stock_shares as f64 / self.config.contract_multiplier as f64;
        self.last_delta + stock_delta
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

    /// Check if attribution is enabled
    pub fn attribution_enabled(&self) -> bool {
        self.attribution_enabled
    }

    /// Get hedge actions for attribution timeline
    ///
    /// Returns reference to all hedge actions executed so far
    pub fn hedge_actions(&self) -> &[HedgeAction] {
        &self.position.hedges
    }

    /// Set entry HV for RV metrics computation
    ///
    /// Call this when using EntryHV delta mode to ensure entry_hv is
    /// included in the realized volatility metrics at finalize.
    pub fn set_entry_hv(&mut self, hv: f64) {
        self.entry_hv = Some(hv);
    }

    /// Process a new spot price observation
    ///
    /// Returns Some(HedgeAction) if a rebalance was executed.
    ///
    /// # State Transitions
    /// 1. Get fresh delta from provider (per-share)
    /// 2. Compute net delta (options + stock)
    /// 3. Check if rehedge needed
    /// 4. If yes, compute shares to trade and execute
    pub async fn update(
        &mut self,
        timestamp: DateTime<Utc>,
        spot: f64,
    ) -> Result<Option<HedgeAction>, String> {
        // Track spot for RV computation
        if self.track_rv {
            self.spot_history.push((timestamp, spot));
        }

        // 1. Get fresh delta from provider (per-share)
        let option_delta = self.delta_provider.compute_delta(spot, timestamp).await?;
        self.last_delta = option_delta;
        self.last_gamma = self.delta_provider.compute_gamma(spot, timestamp);

        // 2. Compute net delta (options + stock hedge)
        let net_delta = self.net_delta();

        // 3. Check if rehedge needed
        let gamma = self.last_gamma.unwrap_or(0.0);
        if !self.config.should_rehedge(net_delta, spot, gamma) {
            return Ok(None);
        }

        // 4. Calculate shares to trade (multiplier applied INSIDE shares_to_hedge)
        let shares = self.config.shares_to_hedge(net_delta);
        if shares == 0 {
            return Ok(None);
        }

        // 5. Execute hedge
        let delta_before = net_delta;
        self.stock_shares += shares;
        let delta_after = self.net_delta();

        let action = HedgeAction {
            timestamp,
            shares,
            spot_price: spot,
            delta_before,
            delta_after,
            cost: self.config.transaction_cost_per_share * Decimal::from(shares.abs()),
        };

        self.position.add_hedge(action.clone());

        Ok(Some(action))
    }

    /// Finalize and compute P&L
    pub fn finalize(mut self, exit_spot: f64, entry_iv: Option<f64>, exit_iv: Option<f64>) -> HedgePosition {
        self.position.unrealized_pnl = self.position.calculate_pnl(exit_spot);

        // Compute unwind cost for closing the hedge at exit (Issue A fix)
        // The unwind trade requires selling/buying back all remaining shares
        let unwind_shares = self.position.cumulative_shares.abs();
        let unwind_cost = self.config.transaction_cost_per_share * Decimal::from(unwind_shares);
        self.position.unwind_cost = unwind_cost;
        self.position.total_cost += unwind_cost;

        // Compute average hedge price for capital metrics
        self.position.compute_avg_hedge_price();

        // Compute RV metrics if tracking was enabled
        if self.track_rv && !self.spot_history.is_empty() {
            self.position.realized_vol_metrics = Some(
                RealizedVolatilityMetrics::from_spot_history(
                    &self.spot_history,
                    self.entry_hv,  // Use stored entry_hv from set_entry_hv()
                    entry_iv,
                    exit_iv,
                )
            );
            self.position.spot_history = self.spot_history;
        }

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

// =============================================================================
// Attribution Configuration
// =============================================================================

/// Snapshot timing configuration for P&L attribution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotTimes {
    /// Market open (9:30 ET) and close (16:00 ET)
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

impl Default for SnapshotTimes {
    fn default() -> Self {
        SnapshotTimes::OpenClose
    }
}

/// Volatility source for Greeks recomputation in attribution
///
/// Similar to DeltaComputation but specifically for P&L attribution analysis.
/// Determines how Greeks are recomputed when collecting daily snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum VolatilitySource {
    /// Use IV at trade entry (fixed throughout holding period)
    EntryIV,

    /// Use Historical Volatility at trade entry
    EntryHV {
        /// Lookback window in days
        window: u32,
    },

    /// Recompute from current market IV surface (most accurate)
    CurrentMarketIV,

    /// Recompute HV at each snapshot
    CurrentHV {
        /// Lookback window in days
        window: u32,
    },

    /// Use historical average IV over lookback period
    HistoricalAverageIV {
        /// Lookback period in days
        lookback_days: u32,
    },
}

impl Default for VolatilitySource {
    fn default() -> Self {
        VolatilitySource::CurrentMarketIV
    }
}

/// Configuration for P&L attribution computation
///
/// Controls how position snapshots are collected and Greeks are recomputed
/// for daily P&L attribution analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionConfig {
    /// Whether to compute attribution (adds overhead)
    pub enabled: bool,

    /// Volatility source for Greeks recomputation
    #[serde(default)]
    pub vol_source: VolatilitySource,

    /// Snapshot times configuration
    #[serde(default)]
    pub snapshot_times: SnapshotTimes,
}

impl Default for AttributionConfig {
    fn default() -> Self {
        Self {
            enabled: false,  // Opt-in
            vol_source: VolatilitySource::CurrentMarketIV,
            snapshot_times: SnapshotTimes::OpenClose,
        }
    }
}
