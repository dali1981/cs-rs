use chrono::{NaiveDate, DateTime, Utc};
use rust_decimal::Decimal;
use finq_core::OptionType;
use serde::{Serialize, Deserialize};

use crate::value_objects::*;

/// Earnings event for a symbol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarningsEvent {
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub company_name: Option<String>,
    pub eps_forecast: Option<Decimal>,
    pub market_cap: Option<u64>,
}

impl EarningsEvent {
    pub fn new(symbol: String, earnings_date: NaiveDate, earnings_time: EarningsTime) -> Self {
        Self {
            symbol,
            earnings_date,
            earnings_time,
            company_name: None,
            eps_forecast: None,
            market_cap: None,
        }
    }

    pub fn with_market_cap(mut self, market_cap: u64) -> Self {
        self.market_cap = Some(market_cap);
        self
    }

    pub fn with_company_name(mut self, name: String) -> Self {
        self.company_name = Some(name);
        self
    }
}

/// Single option leg
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionLeg {
    pub symbol: String,
    pub strike: Strike,
    pub expiration: NaiveDate,
    pub option_type: OptionType,
}

impl OptionLeg {
    pub fn new(
        symbol: String,
        strike: Strike,
        expiration: NaiveDate,
        option_type: OptionType,
    ) -> Self {
        Self { symbol, strike, expiration, option_type }
    }

    /// Generate OCC ticker (e.g., "O:AAPL250117C00180000")
    pub fn occ_ticker(&self) -> String {
        let opt_char = match self.option_type {
            OptionType::Call => 'C',
            OptionType::Put => 'P',
        };
        // Convert strike to cents (multiply by 1000 for OCC format)
        let strike_millis = self.strike.value() * Decimal::from(1000);
        // Round to nearest integer and convert
        let strike_int = strike_millis.round().to_string()
            .split('.')
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        format!(
            "O:{}{}{}{}",
            self.symbol,
            self.expiration.format("%y%m%d"),
            opt_char,
            format!("{:08}", strike_int)
        )
    }

    /// Days to expiry from given date
    pub fn dte(&self, from: NaiveDate) -> i32 {
        (self.expiration - from).num_days() as i32
    }
}

/// Calendar spread = short near-term + long far-term
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSpread {
    pub short_leg: OptionLeg,
    pub long_leg: OptionLeg,
}

impl CalendarSpread {
    pub fn new(short: OptionLeg, long: OptionLeg) -> Result<Self, ValidationError> {
        if short.symbol != long.symbol {
            return Err(ValidationError::SymbolMismatch(
                short.symbol.clone(),
                long.symbol.clone(),
            ));
        }
        if short.expiration >= long.expiration {
            return Err(ValidationError::ExpirationMismatch {
                short: short.expiration,
                long: long.expiration,
            });
        }
        Ok(Self { short_leg: short, long_leg: long })
    }

    pub fn symbol(&self) -> &str { &self.short_leg.symbol }
    pub fn strike(&self) -> Strike { self.short_leg.strike }
    pub fn option_type(&self) -> OptionType { self.short_leg.option_type }
    pub fn short_expiry(&self) -> NaiveDate { self.short_leg.expiration }
    pub fn long_expiry(&self) -> NaiveDate { self.long_leg.expiration }
}

/// Iron butterfly = short ATM straddle + long OTM wings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IronButterfly {
    pub short_call: OptionLeg,
    pub short_put: OptionLeg,
    pub long_call: OptionLeg,
    pub long_put: OptionLeg,
}

/// Long straddle = long ATM call + long ATM put
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Straddle {
    pub call_leg: OptionLeg,
    pub put_leg: OptionLeg,
}

