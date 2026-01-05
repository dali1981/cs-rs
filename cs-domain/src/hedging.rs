use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

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
            contract_multiplier: 100,
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
