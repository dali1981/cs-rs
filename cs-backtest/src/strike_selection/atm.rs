use cs_domain::entities::{
    CalendarSpread, CalendarStraddle, IronButterfly, LongIronButterfly, LongStraddle, OptionLeg,
    ShortStraddle,
};
use cs_domain::strike_selection::{
    find_closest_strike, select_expirations, ExpirationCriteria, SelectionError, StrategyError,
    TradeSelectionCriteria,
};
use cs_domain::value_objects::{SpotPrice, Strike};
use cs_domain::strike_selection::StrikeMatchMode;
use super::StrikeSelector;
use chrono::NaiveDate;
use cs_analytics::{DeltaVolSurface, IVSurface, bs_delta};
use finq_core::OptionType;
use rust_decimal::Decimal;

/// ATM strategy — select strike closest to spot.
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
    pub fn new(criteria: TradeSelectionCriteria) -> Self {
        Self {
            criteria,
            strike_match_mode: StrikeMatchMode::default(),
            risk_free_rate: 0.05,
        }
    }

    pub fn with_strike_match_mode(mut self, mode: StrikeMatchMode) -> Self {
        self.strike_match_mode = mode;
        self
    }

    pub fn with_risk_free_rate(mut self, rate: f64) -> Self {
        self.risk_free_rate = rate;
        self
    }

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

    fn select_straddle_legs(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        min_expiration: NaiveDate,
    ) -> Result<(OptionLeg, OptionLeg), SelectionError> {
        let strikes: Vec<Strike> = surface
            .strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let expirations: Vec<NaiveDate> = surface
            .expirations()
            .into_iter()
            .filter(|&exp| exp > min_expiration)
            .collect();

        if expirations.is_empty() {
            return Err(SelectionError::NoExpirations);
        }

        let expiration = *expirations.iter().min().unwrap();
        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let atm_strike = find_closest_strike(&strikes, spot_f64)?;

        let symbol = surface.underlying().to_string();
        let call_leg = OptionLeg::new(symbol.clone(), atm_strike, expiration, OptionType::Call);
        let put_leg = OptionLeg::new(symbol, atm_strike, expiration, OptionType::Put);

        Ok((call_leg, put_leg))
    }
}

