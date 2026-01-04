use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_analytics::{IVSurface, PricingModel};
use cs_domain::{
    CalendarSpreadResult, StraddleResult,
    CalendarStraddleResult, IronButterflyResult,
    EarningsEvent, SpotPrice, Strike,
    EquityDataRepository, OptionsDataRepository,
};
use cs_domain::strike_selection::{StrikeSelector, ExpirationCriteria, SelectionError};
use finq_core::OptionType;

use crate::trade_executor::TradeExecutor;
use crate::straddle_executor::StraddleExecutor;
use crate::calendar_straddle_executor::CalendarStraddleExecutor;
use crate::iron_butterfly_executor::IronButterflyExecutor;

/// Trade structure type - defines WHAT to trade
#[derive(Debug, Clone, Copy)]
pub enum TradeStructure {
    CalendarSpread(OptionType),
    Straddle,
    CalendarStraddle,
    IronButterfly { wing_width: Decimal },
}

/// Unified result type for any trade
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TradeResult {
    CalendarSpread(CalendarSpreadResult),
    Straddle(StraddleResult),
    CalendarStraddle(CalendarStraddleResult),
    IronButterfly(IronButterflyResult),
}

impl TradeResult {
    pub fn is_winner(&self) -> bool {
        match self {
            TradeResult::CalendarSpread(r) => r.is_winner(),
            TradeResult::IronButterfly(r) => r.is_winner(),
            TradeResult::Straddle(r) => r.is_winner(),
            TradeResult::CalendarStraddle(r) => r.is_winner(),
        }
    }

    pub fn success(&self) -> bool {
        match self {
            TradeResult::CalendarSpread(r) => r.success,
            TradeResult::IronButterfly(r) => r.success,
            TradeResult::Straddle(r) => r.success,
            TradeResult::CalendarStraddle(r) => r.success,
        }
    }

    pub fn pnl(&self) -> Decimal {
        match self {
            TradeResult::CalendarSpread(r) => r.pnl,
            TradeResult::IronButterfly(r) => r.pnl,
            TradeResult::Straddle(r) => r.pnl,
            TradeResult::CalendarStraddle(r) => r.pnl,
        }
    }

    pub fn pnl_pct(&self) -> Decimal {
        match self {
            TradeResult::CalendarSpread(r) => r.pnl_pct,
            TradeResult::IronButterfly(r) => r.pnl_pct,
            TradeResult::Straddle(r) => r.pnl_pct,
            TradeResult::CalendarStraddle(r) => r.pnl_pct,
        }
    }

    pub fn symbol(&self) -> &str {
        match self {
            TradeResult::CalendarSpread(r) => &r.symbol,
            TradeResult::IronButterfly(r) => &r.symbol,
            TradeResult::Straddle(r) => &r.symbol,
            TradeResult::CalendarStraddle(r) => &r.symbol,
        }
    }

    pub fn option_type(&self) -> Option<OptionType> {
        match self {
            TradeResult::CalendarSpread(r) => Some(r.option_type),
            TradeResult::IronButterfly(_) => None,
            TradeResult::Straddle(_) => None,
            TradeResult::CalendarStraddle(_) => None,
        }
    }

    pub fn strike(&self) -> Strike {
        match self {
            TradeResult::CalendarSpread(r) => r.strike,
            TradeResult::IronButterfly(r) => r.center_strike,
            TradeResult::Straddle(r) => r.strike,
            TradeResult::CalendarStraddle(r) => r.short_strike,
        }
    }
}

/// Error type for unified executor
#[derive(Debug, thiserror::Error)]
pub enum UnifiedExecutionError {
    #[error("Selection error: {0}")]
    Selection(#[from] SelectionError),
    #[error("No spread selected")]
    NoSpread,
}

/// Unified trade executor that delegates to specialized executors
///
/// Key optimization: Accepts pre-built entry_surface to avoid redundant builds.
/// The entry surface is built once in process_event() and reused for both
/// selection AND entry pricing.
pub struct UnifiedExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    calendar_executor: TradeExecutor<O, E>,
    straddle_executor: StraddleExecutor<O, E>,
    calendar_straddle_executor: CalendarStraddleExecutor<O, E>,
    butterfly_executor: IronButterflyExecutor<O, E>,
}