impl IronButterfly {
    pub fn new(
        short_call: OptionLeg,
        short_put: OptionLeg,
        long_call: OptionLeg,
        long_put: OptionLeg,
    ) -> Result<Self, ValidationError> {
        // Validate same symbol
        if short_call.symbol != short_put.symbol
            || short_call.symbol != long_call.symbol
            || short_call.symbol != long_put.symbol
        {
            return Err(ValidationError::SymbolMismatch(
                short_call.symbol.clone(),
                short_put.symbol.clone(),
            ));
        }

        // Validate same expiration
        if short_call.expiration != short_put.expiration
            || short_call.expiration != long_call.expiration
            || short_call.expiration != long_put.expiration
        {
            return Err(ValidationError::ExpirationMismatch {
                short: short_call.expiration,
                long: long_call.expiration,
            });
        }

        // Validate center strikes match
        if short_call.strike != short_put.strike {
            return Err(ValidationError::StrikeMismatch {
                call: short_call.strike,
                put: short_put.strike,
            });
        }

        // Validate wing strikes
        if long_call.strike <= short_call.strike {
            return Err(ValidationError::InvalidStrikeOrder(
                "Long call strike must be > short call strike".to_string(),
            ));
        }
        if long_put.strike >= short_put.strike {
            return Err(ValidationError::InvalidStrikeOrder(
                "Long put strike must be < short put strike".to_string(),
            ));
        }

        // Validate option types
        if short_call.option_type != OptionType::Call {
            return Err(ValidationError::InvalidOptionType(
                "Short call must be a Call".to_string(),
            ));
        }
        if short_put.option_type != OptionType::Put {
            return Err(ValidationError::InvalidOptionType(
                "Short put must be a Put".to_string(),
            ));
        }
        if long_call.option_type != OptionType::Call {
            return Err(ValidationError::InvalidOptionType(
                "Long call must be a Call".to_string(),
            ));
        }
        if long_put.option_type != OptionType::Put {
            return Err(ValidationError::InvalidOptionType(
                "Long put must be a Put".to_string(),
            ));
        }

        Ok(Self {
            short_call,
            short_put,
            long_call,
            long_put,
        })
    }

    pub fn symbol(&self) -> &str {
        &self.short_call.symbol
    }

    pub fn center_strike(&self) -> Strike {
        self.short_call.strike
    }

    pub fn upper_strike(&self) -> Strike {
        self.long_call.strike
    }

    pub fn lower_strike(&self) -> Strike {
        self.long_put.strike
    }

    pub fn expiration(&self) -> NaiveDate {
        self.short_call.expiration
    }

    pub fn dte(&self, from: NaiveDate) -> i32 {
        (self.expiration() - from).num_days() as i32
    }

    /// Wing width (upper wing)
    pub fn wing_width(&self) -> Decimal {
        self.upper_strike().value() - self.center_strike().value()
    }
}

impl Straddle {
    pub fn new(call_leg: OptionLeg, put_leg: OptionLeg) -> Result<Self, ValidationError> {
        // Validate same symbol
        if call_leg.symbol != put_leg.symbol {
            return Err(ValidationError::SymbolMismatch(
                call_leg.symbol.clone(),
                put_leg.symbol.clone(),
            ));
        }

        // Validate same expiration
        if call_leg.expiration != put_leg.expiration {
            return Err(ValidationError::ExpirationMismatch {
                short: call_leg.expiration,
                long: put_leg.expiration,
            });
        }

        // Validate same strike
        if call_leg.strike != put_leg.strike {
            return Err(ValidationError::StrikeMismatch {
                call: call_leg.strike,
                put: put_leg.strike,
            });
        }

        // Validate option types
        if call_leg.option_type != OptionType::Call {
            return Err(ValidationError::InvalidOptionType(
                "Call leg must be a Call".to_string(),
            ));
        }
        if put_leg.option_type != OptionType::Put {
            return Err(ValidationError::InvalidOptionType(
                "Put leg must be a Put".to_string(),
            ));
        }

        Ok(Self { call_leg, put_leg })
    }

    pub fn symbol(&self) -> &str {
        &self.call_leg.symbol
    }

    pub fn strike(&self) -> Strike {
        self.call_leg.strike
    }

    pub fn expiration(&self) -> NaiveDate {
        self.call_leg.expiration
    }

    pub fn dte(&self, from: NaiveDate) -> i32 {
        (self.expiration() - from).num_days() as i32
    }
}

/// Calendar straddle = short near-term straddle + long far-term straddle
/// (two calendar spreads at the same strike - one call, one put)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarStraddle {
    pub short_call: OptionLeg,
    pub short_put: OptionLeg,
    pub long_call: OptionLeg,
    pub long_put: OptionLeg,
}

