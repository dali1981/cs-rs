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
}

impl Default for HedgeConfig {
    fn default() -> Self {
        Self {
            strategy: HedgeStrategy::None,
            max_rehedges: None,
            min_hedge_size: 1,
            transaction_cost_per_share: Decimal::ZERO,
            contract_multiplier: CONTRACT_MULTIPLIER,
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
