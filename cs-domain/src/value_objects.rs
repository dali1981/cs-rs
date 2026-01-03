use chrono::{NaiveDate, NaiveTime, DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Serialize, Deserialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Strike must be positive, got {0}")]
    InvalidStrike(Decimal),
    #[error("Expiration mismatch: short {short} must be before long {long}")]
    ExpirationMismatch { short: NaiveDate, long: NaiveDate },
    #[error("Symbol mismatch: {0} != {1}")]
    SymbolMismatch(String, String),
    #[error("Strike mismatch: call strike != put strike")]
    StrikeMismatch { call: Strike, put: Strike },
    #[error("Invalid strike order: {0}")]
    InvalidStrikeOrder(String),
    #[error("Invalid option type: {0}")]
    InvalidOptionType(String),
}

/// Strike price (validated positive)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Strike(Decimal);

impl Strike {
    pub fn new(value: Decimal) -> Result<Self, ValidationError> {
        if value <= Decimal::ZERO {
            return Err(ValidationError::InvalidStrike(value));
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> Decimal { self.0 }
}

impl From<Strike> for f64 {
    fn from(s: Strike) -> f64 {
        s.0.try_into().unwrap_or(0.0)
    }
}

impl TryFrom<f64> for Strike {
    type Error = ValidationError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        let decimal = Decimal::try_from(value)
            .map_err(|_| ValidationError::InvalidStrike(Decimal::ZERO))?;
        Strike::new(decimal)
    }
}

/// Timing configuration for trade entry/exit
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TimingConfig {
    pub entry_hour: u32,
    pub entry_minute: u32,
    pub exit_hour: u32,
    pub exit_minute: u32,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            entry_hour: 9,
            entry_minute: 35,
            exit_hour: 15,
            exit_minute: 55,
        }
    }
}

impl TimingConfig {
    pub fn entry_time(&self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.entry_hour, self.entry_minute, 0).unwrap()
    }

    pub fn exit_time(&self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.exit_hour, self.exit_minute, 0).unwrap()
    }

    pub fn entry_datetime(&self, date: NaiveDate) -> DateTime<Utc> {
        date.and_time(self.entry_time()).and_utc()
    }

    pub fn exit_datetime(&self, date: NaiveDate) -> DateTime<Utc> {
        date.and_time(self.exit_time()).and_utc()
    }
}

/// Earnings announcement timing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EarningsTime {
    BeforeMarketOpen,
    AfterMarketClose,
    Unknown,
}

impl EarningsTime {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "bmo" | "before_market_open" | "pre-market" => Self::BeforeMarketOpen,
            "amc" | "after_market_close" | "post-market" => Self::AfterMarketClose,
            _ => Self::Unknown,
        }
    }
}

/// Spot price with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotPrice {
    pub value: Decimal,
    pub timestamp: DateTime<Utc>,
}

impl SpotPrice {
    pub fn new(value: Decimal, timestamp: DateTime<Utc>) -> Self {
        Self { value, timestamp }
    }

    pub fn to_f64(&self) -> f64 {
        self.value.try_into().unwrap_or(0.0)
    }
}

/// Trade failure reasons
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureReason {
    NoSpotPrice,
    NoOptionsData,
    DegenerateSpread,
    InsufficientExpirations,
    IVRatioFilter,
    PricingError(String),
}

