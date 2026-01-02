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
    /// ATM IV for ~30 DTE options
    pub atm_iv_30d: Option<f64>,
    /// ATM IV for ~60 DTE options
    pub atm_iv_60d: Option<f64>,
    /// ATM IV for ~90 DTE options
    pub atm_iv_90d: Option<f64>,
    /// Term spread: IV_30d - IV_60d (positive = backwardation)
    pub term_spread_30_60: Option<f64>,
    /// Term spread: IV_30d - IV_90d (positive = backwardation)
    pub term_spread_30_90: Option<f64>,
}

impl AtmIvObservation {
    pub fn new(symbol: String, date: NaiveDate, spot: Decimal) -> Self {
        Self {
            symbol,
            date,
            spot,
            atm_iv_30d: None,
            atm_iv_60d: None,
            atm_iv_90d: None,
            term_spread_30_60: None,
            term_spread_30_90: None,
        }
    }

    /// Calculate term spreads from IV values
    pub fn calculate_spreads(&mut self) {
        if let (Some(iv_30), Some(iv_60)) = (self.atm_iv_30d, self.atm_iv_60d) {
            self.term_spread_30_60 = Some(iv_30 - iv_60);
        }
        if let (Some(iv_30), Some(iv_90)) = (self.atm_iv_30d, self.atm_iv_90d) {
            self.term_spread_30_90 = Some(iv_30 - iv_90);
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

/// Configuration for ATM IV computation and earnings detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtmIvConfig {
    /// Target maturities in days (default: [30, 60, 90])
    pub maturity_targets: Vec<u32>,
    /// Tolerance window for maturity matching (default: 7 days)
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
}

impl Default for AtmIvConfig {
    fn default() -> Self {
        Self {
            maturity_targets: vec![30, 60, 90],
            maturity_tolerance: 7,
            atm_strike_method: AtmMethod::default(),
            iv_min_bound: 0.01,
            iv_max_bound: 5.0,
            spike_threshold: 0.20,
            spike_lookback_days: 5,
            crush_threshold: 0.15,
            backwardation_threshold: 0.05,
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
