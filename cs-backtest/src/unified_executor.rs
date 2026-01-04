use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_analytics::{IVSurface, PricingModel};
use cs_domain::{
    CalendarSpread, CalendarSpreadResult, Straddle, StraddleResult,
    CalendarStraddle, CalendarStraddleResult, IronButterfly, IronButterflyResult,
    EarningsEvent, FailureReason, SpotPrice,
    EquityDataRepository, OptionsDataRepository, RepositoryError,
};
use cs_domain::strike_selection::{StrikeSelector, ExpirationCriteria, SelectionError};
use finq_core::OptionType;

use crate::iv_surface_builder::build_iv_surface_minute_aligned;
use crate::spread_pricer::{SpreadPricer, PricingError};
use crate::straddle_pricer::StraddlePricer;
use crate::calendar_straddle_pricer::CalendarStraddlePricer;
use crate::iron_butterfly_pricer::IronButterflyPricer;

/// Trade structure type - defines WHAT to trade
#[derive(Debug, Clone, Copy)]
pub enum TradeStructure {
    CalendarSpread(OptionType),
    Straddle,
    CalendarStraddle,
    IronButterfly { wing_width: Decimal },
}

/// Unified result type for any trade
#[derive(Debug)]
pub enum TradeResult {
    CalendarSpread(CalendarSpreadResult),
    Straddle(StraddleResult),
    CalendarStraddle(CalendarStraddleResult),
    IronButterfly(IronButterflyResult),
}

/// Error type for trade execution
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
    #[error("Pricing error: {0}")]
    Pricing(#[from] PricingError),
    #[error("Selection error: {0}")]
    Selection(#[from] SelectionError),
    #[error("No spot price available")]
    NoSpotPrice,
    #[error("Invalid spread: {0}")]
    InvalidSpread(String),
}

/// Unified trade executor for all trade types
///
/// Key optimization: Accepts pre-built entry_surface to avoid redundant builds.
/// The entry surface is built once in process_event() and reused for both
/// selection AND entry pricing.
pub struct UnifiedTradeExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    pricing_model: PricingModel,
    max_entry_iv: Option<f64>,
}

