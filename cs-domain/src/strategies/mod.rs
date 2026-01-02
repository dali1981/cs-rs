pub mod atm;
pub mod delta;
pub mod iron_butterfly;

use crate::entities::*;
use crate::value_objects::*;
use chrono::NaiveDate;
use cs_analytics::IVSurface;
use finq_core::OptionType;
use thiserror::Error;

pub use atm::ATMStrategy;
pub use delta::{DeltaStrategy, DeltaScanMode};
pub use iron_butterfly::IronButterflyStrategy;

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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TradeSelectionCriteria {
    pub min_short_dte: i32,
    pub max_short_dte: i32,
    pub min_long_dte: i32,
    pub max_long_dte: i32,
    pub target_delta: Option<f64>,
    pub min_iv_ratio: Option<f64>,
    pub max_bid_ask_spread_pct: Option<f64>,
}

impl Default for TradeSelectionCriteria {
    fn default() -> Self {
        Self {
            min_short_dte: 3,    // Match Python: avoid gamma/pin risk
            max_short_dte: 45,   // Match Python: reasonable front month
            min_long_dte: 14,    // Match Python: ensure time value
            max_long_dte: 90,    // Match Python: reasonable back month
            target_delta: None,
            min_iv_ratio: None,
            max_bid_ask_spread_pct: None,
        }
    }
}

/// Strike matching mode for calendar/diagonal spreads
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrikeMatchMode {
    /// Same strike for both legs (true calendar spread)
    SameStrike,
    /// Same delta for both legs (diagonal spread)
    SameDelta,
}

impl Default for StrikeMatchMode {
    fn default() -> Self {
        Self::SameStrike
    }
}

impl StrikeMatchMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "same_strike" | "samestrike" | "calendar" => Some(Self::SameStrike),
            "same_delta" | "samedelta" | "diagonal" => Some(Self::SameDelta),
            _ => None,
        }
    }
}

/// Option chain data for strategy selection
#[derive(Debug)]
pub struct OptionChainData {
    pub expirations: Vec<NaiveDate>,
    pub strikes: Vec<Strike>,
    pub deltas: Option<Vec<(Strike, f64)>>,
    pub volumes: Option<Vec<(Strike, u64)>>,
    pub iv_ratios: Option<Vec<(Strike, f64)>>,
    /// IV surface for delta-space strategies
    pub iv_surface: Option<IVSurface>,
}

/// Strategy trait for strike selection
pub trait TradingStrategy: Send + Sync {
    fn select(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
        option_type: OptionType,
    ) -> Result<CalendarSpread, StrategyError>;
}