impl CalendarStraddle {
    pub fn new(
        short_call: OptionLeg,
        short_put: OptionLeg,
        long_call: OptionLeg,
        long_put: OptionLeg,
    ) -> Result<Self, ValidationError> {
        // Validate all symbols match
        if short_call.symbol != short_put.symbol
            || short_call.symbol != long_call.symbol
            || short_call.symbol != long_put.symbol
        {
            return Err(ValidationError::SymbolMismatch(
                short_call.symbol.clone(),
                short_put.symbol.clone(),
            ));
        }

        // Validate short expiration < long expiration
        if short_call.expiration >= long_call.expiration {
            return Err(ValidationError::ExpirationMismatch {
                short: short_call.expiration,
                long: long_call.expiration,
            });
        }

        // Validate short call expiration == short put expiration
        if short_call.expiration != short_put.expiration {
            return Err(ValidationError::ExpirationMismatch {
                short: short_call.expiration,
                long: short_put.expiration,
            });
        }

        // Validate long call expiration == long put expiration
        if long_call.expiration != long_put.expiration {
            return Err(ValidationError::ExpirationMismatch {
                short: long_call.expiration,
                long: long_put.expiration,
            });
        }

        // Validate short strikes match (same strike for straddle)
        if short_call.strike != short_put.strike {
            return Err(ValidationError::StrikeMismatch {
                call: short_call.strike,
                put: short_put.strike,
            });
        }

        // Validate long strikes match (same strike for straddle)
        if long_call.strike != long_put.strike {
            return Err(ValidationError::StrikeMismatch {
                call: long_call.strike,
                put: long_put.strike,
            });
        }

        // Validate option types
        if short_call.option_type != OptionType::Call {
            return Err(ValidationError::InvalidOptionType(
                "Short call must be a Call".to_string(),
            ));
        }
        if short_put.option_type != OptionType::Put {
            return Err(ValidationError::InvalidOptionType(
                "Short put must be a Put".to_string(),
            ));
        }
        if long_call.option_type != OptionType::Call {
            return Err(ValidationError::InvalidOptionType(
                "Long call must be a Call".to_string(),
            ));
        }
        if long_put.option_type != OptionType::Put {
            return Err(ValidationError::InvalidOptionType(
                "Long put must be a Put".to_string(),
            ));
        }

        Ok(Self {
            short_call,
            short_put,
            long_call,
            long_put,
        })
    }

    pub fn symbol(&self) -> &str {
        &self.short_call.symbol
    }

    /// Strike for the short straddle
    pub fn short_strike(&self) -> Strike {
        self.short_call.strike
    }

    /// Strike for the long straddle
    pub fn long_strike(&self) -> Strike {
        self.long_call.strike
    }

    pub fn short_expiry(&self) -> NaiveDate {
        self.short_call.expiration
    }

    pub fn long_expiry(&self) -> NaiveDate {
        self.long_call.expiration
    }

    /// Days to short expiry from given date
    pub fn short_dte(&self, from: NaiveDate) -> i32 {
        (self.short_expiry() - from).num_days() as i32
    }

    /// Days to long expiry from given date
    pub fn long_dte(&self, from: NaiveDate) -> i32 {
        (self.long_expiry() - from).num_days() as i32
    }
}

/// Trade opportunity generated by strategy
#[derive(Debug, Clone)]
pub struct TradeOpportunity {
    pub spread: CalendarSpread,
    pub earnings_event: EarningsEvent,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub spot_price_at_selection: SpotPrice,
}