impl StrikeSelector for ATMStrategy {
    fn select_calendar_spread(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        option_type: OptionType,
        criteria: &ExpirationCriteria,
    ) -> Result<CalendarSpread, SelectionError> {
        let strikes: Vec<Strike> = surface
            .strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let expirations = surface.expirations();

        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let short_atm_strike = find_closest_strike(&strikes, spot_f64)?;

        let (short_exp, long_exp) = select_expirations(
            &expirations,
            surface.as_of_time().date_naive(),
            criteria.min_short_dte,
            criteria.max_short_dte,
            criteria.min_long_dte,
            criteria.max_long_dte,
        )
        .map_err(|e| match e {
            StrategyError::InsufficientExpirations { needed, available } => {
                SelectionError::InsufficientExpirations { needed, available }
            }
            StrategyError::NoExpirations => SelectionError::NoExpirations,
            _ => SelectionError::NoExpirations,
        })?;

        let long_strike = match self.strike_match_mode {
            StrikeMatchMode::SameStrike => short_atm_strike,
            StrikeMatchMode::SameDelta => {
                let delta_surface =
                    DeltaVolSurface::from_iv_surface(surface, self.risk_free_rate);

                let is_call = option_type == OptionType::Call;
                let short_strike_f64: f64 = short_atm_strike.into();

                let short_slice = delta_surface
                    .slice(short_exp)
                    .ok_or(SelectionError::NoIVSurface)?;
                let short_iv = short_slice
                    .get_iv_at_strike(short_strike_f64)
                    .ok_or(SelectionError::NoIVSurface)?;

                let short_tte = delta_surface
                    .tte(short_exp)
                    .ok_or(SelectionError::NoIVSurface)?;

                let atm_delta = bs_delta(
                    spot_f64,
                    short_strike_f64,
                    short_tte,
                    short_iv,
                    is_call,
                    self.risk_free_rate,
                );

                let theoretical_long_strike = delta_surface
                    .delta_to_strike(atm_delta, long_exp, is_call)
                    .ok_or(SelectionError::NoIVSurface)?;

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

        let symbol = surface.underlying().to_string();
        let short_leg = OptionLeg::new(symbol.clone(), short_atm_strike, short_exp, option_type);
        let long_leg = OptionLeg::new(symbol, long_strike, long_exp, option_type);

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
        let strikes: Vec<Strike> = surface
            .strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let expirations = surface.expirations();

        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let atm_strike = find_closest_strike(&strikes, spot_f64)?;

        let (short_exp, long_exp) = select_expirations(
            &expirations,
            surface.as_of_time().date_naive(),
            criteria.min_short_dte,
            criteria.max_short_dte,
            criteria.min_long_dte,
            criteria.max_long_dte,
        )
        .map_err(|e| match e {
            StrategyError::InsufficientExpirations { needed, available } => {
                SelectionError::InsufficientExpirations { needed, available }
            }
            StrategyError::NoExpirations => SelectionError::NoExpirations,
            _ => SelectionError::NoExpirations,
        })?;

        let symbol = surface.underlying().to_string();
        let short_call =
            OptionLeg::new(symbol.clone(), atm_strike, short_exp, OptionType::Call);
        let short_put = OptionLeg::new(symbol.clone(), atm_strike, short_exp, OptionType::Put);
        let long_call = OptionLeg::new(symbol.clone(), atm_strike, long_exp, OptionType::Call);
        let long_put = OptionLeg::new(symbol, atm_strike, long_exp, OptionType::Put);

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
        let strikes: Vec<Strike> = surface
            .strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let expirations = surface.expirations();
        let reference_date = surface.as_of_time().date_naive();
        let expiration = expirations
            .iter()
            .find(|&&exp| {
                let dte = (exp - reference_date).num_days() as i32;
                dte >= min_dte && dte <= max_dte
            })
            .copied()
            .ok_or(SelectionError::NoExpirations)?;

        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let center = find_closest_strike(&strikes, spot_f64)?;

        let upper_target =
            Strike::new(center.value() + wing_width).map_err(|_| SelectionError::NoStrikes)?;
        let lower_target =
            Strike::new(center.value() - wing_width).map_err(|_| SelectionError::NoStrikes)?;

        let upper = Self::snap_to_strike(upper_target, &strikes, true)?;
        let lower = Self::snap_to_strike(lower_target, &strikes, false)?;

        let symbol = surface.underlying().to_string();
        let short_call = OptionLeg::new(symbol.clone(), center, expiration, OptionType::Call);
        let short_put = OptionLeg::new(symbol.clone(), center, expiration, OptionType::Put);
        let long_call = OptionLeg::new(symbol.clone(), upper, expiration, OptionType::Call);
        let long_put = OptionLeg::new(symbol, lower, expiration, OptionType::Put);

        IronButterfly::new(short_call, short_put, long_call, long_put).map_err(Into::into)
    }

    fn select_long_iron_butterfly(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        wing_width: Decimal,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<LongIronButterfly, SelectionError> {
        let strikes: Vec<Strike> = surface
            .strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let expirations = surface.expirations();
        let reference_date = surface.as_of_time().date_naive();
        let expiration = expirations
            .iter()
            .find(|&&exp| {
                let dte = (exp - reference_date).num_days() as i32;
                dte >= min_dte && dte <= max_dte
            })
            .copied()
            .ok_or(SelectionError::NoExpirations)?;

        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let center = find_closest_strike(&strikes, spot_f64)?;

        let upper_target =
            Strike::new(center.value() + wing_width).map_err(|_| SelectionError::NoStrikes)?;
        let lower_target =
            Strike::new(center.value() - wing_width).map_err(|_| SelectionError::NoStrikes)?;

        let upper = Self::snap_to_strike(upper_target, &strikes, true)?;
        let lower = Self::snap_to_strike(lower_target, &strikes, false)?;

        let symbol = surface.underlying().to_string();
        let center_call = OptionLeg::new(symbol.clone(), center, expiration, OptionType::Call);
        let center_put = OptionLeg::new(symbol.clone(), center, expiration, OptionType::Put);
        let upper_call = OptionLeg::new(symbol.clone(), upper, expiration, OptionType::Call);
        let lower_put = OptionLeg::new(symbol, lower, expiration, OptionType::Put);

        LongIronButterfly::new(center_call, center_put, upper_call, lower_put).map_err(Into::into)
    }

    fn select_iron_butterfly_with_config(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        config: &cs_domain::value_objects::IronButterflyConfig,
        direction: cs_domain::value_objects::TradeDirection,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<IronButterfly, SelectionError> {
        use cs_domain::value_objects::{TradeDirection, WingSelectionMode};

        let strikes: Vec<Strike> = surface
            .strikes()
            .iter()
            .filter_map(|&s| Strike::new(s).ok())
            .collect();

        if strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let expirations = surface.expirations();
        let reference_date = surface.as_of_time().date_naive();
        let expiration = expirations
            .iter()
            .find(|&&exp| {
                let dte = (exp - reference_date).num_days() as i32;
                dte >= min_dte && dte <= max_dte
            })
            .copied()
            .ok_or(SelectionError::NoExpirations)?;

        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let center = find_closest_strike(&strikes, spot_f64)?;

        let (upper, lower) = match config.wing_mode {
            WingSelectionMode::Delta { wing_delta } => {
                let delta_surface =
                    DeltaVolSurface::from_iv_surface(surface, self.risk_free_rate);

                let upper_strike_f64 = delta_surface
                    .delta_to_strike(wing_delta, expiration, true)
                    .ok_or(SelectionError::NoIVSurface)?;

                let lower_strike_f64 = delta_surface
                    .delta_to_strike(-wing_delta, expiration, false)
                    .ok_or(SelectionError::NoIVSurface)?;

                let upper_target = Strike::try_from(upper_strike_f64)
                    .map_err(|_| SelectionError::NoStrikes)?;
                let lower_target = Strike::try_from(lower_strike_f64)
                    .map_err(|_| SelectionError::NoStrikes)?;

                let upper_snapped = Self::snap_to_strike(upper_target, &strikes, true)?;
                let lower_snapped = Self::snap_to_strike(lower_target, &strikes, false)?;

                if config.symmetric {
                    let upper_distance = upper_snapped.value() - center.value();
                    let lower_distance = center.value() - lower_snapped.value();
                    let tolerance = Decimal::new(5, 2);

                    if (upper_distance - lower_distance).abs() > tolerance {
                        let symmetric_distance =
                            (upper_distance + lower_distance) / Decimal::new(2, 0);
                        let target_upper =
                            Strike::new(center.value() + symmetric_distance)
                                .map_err(|_| SelectionError::NoStrikes)?;
                        let target_lower =
                            Strike::new(center.value() - symmetric_distance)
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
                let upper_strike_f64 = spot_f64 * (1.0 + wing_percent);
                let lower_strike_f64 = spot_f64 * (1.0 - wing_percent);

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

        let symbol = surface.underlying().to_string();
        let (short_call_strike, short_put_strike, long_call_strike, long_put_strike) =
            match direction {
                TradeDirection::Short => (center, center, upper, lower),
                TradeDirection::Long => (upper, lower, center, center),
            };

        let short_call =
            OptionLeg::new(symbol.clone(), short_call_strike, expiration, OptionType::Call);
        let short_put =
            OptionLeg::new(symbol.clone(), short_put_strike, expiration, OptionType::Put);
        let long_call =
            OptionLeg::new(symbol.clone(), long_call_strike, expiration, OptionType::Call);
        let long_put =
            OptionLeg::new(symbol, long_put_strike, expiration, OptionType::Put);

        IronButterfly::new(short_call, short_put, long_call, long_put).map_err(Into::into)
    }
}
