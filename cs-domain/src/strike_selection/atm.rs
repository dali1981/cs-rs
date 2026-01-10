use super::*;
use cs_analytics::{DeltaVolSurface, bs_delta, IVSurface};
use chrono::NaiveDate;

/// ATM strategy - select strike closest to spot
///
/// This is the default strike selection strategy. It selects the strike
/// closest to the spot price for all trade types.
pub struct ATMStrategy {
    pub criteria: TradeSelectionCriteria,
    /// Strike matching mode (for calendar spreads)
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

    /// Set strike matching mode (for calendar spreads only)
    pub fn with_strike_match_mode(mut self, mode: StrikeMatchMode) -> Self {
        self.strike_match_mode = mode;
        self
    }

    /// Set risk-free rate
    pub fn with_risk_free_rate(mut self, rate: f64) -> Self {
        self.risk_free_rate = rate;
        self
    }

    /// Select first expiration with sufficient DTE
    fn select_single_expiration(
        expirations: &[NaiveDate],
        reference_date: NaiveDate,
        min_dte: i32,
    ) -> Result<NaiveDate, SelectionError> {
        expirations
            .iter()
            .find(|&&exp| {
                let dte = (exp - reference_date).num_days() as i32;
                dte >= min_dte
            })
            .copied()
            .ok_or(SelectionError::NoExpirations)
    }