/// Completed trade result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSpreadResult {
    // Identification
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub strike: Strike,
    /// Long leg strike if different from short (diagonal spread)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_strike: Option<Strike>,
    pub option_type: OptionType,
    pub short_expiry: NaiveDate,
    pub long_expiry: NaiveDate,

    // Entry
    pub entry_time: DateTime<Utc>,
    pub short_entry_price: Decimal,
    pub long_entry_price: Decimal,
    pub entry_cost: Decimal,

    // Exit
    pub exit_time: DateTime<Utc>,
    pub short_exit_price: Decimal,
    pub long_exit_price: Decimal,
    pub exit_value: Decimal,

    // IV Surface timestamps (actual data used for pricing)
    /// Actual timestamp of IV surface used for entry pricing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_surface_time: Option<DateTime<Utc>>,
    /// Actual timestamp of IV surface used for exit pricing (may differ if forward-lookup was used)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_surface_time: Option<DateTime<Utc>>,

    // P&L
    pub pnl: Decimal,
    pub pnl_per_contract: Decimal,
    pub pnl_pct: Decimal,

    // Greeks at entry
    pub short_delta: Option<f64>,
    pub short_gamma: Option<f64>,
    pub short_theta: Option<f64>,
    pub short_vega: Option<f64>,
    pub long_delta: Option<f64>,
    pub long_gamma: Option<f64>,
    pub long_theta: Option<f64>,
    pub long_vega: Option<f64>,

    // IV at entry/exit
    pub iv_short_entry: Option<f64>,
    pub iv_long_entry: Option<f64>,
    pub iv_short_exit: Option<f64>,
    pub iv_long_exit: Option<f64>,
    /// IV ratio at entry (short_iv / long_iv) - used for filtering trades
    pub iv_ratio_entry: Option<f64>,

    // P&L Attribution
    pub delta_pnl: Option<Decimal>,
    pub gamma_pnl: Option<Decimal>,
    pub theta_pnl: Option<Decimal>,
    pub vega_pnl: Option<Decimal>,
    pub unexplained_pnl: Option<Decimal>,

    // Spot prices
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,

    // Status
    pub success: bool,
    pub failure_reason: Option<FailureReason>,
}

impl CalendarSpreadResult {
    pub fn iv_ratio(&self) -> Option<f64> {
        match (self.iv_short_entry, self.iv_long_entry) {
            (Some(short), Some(long)) if long > 0.0 => Some(short / long),
            _ => None,
        }
    }

    pub fn is_winner(&self) -> bool {
        self.success && self.pnl > Decimal::ZERO
    }

    /// Get effective long strike (falls back to short strike for calendars)
    pub fn long_strike_effective(&self) -> Strike {
        self.long_strike.unwrap_or(self.strike)
    }

    /// Whether this is a diagonal spread (different strikes)
    pub fn is_diagonal(&self) -> bool {
        self.long_strike.is_some_and(|ls| ls != self.strike)
    }
}

/// Iron butterfly trade result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IronButterflyResult {
    // Identification
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub center_strike: Strike,
    pub upper_strike: Strike,
    pub lower_strike: Strike,
    pub expiration: NaiveDate,
    pub wing_width: Decimal,

    // Entry (CREDIT received)
    pub entry_time: DateTime<Utc>,
    pub short_call_entry: Decimal,
    pub short_put_entry: Decimal,
    pub long_call_entry: Decimal,
    pub long_put_entry: Decimal,
    pub entry_credit: Decimal,

    // Exit (cost to close)
    pub exit_time: DateTime<Utc>,
    pub short_call_exit: Decimal,
    pub short_put_exit: Decimal,
    pub long_call_exit: Decimal,
    pub long_put_exit: Decimal,
    pub exit_cost: Decimal,

    // IV Surface timestamps (actual data used for pricing)
    /// Actual timestamp of IV surface used for entry pricing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_surface_time: Option<DateTime<Utc>>,
    /// Actual timestamp of IV surface used for exit pricing (may differ if forward-lookup was used)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_surface_time: Option<DateTime<Utc>>,

    // P&L
    pub pnl: Decimal,
    pub pnl_pct: Decimal,
    pub max_loss: Decimal,

    // Greeks at entry (net position)
    pub net_delta: Option<f64>,
    pub net_gamma: Option<f64>,
    pub net_theta: Option<f64>,
    pub net_vega: Option<f64>,

    // IV at entry/exit
    pub iv_entry: Option<f64>,
    pub iv_exit: Option<f64>,
    pub iv_crush: Option<f64>,

    // P&L Attribution
    pub delta_pnl: Option<Decimal>,
    pub gamma_pnl: Option<Decimal>,
    pub theta_pnl: Option<Decimal>,
    pub vega_pnl: Option<Decimal>,
    pub unexplained_pnl: Option<Decimal>,

    // Spot prices
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,
    pub spot_move: f64,
    pub spot_move_pct: f64,

    // Breakeven analysis
    pub breakeven_up: f64,
    pub breakeven_down: f64,
    pub within_breakeven: bool,

    // Status
    pub success: bool,
    pub failure_reason: Option<FailureReason>,
}

