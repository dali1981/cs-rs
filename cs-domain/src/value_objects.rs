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
        use crate::MarketTime;
        Self {
            entry_hour: MarketTime::DEFAULT_ENTRY.hour,
            entry_minute: MarketTime::DEFAULT_ENTRY.minute,
            exit_hour: MarketTime::DEFAULT_HEDGE_CHECK.hour,
            exit_minute: MarketTime::DEFAULT_HEDGE_CHECK.minute,
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

    // === Expected Move fields (from straddle) ===
    /// ATM straddle price for nearest expiration
    #[serde(default)]
    pub straddle_price_nearest: Option<f64>,
    /// Expected move (%) = Straddle / Spot × 100
    #[serde(default)]
    pub expected_move_pct: Option<f64>,
    /// Expected move using 85% rule = Straddle × 0.85 / Spot × 100
    #[serde(default)]
    pub expected_move_85_pct: Option<f64>,
    /// ATM straddle price for ~30 DTE expiration
    #[serde(default)]
    pub straddle_price_30d: Option<f64>,
    /// Expected move for 30 DTE options (%)
    #[serde(default)]
    pub expected_move_30d_pct: Option<f64>,
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
            straddle_price_nearest: None,
            expected_move_pct: None,
            expected_move_85_pct: None,
            straddle_price_30d: None,
            expected_move_30d_pct: None,
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

/// Direction of price move
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveDirection {
    /// Price moved up
    Up,
    /// Price moved down
    Down,
    /// Price moved less than threshold (typically 0.5%)
    Flat,
}

impl MoveDirection {
    /// Determine move direction from percentage change
    ///
    /// # Arguments
    /// * `pct_change` - Percentage change (positive = up, negative = down)
    /// * `flat_threshold` - Threshold below which move is considered flat (default 0.5%)
    pub fn from_pct_change(pct_change: f64, flat_threshold: f64) -> Self {
        if pct_change.abs() < flat_threshold {
            MoveDirection::Flat
        } else if pct_change > 0.0 {
            MoveDirection::Up
        } else {
            MoveDirection::Down
        }
    }
}

impl Default for MoveDirection {
    fn default() -> Self {
        Self::Flat
    }
}

/// Earnings outcome with expected vs actual move comparison
///
/// Tracks pre-earnings expectations vs post-earnings reality
/// to determine whether gamma (realized move) dominated vega (IV crush).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarningsOutcome {
    /// Stock symbol
    pub symbol: String,
    /// Earnings announcement date
    pub earnings_date: NaiveDate,
    /// Timing of announcement (BMO/AMC)
    pub earnings_time: EarningsTime,

    // === Pre-earnings state (close before) ===
    /// Spot price before earnings
    pub pre_spot: Decimal,
    /// ATM straddle price before earnings
    pub pre_straddle: Decimal,
    /// Expected move as percentage of spot
    pub expected_move_pct: f64,
    /// 30-day IV before earnings
    pub pre_iv_30d: f64,

    // === Post-earnings state (open/close after) ===
    /// Spot price after earnings
    pub post_spot: Decimal,
    /// 30-day IV after earnings (for IV crush calculation)
    pub post_iv_30d: Option<f64>,

    // === Actual move ===
    /// Absolute dollar move
    pub actual_move: Decimal,
    /// Move as percentage of pre_spot
    pub actual_move_pct: f64,
    /// Direction of the move
    pub actual_direction: MoveDirection,

    // === Comparison metrics ===
    /// Move ratio = actual_move_pct / expected_move_pct (>1 = gamma wins)
    pub move_ratio: f64,
    /// IV crush = (pre_iv - post_iv) / pre_iv
    pub iv_crush_pct: Option<f64>,
    /// Did gamma dominate vega? (actual > expected)
    pub gamma_dominated: bool,
}