    /// Snap strike to available strikes (for iron butterfly wings)
    fn snap_to_strike(
        target: Strike,
        available: &[Strike],
        round_up: bool,
    ) -> Result<Strike, SelectionError> {
        available
            .iter()
            .filter(|s| if round_up { **s >= target } else { **s <= target })
            .min_by(|a, b| {
                let a_diff = (a.value() - target.value()).abs();
                let b_diff = (b.value() - target.value()).abs();
                a_diff.partial_cmp(&b_diff).unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
            .ok_or(SelectionError::NoStrikes)
    }

    /// Select straddle legs (shared logic for long/short straddles)
    fn select_straddle_legs(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        min_expiration: NaiveDate,
    ) -> Result<(OptionLeg, OptionLeg), SelectionError> {
        // Get strikes from IV surface
        let strikes: Vec<Strike> = surface.strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        // Filter expirations to those AFTER min_expiration
        let expirations: Vec<NaiveDate> = surface.expirations()
            .into_iter()
            .filter(|&exp| exp > min_expiration)
            .collect();

        if expirations.is_empty() {
            return Err(SelectionError::NoExpirations);
        }

        // Select first valid expiration (soonest after min_expiration)
        let expiration = *expirations.iter().min().unwrap();

        // Select ATM strike (closest to spot)
        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let atm_strike = super::find_closest_strike(&strikes, spot_f64)?;

        // Create legs
        let symbol = surface.underlying().to_string();
        let call_leg = OptionLeg::new(
            symbol.clone(),
            atm_strike,
            expiration,
            OptionType::Call,
        );
        let put_leg = OptionLeg::new(
            symbol,
            atm_strike,
            expiration,
            OptionType::Put,
        );

        Ok((call_leg, put_leg))
    }
}

impl SelectionStrategy for ATMStrategy {
    fn select_calendar_spread(
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
        let short_atm_strike = super::find_closest_strike(&chain_data.strikes, spot_f64)?;

        // Select expirations
        let (short_exp, long_exp) = super::select_expirations(
            &chain_data.expirations,
            event.earnings_date,
            self.criteria.min_short_dte,
            self.criteria.max_short_dte,
            self.criteria.min_long_dte,
            self.criteria.max_long_dte,
        )?;

        // Determine long leg strike based on matching mode
        let long_strike = match self.strike_match_mode {
            StrikeMatchMode::SameStrike => short_atm_strike,
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
                let short_strike_f64: f64 = short_atm_strike.into();

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
            short_atm_strike,
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

    fn select_calendar_straddle(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
    ) -> Result<CalendarStraddle, StrategyError> {
        if chain_data.strikes.is_empty() {
            return Err(StrategyError::NoStrikes);
        }

        // Find ATM strike (same for all 4 legs)
        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let atm_strike = super::find_closest_strike(&chain_data.strikes, spot_f64)?;

        // Select expirations (short near-term, long far-term)
        let (short_exp, long_exp) = super::select_expirations(
            &chain_data.expirations,
            event.earnings_date,
            self.criteria.min_short_dte,
            self.criteria.max_short_dte,
            self.criteria.min_long_dte,
            self.criteria.max_long_dte,
        )?;

        // Build all 4 legs at the same ATM strike
        let short_call = OptionLeg::new(
            event.symbol.clone(),
            atm_strike,
            short_exp,
            OptionType::Call,
        );
        let short_put = OptionLeg::new(
            event.symbol.clone(),
            atm_strike,
            short_exp,
            OptionType::Put,
        );
        let long_call = OptionLeg::new(
            event.symbol.clone(),
            atm_strike,
            long_exp,
            OptionType::Call,
        );
        let long_put = OptionLeg::new(
            event.symbol.clone(),
            atm_strike,
            long_exp,
            OptionType::Put,
        );

        CalendarStraddle::new(short_call, short_put, long_call, long_put).map_err(Into::into)
    }
}

// ============================================================================
// NEW StrikeSelector trait implementation (uses IVSurface directly)
// ============================================================================

impl StrikeSelector for ATMStrategy {
    fn select_calendar_spread(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        option_type: OptionType,
        criteria: &ExpirationCriteria,
    ) -> Result<CalendarSpread, SelectionError> {
        // Get strikes and expirations from IV surface
        let strikes: Vec<Strike> = surface.strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let expirations = surface.expirations();

        // Find ATM strike for short leg
        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let short_atm_strike = super::find_closest_strike(&strikes, spot_f64)?;

        // Convert to StrategyError for select_expirations (backwards compat)
        let (short_exp, long_exp) = super::select_expirations(
            &expirations,
            surface.as_of_time().date_naive(),
            criteria.min_short_dte,
            criteria.max_short_dte,
            criteria.min_long_dte,
            criteria.max_long_dte,
        ).map_err(|e| match e {
            StrategyError::InsufficientExpirations { needed, available } =>
                SelectionError::InsufficientExpirations { needed, available },
            StrategyError::NoExpirations => SelectionError::NoExpirations,
            _ => SelectionError::NoExpirations,
        })?;

        // Determine long leg strike based on matching mode
        let long_strike = match self.strike_match_mode {
            StrikeMatchMode::SameStrike => short_atm_strike,
            StrikeMatchMode::SameDelta => {
                // Build delta-parameterized surface
                let delta_surface = DeltaVolSurface::from_iv_surface(surface, self.risk_free_rate);

                // Calculate delta of ATM strike at short expiration
                let is_call = option_type == OptionType::Call;
                let short_strike_f64: f64 = short_atm_strike.into();

                // Get the IV at the short ATM strike
                let short_slice = delta_surface.slice(short_exp)
                    .ok_or(SelectionError::NoIVSurface)?;
                let short_iv = short_slice.get_iv_at_strike(short_strike_f64)
                    .ok_or(SelectionError::NoIVSurface)?;

                // Calculate time to expiry for short leg
                let short_tte = delta_surface.tte(short_exp)
                    .ok_or(SelectionError::NoIVSurface)?;

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
                    .ok_or(SelectionError::NoIVSurface)?;

                // Find closest tradable strike
                strikes
                    .iter()
                    .min_by(|a, b| {
                        let a_diff = (f64::from(**a) - theoretical_long_strike).abs();
                        let b_diff = (f64::from(**b) - theoretical_long_strike).abs();
                        a_diff.partial_cmp(&b_diff).unwrap()
                    })
                    .copied()
                    .ok_or(SelectionError::NoStrikes)?
            }
        };

        // Build legs - use surface underlying symbol
        let symbol = surface.underlying().to_string();
        let short_leg = OptionLeg::new(
            symbol.clone(),
            short_atm_strike,
            short_exp,
            option_type,
        );
        let long_leg = OptionLeg::new(
            symbol,
            long_strike,
            long_exp,
            option_type,
        );

        CalendarSpread::new(short_leg, long_leg).map_err(Into::into)
    }

    fn select_long_straddle(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        min_expiration: NaiveDate,
    ) -> Result<LongStraddle, SelectionError> {
        let (call_leg, put_leg) = self.select_straddle_legs(spot, surface, min_expiration)?;
        LongStraddle::new(call_leg, put_leg).map_err(Into::into)
    }

    fn select_short_straddle(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        min_expiration: NaiveDate,
    ) -> Result<ShortStraddle, SelectionError> {
        let (call_leg, put_leg) = self.select_straddle_legs(spot, surface, min_expiration)?;
        ShortStraddle::new(call_leg, put_leg).map_err(Into::into)
    }

    fn select_calendar_straddle(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        criteria: &ExpirationCriteria,
    ) -> Result<CalendarStraddle, SelectionError> {
        // Get strikes and expirations from IV surface
        let strikes: Vec<Strike> = surface.strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let expirations = surface.expirations();

        // Find ATM strike (same for all 4 legs)
        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let atm_strike = super::find_closest_strike(&strikes, spot_f64)?;

        // Select expirations (short near-term, long far-term)
        let (short_exp, long_exp) = super::select_expirations(
            &expirations,
            surface.as_of_time().date_naive(),
            criteria.min_short_dte,
            criteria.max_short_dte,
            criteria.min_long_dte,
            criteria.max_long_dte,
        ).map_err(|e| match e {
            StrategyError::InsufficientExpirations { needed, available } =>
                SelectionError::InsufficientExpirations { needed, available },
            StrategyError::NoExpirations => SelectionError::NoExpirations,
            _ => SelectionError::NoExpirations,
        })?;

        // Build all 4 legs at the same ATM strike
        let symbol = surface.underlying().to_string();
        let short_call = OptionLeg::new(
            symbol.clone(),
            atm_strike,
            short_exp,
            OptionType::Call,
        );
        let short_put = OptionLeg::new(
            symbol.clone(),
            atm_strike,
            short_exp,
            OptionType::Put,
        );
        let long_call = OptionLeg::new(
            symbol.clone(),
            atm_strike,
            long_exp,
            OptionType::Call,
        );
        let long_put = OptionLeg::new(
            symbol,
            atm_strike,
            long_exp,
            OptionType::Put,
        );

        CalendarStraddle::new(short_call, short_put, long_call, long_put).map_err(Into::into)
    }

    fn select_iron_butterfly(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        wing_width: Decimal,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<IronButterfly, SelectionError> {
        // Get strikes and expirations from IV surface
        let strikes: Vec<Strike> = surface.strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let expirations = surface.expirations();

        // Select expiration with DTE in range
        let reference_date = surface.as_of_time().date_naive();
        let expiration = expirations
            .iter()
            .find(|&&exp| {
                let dte = (exp - reference_date).num_days() as i32;
                dte >= min_dte && dte <= max_dte
            })
            .copied()
            .ok_or(SelectionError::NoExpirations)?;

        // Find ATM strike for center
        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let center = super::find_closest_strike(&strikes, spot_f64)?;

        // Calculate wing strikes
        let upper_target = Strike::new(center.value() + wing_width)
            .map_err(|_| SelectionError::NoStrikes)?;
        let lower_target = Strike::new(center.value() - wing_width)
            .map_err(|_| SelectionError::NoStrikes)?;

        // Snap to available strikes
        let upper = Self::snap_to_strike(upper_target, &strikes, true)?;
        let lower = Self::snap_to_strike(lower_target, &strikes, false)?;

        // Build legs
        let symbol = surface.underlying().to_string();
        let short_call = OptionLeg::new(
            symbol.clone(),
            center,
            expiration,
            OptionType::Call,
        );
        let short_put = OptionLeg::new(
            symbol.clone(),
            center,
            expiration,
            OptionType::Put,
        );
        let long_call = OptionLeg::new(
            symbol.clone(),
            upper,
            expiration,
            OptionType::Call,
        );
        let long_put = OptionLeg::new(
            symbol,
            lower,
            expiration,
            OptionType::Put,
        );

        IronButterfly::new(short_call, short_put, long_call, long_put)
            .map_err(Into::into)
    }

    fn select_iron_butterfly_with_config(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        config: &crate::value_objects::IronButterflyConfig,
        direction: crate::value_objects::TradeDirection,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<IronButterfly, SelectionError> {
        use crate::value_objects::{WingSelectionMode, TradeDirection};

        // Get strikes and expirations from IV surface
        let strikes: Vec<Strike> = surface.strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let expirations = surface.expirations();

        // Select expiration with DTE in range
        let reference_date = surface.as_of_time().date_naive();
        let expiration = expirations
            .iter()
            .find(|&&exp| {
                let dte = (exp - reference_date).num_days() as i32;
                dte >= min_dte && dte <= max_dte
            })
            .copied()
            .ok_or(SelectionError::NoExpirations)?;

        // Find ATM strike for center
        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let center = super::find_closest_strike(&strikes, spot_f64)?;

        // Select wing strikes based on config
        let (upper, lower) = match config.wing_mode {
            WingSelectionMode::Delta { wing_delta } => {
                // Build delta-parameterized surface
                let delta_surface = DeltaVolSurface::from_iv_surface(surface, self.risk_free_rate);

                // Convert target delta to strikes
                let upper_strike_f64 = delta_surface
                    .delta_to_strike(wing_delta, expiration, true)
                    .ok_or(SelectionError::NoIVSurface)?;

                let lower_strike_f64 = delta_surface
                    .delta_to_strike(-wing_delta, expiration, false)
                    .ok_or(SelectionError::NoIVSurface)?;

                // Convert to Strike and snap to available
                let upper_target = Strike::try_from(upper_strike_f64)
                    .map_err(|_| SelectionError::NoStrikes)?;
                let lower_target = Strike::try_from(lower_strike_f64)
                    .map_err(|_| SelectionError::NoStrikes)?;

                let upper_snapped = Self::snap_to_strike(upper_target, &strikes, true)?;
                let lower_snapped = Self::snap_to_strike(lower_target, &strikes, false)?;

                // Validate symmetric constraint if required
                if config.symmetric {
                    let upper_distance = upper_snapped.value() - center.value();
                    let lower_distance = center.value() - lower_snapped.value();
                    let tolerance = Decimal::new(5, 2); // 0.05

                    // Allow some tolerance for snapping
                    if (upper_distance - lower_distance).abs() > tolerance {
                        // Try to find better symmetric strikes
                        // Use the average distance for symmetry
                        let symmetric_distance = (upper_distance + lower_distance) / Decimal::new(2, 0);
                        let target_upper = Strike::new(center.value() + symmetric_distance)
                            .map_err(|_| SelectionError::NoStrikes)?;
                        let target_lower = Strike::new(center.value() - symmetric_distance)
                            .map_err(|_| SelectionError::NoStrikes)?;

                        (
                            Self::snap_to_strike(target_upper, &strikes, true)?,
                            Self::snap_to_strike(target_lower, &strikes, false)?,
                        )
                    } else {
                        (upper_snapped, lower_snapped)
                    }
                } else {
                    (upper_snapped, lower_snapped)
                }
            }
            WingSelectionMode::Moneyness { wing_percent } => {
                // Calculate wing strikes based on moneyness
                let upper_strike_f64 = spot_f64 * (1.0 + wing_percent);
                let lower_strike_f64 = spot_f64 * (1.0 - wing_percent);

                // Convert to Strike and snap to available
                let upper_target = Strike::try_from(upper_strike_f64)
                    .map_err(|_| SelectionError::NoStrikes)?;
                let lower_target = Strike::try_from(lower_strike_f64)
                    .map_err(|_| SelectionError::NoStrikes)?;

                (
                    Self::snap_to_strike(upper_target, &strikes, true)?,
                    Self::snap_to_strike(lower_target, &strikes, false)?,
                )
            }
        };

        // Build legs based on direction
        let symbol = surface.underlying().to_string();

        let (short_call_strike, short_put_strike, long_call_strike, long_put_strike) = match direction {
            TradeDirection::Short => {
                // Short: short ATM straddle + long OTM wings
                (center, center, upper, lower)
            }
            TradeDirection::Long => {
                // Long: long ATM straddle + short OTM wings (invert)
                (upper, lower, center, center)
            }
        };

        let short_call = OptionLeg::new(
            symbol.clone(),
            short_call_strike,
            expiration,
            OptionType::Call,
        );
        let short_put = OptionLeg::new(
            symbol.clone(),
            short_put_strike,
            expiration,
            OptionType::Put,
        );
        let long_call = OptionLeg::new(
            symbol.clone(),
            long_call_strike,
            expiration,
            OptionType::Call,
        );
        let long_put = OptionLeg::new(
            symbol,
            long_put_strike,
            expiration,
            OptionType::Put,
        );

        IronButterfly::new(short_call, short_put, long_call, long_put)
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};
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

        let result = SelectionStrategy::select_calendar_spread(&strategy, &event, &spot, &chain_data, OptionType::Call);
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

        let result = SelectionStrategy::select_calendar_spread(&strategy, &event, &spot, &chain_data, OptionType::Call);
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

        let result = SelectionStrategy::select_calendar_spread(&strategy, &event, &spot, &chain_data, OptionType::Call);
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

    #[test]
    fn test_atm_strategy_select_calendar_straddle() {
        // Nov 2025 dates
        let strategy = ATMStrategy::default();
        let event = EarningsEvent::new(
            "TEST".to_string(),
            NaiveDate::from_ymd_opt(2025, 11, 6).unwrap(),
            EarningsTime::AfterMarketClose,
        );
        let spot = SpotPrice::new(Decimal::new(100, 0), Utc::now());

        let chain_data = OptionChainData {
            expirations: vec![
                NaiveDate::from_ymd_opt(2025, 11, 14).unwrap(),  // 8 days from earnings
                NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),  // 15 days from earnings
                NaiveDate::from_ymd_opt(2025, 12, 5).unwrap(),   // 29 days from earnings
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
        };

        let result = SelectionStrategy::select_calendar_straddle(&strategy, &event, &spot, &chain_data);
        assert!(result.is_ok());

        let straddle = result.unwrap();
        assert_eq!(straddle.symbol(), "TEST");
        // Should select ATM strike of 100
        assert_eq!(straddle.short_strike().value(), Decimal::new(100, 0));
        assert_eq!(straddle.long_strike().value(), Decimal::new(100, 0));
        // Short expiry should be first valid one (Nov 14)
        assert_eq!(straddle.short_expiry(), NaiveDate::from_ymd_opt(2025, 11, 14).unwrap());
        // Long expiry should be second valid one (Nov 21)
        assert_eq!(straddle.long_expiry(), NaiveDate::from_ymd_opt(2025, 11, 21).unwrap());
    }

    #[test]
    fn test_atm_strategy_select_calendar_straddle_no_strikes() {
        let strategy = ATMStrategy::default();
        let event = EarningsEvent::new(
            "TEST".to_string(),
            NaiveDate::from_ymd_opt(2025, 11, 6).unwrap(),
            EarningsTime::AfterMarketClose,
        );
        let spot = SpotPrice::new(Decimal::new(100, 0), Utc::now());

        let chain_data = OptionChainData {
            expirations: vec![
                NaiveDate::from_ymd_opt(2025, 11, 14).unwrap(),
                NaiveDate::from_ymd_opt(2025, 11, 21).unwrap(),
            ],
            strikes: vec![],  // No strikes
            deltas: None,
            volumes: None,
            iv_ratios: None,
            iv_surface: None,
        };

        let result = SelectionStrategy::select_calendar_straddle(&strategy, &event, &spot, &chain_data);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StrategyError::NoStrikes));
    }
}