impl IronButterflyResult {
    pub fn is_winner(&self) -> bool {
        self.success && self.pnl > Decimal::ZERO
    }

    /// Distance from center to breakeven
    pub fn breakeven_width(&self) -> f64 {
        (self.breakeven_up - self.center_strike.value().try_into().unwrap_or(0.0)).abs()
    }
}

/// Indicates how exit prices were determined
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PricingSource {
    /// Prices from actual market data (minute bars)
    Market,
    /// Prices computed via Black-Scholes model
    Model,
}

/// Long straddle trade result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StraddleResult {
    // Identification
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub strike: Strike,
    pub expiration: NaiveDate,

    // Entry (DEBIT paid)
    pub entry_time: DateTime<Utc>,
    pub call_entry_price: Decimal,
    pub put_entry_price: Decimal,
    pub entry_debit: Decimal,  // Total premium paid

    // Exit (credit received)
    pub exit_time: DateTime<Utc>,
    pub call_exit_price: Decimal,
    pub put_exit_price: Decimal,
    pub exit_credit: Decimal,

    // IV Surface timestamps (actual data used for pricing)
    /// Actual timestamp of IV surface used for entry pricing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_surface_time: Option<DateTime<Utc>>,
    /// Actual timestamp of IV surface used for exit pricing (may differ if forward-lookup was used)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_surface_time: Option<DateTime<Utc>>,

    // Pricing method used at exit
    pub exit_pricing_method: PricingSource,

    // P&L
    pub pnl: Decimal,
    pub pnl_pct: Decimal,

    // Greeks at entry (net position)
    pub net_delta: Option<f64>,
    pub net_gamma: Option<f64>,
    pub net_theta: Option<f64>,
    pub net_vega: Option<f64>,

    // IV at entry/exit
    pub iv_entry: Option<f64>,
    pub iv_exit: Option<f64>,
    pub iv_change: Option<f64>,  // IV expansion (positive = good for long straddle)

    // P&L Attribution
    pub delta_pnl: Option<Decimal>,
    pub gamma_pnl: Option<Decimal>,
    pub theta_pnl: Option<Decimal>,
    pub vega_pnl: Option<Decimal>,
    pub unexplained_pnl: Option<Decimal>,

    // Spot prices
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,
    pub spot_move: f64,
    pub spot_move_pct: f64,

    // Expected move context
    pub expected_move_pct: Option<f64>,  // Straddle / Spot at entry

    // Status
    pub success: bool,
    pub failure_reason: Option<FailureReason>,
}

impl StraddleResult {
    pub fn is_winner(&self) -> bool {
        self.success && self.pnl > Decimal::ZERO
    }
}

