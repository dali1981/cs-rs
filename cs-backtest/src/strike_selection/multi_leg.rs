use cs_domain::entities::CalendarSpread;
use cs_domain::value_objects::Strike;
use cs_domain::strike_selection::{ExpirationCriteria, MultiLegStrikeSelection, SelectionError};
use cs_domain::value_objects::{
    CenterConfig, DistanceSpec, MultiLegStrategyConfig, SpotPrice, SpreadType,
};
use super::StrikeSelector;
use chrono::NaiveDate;
use cs_analytics::IVSurface;
use finq_core::OptionType;
use rust_decimal::Decimal;

/// Unified selector for all symmetric multi-leg volatility strategies.
pub struct SymmetricMultiLegSelector {
    pub risk_free_rate: f64,
}

impl SymmetricMultiLegSelector {
    pub fn new(risk_free_rate: f64) -> Self {
        Self { risk_free_rate }
    }

    fn select_expiration(
        &self,
        surface: &IVSurface,
        _min_dte: i32,
        _max_dte: i32,
    ) -> Result<NaiveDate, SelectionError> {
        let expirations = surface.expirations();
        if expirations.is_empty() {
            return Err(SelectionError::NoExpirations);
        }
        Ok(expirations[0])
    }

    fn get_strikes_for_expiration(
        &self,
        surface: &IVSurface,
        _expiration: NaiveDate,
    ) -> Result<Vec<Strike>, SelectionError> {
        let strikes_decimal = surface.strikes();
        if strikes_decimal.is_empty() {
            return Err(SelectionError::NoStrikes);
        }
        let mut strikes = Vec::new();
        for s in strikes_decimal {
            strikes.push(Strike::new(s)?);
        }
        Ok(strikes)
    }

    fn find_strike_at_moneyness(
        &self,
        surface: &IVSurface,
        moneyness_percent: f64,
    ) -> Result<(Strike, Strike), SelectionError> {
        let spot = surface.spot_price();
        let spot_f64: f64 = spot.try_into().unwrap_or(0.0);

        let upper_val = spot_f64 * (1.0 + moneyness_percent);
        let lower_val = spot_f64 * (1.0 - moneyness_percent);

        let upper_strike =
            Decimal::from_f64_retain(upper_val).ok_or(SelectionError::NoStrikes)?;
        let lower_strike =
            Decimal::from_f64_retain(lower_val).ok_or(SelectionError::NoStrikes)?;

        Ok((Strike::new(lower_strike)?, Strike::new(upper_strike)?))
    }

    fn snap_to_available_strike(
        &self,
        surface: &IVSurface,
        target_strike: Strike,
    ) -> Result<Strike, SelectionError> {
        let available_strikes =
            self.get_strikes_for_expiration(surface, surface.expirations()[0])?;
        if available_strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let target_val: f64 = target_strike.value().try_into().unwrap_or(0.0);
        let mut best = available_strikes[0];
        let mut best_diff = {
            let v: f64 = f64::from(best);
            (v - target_val).abs()
        };

        for strike in available_strikes {
            let v: f64 = f64::from(strike);
            let diff = (v - target_val).abs();
            if diff < best_diff {
                best = strike;
                best_diff = diff;
            }
        }
        Ok(best)
    }

    fn validate_symmetric(
        &self,
        center: Strike,
        upper: Strike,
        lower: Strike,
        tolerance_pct: f64,
    ) -> Result<(), SelectionError> {
        let center_val: f64 = center.value().try_into().unwrap_or(0.0);
        let upper_val: f64 = upper.value().try_into().unwrap_or(0.0);
        let lower_val: f64 = lower.value().try_into().unwrap_or(0.0);

        let upper_dist = upper_val - center_val;
        let lower_dist = center_val - lower_val;

        if (upper_dist - lower_dist).abs() > center_val * tolerance_pct {
            return Err(SelectionError::UnsupportedStrategy(format!(
                "Symmetric constraint violated: upper_dist={}, lower_dist={}",
                upper_dist, lower_dist
            )));
        }
        Ok(())
    }

