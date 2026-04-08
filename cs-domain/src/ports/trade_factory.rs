use crate::entities::{LongStraddle, CalendarSpread, IronButterfly, Strangle, Butterfly, Condor, IronCondor};
use crate::value_objects::{IronButterflyConfig, TradeDirection, MultiLegStrategyConfig};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use thiserror::Error;
use finq_core::OptionType;

/// Port for creating trades from market data
///
/// This abstraction separates trade construction (selection + creation)
/// from trade execution. Implementations query market data repositories,
/// build IV surfaces, and use strike selectors to construct valid trades.
#[async_trait::async_trait]
pub trait TradeFactory: Send + Sync {
    /// Create an ATM straddle at the given date with minimum expiration
    ///
    /// # Arguments
    /// * `symbol` - Ticker symbol
    /// * `as_of` - Date/time to query market data
    /// * `min_expiration` - Minimum required expiration date (options must expire AFTER this)
    ///
    /// # Returns
    /// A LongStraddle with:
    /// - ATM strike (closest to spot price)
    /// - First available expiration after min_expiration
    /// - Both call and put legs at same strike/expiration
    ///
    /// # Errors
    /// Returns error if:
    /// - No market data available
    /// - No valid expirations found
    /// - No strikes available
    /// - IV surface construction fails
    async fn create_atm_straddle(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<LongStraddle, TradeFactoryError>;

    /// Create a calendar spread (short near-term + long far-term at same strike)
    ///
    /// Creates a calendar spread by:
    /// 1. Finding ATM strike (closest to spot)
    /// 2. Selecting short expiration by DTE range
    /// 3. Selecting long expiration by DTE range (must be after short)
    /// 4. Building legs and validating
    ///
    /// # Arguments
    /// * `symbol` - Ticker symbol
    /// * `as_of` - Date/time to query market data
    /// * `min_short_dte` - Minimum days to short expiration
    /// * `max_short_dte` - Maximum days to short expiration
    /// * `min_long_dte` - Minimum days to long expiration
    /// * `option_type` - Call or Put
    ///
    /// # Returns
    /// A CalendarSpread with both legs at same strike, different expirations
    ///
    /// # Errors
    /// Returns error if no suitable expirations/strikes found or selection fails
    async fn create_calendar_spread(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_short_dte: u32,
        max_short_dte: u32,
        min_long_dte: u32,
        option_type: OptionType,
    ) -> Result<CalendarSpread, TradeFactoryError>;

    /// Create an iron butterfly (short ATM straddle + long OTM wings)
    ///
    /// Creates an iron butterfly by:
    /// 1. Finding ATM strike (center)
    /// 2. Selecting expiration by DTE range
    /// 3. Calculating upper/lower wing strikes from center ± wing_width
    /// 4. Building 4 legs (short call, short put, long call, long put)
    /// 5. Validating construction
    ///
    /// # Arguments
    /// * `symbol` - Ticker symbol
    /// * `as_of` - Date/time to query market data
    /// * `min_expiration` - Minimum required expiration date
    /// * `wing_width` - Distance from center strike to upper/lower wing (e.g., 10.0 for $10)
    ///
    /// # Returns
    /// An IronButterfly with short center straddle and long OTM wings
    ///
    /// # Errors
    /// Returns error if no suitable expirations/strikes found or selection fails
    async fn create_iron_butterfly(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        wing_width: Decimal,
    ) -> Result<IronButterfly, TradeFactoryError>;

    /// Create an iron butterfly with advanced wing positioning configuration
    ///
    /// Creates an iron butterfly with configurable wing selection strategy:
    /// - Delta-based: Select wings by delta (e.g., 25-delta OTM)
    /// - Moneyness-based: Select wings by % OTM (e.g., 10% OTM)
    /// - Symmetric: Enforce equal wing width on both sides
    /// - Direction: Short (default) or Long (inverted)
    ///
    /// # Arguments
    /// * `symbol` - Ticker symbol
    /// * `as_of` - Date/time to query market data
    /// * `min_expiration` - Minimum required expiration date
    /// * `config` - Wing selection configuration (mode, symmetry)
    /// * `direction` - Trade direction (Short or Long)
    ///
    /// # Returns
    /// An IronButterfly configured per the provided settings
    ///
    /// # Errors
    /// Returns error if no suitable expirations/strikes found or selection fails
    async fn create_iron_butterfly_advanced(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &IronButterflyConfig,
        direction: TradeDirection,
    ) -> Result<IronButterfly, TradeFactoryError>;

    /// Create a Strangle (OTM call + OTM put) with unified configuration
    ///
    /// Creates a strangle by:
    /// 1. Using the SymmetricMultiLegSelector to find wing strikes
    /// 2. Supporting both delta-based and moneyness-based selection
    /// 3. Enforcing symmetric wing constraints
    /// 4. Supporting both long and short directions
    ///
    /// # Arguments
    /// * `symbol` - Ticker symbol
    /// * `as_of` - Date/time to query market data
    /// * `min_expiration` - Minimum required expiration date
    /// * `config` - Multi-leg strategy configuration (wings, direction)
    ///
    /// # Returns
    /// A Strangle trade with configured wings and direction
    async fn create_strangle(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &MultiLegStrategyConfig,
    ) -> Result<Strangle, TradeFactoryError>;

    /// Create a Butterfly (2x ATM ± OTM wings) with unified configuration
    async fn create_butterfly(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &MultiLegStrategyConfig,
    ) -> Result<Butterfly, TradeFactoryError>;

    /// Create a Condor (near ATM ± far wings) with unified configuration
    async fn create_condor(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &MultiLegStrategyConfig,
    ) -> Result<Condor, TradeFactoryError>;

    /// Create an IronCondor (near spread ± far wings) with unified configuration
    async fn create_iron_condor(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &MultiLegStrategyConfig,
    ) -> Result<IronCondor, TradeFactoryError>;

    /// Query available expiration dates for a symbol at a given time
    ///
    /// # Arguments
    /// * `symbol` - Ticker symbol
    /// * `as_of` - Date/time to query market data
    ///
    /// # Returns
    /// Sorted list of expiration dates available in the market data
    ///
    /// # Errors
    /// Returns error if no market data available or IV surface fails to build
    async fn available_expirations(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<NaiveDate>, TradeFactoryError>;
}

/// Errors that can occur during trade creation
#[derive(Debug, Error)]
pub enum TradeFactoryError {
    #[error("No expirations available")]
    NoExpirations,

    #[error("No strikes available")]
    NoStrikes,

    #[error("Data error: {0}")]
    DataError(String),

    #[error("Selection error: {0}")]
    SelectionError(String),
}
