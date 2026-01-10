pub mod atm;
pub mod delta;
pub mod iron_butterfly;
pub mod straddle;
pub mod multi_leg;

use crate::entities::*;
use crate::value_objects::*;
use chrono::NaiveDate;
use cs_analytics::IVSurface;
use finq_core::OptionType;
use rust_decimal::Decimal;
use thiserror::Error;

pub use atm::ATMStrategy;
pub use delta::{DeltaStrategy, DeltaScanMode};
pub use iron_butterfly::IronButterflyStrategy;
pub use straddle::StraddleStrategy;
pub use multi_leg::SymmetricMultiLegSelector;

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
            StrategyError::InsufficientExpirations { needed, available } =>
                SelectionError::InsufficientExpirations { needed, available },
            StrategyError::NoDeltaData | StrategyError::NoLiquidityData =>
                SelectionError::NoIVSurface,
            StrategyError::SpreadCreation(e) => SelectionError::SpreadCreation(e),
            StrategyError::UnsupportedStrategy(s) => SelectionError::UnsupportedStrategy(s),
        }
    }
}

// Backwards compatibility alias
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

/// Option chain data for strategy selection (backwards compatibility)
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

/// Criteria for selecting expirations
#[derive(Debug, Clone)]
pub struct ExpirationCriteria {
    pub min_short_dte: i32,
    pub max_short_dte: i32,
    pub min_long_dte: i32,
    pub max_long_dte: i32,
}

/// Strike selection result for multi-leg strategies
#[derive(Debug, Clone)]
pub struct MultiLegStrikeSelection {
    /// Center strikes (1 for Strangle, 2 for Butterfly/Straddle)
    pub center_strikes: Vec<Strike>,
    /// Near/inner wing strikes (for Condor strategies)
    pub near_strikes: Option<Vec<Strike>>,
    /// Far/outer wing strikes
    pub far_strikes: Option<Vec<Strike>>,
    /// Selected expiration date
    pub expiration: NaiveDate,
}

impl ExpirationCriteria {
    pub fn new(min_short_dte: i32, max_short_dte: i32, min_long_dte: i32, max_long_dte: i32) -> Self {
        Self { min_short_dte, max_short_dte, min_long_dte, max_long_dte }
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

/// New trait for strike selection using IVSurface directly
///
/// All trade types default to ATM. Only calendar spreads can use delta-based selection.
pub trait StrikeSelector: Send + Sync {
    /// Select a calendar spread (can use ATM or Delta)
    fn select_calendar_spread(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        option_type: OptionType,
        criteria: &ExpirationCriteria,
    ) -> Result<CalendarSpread, SelectionError>;

    /// Select a long straddle (always ATM)
    ///
    /// # Arguments
    /// * `spot` - Current spot price
    /// * `surface` - IV surface with available expirations
    /// * `min_expiration` - Minimum required expiration date (options must expire AFTER this date)
    fn select_long_straddle(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _min_expiration: NaiveDate,
    ) -> Result<LongStraddle, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Long straddle not supported by this selector".to_string()
        ))
    }

    /// Select a short straddle (always ATM)
    ///
    /// # Arguments
    /// * `spot` - Current spot price
    /// * `surface` - IV surface with available expirations
    /// * `min_expiration` - Minimum required expiration date (options must expire AFTER this date)
    fn select_short_straddle(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _min_expiration: NaiveDate,
    ) -> Result<ShortStraddle, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Short straddle not supported by this selector".to_string()
        ))
    }

    /// Select a straddle (always ATM) - DEPRECATED
    #[deprecated(since = "0.3.0", note = "Use select_long_straddle or select_short_straddle")]
    fn select_straddle(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        min_expiration: NaiveDate,
    ) -> Result<LongStraddle, SelectionError> {
        self.select_long_straddle(spot, surface, min_expiration)
    }

    /// Select a calendar straddle (always ATM)
    fn select_calendar_straddle(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _criteria: &ExpirationCriteria,
    ) -> Result<CalendarStraddle, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Calendar straddle not supported by this selector".to_string()
        ))
    }

    /// Select an iron butterfly (ATM center + wings)
    fn select_iron_butterfly(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _wing_width: Decimal,
        _min_dte: i32,
        _max_dte: i32,
    ) -> Result<IronButterfly, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Iron butterfly not supported by this selector".to_string()
        ))
    }

    /// Select an iron butterfly with advanced wing positioning configuration
    fn select_iron_butterfly_with_config(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _config: &crate::value_objects::IronButterflyConfig,
        _direction: crate::value_objects::TradeDirection,
        _min_dte: i32,
        _max_dte: i32,
    ) -> Result<IronButterfly, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Advanced iron butterfly selection not supported by this selector".to_string()
        ))
    }

    /// Select a LONG iron butterfly (buy ATM straddle, sell wings - profits from volatility)
    fn select_long_iron_butterfly(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _wing_width: Decimal,
        _min_dte: i32,
        _max_dte: i32,
    ) -> Result<LongIronButterfly, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Long iron butterfly not supported by this selector".to_string()
        ))
    }

    /// Select strikes for a multi-leg volatility strategy
    fn select_multi_leg(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _config: &crate::value_objects::MultiLegStrategyConfig,
        _min_dte: i32,
        _max_dte: i32,
    ) -> Result<MultiLegStrikeSelection, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Multi-leg selection not supported by this selector".to_string()
        ))
    }
}