/// ATM (At-The-Money) IV observation for a single trading day
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtmIvObservation {
    pub symbol: String,
    pub date: NaiveDate,
    pub spot: Decimal,

    // === Rolling TTE fields (existing) ===
    /// ATM IV for nearest expiration (>min_dte to avoid expiry effects)
    pub atm_iv_nearest: Option<f64>,
    /// DTE of nearest expiration used
    pub nearest_dte: Option<i64>,
    /// ATM IV for ~30 DTE options (rolling)
    pub atm_iv_30d: Option<f64>,
    /// ATM IV for ~60 DTE options (rolling)
    pub atm_iv_60d: Option<f64>,
    /// ATM IV for ~90 DTE options (rolling)
    pub atm_iv_90d: Option<f64>,

    // === Constant-Maturity fields (new) ===
    /// Constant-maturity ATM IV at exactly 7 DTE
    #[serde(default)]
    pub cm_iv_7d: Option<f64>,
    /// Constant-maturity ATM IV at exactly 14 DTE
    #[serde(default)]
    pub cm_iv_14d: Option<f64>,
    /// Constant-maturity ATM IV at exactly 21 DTE
    #[serde(default)]
    pub cm_iv_21d: Option<f64>,
    /// Constant-maturity ATM IV at exactly 30 DTE
    #[serde(default)]
    pub cm_iv_30d: Option<f64>,
    /// Constant-maturity ATM IV at exactly 60 DTE
    #[serde(default)]
    pub cm_iv_60d: Option<f64>,
    /// Constant-maturity ATM IV at exactly 90 DTE
    #[serde(default)]
    pub cm_iv_90d: Option<f64>,
    /// Was constant-maturity interpolated (true) or extrapolated (false)?
    #[serde(default)]
    pub cm_interpolated: Option<bool>,
    /// Number of expirations used for interpolation
    #[serde(default)]
    pub cm_num_expirations: Option<usize>,

    // === Term spreads ===
    /// Term spread: IV_30d - IV_60d (positive = backwardation)
    pub term_spread_30_60: Option<f64>,
    /// Term spread: IV_30d - IV_90d (positive = backwardation)
    pub term_spread_30_90: Option<f64>,
    /// Constant-maturity term spread: CM_IV_7d - CM_IV_30d
    #[serde(default)]
    pub cm_spread_7_30: Option<f64>,
    /// Constant-maturity term spread: CM_IV_30d - CM_IV_60d
    #[serde(default)]
    pub cm_spread_30_60: Option<f64>,
    /// Constant-maturity term spread: CM_IV_30d - CM_IV_90d
    #[serde(default)]
    pub cm_spread_30_90: Option<f64>,

    // === Historical Volatility fields ===
    /// 10-day realized volatility
    #[serde(default)]
    pub hv_10d: Option<f64>,
    /// 20-day realized volatility
    #[serde(default)]
    pub hv_20d: Option<f64>,
    /// 30-day realized volatility (default window)
    #[serde(default)]
    pub hv_30d: Option<f64>,
    /// 60-day realized volatility
    #[serde(default)]
    pub hv_60d: Option<f64>,

    // === IV vs HV spreads ===
    /// IV-HV spread at 30d (volatility risk premium)
    #[serde(default)]
    pub iv_hv_spread_30d: Option<f64>,
}

impl AtmIvObservation {
    pub fn new(symbol: String, date: NaiveDate, spot: Decimal) -> Self {
        Self {
            symbol,
            date,
            spot,
            atm_iv_nearest: None,
            nearest_dte: None,
            atm_iv_30d: None,
            atm_iv_60d: None,
            atm_iv_90d: None,
            cm_iv_7d: None,
            cm_iv_14d: None,
            cm_iv_21d: None,
            cm_iv_30d: None,
            cm_iv_60d: None,
            cm_iv_90d: None,
            cm_interpolated: None,
            cm_num_expirations: None,
            term_spread_30_60: None,
            term_spread_30_90: None,
            cm_spread_7_30: None,
            cm_spread_30_60: None,
            cm_spread_30_90: None,
            hv_10d: None,
            hv_20d: None,
            hv_30d: None,
            hv_60d: None,
            iv_hv_spread_30d: None,
        }
    }