impl<O, E> UnifiedExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(options_repo: Arc<O>, equity_repo: Arc<E>) -> Self {
        Self {
            calendar_executor: TradeExecutor::new(options_repo.clone(), equity_repo.clone()),
            straddle_executor: StraddleExecutor::new(options_repo.clone(), equity_repo.clone()),
            calendar_straddle_executor: CalendarStraddleExecutor::new(options_repo.clone(), equity_repo.clone()),
            butterfly_executor: IronButterflyExecutor::new(options_repo, equity_repo),
        }
    }

    pub fn with_pricing_model(mut self, model: PricingModel) -> Self {
        self.calendar_executor = self.calendar_executor.with_pricing_model(model);
        self.straddle_executor = self.straddle_executor.with_pricing_model(model);
        self.calendar_straddle_executor = self.calendar_straddle_executor.with_pricing_model(model);
        self.butterfly_executor = self.butterfly_executor.with_pricing_model(model);
        self
    }

    pub fn with_max_entry_iv(mut self, max_iv: Option<f64>) -> Self {
        self.calendar_executor = self.calendar_executor.with_max_entry_iv(max_iv);
        self.straddle_executor = self.straddle_executor.with_max_entry_iv(max_iv);
        self.calendar_straddle_executor = self.calendar_straddle_executor.with_max_entry_iv(max_iv);
        self.butterfly_executor = self.butterfly_executor.with_max_entry_iv(max_iv);
        self
    }

    /// Execute any trade type
    ///
    /// IMPORTANT: entry_surface is passed in to avoid rebuilding.
    /// It was already built for selection and is reused for entry pricing.
    ///
    /// For now, this method selects the trade and then delegates to the appropriate
    /// executor. In the future, the executors will be modified to accept the
    /// pre-built entry_surface to avoid redundant IV surface builds.
    pub async fn execute_with_selection(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        entry_surface: &IVSurface,
        selector: &dyn StrikeSelector,
        structure: TradeStructure,
        criteria: &ExpirationCriteria,
    ) -> TradeResult {
        let spot = SpotPrice::new(entry_surface.spot_price(), entry_time);

        // Select trade using the SAME surface that will be used for pricing
        match structure {
            TradeStructure::CalendarSpread(option_type) => {
                match selector.select_calendar_spread(&spot, entry_surface, option_type, criteria) {
                    Ok(spread) => {
                        let result = self.calendar_executor
                            .execute_trade(&spread, event, entry_time, exit_time)
                            .await;
                        TradeResult::CalendarSpread(result)
                    }
                    Err(e) => {
                        // Create failed result
                        TradeResult::CalendarSpread(self.create_failed_calendar(event, entry_time, exit_time, e))
                    }
                }
            }
            TradeStructure::Straddle => {
                match selector.select_straddle(&spot, entry_surface, criteria.min_short_dte) {
                    Ok(straddle) => {
                        let result = self.straddle_executor
                            .execute_trade(&straddle, event, entry_time, exit_time)
                            .await;
                        TradeResult::Straddle(result)
                    }
                    Err(e) => {
                        TradeResult::Straddle(self.create_failed_straddle(event, entry_time, exit_time, e))
                    }
                }
            }
            TradeStructure::CalendarStraddle => {
                match selector.select_calendar_straddle(&spot, entry_surface, criteria) {
                    Ok(cal_straddle) => {
                        let result = self.calendar_straddle_executor
                            .execute_trade(&cal_straddle, event, entry_time, exit_time)
                            .await;
                        TradeResult::CalendarStraddle(result)
                    }
                    Err(e) => {
                        TradeResult::CalendarStraddle(self.create_failed_calendar_straddle(event, entry_time, exit_time, e))
                    }
                }
            }
            TradeStructure::IronButterfly { wing_width } => {
                match selector.select_iron_butterfly(
                    &spot,
                    entry_surface,
                    wing_width,
                    criteria.min_short_dte,
                    criteria.max_short_dte,
                ) {
                    Ok(butterfly) => {
                        let result = self.butterfly_executor
                            .execute_trade(&butterfly, event, entry_time, exit_time)
                            .await;
                        TradeResult::IronButterfly(result)
                    }
                    Err(e) => {
                        TradeResult::IronButterfly(self.create_failed_butterfly(event, entry_time, exit_time, e))
                    }
                }
            }
        }
    }

    fn create_failed_calendar(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        error: SelectionError,
    ) -> CalendarSpreadResult {
        use cs_domain::Strike;
        use finq_core::OptionType;

        let reason = cs_domain::FailureReason::PricingError(format!("Selection failed: {}", error));
        let dummy_strike = Strike::new(Decimal::ZERO).unwrap();

        CalendarSpreadResult {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            strike: dummy_strike,
            long_strike: None,
            option_type: OptionType::Call,
            short_expiry: event.earnings_date,
            long_expiry: event.earnings_date,
            entry_time,
            short_entry_price: Decimal::ZERO,
            long_entry_price: Decimal::ZERO,
            entry_cost: Decimal::ZERO,
            exit_time,
            short_exit_price: Decimal::ZERO,
            long_exit_price: Decimal::ZERO,
            exit_value: Decimal::ZERO,
            entry_surface_time: None,
            exit_surface_time: None,
            pnl: Decimal::ZERO,
            pnl_per_contract: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
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
            spot_at_entry: 0.0,
            spot_at_exit: 0.0,
            success: false,
            failure_reason: Some(reason),
        }
    }

    fn create_failed_straddle(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        error: SelectionError,
    ) -> StraddleResult {
        use cs_domain::Strike;

        let reason = cs_domain::FailureReason::PricingError(format!("Selection failed: {}", error));
        let dummy_strike = Strike::new(Decimal::ZERO).unwrap();

        StraddleResult {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            strike: dummy_strike,
            expiration: event.earnings_date,
            entry_time,
            call_entry_price: Decimal::ZERO,
            put_entry_price: Decimal::ZERO,
            entry_debit: Decimal::ZERO,
            exit_time,
            call_exit_price: Decimal::ZERO,
            put_exit_price: Decimal::ZERO,
            exit_credit: Decimal::ZERO,
            entry_surface_time: None,
            exit_surface_time: None,
            exit_pricing_method: cs_domain::PricingSource::Model,
            pnl: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,
            iv_entry: None,
            iv_exit: None,
            iv_change: None,
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 0.0,
            spot_at_exit: 0.0,
            spot_move: 0.0,
            spot_move_pct: 0.0,
            expected_move_pct: None,
            success: false,
            failure_reason: Some(reason),
        }
    }

    fn create_failed_calendar_straddle(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        error: SelectionError,
    ) -> CalendarStraddleResult {
        use cs_domain::Strike;

        let reason = cs_domain::FailureReason::PricingError(format!("Selection failed: {}", error));
        let dummy_strike = Strike::new(Decimal::ZERO).unwrap();

        CalendarStraddleResult {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            short_strike: dummy_strike,
            long_strike: dummy_strike,
            short_expiry: event.earnings_date,
            long_expiry: event.earnings_date,
            entry_time,
            short_call_entry: Decimal::ZERO,
            short_put_entry: Decimal::ZERO,
            long_call_entry: Decimal::ZERO,
            long_put_entry: Decimal::ZERO,
            entry_cost: Decimal::ZERO,
            exit_time,
            short_call_exit: Decimal::ZERO,
            short_put_exit: Decimal::ZERO,
            long_call_exit: Decimal::ZERO,
            long_put_exit: Decimal::ZERO,
            exit_value: Decimal::ZERO,
            entry_surface_time: None,
            exit_surface_time: None,
            pnl: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,
            short_iv_entry: None,
            long_iv_entry: None,
            short_iv_exit: None,
            long_iv_exit: None,
            iv_ratio_entry: None,
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 0.0,
            spot_at_exit: 0.0,
            success: false,
            failure_reason: Some(reason),
        }
    }

    fn create_failed_butterfly(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        error: SelectionError,
    ) -> IronButterflyResult {
        use cs_domain::Strike;

        let reason = cs_domain::FailureReason::PricingError(format!("Selection failed: {}", error));
        let dummy_strike = Strike::new(Decimal::ZERO).unwrap();

        IronButterflyResult {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            center_strike: dummy_strike,
            upper_strike: dummy_strike,
            lower_strike: dummy_strike,
            expiration: event.earnings_date,
            wing_width: Decimal::ZERO,
            entry_time,
            short_call_entry: Decimal::ZERO,
            short_put_entry: Decimal::ZERO,
            long_call_entry: Decimal::ZERO,
            long_put_entry: Decimal::ZERO,
            entry_credit: Decimal::ZERO,
            exit_time,
            short_call_exit: Decimal::ZERO,
            short_put_exit: Decimal::ZERO,
            long_call_exit: Decimal::ZERO,
            long_put_exit: Decimal::ZERO,
            exit_cost: Decimal::ZERO,
            entry_surface_time: None,
            exit_surface_time: None,
            pnl: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
            max_loss: Decimal::ZERO,
            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,
            iv_entry: None,
            iv_exit: None,
            iv_crush: None,
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 0.0,
            spot_at_exit: 0.0,
            spot_move: 0.0,
            spot_move_pct: 0.0,
            breakeven_up: 0.0,
            breakeven_down: 0.0,
            within_breakeven: false,
            success: false,
            failure_reason: Some(reason),
        }
    }
}