impl EarningsOutcome {
    /// Create a new earnings outcome from pre and post data
    pub fn new(
        symbol: String,
        earnings_date: NaiveDate,
        earnings_time: EarningsTime,
        pre_spot: Decimal,
        pre_straddle: Decimal,
        pre_iv_30d: f64,
        post_spot: Decimal,
        post_iv_30d: Option<f64>,
    ) -> Self {
        let pre_spot_f64: f64 = pre_spot.try_into().unwrap_or(0.0);
        let post_spot_f64: f64 = post_spot.try_into().unwrap_or(0.0);
        let pre_straddle_f64: f64 = pre_straddle.try_into().unwrap_or(0.0);

        // Expected move = straddle / spot * 100
        let expected_move_pct = if pre_spot_f64 > 0.0 {
            (pre_straddle_f64 / pre_spot_f64) * 100.0
        } else {
            0.0
        };

        // Actual move
        let actual_move = if post_spot >= pre_spot {
            post_spot - pre_spot
        } else {
            pre_spot - post_spot
        };
        let actual_move_f64: f64 = actual_move.try_into().unwrap_or(0.0);

        let actual_move_pct = if pre_spot_f64 > 0.0 {
            (actual_move_f64 / pre_spot_f64) * 100.0
        } else {
            0.0
        };

        // Direction
        let pct_change = if pre_spot_f64 > 0.0 {
            ((post_spot_f64 - pre_spot_f64) / pre_spot_f64) * 100.0
        } else {
            0.0
        };
        let actual_direction = MoveDirection::from_pct_change(pct_change, 0.5);

        // Move ratio
        let move_ratio = if expected_move_pct > 0.0 {
            actual_move_pct / expected_move_pct
        } else {
            0.0
        };

        // IV crush
        let iv_crush_pct = post_iv_30d.map(|post_iv| {
            if pre_iv_30d > 0.0 {
                (pre_iv_30d - post_iv) / pre_iv_30d
            } else {
                0.0
            }
        });

        // Gamma dominated if actual > expected
        let gamma_dominated = actual_move_pct > expected_move_pct;

        Self {
            symbol,
            earnings_date,
            earnings_time,
            pre_spot,
            pre_straddle,
            expected_move_pct,
            pre_iv_30d,
            post_spot,
            post_iv_30d,
            actual_move,
            actual_move_pct,
            actual_direction,
            move_ratio,
            iv_crush_pct,
            gamma_dominated,
        }
    }
}

/// Summary statistics for earnings analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarningsSummaryStats {
    /// Total earnings events analyzed
    pub total_events: usize,
    /// Number where actual > expected (gamma won)
    pub gamma_dominated_count: usize,
    /// Number where actual < expected (vega won)
    pub vega_dominated_count: usize,
    /// Average of actual/expected ratios
    pub avg_move_ratio: f64,
    /// Average IV crush percentage
    pub avg_iv_crush_pct: f64,
    /// Average expected move percentage
    pub avg_expected_move_pct: f64,
    /// Average actual move percentage
    pub avg_actual_move_pct: f64,
    /// Number of upward moves
    pub up_moves: usize,
    /// Number of downward moves
    pub down_moves: usize,
}

impl EarningsSummaryStats {
    /// Compute summary statistics from a list of earnings outcomes
    pub fn from_outcomes(outcomes: &[EarningsOutcome]) -> Self {
        if outcomes.is_empty() {
            return Self {
                total_events: 0,
                gamma_dominated_count: 0,
                vega_dominated_count: 0,
                avg_move_ratio: 0.0,
                avg_iv_crush_pct: 0.0,
                avg_expected_move_pct: 0.0,
                avg_actual_move_pct: 0.0,
                up_moves: 0,
                down_moves: 0,
            };
        }

        let total = outcomes.len();
        let gamma_count = outcomes.iter().filter(|o| o.gamma_dominated).count();
        let vega_count = total - gamma_count;

        let avg_move_ratio = outcomes.iter().map(|o| o.move_ratio).sum::<f64>() / total as f64;
        let avg_expected = outcomes.iter().map(|o| o.expected_move_pct).sum::<f64>() / total as f64;
        let avg_actual = outcomes.iter().map(|o| o.actual_move_pct).sum::<f64>() / total as f64;

        let iv_crushes: Vec<f64> = outcomes.iter().filter_map(|o| o.iv_crush_pct).collect();
        let avg_crush = if iv_crushes.is_empty() {
            0.0
        } else {
            iv_crushes.iter().sum::<f64>() / iv_crushes.len() as f64
        };

        let up_moves = outcomes.iter().filter(|o| o.actual_direction == MoveDirection::Up).count();
        let down_moves = outcomes.iter().filter(|o| o.actual_direction == MoveDirection::Down).count();

        Self {
            total_events: total,
            gamma_dominated_count: gamma_count,
            vega_dominated_count: vega_count,
            avg_move_ratio,
            avg_iv_crush_pct: avg_crush,
            avg_expected_move_pct: avg_expected,
            avg_actual_move_pct: avg_actual,
            up_moves,
            down_moves,
        }
    }
}

