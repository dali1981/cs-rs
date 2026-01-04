use crate::entities::*;
use crate::value_objects::*;
use super::{StrategyError, OptionChainData, SelectionStrategy};
use chrono::NaiveDate;
use finq_core::OptionType;
use rust_decimal::Decimal;

/// Iron butterfly strategy configuration
#[derive(Debug, Clone)]
pub struct IronButterflyStrategy {
    pub wing_width: Decimal,
    pub min_dte: i32,
    pub max_dte: i32,
}

impl IronButterflyStrategy {
    pub fn new(wing_width: Decimal, min_dte: i32, max_dte: i32) -> Self {
        Self {
            wing_width,
            min_dte,
            max_dte,
        }
    }
}

impl SelectionStrategy for IronButterflyStrategy {
    fn select_calendar_spread(
        &self,
        _event: &EarningsEvent,
        _spot: &SpotPrice,
        _chain_data: &OptionChainData,
        _option_type: OptionType,
    ) -> Result<CalendarSpread, StrategyError> {
        Err(StrategyError::UnsupportedStrategy(
            "IronButterflyStrategy only supports iron butterfly selection, not calendar spreads".to_string()
        ))
    }

    fn select_iron_butterfly(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
    ) -> Result<IronButterfly, StrategyError> {
        // 1. Select expiration
        let expiration = self.select_expiration(event, &chain_data.expirations)?;

        // 2. Find ATM strike for center
        let spot_f64: f64 = spot.value.try_into().unwrap_or(0.0);
        let center = super::find_closest_strike(&chain_data.strikes, spot_f64)?;

        // 3. Calculate wing strikes
        let upper_target = Strike::new(center.value() + self.wing_width)
            .map_err(|_| StrategyError::NoStrikes)?;
        let lower_target = Strike::new(center.value() - self.wing_width)
            .map_err(|_| StrategyError::NoStrikes)?;

        // 4. Snap to available strikes
        let upper = self.snap_to_strike(upper_target, &chain_data.strikes, true)?;
        let lower = self.snap_to_strike(lower_target, &chain_data.strikes, false)?;

        // 5. Build legs
        let short_call = OptionLeg::new(
            event.symbol.clone(),
            center,
            expiration,
            OptionType::Call,
        );
        let short_put = OptionLeg::new(
            event.symbol.clone(),
            center,
            expiration,
            OptionType::Put,
        );
        let long_call = OptionLeg::new(
            event.symbol.clone(),
            upper,
            expiration,
            OptionType::Call,
        );
        let long_put = OptionLeg::new(
            event.symbol.clone(),
            lower,
            expiration,
            OptionType::Put,
        );

        IronButterfly::new(short_call, short_put, long_call, long_put)
            .map_err(Into::into)
    }
}

impl IronButterflyStrategy {
    fn select_expiration(
        &self,
        event: &EarningsEvent,
        expirations: &[NaiveDate],
    ) -> Result<NaiveDate, StrategyError> {
        // Use the FIRST expiration >= earnings_date with DTE in range
        expirations
            .iter()
            .filter(|&exp| {
                let dte = (*exp - event.earnings_date).num_days() as i32;
                dte >= self.min_dte && dte <= self.max_dte
            })
            .next()
            .copied()
            .ok_or(StrategyError::NoExpirations)
    }

    fn snap_to_strike(
        &self,
        target: Strike,
        available: &[Strike],
        round_up: bool,
    ) -> Result<Strike, StrategyError> {
        // Find closest available strike >= target (if round_up) or <= target
        available
            .iter()
            .filter(|s| if round_up { **s >= target } else { **s <= target })
            .min_by(|a, b| {
                let a_diff = (a.value() - target.value()).abs();
                let b_diff = (b.value() - target.value()).abs();
                a_diff.partial_cmp(&b_diff).unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
            .ok_or(StrategyError::NoStrikes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::find_closest_strike;
    use chrono::Utc;

    #[test]
    fn test_iron_butterfly_strategy_atm_strike() {
        let _strategy = IronButterflyStrategy::new(
            Decimal::new(10, 0),
            3,
            45,
        );

        let spot = SpotPrice::new(Decimal::new(180, 0), Utc::now());
        let strikes = vec![
            Strike::new(Decimal::new(170, 0)).unwrap(),
            Strike::new(Decimal::new(175, 0)).unwrap(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            Strike::new(Decimal::new(185, 0)).unwrap(),
            Strike::new(Decimal::new(190, 0)).unwrap(),
        ];

        let spot_f64: f64 = spot.value.try_into().unwrap();
        let atm = find_closest_strike(&strikes, spot_f64).unwrap();
        assert_eq!(atm.value(), Decimal::new(180, 0));
    }

    #[test]
    fn test_iron_butterfly_snap_to_strike() {
        let strategy = IronButterflyStrategy::new(
            Decimal::new(10, 0),
            3,
            45,
        );

        let strikes = vec![
            Strike::new(Decimal::new(170, 0)).unwrap(),
            Strike::new(Decimal::new(175, 0)).unwrap(),
            Strike::new(Decimal::new(180, 0)).unwrap(),
            Strike::new(Decimal::new(185, 0)).unwrap(),
            Strike::new(Decimal::new(190, 0)).unwrap(),
            Strike::new(Decimal::new(195, 0)).unwrap(),
        ];

        // Test rounding up
        let target = Strike::new(Decimal::new(188, 0)).unwrap();
        let snapped = strategy.snap_to_strike(target, &strikes, true).unwrap();
        assert_eq!(snapped.value(), Decimal::new(190, 0));

        // Test rounding down
        let target = Strike::new(Decimal::new(172, 0)).unwrap();
        let snapped = strategy.snap_to_strike(target, &strikes, false).unwrap();
        assert_eq!(snapped.value(), Decimal::new(170, 0));
    }
}