    fn build_center_strikes(
        &self,
        surface: &IVSurface,
        center: CenterConfig,
    ) -> Result<Vec<Strike>, SelectionError> {
        let spot = surface.spot_price();
        let available_strikes =
            self.get_strikes_for_expiration(surface, surface.expirations()[0])?;

        if available_strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let spot_f64: f64 = spot.try_into().unwrap_or(0.0);
        let mut atm_strike = available_strikes[0];
        let mut best_diff = {
            let v: f64 = atm_strike.value().try_into().unwrap_or(0.0);
            (v - spot_f64).abs()
        };

        for strike in &available_strikes {
            let v: f64 = f64::from(*strike);
            let diff = (v - spot_f64).abs();
            if diff < best_diff {
                atm_strike = *strike;
                best_diff = diff;
            }
        }

        Ok(vec![atm_strike; center.multiplicity as usize])
    }
}

impl StrikeSelector for SymmetricMultiLegSelector {
    fn select_calendar_spread(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _option_type: OptionType,
        _criteria: &ExpirationCriteria,
    ) -> Result<CalendarSpread, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Calendar spread not supported by multi-leg selector".to_string(),
        ))
    }

    fn select_multi_leg(
        &self,
        _spot: &SpotPrice,
        surface: &IVSurface,
        config: &MultiLegStrategyConfig,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<MultiLegStrikeSelection, SelectionError> {
        let expiration = self.select_expiration(surface, min_dte, max_dte)?;
        let center_strikes = self.build_center_strikes(surface, config.center)?;

        let (near_strikes, far_strikes) = match config.wings.spread_type {
            SpreadType::Simple { distance_from_center } => {
                match distance_from_center {
                    DistanceSpec::Moneyness(moneyness_pct) => {
                        let (put_strike, call_strike) =
                            self.find_strike_at_moneyness(surface, moneyness_pct)?;

                        let put_snapped = self.snap_to_available_strike(surface, put_strike)?;
                        let call_snapped = self.snap_to_available_strike(surface, call_strike)?;

                        let center_strike = center_strikes
                            .first()
                            .cloned()
                            .ok_or(SelectionError::NoStrikes)?;

                        if config.wings.symmetric {
                            self.validate_symmetric(
                                center_strike,
                                call_snapped,
                                put_snapped,
                                0.05,
                            )?;
                        }

                        (None, Some(vec![put_snapped, call_snapped]))
                    }
                    DistanceSpec::Delta(_) => {
                        return Err(SelectionError::UnsupportedStrategy(
                            "Delta-based multi-leg selection not yet implemented".to_string(),
                        ))
                    }
                }
            }
            SpreadType::Double { near_distance, far_distance } => {
                match (near_distance, far_distance) {
                    (DistanceSpec::Moneyness(near_pct), DistanceSpec::Moneyness(far_pct)) => {
                        let (put_near, call_near) =
                            self.find_strike_at_moneyness(surface, near_pct)?;
                        let (put_far, call_far) =
                            self.find_strike_at_moneyness(surface, far_pct)?;

                        let put_near_snap =
                            self.snap_to_available_strike(surface, put_near)?;
                        let call_near_snap =
                            self.snap_to_available_strike(surface, call_near)?;
                        let put_far_snap = self.snap_to_available_strike(surface, put_far)?;
                        let call_far_snap =
                            self.snap_to_available_strike(surface, call_far)?;

                        (
                            Some(vec![put_near_snap, call_near_snap]),
                            Some(vec![put_far_snap, call_far_snap]),
                        )
                    }
                    _ => {
                        return Err(SelectionError::UnsupportedStrategy(
                            "Mixed delta/moneyness double spreads not yet implemented"
                                .to_string(),
                        ))
                    }
                }
            }
        };

        Ok(MultiLegStrikeSelection {
            center_strikes,
            near_strikes,
            far_strikes,
            expiration,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selector_creation() {
        let selector = SymmetricMultiLegSelector::new(0.05);
        assert_eq!(selector.risk_free_rate, 0.05);
    }
}