/// Option strategy type (the trade structure)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionStrategy {
    /// Calendar spread (or diagonal spread if using same-delta matching)
    CalendarSpread,
    /// Iron butterfly (short straddle with protective wings)
    IronButterfly,
    /// Long straddle (long ATM call + long ATM put)
    Straddle,
    /// Calendar straddle (short near-term straddle + long far-term straddle)
    CalendarStraddle,
    /// Strangle (OTM call + OTM put, symmetric wings)
    Strangle,
    /// Butterfly (2x ATM ± OTM wings)
    Butterfly,
    /// Condor (near spread ± far wings)
    Condor,
    /// Iron condor (near spread ± far wings)
    IronCondor,
}

impl Default for OptionStrategy {
    fn default() -> Self {
        Self::CalendarSpread
    }
}

/// Selection strategy trait - determines HOW to select strikes/expirations
///
/// This was previously called "TradingStrategy" but renamed to clarify that
/// it's about SELECTION logic, not the trade structure itself.
pub trait SelectionStrategy: Send + Sync {
    /// Select a calendar spread opportunity
    fn select_calendar_spread(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
        option_type: OptionType,
    ) -> Result<CalendarSpread, StrategyError>;

    /// Select an iron butterfly opportunity
    ///
    /// Not all selection strategies need to support iron butterfly.
    /// Default implementation returns an error.
    fn select_iron_butterfly(
        &self,
        _event: &EarningsEvent,
        _spot: &SpotPrice,
        _chain_data: &OptionChainData,
    ) -> Result<IronButterfly, StrategyError> {
        Err(StrategyError::UnsupportedStrategy(
            "Iron butterfly not supported by this selection strategy".to_string()
        ))
    }

    /// Select a long straddle opportunity
    ///
    /// Selects ATM strike and first expiration AFTER earnings date.
    /// Default implementation returns UnsupportedStrategy error.
    fn select_long_straddle(
        &self,
        _event: &EarningsEvent,
        _spot: &SpotPrice,
        _chain_data: &OptionChainData,
    ) -> Result<LongStraddle, StrategyError> {
        Err(StrategyError::UnsupportedStrategy(
            "Long straddle not supported by this selection strategy".to_string()
        ))
    }

    /// Select a short straddle opportunity
    ///
    /// Selects ATM strike and first expiration AFTER earnings date.
    /// Default implementation returns UnsupportedStrategy error.
    fn select_short_straddle(
        &self,
        _event: &EarningsEvent,
        _spot: &SpotPrice,
        _chain_data: &OptionChainData,
    ) -> Result<ShortStraddle, StrategyError> {
        Err(StrategyError::UnsupportedStrategy(
            "Short straddle not supported by this selection strategy".to_string()
        ))
    }

    /// Select a straddle opportunity - DEPRECATED
    #[deprecated(since = "0.3.0", note = "Use select_long_straddle or select_short_straddle")]
    fn select_straddle(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
    ) -> Result<LongStraddle, StrategyError> {
        self.select_long_straddle(event, spot, chain_data)
    }

    /// Select a calendar straddle opportunity
    ///
    /// Combines two calendar spreads (call and put) at the same ATM strike.
    /// Short near-term straddle + long far-term straddle.
    /// Default implementation returns UnsupportedStrategy error.
    fn select_calendar_straddle(
        &self,
        _event: &EarningsEvent,
        _spot: &SpotPrice,
        _chain_data: &OptionChainData,
    ) -> Result<CalendarStraddle, StrategyError> {
        Err(StrategyError::UnsupportedStrategy(
            "Calendar straddle not supported by this selection strategy".to_string()
        ))
    }
}

// Backwards compatibility: TradingStrategy is an alias for SelectionStrategy
#[deprecated(since = "0.2.0", note = "Use SelectionStrategy instead")]
pub trait TradingStrategy: SelectionStrategy {}

// ============================================================================
// Shared utility functions
// ============================================================================

/// Select short and long expirations for calendar/diagonal spreads
///
/// # Arguments
/// * `expirations` - Available expiration dates
/// * `reference_date` - Date to calculate DTE from (typically earnings date)
/// * `min_short_dte` / `max_short_dte` - DTE range for short leg
/// * `min_long_dte` / `max_long_dte` - DTE range for long leg
///
/// # Returns
/// Tuple of (short_expiration, long_expiration)
///
/// # Errors
/// * `InsufficientExpirations` if fewer than 2 expirations available
/// * `NoExpirations` if no expiration meets the short leg criteria
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

    // Find short expiry (first one meeting min/max DTE)
    let short_exp = sorted
        .iter()
        .find(|&&exp| {
            let dte = (*exp - reference_date).num_days();
            dte >= min_short_dte as i64 && dte <= max_short_dte as i64
        })
        .ok_or(StrategyError::NoExpirations)?;

    // Find long expiry (first one after short meeting min/max DTE)
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
///
/// # Arguments
/// * `strikes` - Available strikes
/// * `target` - Target price (typically spot price or theoretical strike)
///
/// # Returns
/// The strike closest to target
///
/// # Errors
/// * `NoStrikes` if strikes slice is empty
pub fn find_closest_strike(strikes: &[Strike], target: f64) -> Result<Strike, StrategyError> {
    strikes
        .iter()
        .min_by(|a, b| {
            let a_diff = (f64::from(**a) - target).abs();
            let b_diff = (f64::from(**b) - target).abs();
            a_diff.partial_cmp(&b_diff).unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .ok_or(StrategyError::NoStrikes)
}
