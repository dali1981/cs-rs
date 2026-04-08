use cs_domain::entities::{
    CalendarSpread, CalendarStraddle, IronButterfly, LongIronButterfly, LongStraddle, OptionLeg,
    ShortStraddle,
};
use cs_domain::strike_selection::{
    find_closest_strike, select_expirations, ExpirationCriteria, SelectionError,
    TradeSelectionCriteria, StrikeMatchMode,
};
use cs_domain::value_objects::SpotPrice;
use super::{ATMStrategy, StrikeSelector};
use chrono::NaiveDate;
use cs_analytics::{DeltaVolSurface, IVSurface, OpportunityAnalyzer, OpportunityAnalyzerConfig, linspace};
use finq_core::OptionType;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

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
    pub criteria: TradeSelectionCriteria,
    pub target_delta: f64,
    pub risk_free_rate: f64,
    pub scan_mode: DeltaScanMode,
    pub delta_range: (f64, f64),
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

    pub fn scanning(delta_range: (f64, f64), steps: usize, criteria: TradeSelectionCriteria) -> Self {
        Self {
            criteria,
            target_delta: 0.50,
            risk_free_rate: 0.05,
            scan_mode: DeltaScanMode::Scan { steps },
            delta_range,
            strike_match_mode: StrikeMatchMode::default(),
        }
    }

    pub fn with_risk_free_rate(mut self, rate: f64) -> Self {
        self.risk_free_rate = rate;
        self
    }

    pub fn with_strike_match_mode(mut self, mode: StrikeMatchMode) -> Self {
        self.strike_match_mode = mode;
        self
    }
}

impl StrikeSelector for DeltaStrategy {
    fn select_calendar_spread(
        &self,
        _spot: &SpotPrice,
        surface: &IVSurface,
        option_type: OptionType,
        criteria: &ExpirationCriteria,
    ) -> Result<CalendarSpread, SelectionError> {
        tracing::debug!(
            symbol = surface.underlying(),
            surface_points = surface.points().len(),
            option_type = ?option_type,
            target_delta = self.target_delta,
            "Delta selection: starting calendar spread selection"
        );

        let delta_surface = DeltaVolSurface::from_iv_surface(surface, self.risk_free_rate);
        let expirations = surface.expirations();

        let (short_exp, long_exp) = select_expirations(
            &expirations,
            surface.as_of_time().date_naive(),
            criteria.min_short_dte,
            criteria.max_short_dte,
            criteria.min_long_dte,
            criteria.max_long_dte,
        )
        .map_err(|e| {
            tracing::warn!(symbol = surface.underlying(), error = %e, "Delta selection: expiration selection failed");
            e
        })?;

        tracing::debug!(
            symbol = surface.underlying(),
            short_exp = %short_exp,
            long_exp = %long_exp,
            "Delta selection: selected expirations"
        );

        let target_delta = match self.scan_mode {
            DeltaScanMode::Fixed => self.target_delta,
            DeltaScanMode::Scan { steps } => {
                let config = OpportunityAnalyzerConfig {
                    min_iv_ratio: self.criteria.min_iv_ratio.unwrap_or(1.0),
                    delta_targets: linspace(self.delta_range.0, self.delta_range.1, steps),
                };
                let analyzer = OpportunityAnalyzer::new(config);
                let opportunities =
                    analyzer.find_opportunities(&delta_surface, short_exp, long_exp);
                opportunities
                    .first()
                    .map(|opp| opp.target_delta)
                    .unwrap_or(self.target_delta)
            }
        };

        let is_call = option_type == OptionType::Call;

        let theoretical_short_strike = delta_surface
            .delta_to_strike(target_delta, short_exp, is_call)
            .ok_or(SelectionError::NoIVSurface)?;

        let strikes: Vec<cs_domain::value_objects::Strike> = surface
            .strikes()
            .iter()
            .filter_map(|&s| cs_domain::value_objects::Strike::new(s).ok())
            .collect();

        let short_strike = find_closest_strike(&strikes, theoretical_short_strike)?;

        let long_strike = match self.strike_match_mode {
            StrikeMatchMode::SameStrike => short_strike,
            StrikeMatchMode::SameDelta => {
                let theoretical_long_strike = delta_surface
                    .delta_to_strike(target_delta, long_exp, is_call)
                    .ok_or(SelectionError::NoIVSurface)?;
                find_closest_strike(&strikes, theoretical_long_strike)?
            }
        };

        let symbol = surface.underlying().to_string();
        let short_leg = OptionLeg::new(symbol.clone(), short_strike, short_exp, option_type);
        let long_leg = OptionLeg::new(symbol, long_strike, long_exp, option_type);

        CalendarSpread::new(short_leg, long_leg).map_err(Into::into)
    }

    fn select_long_straddle(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        min_expiration: NaiveDate,
    ) -> Result<LongStraddle, SelectionError> {
        let atm_strategy = ATMStrategy::new(self.criteria.clone());
        StrikeSelector::select_long_straddle(&atm_strategy, spot, surface, min_expiration)
    }

    fn select_short_straddle(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        min_expiration: NaiveDate,
    ) -> Result<ShortStraddle, SelectionError> {
        let atm_strategy = ATMStrategy::new(self.criteria.clone());
        StrikeSelector::select_short_straddle(&atm_strategy, spot, surface, min_expiration)
    }

    fn select_calendar_straddle(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        criteria: &ExpirationCriteria,
    ) -> Result<CalendarStraddle, SelectionError> {
        let atm_strategy = ATMStrategy::new(self.criteria.clone());
        StrikeSelector::select_calendar_straddle(&atm_strategy, spot, surface, criteria)
    }

    fn select_iron_butterfly(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        wing_width: Decimal,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<IronButterfly, SelectionError> {
        let atm_strategy = ATMStrategy::new(self.criteria.clone());
        StrikeSelector::select_iron_butterfly(&atm_strategy, spot, surface, wing_width, min_dte, max_dte)
    }

    fn select_long_iron_butterfly(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        wing_width: Decimal,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<LongIronButterfly, SelectionError> {
        let atm_strategy = ATMStrategy::new(self.criteria.clone());
        StrikeSelector::select_long_iron_butterfly(
            &atm_strategy,
            spot,
            surface,
            wing_width,
            min_dte,
            max_dte,
        )
    }
}
