// Delta-space trading strategy for calendar spreads

use finq_core::OptionType;
use serde::{Deserialize, Serialize};

use cs_analytics::{
    DeltaVolSurface, OpportunityAnalyzer, OpportunityAnalyzerConfig,
    linspace,
};

use super::{OptionChainData, StrategyError, TradeSelectionCriteria, SelectionStrategy, StrikeMatchMode};
use crate::entities::{CalendarSpread, EarningsEvent, OptionLeg};
use crate::value_objects::SpotPrice;

/// Delta scan mode for strategy
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DeltaScanMode {
    /// Use a fixed target delta
    Fixed,
    /// Scan a range of deltas to find the best opportunity
    Scan { steps: usize },
}

impl Default for DeltaScanMode {
    fn default() -> Self {
        DeltaScanMode::Fixed
    }
}

/// Delta-space trading strategy.
///
/// This strategy:
/// 1. Builds a delta-parameterized IV surface from market data
/// 2. Analyzes term structure for calendar spread opportunities
/// 3. Selects the optimal delta based on IV ratio and liquidity
/// 4. Maps the target delta to the closest tradable strike
#[derive(Debug, Clone)]
pub struct DeltaStrategy {
    /// Trade selection criteria
    pub criteria: TradeSelectionCriteria,
    /// Target delta (for call side, 0-1)
    pub target_delta: f64,
    /// Risk-free rate for delta calculations
    pub risk_free_rate: f64,
    /// How to select delta (fixed or scan)
    pub scan_mode: DeltaScanMode,
    /// Delta range for scanning (min, max)
    pub delta_range: (f64, f64),
    /// Strike matching mode
    pub strike_match_mode: StrikeMatchMode,
}

impl Default for DeltaStrategy {
    fn default() -> Self {
        Self {
            criteria: TradeSelectionCriteria::default(),
            target_delta: 0.50,
            risk_free_rate: 0.05,
            scan_mode: DeltaScanMode::Fixed,
            delta_range: (0.25, 0.75),
            strike_match_mode: StrikeMatchMode::default(),
        }
    }
}

impl DeltaStrategy {
    /// Create a new delta strategy with fixed target delta
    pub fn fixed(target_delta: f64, criteria: TradeSelectionCriteria) -> Self {
        Self {
            criteria,
            target_delta,
            risk_free_rate: 0.05,
            scan_mode: DeltaScanMode::Fixed,
            delta_range: (0.25, 0.75),
            strike_match_mode: StrikeMatchMode::default(),
        }
    }

    /// Create a new delta strategy that scans for best delta
    pub fn scanning(
        delta_range: (f64, f64),
        steps: usize,
        criteria: TradeSelectionCriteria,
    ) -> Self {
        Self {
            criteria,
            target_delta: 0.50, // Default, will be overridden by scan
            risk_free_rate: 0.05,
            scan_mode: DeltaScanMode::Scan { steps },
            delta_range,
            strike_match_mode: StrikeMatchMode::default(),
        }
    }

    /// Set risk-free rate
    pub fn with_risk_free_rate(mut self, rate: f64) -> Self {
        self.risk_free_rate = rate;
        self
    }

    /// Set strike matching mode
    pub fn with_strike_match_mode(mut self, mode: StrikeMatchMode) -> Self {
        self.strike_match_mode = mode;
        self
    }
}

