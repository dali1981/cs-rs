use crate::entities::*;
use crate::value_objects::*;
use chrono::NaiveDate;
use cs_analytics::IVSurface;
use finq_core::OptionType;
use rust_decimal::Decimal;

use super::{SelectionError, MultiLegStrikeSelection, ExpirationCriteria};

/// Unified selector for all symmetric multi-leg volatility strategies
///
/// Handles strike selection for:
/// - Strangle (2-leg: 1 OTM call + 1 OTM put)
/// - Butterfly (4-leg: 2x ATM straddle + OTM wings)
/// - IronButterfly (4-leg: 1x ATM straddle + OTM wings)
/// - Condor (4-leg: 1x near straddle + far wings)
/// - IronCondor (4-leg: 2-leg near spread + 2-leg far wings)
pub struct SymmetricMultiLegSelector {
    pub risk_free_rate: f64,
}

impl SymmetricMultiLegSelector {
    pub fn new(risk_free_rate: f64) -> Self {
        Self { risk_free_rate }
    }

    /// Select expiration that matches DTE criteria
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

        // Return the first available expiration
        Ok(expirations[0])
    }

    /// Get available strikes for an expiration
    fn get_strikes_for_expiration(
        &self,
        surface: &IVSurface,
        _expiration: NaiveDate,
    ) -> Result<Vec<Strike>, SelectionError> {
        let strikes_decimal = surface.strikes();
        if strikes_decimal.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        let mut strikes: Vec<Strike> = Vec::new();
        for strike_decimal in strikes_decimal {
            strikes.push(Strike::new(strike_decimal)?);
        }
        Ok(strikes)
    }

    /// Find strike at the given moneyness level
    fn find_strike_at_moneyness(
        &self,
        surface: &IVSurface,
        moneyness_percent: f64,
    ) -> Result<(Strike, Strike), SelectionError> {
        let spot = surface.spot_price();
        let spot_f64: f64 = spot.try_into().unwrap_or(0.0);

        let upper_val = spot_f64 * (1.0 + moneyness_percent);
        let lower_val = spot_f64 * (1.0 - moneyness_percent);

        let upper_strike = Decimal::from_f64_retain(upper_val)
            .ok_or(SelectionError::NoStrikes)?;
        let lower_strike = Decimal::from_f64_retain(lower_val)
            .ok_or(SelectionError::NoStrikes)?;

        let upper = Strike::new(upper_strike)?;
        let lower = Strike::new(lower_strike)?;

        Ok((lower, upper))
    }

    /// Snap strike to nearest available strike in the IV surface
    fn snap_to_available_strike(
        &self,
        surface: &IVSurface,
        target_strike: Strike,
    ) -> Result<Strike, SelectionError> {
        let available_strikes = self.get_strikes_for_expiration(surface, surface.expirations()[0])?;
        if available_strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        // Find closest available strike
        let target_val: f64 = target_strike.value().try_into().unwrap_or(0.0);
        let mut best = available_strikes[0];
        let mut best_diff = {
            let best_val: f64 = best.value().try_into().unwrap_or(0.0);
            (best_val - target_val).abs()
        };

        for strike in available_strikes {
            let strike_val: f64 = strike.value().try_into().unwrap_or(0.0);
            let diff = (strike_val - target_val).abs();
            if diff < best_diff {
                best = strike;
                best_diff = diff;
            }
        }

        Ok(best)
    }

    /// Validate symmetric constraint between upper and lower strikes
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
            return Err(SelectionError::UnsupportedStrategy(
                format!(
                    "Symmetric constraint violated: upper_dist={}, lower_dist={}",
                    upper_dist, lower_dist
                ),
            ));
        }

        Ok(())
    }

    /// Build center strikes based on center configuration
    fn build_center_strikes(
        &self,
        surface: &IVSurface,
        center: CenterConfig,
    ) -> Result<Vec<Strike>, SelectionError> {
        let spot = surface.spot_price();
        let available_strikes = self.get_strikes_for_expiration(surface, surface.expirations()[0])?;

        if available_strikes.is_empty() {
            return Err(SelectionError::NoStrikes);
        }

        // Find strike closest to spot
        let spot_f64: f64 = spot.try_into().unwrap_or(0.0);
        let mut atm_strike = available_strikes[0];
        let mut best_diff = {
            let strike_val: f64 = atm_strike.value().try_into().unwrap_or(0.0);
            (strike_val - spot_f64).abs()
        };

        for strike in &available_strikes {
            let strike_val: f64 = strike.value().try_into().unwrap_or(0.0);
            let diff = (strike_val - spot_f64).abs();
            if diff < best_diff {
                atm_strike = *strike;
                best_diff = diff;
            }
        }

        // Return multiplicity number of center strikes
        Ok(vec![atm_strike; center.multiplicity as usize])
    }
}

