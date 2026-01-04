use crate::entities::*;
use crate::value_objects::*;
use crate::strategies::{StrategyError, SelectionStrategy, OptionChainData};
use chrono::NaiveDate;
use finq_core::OptionType;

/// Straddle selection strategy
///
/// Selects ATM straddle with minimum DTE from entry.
/// This ensures we have enough time for the trade to work.
pub struct StraddleStrategy {
    pub min_dte_from_entry: i32,  // Minimum days from entry to expiration
    pub entry_date: NaiveDate,     // When we enter the trade
}

impl Default for StraddleStrategy {
    fn default() -> Self {
        Self {
            min_dte_from_entry: 7,  // At least 7 days from entry
            entry_date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(), // Placeholder
        }
    }
}

impl StraddleStrategy {
    /// Create with custom minimum DTE from entry
    pub fn with_min_dte(min_dte: i32, entry_date: NaiveDate) -> Self {
        Self {
            min_dte_from_entry: min_dte,
            entry_date,
        }
    }

    /// Find first expiration with sufficient DTE from entry
    fn select_expiration(
        &self,
        expirations: &[NaiveDate],
    ) -> Option<NaiveDate> {
        expirations
            .iter()
            .filter(|&&exp| {
                let dte = (exp - self.entry_date).num_days() as i32;
                dte >= self.min_dte_from_entry
            })
            .min()
            .copied()
    }
}

impl SelectionStrategy for StraddleStrategy {
    fn select_calendar_spread(
        &self,
        _event: &EarningsEvent,
        _spot: &SpotPrice,
        _chain_data: &OptionChainData,
        _option_type: OptionType,
    ) -> Result<CalendarSpread, StrategyError> {
        Err(StrategyError::UnsupportedStrategy(
            "Calendar spread not supported by StraddleStrategy".into()
        ))
    }

    fn select_iron_butterfly(
        &self,
        _event: &EarningsEvent,
        _spot: &SpotPrice,
        _chain_data: &OptionChainData,
    ) -> Result<IronButterfly, StrategyError> {
        Err(StrategyError::UnsupportedStrategy(
            "Iron butterfly not supported by StraddleStrategy".into()
        ))
    }

    fn select_straddle(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
    ) -> Result<Straddle, StrategyError> {
        // Select first expiration with sufficient DTE from entry
        let expiration = self.select_expiration(&chain_data.expirations)
            .ok_or(StrategyError::NoExpirations)?;

        // Select ATM strike (closest to spot)
        let spot_value = spot.to_f64();
        let atm_strike = super::find_closest_strike(&chain_data.strikes, spot_value)?;

        // Create legs
        let call_leg = OptionLeg::new(
            event.symbol.clone(),
            atm_strike,
            expiration,
            OptionType::Call,
        );
        let put_leg = OptionLeg::new(
            event.symbol.clone(),
            atm_strike,
            expiration,
            OptionType::Put,
        );

        Straddle::new(call_leg, put_leg)
            .map_err(StrategyError::SpreadCreation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use chrono::Utc;

    #[test]
    fn test_select_expiration_after_earnings() {
        let strategy = StraddleStrategy::default();
        let earnings_date = NaiveDate::from_ymd_opt(2025, 1, 30).unwrap();

        let expirations = vec![
            NaiveDate::from_ymd_opt(2025, 1, 24).unwrap(),  // Before earnings
            NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),  // After earnings (closest)
            NaiveDate::from_ymd_opt(2025, 2, 7).unwrap(),   // After earnings
        ];

        let selected = strategy.select_expiration(&expirations, earnings_date);
        assert_eq!(selected, Some(NaiveDate::from_ymd_opt(2025, 1, 31).unwrap()));
    }

    #[test]
    fn test_select_straddle() {
        let strategy = StraddleStrategy::default();
        let event = EarningsEvent::new(
            "AAPL".into(),
            NaiveDate::from_ymd_opt(2025, 1, 30).unwrap(),
            EarningsTime::AfterMarketClose,
        );

        let chain_data = OptionChainData {
            expirations: vec![
                NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
                NaiveDate::from_ymd_opt(2025, 2, 7).unwrap(),
            ],
            strikes: vec![
                Strike::new(Decimal::new(175, 0)).unwrap(),
                Strike::new(Decimal::new(180, 0)).unwrap(),
                Strike::new(Decimal::new(185, 0)).unwrap(),
            ],
            deltas: None,
            volumes: None,
            iv_ratios: None,
            iv_surface: None,
        };

        let spot = SpotPrice::new(Decimal::new(180, 0), Utc::now());
        let straddle = strategy.select_straddle(&event, &spot, &chain_data).unwrap();

        // Should select Jan 31 (first after earnings) and 180 strike (ATM)
        assert_eq!(straddle.expiration(), NaiveDate::from_ymd_opt(2025, 1, 31).unwrap());
        assert_eq!(straddle.strike().value(), Decimal::new(180, 0));
        assert_eq!(straddle.symbol(), "AAPL");
    }

    #[test]
    fn test_select_straddle_no_post_earnings_expiration() {
        let strategy = StraddleStrategy::default();
        let event = EarningsEvent::new(
            "AAPL".into(),
            NaiveDate::from_ymd_opt(2025, 1, 30).unwrap(),
            EarningsTime::AfterMarketClose,
        );

        let chain_data = OptionChainData {
            expirations: vec![
                NaiveDate::from_ymd_opt(2025, 1, 24).unwrap(),  // Before earnings
            ],
            strikes: vec![Strike::new(Decimal::new(180, 0)).unwrap()],
            deltas: None,
            volumes: None,
            iv_ratios: None,
            iv_surface: None,
        };

        let spot = SpotPrice::new(Decimal::new(180, 0), Utc::now());
        let result = strategy.select_straddle(&event, &spot, &chain_data);

        assert!(matches!(result, Err(StrategyError::NoExpirations)));
    }
}