/// Calendar straddle trade result
/// (short near-term straddle + long far-term straddle)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarStraddleResult {
    // Identification
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub short_strike: Strike,
    pub long_strike: Strike,
    pub short_expiry: NaiveDate,
    pub long_expiry: NaiveDate,

    // Entry (4 leg prices)
    pub entry_time: DateTime<Utc>,
    pub short_call_entry: Decimal,
    pub short_put_entry: Decimal,
    pub long_call_entry: Decimal,
    pub long_put_entry: Decimal,
    pub entry_cost: Decimal,  // Net debit: (long_call + long_put) - (short_call + short_put)

    // Exit (4 leg prices)
    pub exit_time: DateTime<Utc>,
    pub short_call_exit: Decimal,
    pub short_put_exit: Decimal,
    pub long_call_exit: Decimal,
    pub long_put_exit: Decimal,
    pub exit_value: Decimal,

    // IV Surface timestamps (actual data used for pricing)
    /// Actual timestamp of IV surface used for entry pricing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_surface_time: Option<DateTime<Utc>>,
    /// Actual timestamp of IV surface used for exit pricing (may differ if forward-lookup was used)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_surface_time: Option<DateTime<Utc>>,

    // P&L
    pub pnl: Decimal,
    pub pnl_pct: Decimal,

    // Greeks at entry (net position)
    pub net_delta: Option<f64>,
    pub net_gamma: Option<f64>,
    pub net_theta: Option<f64>,
    pub net_vega: Option<f64>,

    // IV tracking (average of call/put at each expiration)
    pub short_iv_entry: Option<f64>,
    pub long_iv_entry: Option<f64>,
    pub short_iv_exit: Option<f64>,
    pub long_iv_exit: Option<f64>,
    pub iv_ratio_entry: Option<f64>,  // short_iv / long_iv

    // P&L Attribution
    pub delta_pnl: Option<Decimal>,
    pub gamma_pnl: Option<Decimal>,
    pub theta_pnl: Option<Decimal>,
    pub vega_pnl: Option<Decimal>,
    pub unexplained_pnl: Option<Decimal>,

    // Spot prices
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,

    // Status
    pub success: bool,
    pub failure_reason: Option<FailureReason>,
}

impl CalendarStraddleResult {
    pub fn is_winner(&self) -> bool {
        self.success && self.pnl > Decimal::ZERO
    }

    pub fn iv_ratio(&self) -> Option<f64> {
        match (self.short_iv_entry, self.long_iv_entry) {
            (Some(short), Some(long)) if long > 0.0 => Some(short / long),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_earnings_event_new() {
        let event = EarningsEvent::new(
            "AAPL".to_string(),
            NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            EarningsTime::AfterMarketClose,
        );

        assert_eq!(event.symbol, "AAPL");
        assert_eq!(event.earnings_time, EarningsTime::AfterMarketClose);
        assert!(event.market_cap.is_none());
    }

    #[test]
    fn test_earnings_event_builder() {
        let event = EarningsEvent::new(
            "AAPL".to_string(),
            NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            EarningsTime::AfterMarketClose,
        )
        .with_market_cap(3_000_000_000_000)
        .with_company_name("Apple Inc.".to_string());

        assert_eq!(event.market_cap, Some(3_000_000_000_000));
        assert_eq!(event.company_name, Some("Apple Inc.".to_string()));
    }

    #[test]
    fn test_option_leg_occ_ticker() {
        let leg = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
            OptionType::Call,
        );

        let ticker = leg.occ_ticker();
        assert_eq!(ticker, "O:AAPL250117C00180000");
    }

    #[test]
    fn test_option_leg_occ_ticker_put() {
        let leg = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(1755, 1)).unwrap(), // 175.5
            NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
            OptionType::Put,
        );