impl<O, E> UnifiedTradeExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(options_repo: Arc<O>, equity_repo: Arc<E>) -> Self {
        Self {
            options_repo,
            equity_repo,
            pricing_model: PricingModel::StickyStrike,
            max_entry_iv: None,
        }
    }

    pub fn with_pricing_model(mut self, model: PricingModel) -> Self {
        self.pricing_model = model;
        self
    }

    pub fn with_max_entry_iv(mut self, max_iv: Option<f64>) -> Self {
        self.max_entry_iv = max_iv;
        self
    }

    /// Execute any trade type
    ///
    /// IMPORTANT: entry_surface is passed in to avoid rebuilding.
    /// It was already built for selection and is reused for entry pricing.
    pub async fn execute<S: StrikeSelector>(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        entry_surface: &IVSurface,  // Passed in, already built
        selector: &S,
        structure: TradeStructure,
        criteria: &ExpirationCriteria,
    ) -> TradeResult {
        match self.try_execute(event, entry_time, exit_time, entry_surface, selector, structure, criteria).await {
            Ok(result) => result,
            Err(e) => self.create_failed_result(event, entry_time, exit_time, structure, e),
        }
    }

    async fn try_execute<S: StrikeSelector>(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        entry_surface: &IVSurface,
        selector: &S,
        structure: TradeStructure,
        criteria: &ExpirationCriteria,
    ) -> Result<TradeResult, ExecutionError> {
        let spot = SpotPrice::new(entry_surface.spot_price(), entry_time);

        // Select trade using the SAME surface used for pricing
        let trade_entity = match structure {
            TradeStructure::CalendarSpread(option_type) => {
                let spread = selector.select_calendar_spread(&spot, entry_surface, option_type, criteria)?;
                TradeEntity::CalendarSpread(spread)
            }
            TradeStructure::Straddle => {
                let straddle = selector.select_straddle(&spot, entry_surface, criteria.min_short_dte)?;
                TradeEntity::Straddle(straddle)
            }
            TradeStructure::CalendarStraddle => {
                let cal_straddle = selector.select_calendar_straddle(&spot, entry_surface, criteria)?;
                TradeEntity::CalendarStraddle(cal_straddle)
            }
            TradeStructure::IronButterfly { wing_width } => {
                let butterfly = selector.select_iron_butterfly(
                    &spot,
                    entry_surface,
                    wing_width,
                    criteria.min_short_dte,
                    criteria.max_short_dte,
                )?;
                TradeEntity::IronButterfly(butterfly)
            }
        };

        // Get entry spot price
        let entry_spot = self.equity_repo
            .get_spot_price(event.symbol(), entry_time)
            .await?;

        // Get exit spot price
        let exit_spot = self.equity_repo
            .get_spot_price(event.symbol(), exit_time)
            .await?;

        // Get option chain data for entry and exit
        let entry_chain = self.options_repo
            .get_option_bars_at_time(event.symbol(), entry_time)
            .await?;

        let (exit_chain, _exit_surface_time) = self.options_repo
            .get_option_bars_at_or_after_time(event.symbol(), exit_time, 30)
            .await?;

        // Build exit surface (different time, must build)
        let exit_surface = build_iv_surface_minute_aligned(
            &exit_chain,
            self.equity_repo.as_ref(),
            event.symbol(),
        ).await;

        // Price and validate based on trade type
        match trade_entity {
            TradeEntity::CalendarSpread(spread) => {
                let pricer = SpreadPricer::new().with_pricing_model(self.pricing_model);

                let entry_pricing = pricer.price_spread_with_surface(
                    &spread,
                    &entry_chain,
                    entry_spot.to_f64(),
                    entry_time,
                    Some(entry_surface),
                )?;

                self.validate_entry_iv(&entry_pricing.short_leg.iv, &entry_pricing.long_leg.iv)?;
                self.validate_calendar_entry_cost(entry_pricing.net_cost)?;

                let exit_pricing = pricer.price_spread_with_surface(
                    &spread,
                    &exit_chain,
                    exit_spot.to_f64(),
                    exit_time,
                    exit_surface.as_ref(),
                )?;

                let result = CalendarSpreadResult::from_pricing(
                    spread,
                    event.clone(),
                    entry_pricing,
                    exit_pricing,
                    entry_spot,
                    exit_spot,
                    entry_time,
                    exit_time,
                );

                Ok(TradeResult::CalendarSpread(result))
            }
            TradeEntity::Straddle(straddle) => {
                let spread_pricer = SpreadPricer::new().with_pricing_model(self.pricing_model);
                let pricer = StraddlePricer::new(spread_pricer);

                let entry_pricing = pricer.price_straddle_with_surface(
                    &straddle,
                    &entry_chain,
                    entry_spot.to_f64(),
                    entry_time,
                    Some(entry_surface),
                )?;

                self.validate_straddle_entry_cost(entry_pricing.total_cost)?;

                let exit_pricing = pricer.price_straddle_with_surface(
                    &straddle,
                    &exit_chain,
                    exit_spot.to_f64(),
                    exit_time,
                    exit_surface.as_ref(),
                )?;

                let result = StraddleResult::from_pricing(
                    straddle,
                    event.clone(),
                    entry_pricing,
                    exit_pricing,
                    entry_spot,
                    exit_spot,
                    entry_time,
                    exit_time,
                );

                Ok(TradeResult::Straddle(result))
            }
            TradeEntity::CalendarStraddle(cal_straddle) => {
                let spread_pricer = SpreadPricer::new().with_pricing_model(self.pricing_model);
                let pricer = CalendarStraddlePricer::new(spread_pricer);

                let entry_pricing = pricer.price_calendar_straddle_with_surface(
                    &cal_straddle,
                    &entry_chain,
                    entry_spot.to_f64(),
                    entry_time,
                    Some(entry_surface),
                )?;

                self.validate_calendar_entry_cost(entry_pricing.net_cost)?;

                let exit_pricing = pricer.price_calendar_straddle_with_surface(
                    &cal_straddle,
                    &exit_chain,
                    exit_spot.to_f64(),
                    exit_time,
                    exit_surface.as_ref(),
                )?;

                let result = CalendarStraddleResult::from_pricing(
                    cal_straddle,
                    event.clone(),
                    entry_pricing,
                    exit_pricing,
                    entry_spot,
                    exit_spot,
                    entry_time,
                    exit_time,
                );

                Ok(TradeResult::CalendarStraddle(result))
            }
            TradeEntity::IronButterfly(butterfly) => {
                let spread_pricer = SpreadPricer::new().with_pricing_model(self.pricing_model);
                let pricer = IronButterflyPricer::new(spread_pricer);

                let entry_pricing = pricer.price_iron_butterfly_with_surface(
                    &butterfly,
                    &entry_chain,
                    entry_spot.to_f64(),
                    entry_time,
                    Some(entry_surface),
                )?;

                // Iron butterfly is a credit spread (net credit at entry)
                if entry_pricing.net_credit <= Decimal::ZERO {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Expected net credit, got debit: {}",
                        entry_pricing.net_credit
                    )));
                }

                let exit_pricing = pricer.price_iron_butterfly_with_surface(
                    &butterfly,
                    &exit_chain,
                    exit_spot.to_f64(),
                    exit_time,
                    exit_surface.as_ref(),
                )?;

                let result = IronButterflyResult::from_pricing(
                    butterfly,
                    event.clone(),
                    entry_pricing,
                    exit_pricing,
                    entry_spot,
                    exit_spot,
                    entry_time,
                    exit_time,
                );

                Ok(TradeResult::IronButterfly(result))
            }
        }
    }

    fn validate_entry_iv(&self, short_iv: &Option<f64>, long_iv: &Option<f64>) -> Result<(), ExecutionError> {
        if let Some(max_iv) = self.max_entry_iv {
            if let Some(iv) = short_iv {
                if *iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Short leg IV too high: {:.1}% > {:.1}%",
                        iv * 100.0,
                        max_iv * 100.0,
                    )));
                }
            }
            if let Some(iv) = long_iv {
                if *iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Long leg IV too high: {:.1}% > {:.1}%",
                        iv * 100.0,
                        max_iv * 100.0,
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_calendar_entry_cost(&self, net_cost: Decimal) -> Result<(), ExecutionError> {
        if net_cost <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(format!(
                "Negative entry cost: {}",
                net_cost
            )));
        }

        let min_entry_cost = Decimal::new(5, 2); // $0.05
        if net_cost < min_entry_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Entry cost too small: {} < {}",
                net_cost,
                min_entry_cost
            )));
        }

        Ok(())
    }

    fn validate_straddle_entry_cost(&self, total_cost: Decimal) -> Result<(), ExecutionError> {
        if total_cost <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(format!(
                "Invalid straddle cost: {}",
                total_cost
            )));
        }

        let min_cost = Decimal::new(10, 2); // $0.10
        if total_cost < min_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Straddle cost too small: {} < {}",
                total_cost,
                min_cost
            )));
        }

        Ok(())
    }

    fn create_failed_result(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        structure: TradeStructure,
        error: ExecutionError,
    ) -> TradeResult {
        let reason = match error {
            ExecutionError::Repository(ref e) => format!("Repository error: {}", e),
            ExecutionError::Pricing(ref e) => format!("Pricing error: {}", e),
            ExecutionError::Selection(ref e) => format!("Selection error: {}", e),
            ExecutionError::NoSpotPrice => "No spot price available".to_string(),
            ExecutionError::InvalidSpread(ref msg) => msg.clone(),
        };

        let failure_reason = FailureReason::Other(reason);

        match structure {
            TradeStructure::CalendarSpread(_) => {
                TradeResult::CalendarSpread(CalendarSpreadResult::failed(
                    event.clone(),
                    entry_time,
                    exit_time,
                    failure_reason,
                ))
            }
            TradeStructure::Straddle => {
                TradeResult::Straddle(StraddleResult::failed(
                    event.clone(),
                    entry_time,
                    exit_time,
                    failure_reason,
                ))
            }
            TradeStructure::CalendarStraddle => {
                TradeResult::CalendarStraddle(CalendarStraddleResult::failed(
                    event.clone(),
                    entry_time,
                    exit_time,
                    failure_reason,
                ))
            }
            TradeStructure::IronButterfly { .. } => {
                TradeResult::IronButterfly(IronButterflyResult::failed(
                    event.clone(),
                    entry_time,
                    exit_time,
                    failure_reason,
                ))
            }
        }
    }
}

/// Internal enum to hold selected trade entity
enum TradeEntity {
    CalendarSpread(CalendarSpread),
    Straddle(Straddle),
    CalendarStraddle(CalendarStraddle),
    IronButterfly(IronButterfly),
}