impl SelectionStrategy for DeltaStrategy {
    fn select_calendar_spread(
        &self,
        event: &EarningsEvent,
        _spot: &SpotPrice,
        chain_data: &OptionChainData,
        option_type: OptionType,
    ) -> Result<CalendarSpread, StrategyError> {
        // Get IV surface from chain data
        let iv_surface = chain_data
            .iv_surface
            .as_ref()
            .ok_or(StrategyError::NoDeltaData)?;

        // Build delta-parameterized surface
        let delta_surface = DeltaVolSurface::from_iv_surface(iv_surface, self.risk_free_rate);

        // Select expirations based on criteria
        let (short_exp, long_exp) = super::select_expirations(
            &chain_data.expirations,
            event.earnings_date,
            self.criteria.min_short_dte,
            self.criteria.max_short_dte,
            self.criteria.min_long_dte,
            self.criteria.max_long_dte,
        )?;

        // Determine target delta (fixed or via scanning)
        let target_delta = match self.scan_mode {
            DeltaScanMode::Fixed => self.target_delta,
            DeltaScanMode::Scan { steps } => {
                let config = OpportunityAnalyzerConfig {
                    min_iv_ratio: self.criteria.min_iv_ratio.unwrap_or(1.0),
                    delta_targets: linspace(self.delta_range.0, self.delta_range.1, steps),
                };
                let analyzer = OpportunityAnalyzer::new(config);
                let opportunities = analyzer.find_opportunities(&delta_surface, short_exp, long_exp);

                opportunities
                    .first()
                    .map(|opp| opp.target_delta)
                    .unwrap_or(self.target_delta)
            }
        };

        // For puts, use the equivalent put delta
        let is_call = option_type == OptionType::Call;

        // Map delta to theoretical strike for short leg
        let theoretical_short_strike = delta_surface
            .delta_to_strike(target_delta, short_exp, is_call)
            .ok_or(StrategyError::NoDeltaData)?;

        // Find closest tradable strike for short leg
        let short_strike = super::find_closest_strike(&chain_data.strikes, theoretical_short_strike)?;

        // Determine long leg strike based on matching mode
        let long_strike = match self.strike_match_mode {
            StrikeMatchMode::SameStrike => short_strike,
            StrikeMatchMode::SameDelta => {
                // Map same delta to strike using LONG expiration
                let theoretical_long_strike = delta_surface
                    .delta_to_strike(target_delta, long_exp, is_call)
                    .ok_or(StrategyError::NoDeltaData)?;
                super::find_closest_strike(&chain_data.strikes, theoretical_long_strike)?
            }
        };

        // Build spread
        let short_leg = OptionLeg::new(
            event.symbol.clone(),
            short_strike,
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{find_closest_strike, select_expirations};
    use crate::value_objects::Strike;
    use chrono::{DateTime, NaiveDate, Utc};
    use cs_analytics::{IVPoint, IVSurface};
    use rust_decimal::Decimal;

    fn create_test_chain_data() -> OptionChainData {
        let base_date = NaiveDate::from_ymd_opt(2025, 6, 20).unwrap();
        let now = DateTime::<Utc>::from_naive_utc_and_offset(
            base_date.and_hms_opt(10, 0, 0).unwrap(),
            Utc,
        );

        // Create IV surface
        let iv_points = vec![
            // Short expiry (7 days) - high IV for earnings
            IVPoint {
                strike: Decimal::new(95, 0),
                expiration: base_date + chrono::Duration::days(7),
                iv: 0.45,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250627C95".to_string(),
            },
            IVPoint {
                strike: Decimal::new(100, 0),
                expiration: base_date + chrono::Duration::days(7),
                iv: 0.40,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250627C100".to_string(),
            },
            IVPoint {
                strike: Decimal::new(105, 0),
                expiration: base_date + chrono::Duration::days(7),
                iv: 0.42,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250627C105".to_string(),
            },
            // Long expiry (30 days) - normal IV
            IVPoint {
                strike: Decimal::new(95, 0),
                expiration: base_date + chrono::Duration::days(30),
                iv: 0.32,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250720C95".to_string(),
            },
            IVPoint {
                strike: Decimal::new(100, 0),
                expiration: base_date + chrono::Duration::days(30),
                iv: 0.28,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250720C100".to_string(),
            },
            IVPoint {
                strike: Decimal::new(105, 0),
                expiration: base_date + chrono::Duration::days(30),
                iv: 0.30,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250720C105".to_string(),
            },
        ];

        let iv_surface = IVSurface::new(
            iv_points,
            "TEST".to_string(),
            now,
            Decimal::new(100, 0),
        );

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
            iv_surface: Some(iv_surface),
        }
    }

    #[test]
    fn test_delta_strategy_fixed_select() {
        let strategy = DeltaStrategy::fixed(0.50, TradeSelectionCriteria::default());
        let event = EarningsEvent::new(
            "TEST".to_string(),
            NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            crate::value_objects::EarningsTime::AfterMarketClose,
        );
        let spot = SpotPrice::new(Decimal::new(100, 0), Utc::now());
        let chain_data = create_test_chain_data();

        let result = strategy.select_calendar_spread(&event, &spot, &chain_data, OptionType::Call);
        assert!(result.is_ok(), "Strategy should select successfully: {:?}", result);

        let spread = result.unwrap();
        assert_eq!(spread.symbol(), "TEST");
        // 50 delta strike should be near ATM (100)
        let strike_f64: f64 = spread.strike().into();
        assert!((strike_f64 - 100.0).abs() <= 10.0, "Strike {} should be near ATM", strike_f64);
    }

    #[test]
    fn test_delta_strategy_scan_select() {
        let strategy = DeltaStrategy::scanning(
            (0.25, 0.75),
            5,
            TradeSelectionCriteria::default(),
        );
        let event = EarningsEvent::new(
            "TEST".to_string(),
            NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            crate::value_objects::EarningsTime::AfterMarketClose,
        );
        let spot = SpotPrice::new(Decimal::new(100, 0), Utc::now());
        let chain_data = create_test_chain_data();

        let result = strategy.select_calendar_spread(&event, &spot, &chain_data, OptionType::Call);
        assert!(result.is_ok(), "Scan strategy should select successfully: {:?}", result);
    }

    #[test]
    fn test_delta_strategy_no_iv_surface() {
        let strategy = DeltaStrategy::default();
        let event = EarningsEvent::new(
            "TEST".to_string(),
            NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            crate::value_objects::EarningsTime::AfterMarketClose,
        );
        let spot = SpotPrice::new(Decimal::new(100, 0), Utc::now());

        // Chain data without IV surface
        let chain_data = OptionChainData {
            expirations: vec![
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                NaiveDate::from_ymd_opt(2025, 7, 20).unwrap(),
            ],
            strikes: vec![
                Strike::new(Decimal::new(100, 0)).unwrap(),
            ],
            deltas: None,
            volumes: None,
            iv_ratios: None,
            iv_surface: None,
        };

        let result = strategy.select_calendar_spread(&event, &spot, &chain_data, OptionType::Call);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StrategyError::NoDeltaData));
    }

    #[test]
    fn test_find_closest_strike() {
        let strikes = vec![
            Strike::new(Decimal::new(95, 0)).unwrap(),
            Strike::new(Decimal::new(100, 0)).unwrap(),
            Strike::new(Decimal::new(105, 0)).unwrap(),
        ];

        let result = find_closest_strike(&strikes, 98.0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().value(), Decimal::new(100, 0));

        let result = find_closest_strike(&strikes, 102.5);
        assert!(result.is_ok());
        // 102.5 is equidistant but 100 should win (first encounter)
        let strike = result.unwrap();
        assert!(strike.value() == Decimal::new(100, 0) || strike.value() == Decimal::new(105, 0));
    }

    #[test]
    fn test_select_expirations() {
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
}