        let ticker = leg.occ_ticker();
        assert_eq!(ticker, "O:AAPL250117P00175500");
    }

    #[test]
    fn test_option_leg_dte() {
        let leg = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
            OptionType::Call,
        );

        let from_date = NaiveDate::from_ymd_opt(2025, 1, 10).unwrap();
        assert_eq!(leg.dte(from_date), 7);
    }

    #[test]
    fn test_calendar_spread_valid() {
        let short = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
            OptionType::Call,
        );

        let long = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
            OptionType::Call,
        );

        let spread = CalendarSpread::new(short, long);
        assert!(spread.is_ok());

        let spread = spread.unwrap();
        assert_eq!(spread.symbol(), "AAPL");
        assert_eq!(spread.option_type(), OptionType::Call);
    }

    #[test]
    fn test_calendar_spread_symbol_mismatch() {
        let short = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
            OptionType::Call,
        );

        let long = OptionLeg::new(
            "GOOGL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
            OptionType::Call,
        );

        let spread = CalendarSpread::new(short, long);
        assert!(spread.is_err());
        assert!(matches!(spread.unwrap_err(), ValidationError::SymbolMismatch(_, _)));
    }

    #[test]
    fn test_calendar_spread_expiration_mismatch() {
        let short = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
            OptionType::Call,
        );

        let long = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
            OptionType::Call,
        );

        let spread = CalendarSpread::new(short, long);
        assert!(spread.is_err());
        assert!(matches!(spread.unwrap_err(), ValidationError::ExpirationMismatch { .. }));
    }

    #[test]
    fn test_calendar_spread_result_iv_ratio() {
        let result = CalendarSpreadResult {
            symbol: "AAPL".to_string(),
            earnings_date: NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            earnings_time: EarningsTime::AfterMarketClose,
            strike: Strike::new(Decimal::new(180, 0)).unwrap(),
            long_strike: None,
            option_type: OptionType::Call,
            short_expiry: NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
            long_expiry: NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
            entry_time: Utc::now(),
            short_entry_price: Decimal::new(5, 0),
            long_entry_price: Decimal::new(6, 0),
            entry_cost: Decimal::new(1, 0),
            exit_time: Utc::now(),
            short_exit_price: Decimal::new(2, 0),
            long_exit_price: Decimal::new(4, 0),
            exit_value: Decimal::new(2, 0),
            pnl: Decimal::new(1, 0),
            pnl_per_contract: Decimal::new(1, 0),
            pnl_pct: Decimal::new(100, 0),
            short_delta: None,
            short_gamma: None,
            short_theta: None,
            short_vega: None,
            long_delta: None,
            long_gamma: None,
            long_theta: None,
            long_vega: None,
            iv_short_entry: Some(0.30),
            iv_long_entry: Some(0.25),
            iv_short_exit: None,
            iv_long_exit: None,
            iv_ratio_entry: Some(1.2),
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 180.0,
            spot_at_exit: 182.0,
            success: true,
            failure_reason: None,
        };

        assert_eq!(result.iv_ratio(), Some(1.2));
    }

    #[test]
    fn test_calendar_spread_result_is_winner() {
        let mut result = CalendarSpreadResult {
            symbol: "AAPL".to_string(),
            earnings_date: NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            earnings_time: EarningsTime::AfterMarketClose,
            strike: Strike::new(Decimal::new(180, 0)).unwrap(),
            long_strike: None,
            option_type: OptionType::Call,
            short_expiry: NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
            long_expiry: NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
            entry_time: Utc::now(),
            short_entry_price: Decimal::new(5, 0),
            long_entry_price: Decimal::new(6, 0),
            entry_cost: Decimal::new(1, 0),
            exit_time: Utc::now(),
            short_exit_price: Decimal::new(2, 0),
            long_exit_price: Decimal::new(4, 0),
            exit_value: Decimal::new(2, 0),
            pnl: Decimal::new(1, 0),
            pnl_per_contract: Decimal::new(1, 0),
            pnl_pct: Decimal::new(100, 0),
            short_delta: None,
            short_gamma: None,
            short_theta: None,
            short_vega: None,
            long_delta: None,
            long_gamma: None,
            long_theta: None,
            long_vega: None,
            iv_short_entry: None,
            iv_long_entry: None,
            iv_short_exit: None,
            iv_long_exit: None,
            iv_ratio_entry: None,
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 180.0,
            spot_at_exit: 182.0,
            success: true,
            failure_reason: None,
        };

        assert!(result.is_winner());

        result.pnl = Decimal::new(-1, 0);
        assert!(!result.is_winner());

        result.pnl = Decimal::new(1, 0);
        result.success = false;
        assert!(!result.is_winner());
    }

    #[test]
    fn test_calendar_straddle_valid() {
        // Nov 2025 dates
        let short_call = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),  // Near-term
            OptionType::Call,
        );
        let short_put = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),
            OptionType::Put,
        );
        let long_call = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),  // Far-term
            OptionType::Call,
        );
        let long_put = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),
            OptionType::Put,
        );

        let straddle = CalendarStraddle::new(short_call, short_put, long_call, long_put);
        assert!(straddle.is_ok());

        let straddle = straddle.unwrap();
        assert_eq!(straddle.symbol(), "AAPL");
        assert_eq!(straddle.short_strike().value(), Decimal::new(180, 0));
        assert_eq!(straddle.long_strike().value(), Decimal::new(180, 0));
        assert_eq!(straddle.short_expiry(), NaiveDate::from_ymd_opt(2025, 11, 7).unwrap());
        assert_eq!(straddle.long_expiry(), NaiveDate::from_ymd_opt(2025, 11, 21).unwrap());
    }

    #[test]
    fn test_calendar_straddle_symbol_mismatch() {
        let short_call = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),
            OptionType::Call,
        );
        let short_put = OptionLeg::new(
            "GOOGL".to_string(),  // Different symbol
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),
            OptionType::Put,
        );
        let long_call = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),
            OptionType::Call,
        );
        let long_put = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),
            OptionType::Put,
        );

        let straddle = CalendarStraddle::new(short_call, short_put, long_call, long_put);
        assert!(straddle.is_err());
        assert!(matches!(straddle.unwrap_err(), ValidationError::SymbolMismatch(_, _)));
    }

    #[test]
    fn test_calendar_straddle_expiration_mismatch() {
        // Short expiration > long expiration (invalid)
        let short_call = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),  // Far-term in short position
            OptionType::Call,
        );
        let short_put = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),
            OptionType::Put,
        );
        let long_call = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),  // Near-term in long position
            OptionType::Call,
        );
        let long_put = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),
            OptionType::Put,
        );

        let straddle = CalendarStraddle::new(short_call, short_put, long_call, long_put);
        assert!(straddle.is_err());
        assert!(matches!(straddle.unwrap_err(), ValidationError::ExpirationMismatch { .. }));
    }

    #[test]
    fn test_calendar_straddle_strike_mismatch() {
        // Short call/put have different strikes (invalid)
        let short_call = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),
            OptionType::Call,
        );
        let short_put = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(175, 0)).unwrap(),  // Different strike
            NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),
            OptionType::Put,
        );
        let long_call = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),
            OptionType::Call,
        );
        let long_put = OptionLeg::new(
            "AAPL".to_string(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),
            OptionType::Put,
        );

        let straddle = CalendarStraddle::new(short_call, short_put, long_call, long_put);
        assert!(straddle.is_err());
        assert!(matches!(straddle.unwrap_err(), ValidationError::StrikeMismatch { .. }));
    }

    #[test]
    fn test_calendar_straddle_result_is_winner() {
        let result = CalendarStraddleResult {
            symbol: "AAPL".to_string(),
            earnings_date: NaiveDate::from_ymd_opt(2025, 11, 6).unwrap(),
            earnings_time: EarningsTime::AfterMarketClose,
            short_strike: Strike::new(Decimal::new(180, 0)).unwrap(),
            long_strike: Strike::new(Decimal::new(180, 0)).unwrap(),
            short_expiry: NaiveDate::from_ymd_opt(2025, 11, 7).unwrap(),
            long_expiry: NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),
            entry_time: Utc::now(),
            short_call_entry: Decimal::new(3, 0),
            short_put_entry: Decimal::new(3, 0),
            long_call_entry: Decimal::new(5, 0),
            long_put_entry: Decimal::new(5, 0),
            entry_cost: Decimal::new(4, 0),  // (5+5) - (3+3) = 4
            exit_time: Utc::now(),
            short_call_exit: Decimal::new(1, 0),
            short_put_exit: Decimal::new(1, 0),
            long_call_exit: Decimal::new(4, 0),
            long_put_exit: Decimal::new(4, 0),
            exit_value: Decimal::new(6, 0),  // (4+4) - (1+1) = 6
            pnl: Decimal::new(2, 0),  // 6 - 4 = 2
            pnl_pct: Decimal::new(50, 0),
            net_delta: Some(0.0),
            net_gamma: Some(0.05),
            net_theta: Some(-0.02),
            net_vega: Some(0.10),
            short_iv_entry: Some(0.45),
            long_iv_entry: Some(0.35),
            short_iv_exit: Some(0.25),
            long_iv_exit: Some(0.30),
            iv_ratio_entry: Some(1.29),
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 180.0,
            spot_at_exit: 182.0,
            success: true,
            failure_reason: None,
        };

        assert!(result.is_winner());
        assert_eq!(result.iv_ratio(), Some(0.45 / 0.35));
    }
}