/// Direction of a multi-leg trade (long or short)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeDirection {
    Long,
    Short,
}

impl Default for TradeDirection {
    fn default() -> Self {
        TradeDirection::Short
    }
}

impl From<&str> for TradeDirection {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "long" => TradeDirection::Long,
            _ => TradeDirection::Short,
        }
    }
}

impl std::fmt::Display for TradeDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TradeDirection::Long => write!(f, "long"),
            TradeDirection::Short => write!(f, "short"),
        }
    }
}

/// Wing selection mode for iron butterfly positioning
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WingSelectionMode {
    /// Delta-based selection: e.g., 0.25 for 25-delta OTM wings
    Delta { wing_delta: f64 },
    /// Moneyness-based selection: e.g., 0.10 for 10% OTM wings
    Moneyness { wing_percent: f64 },
}

impl WingSelectionMode {
    /// Parse wing mode from CLI argument (e.g., "delta:0.25" or "moneyness:0.10")
    pub fn from_cli_arg(arg: &str) -> Result<Self, String> {
        let parts: Vec<&str> = arg.split(':').collect();
        match parts.as_slice() {
            ["delta", val] => {
                let wing_delta = val.parse::<f64>()
                    .map_err(|_| format!("Invalid delta value: {}", val))?;
                if !(0.0..=1.0).contains(&wing_delta) {
                    return Err(format!("Delta must be between 0.0 and 1.0, got {}", wing_delta));
                }
                Ok(WingSelectionMode::Delta { wing_delta })
            }
            ["moneyness", val] => {
                let wing_percent = val.parse::<f64>()
                    .map_err(|_| format!("Invalid moneyness value: {}", val))?;
                if !(0.0..=1.0).contains(&wing_percent) {
                    return Err(format!("Moneyness percent must be between 0.0 and 1.0, got {}", wing_percent));
                }
                Ok(WingSelectionMode::Moneyness { wing_percent })
            }
            _ => Err(format!("Invalid wing mode format: '{}'. Use 'delta:0.25' or 'moneyness:0.10'", arg)),
        }
    }

    /// Create a delta-based config with defaults
    pub fn default_delta() -> Self {
        WingSelectionMode::Delta { wing_delta: 0.25 }
    }
}

impl Default for WingSelectionMode {
    fn default() -> Self {
        Self::default_delta()
    }
}

impl std::fmt::Display for WingSelectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WingSelectionMode::Delta { wing_delta } => write!(f, "delta:{}", wing_delta),
            WingSelectionMode::Moneyness { wing_percent } => write!(f, "moneyness:{}", wing_percent),
        }
    }
}

/// Configuration for iron butterfly wing positioning
#[derive(Debug, Clone)]
pub struct IronButterflyConfig {
    /// How to select wing strikes
    pub wing_mode: WingSelectionMode,
    /// Whether wings should be symmetric (equal width on both sides)
    pub symmetric: bool,
}

impl IronButterflyConfig {
    /// Create a new iron butterfly configuration
    pub fn new(wing_mode: WingSelectionMode, symmetric: bool) -> Self {
        Self { wing_mode, symmetric }
    }

    /// Create default configuration (25-delta symmetric)
    pub fn default_delta() -> Self {
        Self {
            wing_mode: WingSelectionMode::default_delta(),
            symmetric: true,
        }
    }

    /// Create configuration from CLI argument
    pub fn from_cli_arg(arg: &str) -> Result<Self, String> {
        let wing_mode = WingSelectionMode::from_cli_arg(arg)?;
        Ok(Self {
            wing_mode,
            symmetric: true,  // Always symmetric for now
        })
    }

    /// Parse wing mode from optional CLI argument, using default if not provided
    pub fn from_cli_arg_optional(arg: Option<&str>) -> Result<Self, String> {
        match arg {
            Some(s) => Self::from_cli_arg(s),
            None => Ok(Self::default_delta()),
        }
    }
}

