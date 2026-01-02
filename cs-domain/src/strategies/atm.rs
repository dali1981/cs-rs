use super::*;
use chrono::NaiveDate;
use cs_analytics::{DeltaVolSurface, bs_delta};

/// ATM strategy - select strike closest to spot
pub struct ATMStrategy {
    pub criteria: TradeSelectionCriteria,
    /// Strike matching mode
    pub strike_match_mode: StrikeMatchMode,
    /// Risk-free rate for delta calculations (used when strike_match_mode is SameDelta)
    pub risk_free_rate: f64,
}

impl Default for ATMStrategy {
    fn default() -> Self {
        Self {
            criteria: TradeSelectionCriteria::default(),
            strike_match_mode: StrikeMatchMode::default(),
            risk_free_rate: 0.05,
        }
    }
}

impl ATMStrategy {
    /// Create a new ATM strategy
    pub fn new(criteria: TradeSelectionCriteria) -> Self {
        Self {
            criteria,
            strike_match_mode: StrikeMatchMode::default(),
            risk_free_rate: 0.05,
        }
    }

    /// Set strike matching mode
    pub fn with_strike_match_mode(mut self, mode: StrikeMatchMode) -> Self {
        self.strike_match_mode = mode;
        self
    }

    /// Set risk-free rate
    pub fn with_risk_free_rate(mut self, rate: f64) -> Self {
        self.risk_free_rate = rate;
        self
    }
}

impl TradingStrategy for ATMStrategy {
    fn select(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
        option_type: OptionType,
    ) -> Result<CalendarSpread, StrategyError> {
        if chain_data.strikes.is_empty() {
            return Err(StrategyError::NoStrikes);
        }

        // Find ATM strike for short leg
        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let short_atm_strike = chain_data.strikes
            .iter()
            .min_by(|a, b| {
                let a_diff = (f64::from(**a) - spot_f64).abs();
                let b_diff = (f64::from(**b) - spot_f64).abs();
                a_diff.partial_cmp(&b_diff).unwrap()
            })
            .ok_or(StrategyError::NoStrikes)?;

        // Select expirations
        let (short_exp, long_exp) = select_expirations(
            &chain_data.expirations,
            event.earnings_date,
            self.criteria.min_short_dte,
            self.criteria.max_short_dte,
            self.criteria.min_long_dte,
            self.criteria.max_long_dte,
        )?;

        // Determine long leg strike based on matching mode
        let long_strike = match self.strike_match_mode {
            StrikeMatchMode::SameStrike => *short_atm_strike,
            StrikeMatchMode::SameDelta => {
                // Need IV surface to calculate delta
                let iv_surface = chain_data
                    .iv_surface
                    .as_ref()
                    .ok_or(StrategyError::NoDeltaData)?;

                // Build delta-parameterized surface
                let delta_surface = DeltaVolSurface::from_iv_surface(iv_surface, self.risk_free_rate);

                // Calculate delta of ATM strike at short expiration
                let is_call = option_type == OptionType::Call;
                let short_strike_f64: f64 = (*short_atm_strike).into();

                // Get the IV at the short ATM strike
                let short_slice = delta_surface.slice(short_exp)
                    .ok_or(StrategyError::NoDeltaData)?;
                let short_iv = short_slice.get_iv_at_strike(short_strike_f64)
                    .ok_or(StrategyError::NoDeltaData)?;

                // Calculate time to expiry for short leg
                let short_tte = delta_surface.tte(short_exp)
                    .ok_or(StrategyError::NoDeltaData)?;

                // Calculate delta at ATM strike
                let atm_delta = bs_delta(
                    spot_f64,
                    short_strike_f64,
                    short_tte,
                    short_iv,
                    is_call,
                    self.risk_free_rate,
                );

                // Find strike at long expiry with same delta
                let theoretical_long_strike = delta_surface
                    .delta_to_strike(atm_delta, long_exp, is_call)
                    .ok_or(StrategyError::NoDeltaData)?;

                // Find closest tradable strike
                chain_data.strikes
                    .iter()
                    .min_by(|a, b| {
                        let a_diff = (f64::from(**a) - theoretical_long_strike).abs();
                        let b_diff = (f64::from(**b) - theoretical_long_strike).abs();
                        a_diff.partial_cmp(&b_diff).unwrap()
                    })
                    .copied()
                    .ok_or(StrategyError::NoStrikes)?
            }
        };

        let short_leg = OptionLeg::new(
            event.symbol.clone(),
            *short_atm_strike,
            short_exp,
            option_type,
        );
        let long_leg = OptionLeg::new(
            event.symbol.clone(),
            long_strike,
            long_exp,
            option_type,
        );

        CalendarSpread::new(short_leg, long_leg).map_err(Into::into)
    }
}