impl super::StrikeSelector for SymmetricMultiLegSelector {
    fn select_calendar_spread(
        &self,
        _spot: &SpotPrice,
        _surface: &IVSurface,
        _option_type: OptionType,
        _criteria: &ExpirationCriteria,
    ) -> Result<CalendarSpread, SelectionError> {
        Err(SelectionError::UnsupportedStrategy(
            "Calendar spread not supported by multi-leg selector".to_string()
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
        // 1. Select expiration
        let expiration = self.select_expiration(surface, min_dte, max_dte)?;

        // 2. Build center strikes
        let center_strikes = self.build_center_strikes(surface, config.center)?;

        // 3. Select wings based on spread type
        let (near_strikes, far_strikes) = match config.wings.spread_type {
            SpreadType::Simple { distance_from_center } => {
                // Simple spread: one distance for wings
                match distance_from_center {
                    DistanceSpec::Moneyness(moneyness_pct) => {
                        let (put_strike, call_strike) = self.find_strike_at_moneyness(
                            surface,
                            moneyness_pct,
                        )?;

                        // Snap to available strikes
                        let put_snapped = self.snap_to_available_strike(surface, put_strike)?;
                        let call_snapped = self.snap_to_available_strike(surface, call_strike)?;

                        // For simple spreads, no near/far distinction
                        let center_strike = center_strikes.first().cloned()
                            .ok_or(SelectionError::NoStrikes)?;

                        // Validate symmetric constraint if enabled
                        if config.wings.symmetric {
                            self.validate_symmetric(center_strike, call_snapped, put_snapped, 0.05)?;
                        }

                        (None, Some(vec![put_snapped, call_snapped]))
                    }
                    DistanceSpec::Delta(_) => {
                        // Delta-based selection not yet implemented
                        return Err(SelectionError::UnsupportedStrategy(
                            "Delta-based multi-leg selection not yet implemented".to_string()
                        ));
                    }
                }
            }
            SpreadType::Double { near_distance, far_distance } => {
                // Double spread: two distances for inner and outer wings
                // For now, only support moneyness-based double spreads
                match (near_distance, far_distance) {
                    (DistanceSpec::Moneyness(near_pct), DistanceSpec::Moneyness(far_pct)) => {
                        let (put_near, call_near) = self.find_strike_at_moneyness(surface, near_pct)?;
                        let (put_far, call_far) = self.find_strike_at_moneyness(surface, far_pct)?;

                        // Snap to available strikes
                        let put_near_snap = self.snap_to_available_strike(surface, put_near)?;
                        let call_near_snap = self.snap_to_available_strike(surface, call_near)?;
                        let put_far_snap = self.snap_to_available_strike(surface, put_far)?;
                        let call_far_snap = self.snap_to_available_strike(surface, call_far)?;

                        (
                            Some(vec![put_near_snap, call_near_snap]),
                            Some(vec![put_far_snap, call_far_snap]),
                        )
                    }
                    _ => {
                        return Err(SelectionError::UnsupportedStrategy(
                            "Mixed delta/moneyness double spreads not yet implemented".to_string()
                        ));
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