impl Default for IronButterflyConfig {
    fn default() -> Self {
        Self::default_delta()
    }
}

impl std::fmt::Display for IronButterflyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.wing_mode)
    }
}

/// How distance from center is specified (delta or moneyness percent)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistanceSpec {
    /// Delta-based distance (e.g., 0.25 for 25-delta)
    Delta(f64),
    /// Moneyness-based distance (e.g., 0.10 for 10% OTM)
    Moneyness(f64),
}

impl DistanceSpec {
    /// Parse distance spec from CLI argument (e.g., "delta:0.25" or "moneyness:0.10")
    pub fn from_cli_arg(arg: &str) -> Result<Self, String> {
        let parts: Vec<&str> = arg.split(':').collect();
        match parts.as_slice() {
            ["delta", val] => {
                let delta = val.parse::<f64>()
                    .map_err(|_| format!("Invalid delta value: {}", val))?;
                if !(0.0..=1.0).contains(&delta) {
                    return Err(format!("Delta must be between 0.0 and 1.0, got {}", delta));
                }
                Ok(DistanceSpec::Delta(delta))
            }
            ["moneyness", val] => {
                let percent = val.parse::<f64>()
                    .map_err(|_| format!("Invalid moneyness value: {}", val))?;
                if !(0.0..=1.0).contains(&percent) {
                    return Err(format!("Moneyness percent must be between 0.0 and 1.0, got {}", percent));
                }
                Ok(DistanceSpec::Moneyness(percent))
            }
            _ => Err(format!("Invalid distance spec format: '{}'. Use 'delta:0.25' or 'moneyness:0.10'", arg)),
        }
    }
}

impl std::fmt::Display for DistanceSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DistanceSpec::Delta(delta) => write!(f, "delta:{}", delta),
            DistanceSpec::Moneyness(percent) => write!(f, "moneyness:{}", percent),
        }
    }
}

/// Spread type defines the wing structure for multi-leg strategies
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpreadType {
    /// Single distance from center (e.g., Strangle: OTM distance)
    Simple { distance_from_center: DistanceSpec },

    /// Two distances for inner/outer strikes (e.g., Condor: near and far)
    Double {
        near_distance: DistanceSpec,
        far_distance: DistanceSpec,
    },
}

impl SpreadType {
    /// Parse spread type from CLI arguments
    /// Format: "simple:delta:0.25" or "double:delta:0.20,0.10"
    pub fn from_cli_arg(arg: &str) -> Result<Self, String> {
        let parts: Vec<&str> = arg.split(':').collect();
        match parts.as_slice() {
            ["simple", _] => {
                let distance = DistanceSpec::from_cli_arg(&format!("{}:{}", parts[1], parts.get(2).unwrap_or(&"")))?;
                Ok(SpreadType::Simple { distance_from_center: distance })
            }
            ["double", _] => {
                let distances: Vec<&str> = parts.get(2).unwrap_or(&"").split(',').collect();
                if distances.len() != 2 {
                    return Err("Double spread requires two distances (near,far)".to_string());
                }
                let near = DistanceSpec::from_cli_arg(&format!("{}:{}", parts[1], distances[0]))?;
                let far = DistanceSpec::from_cli_arg(&format!("{}:{}", parts[1], distances[1]))?;
                Ok(SpreadType::Double { near_distance: near, far_distance: far })
            }
            _ => Err(format!("Invalid spread type format: '{}'", arg)),
        }
    }
}

/// Center strike configuration for the strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CenterConfig {
    /// Number of legs at center (1 for Strangle, 2 for Butterfly, etc.)
    pub multiplicity: u32,
    /// Whether center is a straddle (both call and put) or spread
    pub is_straddle: bool,
}

impl CenterConfig {
    pub fn new(multiplicity: u32, is_straddle: bool) -> Self {
        Self { multiplicity, is_straddle }
    }
}

/// Multi-leg strategy type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiLegStrategyType {
    Strangle,
    Butterfly,
    IronButterfly,
    Condor,
    IronCondor,
}

impl std::fmt::Display for MultiLegStrategyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MultiLegStrategyType::Strangle => write!(f, "strangle"),
            MultiLegStrategyType::Butterfly => write!(f, "butterfly"),
            MultiLegStrategyType::IronButterfly => write!(f, "iron-butterfly"),
            MultiLegStrategyType::Condor => write!(f, "condor"),
            MultiLegStrategyType::IronCondor => write!(f, "iron-condor"),
        }
    }
}