    /// Calculate term spreads from IV values
    pub fn calculate_spreads(&mut self) {
        // Rolling spreads
        if let (Some(iv_30), Some(iv_60)) = (self.atm_iv_30d, self.atm_iv_60d) {
            self.term_spread_30_60 = Some(iv_30 - iv_60);
        }
        if let (Some(iv_30), Some(iv_90)) = (self.atm_iv_30d, self.atm_iv_90d) {
            self.term_spread_30_90 = Some(iv_30 - iv_90);
        }

        // Constant-maturity spreads
        if let (Some(iv_7), Some(iv_30)) = (self.cm_iv_7d, self.cm_iv_30d) {
            self.cm_spread_7_30 = Some(iv_7 - iv_30);
        }
        if let (Some(iv_30), Some(iv_60)) = (self.cm_iv_30d, self.cm_iv_60d) {
            self.cm_spread_30_60 = Some(iv_30 - iv_60);
        }
        if let (Some(iv_30), Some(iv_90)) = (self.cm_iv_30d, self.cm_iv_90d) {
            self.cm_spread_30_90 = Some(iv_30 - iv_90);
        }
    }
}

/// ATM strike selection method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AtmMethod {
    /// Strike closest to spot (default)
    Closest,
    /// Strike immediately below spot
    BelowSpot,
    /// Strike immediately above spot
    AboveSpot,
}

impl Default for AtmMethod {
    fn default() -> Self {
        Self::Closest
    }
}

/// Interpolation method for term structure IVs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IvInterpolationMethod {
    /// Rolling TTE: Use closest expiration within tolerance (existing behavior)
    Rolling,
    /// Constant Maturity: Interpolate in variance space to exact target DTEs
    ConstantMaturity,
}

impl Default for IvInterpolationMethod {
    fn default() -> Self {
        Self::Rolling
    }
}

/// Configuration for ATM IV computation and earnings detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtmIvConfig {
    /// Target maturities in days (default: [7, 14, 21, 30, 60, 90])
    pub maturity_targets: Vec<u32>,
    /// Tolerance window for maturity matching (default: 7 days)
    /// Only used in Rolling mode
    pub maturity_tolerance: u32,
    /// ATM strike selection method
    pub atm_strike_method: AtmMethod,
    /// Minimum valid IV (default: 0.01)
    pub iv_min_bound: f64,
    /// Maximum valid IV (default: 5.0)
    pub iv_max_bound: f64,
    /// IV spike threshold for detection (default: 0.20 = 20%)
    pub spike_threshold: f64,
    /// Lookback window for spike detection (default: 5 days)
    pub spike_lookback_days: usize,
    /// IV crush threshold for detection (default: 0.15 = 15%)
    pub crush_threshold: f64,
    /// Backwardation threshold (default: 0.05 = 5%)
    pub backwardation_threshold: f64,
    /// Interpolation method (default: Rolling for backward compatibility)
    #[serde(default)]
    pub interpolation_method: IvInterpolationMethod,
    /// Minimum DTE for expiration inclusion (default: 3)
    /// Used in ConstantMaturity mode to avoid expiry effects
    #[serde(default = "default_min_dte")]
    pub min_dte: i64,
}

fn default_min_dte() -> i64 {
    3
}

impl Default for AtmIvConfig {
    fn default() -> Self {
        Self {
            maturity_targets: vec![7, 14, 21, 30, 60, 90],
            maturity_tolerance: 7,
            atm_strike_method: AtmMethod::default(),
            iv_min_bound: 0.01,
            iv_max_bound: 5.0,
            spike_threshold: 0.20,
            spike_lookback_days: 5,
            crush_threshold: 0.15,
            backwardation_threshold: 0.05,
            interpolation_method: IvInterpolationMethod::default(),
            min_dte: 3,
        }
    }
}

/// Configuration for Historical Volatility computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HvConfig {
    /// Rolling window sizes in days (default: [10, 20, 30, 60])
    pub windows: Vec<usize>,
    /// Annualization factor (default: 252.0 trading days per year)
    pub annualization_factor: f64,
    /// Minimum data points required before computing HV (default: 20)
    pub min_data_points: usize,
}