fn select_expirations(
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;

    fn create_test_chain_data() -> OptionChainData {
        let base_date = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();
        OptionChainData {
            expirations: vec![
                base_date + chrono::Duration::days(7),
                base_date + chrono::Duration::days(14),
                base_date + chrono::Duration::days(30),
            ],
            strikes: vec![
                Strike::new(Decimal::new(95, 0)).unwrap(),
                Strike::new(Decimal::new(100, 0)).unwrap(),
                Strike::new(Decimal::new(105, 0)).unwrap(),
            ],
            deltas: None,
            volumes: None,
            iv_ratios: None,
            iv_surface: None,
        }
    }

    #[test]
    fn test_atm_strategy_select_atm_strike() {
        let strategy = ATMStrategy::default();
        let event = EarningsEvent::new(
            "TEST".to_string(),
            NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            EarningsTime::AfterMarketClose,
        );
        let spot = SpotPrice::new(Decimal::new(100, 0), Utc::now());
        let chain_data = create_test_chain_data();

        let result = strategy.select(&event, &spot, &chain_data, OptionType::Call);
        assert!(result.is_ok());

        let spread = result.unwrap();
        assert_eq!(spread.strike().value(), Decimal::new(100, 0));
    }

    #[test]
    fn test_atm_strategy_select_closest_strike() {
        let strategy = ATMStrategy::default();
        let event = EarningsEvent::new(
            "TEST".to_string(),
            NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            EarningsTime::AfterMarketClose,
        );
        let spot = SpotPrice::new(Decimal::new(102, 0), Utc::now());
        let chain_data = create_test_chain_data();

        let result = strategy.select(&event, &spot, &chain_data, OptionType::Call);
        assert!(result.is_ok());

        let spread = result.unwrap();
        // Should select 100 strike (closer than 105)
        assert_eq!(spread.strike().value(), Decimal::new(100, 0));
    }

    #[test]
    fn test_atm_strategy_no_strikes() {
        let strategy = ATMStrategy::default();
        let event = EarningsEvent::new(
            "TEST".to_string(),
            NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            EarningsTime::AfterMarketClose,
        );
        let spot = SpotPrice::new(Decimal::new(100, 0), Utc::now());
        let mut chain_data = create_test_chain_data();
        chain_data.strikes.clear();

        let result = strategy.select(&event, &spot, &chain_data, OptionType::Call);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StrategyError::NoStrikes));
    }

    #[test]
    fn test_select_expirations_valid() {
        let base_date = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();
        let expirations = vec![
            base_date + chrono::Duration::days(7),
            base_date + chrono::Duration::days(14),
            base_date + chrono::Duration::days(30),
        ];

        let result = select_expirations(&expirations, base_date, 3, 45, 14, 90);
        assert!(result.is_ok());

        let (short, long) = result.unwrap();
        assert_eq!(short, base_date + chrono::Duration::days(7));
        assert_eq!(long, base_date + chrono::Duration::days(14));
    }

    #[test]
    fn test_select_expirations_insufficient() {
        let base_date = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();
        let expirations = vec![
            base_date + chrono::Duration::days(7),
        ];

        let result = select_expirations(&expirations, base_date, 3, 45, 14, 90);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StrategyError::InsufficientExpirations { .. }));
    }

    #[test]
    fn test_select_expirations_min_dte() {
        let base_date = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();
        let expirations = vec![
            base_date + chrono::Duration::days(3),
            base_date + chrono::Duration::days(7),
            base_date + chrono::Duration::days(14),
        ];

        // min_short_dte = 5, should skip first expiration
        let result = select_expirations(&expirations, base_date, 5, 45, 10, 90);
        assert!(result.is_ok());

        let (short, long) = result.unwrap();
        assert_eq!(short, base_date + chrono::Duration::days(7));
        assert_eq!(long, base_date + chrono::Duration::days(14));
    }

    #[test]
    fn test_select_expirations_max_dte() {
        let base_date = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();
        let expirations = vec![
            base_date + chrono::Duration::days(50),  // Too far for short (max 45)
            base_date + chrono::Duration::days(100), // Too far for short AND long (max 90)
            base_date + chrono::Duration::days(120), // Too far for both
        ];

        // max_short_dte = 45, max_long_dte = 90
        // All expirations exceed max_short_dte, so no valid short leg
        let result = select_expirations(&expirations, base_date, 3, 45, 14, 90);
        assert!(result.is_err());
    }
}
