// Strike selection strategies that use IVSurface for trade selection.
// These live in cs-backtest because they depend on cs-analytics (IVSurface).
// Pure selection types (ExpirationCriteria, TradeSelectionCriteria, etc.)
// remain in cs-domain.

pub mod atm;
pub mod delta;
pub mod multi_leg;

pub use atm::ATMStrategy;
pub use delta::{DeltaStrategy, DeltaScanMode};
pub use multi_leg::SymmetricMultiLegSelector;

use cs_domain::entities::{
    CalendarSpread, CalendarStraddle, IronButterfly, LongIronButterfly, LongStraddle, ShortStraddle,
};
use cs_domain::strike_selection::{ExpirationCriteria, MultiLegStrikeSelection, SelectionError};
use cs_domain::value_objects::{MultiLegStrategyConfig, SpotPrice};
use chrono::NaiveDate;
use cs_analytics::IVSurface;
use finq_core::OptionType;
use rust_decimal::Decimal;

/// New trait for strike selection using IVSurface directly.
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
    fn select_long_straddle(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _min_expiration: NaiveDate,
    ) -> Result<LongStraddle, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Long straddle not supported by this selector".to_string(),
        ))
    }

    /// Select a short straddle (always ATM)
    fn select_short_straddle(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _min_expiration: NaiveDate,
    ) -> Result<ShortStraddle, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Short straddle not supported by this selector".to_string(),
        ))
    }

    /// Select a calendar straddle (always ATM)
    fn select_calendar_straddle(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _criteria: &ExpirationCriteria,
    ) -> Result<CalendarStraddle, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Calendar straddle not supported by this selector".to_string(),
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
            "Iron butterfly not supported by this selector".to_string(),
        ))
    }

    /// Select an iron butterfly with advanced wing positioning configuration
    fn select_iron_butterfly_with_config(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _config: &cs_domain::value_objects::IronButterflyConfig,
        _direction: cs_domain::value_objects::TradeDirection,
        _min_dte: i32,
        _max_dte: i32,
    ) -> Result<IronButterfly, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Advanced iron butterfly selection not supported by this selector".to_string(),
        ))
    }

    /// Select a LONG iron butterfly (buy ATM straddle, sell wings)
    fn select_long_iron_butterfly(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _wing_width: Decimal,
        _min_dte: i32,
        _max_dte: i32,
    ) -> Result<LongIronButterfly, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Long iron butterfly not supported by this selector".to_string(),
        ))
    }

    /// Select strikes for a multi-leg volatility strategy
    fn select_multi_leg(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _config: &MultiLegStrategyConfig,
        _min_dte: i32,
        _max_dte: i32,
    ) -> Result<MultiLegStrikeSelection, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Multi-leg selection not supported by this selector".to_string(),
        ))
    }
}