impl Default for HvConfig {
    fn default() -> Self {
        Self {
            windows: vec![10, 20, 30, 60],
            annualization_factor: 252.0,
            min_data_points: 20,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn test_strike_valid() {
        let strike = Strike::new(Decimal::new(100, 0));
        assert!(strike.is_ok());
        assert_eq!(strike.unwrap().value(), Decimal::new(100, 0));
    }

    #[test]
    fn test_strike_invalid_zero() {
        let strike = Strike::new(Decimal::ZERO);
        assert!(strike.is_err());
    }

    #[test]
    fn test_strike_invalid_negative() {
        let strike = Strike::new(Decimal::new(-100, 0));
        assert!(strike.is_err());
    }

    #[test]
    fn test_strike_to_f64() {
        let strike = Strike::new(Decimal::new(1055, 1)).unwrap(); // 105.5
        let f64_value: f64 = strike.into();
        assert_eq!(f64_value, 105.5);
    }

    #[test]
    fn test_strike_ordering() {
        let s1 = Strike::new(Decimal::new(100, 0)).unwrap();
        let s2 = Strike::new(Decimal::new(105, 0)).unwrap();
        assert!(s1 < s2);
    }

    #[test]
    fn test_timing_config_default() {
        let config = TimingConfig::default();
        assert_eq!(config.entry_hour, 9);
        assert_eq!(config.entry_minute, 35);
        assert_eq!(config.exit_hour, 15);
        assert_eq!(config.exit_minute, 55);
    }

    #[test]
    fn test_timing_config_entry_time() {
        let config = TimingConfig::default();
        let time = config.entry_time();
        assert_eq!(time.hour(), 9);
        assert_eq!(time.minute(), 35);
    }

    #[test]
    fn test_timing_config_entry_datetime() {
        let config = TimingConfig::default();
        let date = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();
        let dt = config.entry_datetime(date);

        assert_eq!(dt.date_naive(), date);
        assert_eq!(dt.time().hour(), 9);
        assert_eq!(dt.time().minute(), 35);
    }

    #[test]
    fn test_earnings_time_from_str() {
        assert_eq!(EarningsTime::from_str("bmo"), EarningsTime::BeforeMarketOpen);
        assert_eq!(EarningsTime::from_str("BMO"), EarningsTime::BeforeMarketOpen);
        assert_eq!(EarningsTime::from_str("before_market_open"), EarningsTime::BeforeMarketOpen);
        assert_eq!(EarningsTime::from_str("amc"), EarningsTime::AfterMarketClose);
        assert_eq!(EarningsTime::from_str("AMC"), EarningsTime::AfterMarketClose);
        assert_eq!(EarningsTime::from_str("after_market_close"), EarningsTime::AfterMarketClose);
        assert_eq!(EarningsTime::from_str("unknown"), EarningsTime::Unknown);
    }

    #[test]
    fn test_spot_price_new() {
        let now = Utc::now();
        let spot = SpotPrice::new(Decimal::new(10050, 2), now); // 100.50
        assert_eq!(spot.value, Decimal::new(10050, 2));
        assert_eq!(spot.timestamp, now);
    }

    #[test]
    fn test_spot_price_to_f64() {
        let now = Utc::now();
        let spot = SpotPrice::new(Decimal::new(10050, 2), now); // 100.50
        assert_eq!(spot.to_f64(), 100.50);
    }

    #[test]
    fn test_failure_reason_equality() {
        assert_eq!(FailureReason::NoSpotPrice, FailureReason::NoSpotPrice);
        assert_ne!(FailureReason::NoSpotPrice, FailureReason::NoOptionsData);

        let err1 = FailureReason::PricingError("test".to_string());
        let err2 = FailureReason::PricingError("test".to_string());
        assert_eq!(err1, err2);
    }
}