impl std::str::FromStr for MultiLegStrategyType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "strangle" => Ok(MultiLegStrategyType::Strangle),
            "butterfly" => Ok(MultiLegStrategyType::Butterfly),
            "iron-butterfly" | "ironbutterfly" => Ok(MultiLegStrategyType::IronButterfly),
            "condor" => Ok(MultiLegStrategyType::Condor),
            "iron-condor" | "ironcondor" => Ok(MultiLegStrategyType::IronCondor),
            _ => Err(format!("Unknown strategy type: {}", s)),
        }
    }
}

/// Wing configuration for symmetric multi-leg strategies
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SymmetricWingConfig {
    pub spread_type: SpreadType,
    pub symmetric: bool,
}

impl SymmetricWingConfig {
    pub fn new(spread_type: SpreadType, symmetric: bool) -> Self {
        Self { spread_type, symmetric }
    }
}

/// Unified configuration for all multi-leg volatility strategies
#[derive(Debug, Clone)]
pub struct MultiLegStrategyConfig {
    pub strategy_type: MultiLegStrategyType,
    pub center: CenterConfig,
    pub wings: SymmetricWingConfig,
    pub direction: TradeDirection,
}

impl MultiLegStrategyConfig {
    pub fn new(
        strategy_type: MultiLegStrategyType,
        center: CenterConfig,
        wings: SymmetricWingConfig,
        direction: TradeDirection,
    ) -> Self {
        Self { strategy_type, center, wings, direction }
    }

    /// Create a Strangle configuration (25-delta, short direction)
    pub fn strangle_delta(wing_delta: f64) -> Self {
        Self {
            strategy_type: MultiLegStrategyType::Strangle,
            center: CenterConfig::new(1, false),
            wings: SymmetricWingConfig::new(
                SpreadType::Simple { distance_from_center: DistanceSpec::Delta(wing_delta) },
                true,
            ),
            direction: TradeDirection::Short,
        }
    }

    /// Create a Butterfly configuration (25-delta, short direction)
    pub fn butterfly_delta(wing_delta: f64) -> Self {
        Self {
            strategy_type: MultiLegStrategyType::Butterfly,
            center: CenterConfig::new(2, true),
            wings: SymmetricWingConfig::new(
                SpreadType::Simple { distance_from_center: DistanceSpec::Delta(wing_delta) },
                true,
            ),
            direction: TradeDirection::Short,
        }
    }

    /// Create an IronCondor configuration (20-delta near, 10-delta far, short direction)
    pub fn iron_condor_delta(near_delta: f64, far_delta: f64) -> Self {
        Self {
            strategy_type: MultiLegStrategyType::IronCondor,
            center: CenterConfig::new(2, false),
            wings: SymmetricWingConfig::new(
                SpreadType::Double {
                    near_distance: DistanceSpec::Delta(near_delta),
                    far_distance: DistanceSpec::Delta(far_delta),
                },
                true,
            ),
            direction: TradeDirection::Short,
        }
    }

    /// Create a Condor configuration (25-delta near, 35-delta far, short direction)
    pub fn condor_delta(near_delta: f64, far_delta: f64) -> Self {
        Self {
            strategy_type: MultiLegStrategyType::Condor,
            center: CenterConfig::new(1, true),
            wings: SymmetricWingConfig::new(
                SpreadType::Double {
                    near_distance: DistanceSpec::Delta(near_delta),
                    far_distance: DistanceSpec::Delta(far_delta),
                },
                true,
            ),
            direction: TradeDirection::Short,
        }
    }

    /// Create an IronButterfly configuration (25-delta, short direction)
    pub fn iron_butterfly_delta(wing_delta: f64) -> Self {
        Self {
            strategy_type: MultiLegStrategyType::IronButterfly,
            center: CenterConfig::new(1, true),
            wings: SymmetricWingConfig::new(
                SpreadType::Simple { distance_from_center: DistanceSpec::Delta(wing_delta) },
                true,
            ),
            direction: TradeDirection::Short,
        }
    }

