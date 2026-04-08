use crate::entities::*;
use crate::value_objects::*;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use thiserror::Error;

/// Error type for strike selection
#[derive(Error, Debug)]
pub enum SelectionError {
    #[error("No strikes available")]
    NoStrikes,
    #[error("No expirations available")]
    NoExpirations,
    #[error("Insufficient expirations: need {needed}, have {available}")]
    InsufficientExpirations { needed: usize, available: usize },
    #[error("No IV surface for delta-based selection")]
    NoIVSurface,
    #[error("Spread creation failed: {0}")]
    SpreadCreation(#[from] ValidationError),
    #[error("Unsupported strategy: {0}")]
    UnsupportedStrategy(String),
}

// Convert from StrategyError to SelectionError
impl From<StrategyError> for SelectionError {
    fn from(err: StrategyError) -> Self {
        match err {
            StrategyError::NoStrikes => SelectionError::NoStrikes,
            StrategyError::NoExpirations => SelectionError::NoExpirations,
            StrategyError::InsufficientExpirations { needed, available } => {
                SelectionError::InsufficientExpirations { needed, available }
            }
            StrategyError::NoDeltaData | StrategyError::NoLiquidityData => {
                SelectionError::NoIVSurface
            }
            StrategyError::SpreadCreation(e) => SelectionError::SpreadCreation(e),
            StrategyError::UnsupportedStrategy(s) => SelectionError::UnsupportedStrategy(s),
        }
    }
}

/// Backwards-compatibility error type used by SelectionStrategy implementations
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
    #[error("Unsupported strategy: {0}")]
    UnsupportedStrategy(String),
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
            min_short_dte: 3,
            max_short_dte: 45,
            min_long_dte: 14,
            max_long_dte: 90,
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

/// Criteria for selecting expirations
#[derive(Debug, Clone)]
pub struct ExpirationCriteria {
    pub min_short_dte: i32,
    pub max_short_dte: i32,
    pub min_long_dte: i32,
    pub max_long_dte: i32,
}

impl ExpirationCriteria {
    pub fn new(
        min_short_dte: i32,
        max_short_dte: i32,
        min_long_dte: i32,
        max_long_dte: i32,
    ) -> Self {
        Self {
            min_short_dte,
            max_short_dte,
            min_long_dte,
            max_long_dte,
        }
    }
}

impl From<&TradeSelectionCriteria> for ExpirationCriteria {
    fn from(criteria: &TradeSelectionCriteria) -> Self {
        Self {
            min_short_dte: criteria.min_short_dte,
            max_short_dte: criteria.max_short_dte,
            min_long_dte: criteria.min_long_dte,
            max_long_dte: criteria.max_long_dte,
        }
    }
}

/// Strike selection result for multi-leg strategies
#[derive(Debug, Clone)]
pub struct MultiLegStrikeSelection {
    pub center_strikes: Vec<Strike>,
    pub near_strikes: Option<Vec<Strike>>,
    pub far_strikes: Option<Vec<Strike>>,
    pub expiration: NaiveDate,
}

/// Option strategy type (the trade structure)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionStrategy {
    CalendarSpread,
    IronButterfly,
    Straddle,
    CalendarStraddle,
    Strangle,
    Butterfly,
    Condor,
    IronCondor,
}

impl Default for OptionStrategy {
    fn default() -> Self {
        Self::CalendarSpread
    }
}

/// Select short and long expirations for calendar/diagonal spreads
pub fn select_expirations(
    expirations: &[NaiveDate],
    reference_date: NaiveDate,
    min_short_dte: i32,
    max_short_dte: i32,
    min_long_dte: i32,
    max_long_dte: i32,
) -> Result<(NaiveDate, NaiveDate), StrategyError> {
    if expirations.len() < 2 {
        return Err(StrategyError::InsufficientExpirations {
            needed: 2,
            available: expirations.len(),
        });
    }

    let mut sorted: Vec<_> = expirations.iter().collect();
    sorted.sort();

    let short_exp = sorted
        .iter()
        .find(|&&exp| {
            let dte = (*exp - reference_date).num_days();
            dte >= min_short_dte as i64 && dte <= max_short_dte as i64
        })
        .ok_or(StrategyError::NoExpirations)?;

    let long_exp = sorted
        .iter()
        .find(|&&exp| {
            if exp <= short_exp {
                return false;
            }
            let dte = (*exp - reference_date).num_days();
            dte >= min_long_dte as i64 && dte <= max_long_dte as i64
        })
        .ok_or(StrategyError::InsufficientExpirations {
            needed: 2,
            available: 1,
        })?;

    Ok((**short_exp, **long_exp))
}

/// Find the strike closest to the given spot price
pub fn find_closest_strike(strikes: &[Strike], target: f64) -> Result<Strike, StrategyError> {
    strikes
        .iter()
        .min_by(|a, b| {
            let a_diff = (f64::from(**a) - target).abs();
            let b_diff = (f64::from(**b) - target).abs();
            a_diff
                .partial_cmp(&b_diff)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .ok_or(StrategyError::NoStrikes)
}