    /// Set direction and return self for builder pattern
    pub fn with_direction(mut self, direction: TradeDirection) -> Self {
        self.direction = direction;
        self
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

    #[test]
    fn test_trade_direction_default() {
        assert_eq!(TradeDirection::default(), TradeDirection::Short);
    }

    #[test]
    fn test_trade_direction_from_str() {
        assert_eq!(TradeDirection::from("long"), TradeDirection::Long);
        assert_eq!(TradeDirection::from("Long"), TradeDirection::Long);
        assert_eq!(TradeDirection::from("LONG"), TradeDirection::Long);
        assert_eq!(TradeDirection::from("short"), TradeDirection::Short);
        assert_eq!(TradeDirection::from("Short"), TradeDirection::Short);
        assert_eq!(TradeDirection::from("invalid"), TradeDirection::Short);
    }

    #[test]
    fn test_trade_direction_display() {
        assert_eq!(TradeDirection::Long.to_string(), "long");
        assert_eq!(TradeDirection::Short.to_string(), "short");
    }

    #[test]
    fn test_wing_selection_mode_default_delta() {
        let mode = WingSelectionMode::default();
        match mode {
            WingSelectionMode::Delta { wing_delta } => {
                assert_eq!(wing_delta, 0.25);
            }
            _ => panic!("Expected Delta mode"),
        }
    }

    #[test]
    fn test_wing_selection_mode_from_cli_delta() {
        let mode = WingSelectionMode::from_cli_arg("delta:0.25").unwrap();
        match mode {
            WingSelectionMode::Delta { wing_delta } => {
                assert_eq!(wing_delta, 0.25);
            }
            _ => panic!("Expected Delta mode"),
        }
    }

    #[test]
    fn test_wing_selection_mode_from_cli_moneyness() {
        let mode = WingSelectionMode::from_cli_arg("moneyness:0.10").unwrap();
        match mode {
            WingSelectionMode::Moneyness { wing_percent } => {
                assert_eq!(wing_percent, 0.10);
            }
            _ => panic!("Expected Moneyness mode"),
        }
    }

    #[test]
    fn test_wing_selection_mode_from_cli_invalid_format() {
        assert!(WingSelectionMode::from_cli_arg("invalid").is_err());
        assert!(WingSelectionMode::from_cli_arg("delta:invalid").is_err());
        assert!(WingSelectionMode::from_cli_arg("delta:1.5").is_err());
    }

    #[test]
    fn test_wing_selection_mode_display() {
        let delta_mode = WingSelectionMode::Delta { wing_delta: 0.25 };
        assert_eq!(delta_mode.to_string(), "delta:0.25");

        let moneyness_mode = WingSelectionMode::Moneyness { wing_percent: 0.10 };
        assert_eq!(moneyness_mode.to_string(), "moneyness:0.1");
    }

    #[test]
    fn test_iron_butterfly_config_default() {
        let config = IronButterflyConfig::default();
        assert_eq!(config.symmetric, true);
        match config.wing_mode {
            WingSelectionMode::Delta { wing_delta } => {
                assert_eq!(wing_delta, 0.25);
            }
            _ => panic!("Expected Delta mode"),
        }
    }

    #[test]
    fn test_iron_butterfly_config_from_cli_arg() {
        let config = IronButterflyConfig::from_cli_arg("delta:0.15").unwrap();
        assert_eq!(config.symmetric, true);
        match config.wing_mode {
            WingSelectionMode::Delta { wing_delta } => {
                assert_eq!(wing_delta, 0.15);
            }
            _ => panic!("Expected Delta mode"),
        }
    }

    #[test]
    fn test_iron_butterfly_config_from_cli_arg_optional() {
        let config = IronButterflyConfig::from_cli_arg_optional(None).unwrap();
        assert_eq!(config.symmetric, true);
        match config.wing_mode {
            WingSelectionMode::Delta { wing_delta } => {
                assert_eq!(wing_delta, 0.25);
            }
            _ => panic!("Expected Delta mode"),
        }

        let config = IronButterflyConfig::from_cli_arg_optional(Some("moneyness:0.10")).unwrap();
        match config.wing_mode {
            WingSelectionMode::Moneyness { wing_percent } => {
                assert_eq!(wing_percent, 0.10);
            }
            _ => panic!("Expected Moneyness mode"),
        }
    }
}
