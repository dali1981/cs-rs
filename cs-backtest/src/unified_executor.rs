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
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TradeStructure {
    CalendarSpread(OptionType),
    Straddle,
    CalendarStraddle,
    IronButterfly { wing_width: Decimal },
}

/// Unified result type for any trade
///
/// Each variant contains full trade data. The Failed variant contains only metadata
/// (no Strike, no prices), eliminating the need for dummy values.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "trade_type", rename_all = "snake_case")]
pub enum TradeResult {
    CalendarSpread(CalendarSpreadResult),
    Straddle(StraddleResult),
    CalendarStraddle(CalendarStraddleResult),
    IronButterfly(IronButterflyResult),
    Failed(FailedTrade),
}

/// A trade that failed before completion
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FailedTrade {
    pub symbol: String,
    pub earnings_date: chrono::NaiveDate,
    pub earnings_time: cs_domain::EarningsTime,
    pub trade_structure: TradeStructure,
    pub reason: cs_domain::FailureReason,
    pub phase: String,  // "selection", "entry_pricing", "exit_pricing", etc.
    pub details: Option<String>,
}

impl TradeResult {
    pub fn is_winner(&self) -> bool {
        match self {
            TradeResult::CalendarSpread(r) => r.is_winner(),
            TradeResult::IronButterfly(r) => r.is_winner(),
            TradeResult::Straddle(r) => r.is_winner(),
            TradeResult::CalendarStraddle(r) => r.is_winner(),
            TradeResult::Failed(_) => false,
        }
    }

    pub fn success(&self) -> bool {
        !matches!(self, TradeResult::Failed(_))
    }

    pub fn pnl(&self) -> Decimal {
        match self {
            TradeResult::CalendarSpread(r) => r.pnl,
            TradeResult::IronButterfly(r) => r.pnl,
            TradeResult::Straddle(r) => r.pnl,
            TradeResult::CalendarStraddle(r) => r.pnl,
            TradeResult::Failed(_) => Decimal::ZERO,
        }
    }

    pub fn pnl_pct(&self) -> Decimal {
        match self {
            TradeResult::CalendarSpread(r) => r.pnl_pct,
            TradeResult::IronButterfly(r) => r.pnl_pct,
            TradeResult::Straddle(r) => r.pnl_pct,
            TradeResult::CalendarStraddle(r) => r.pnl_pct,
            TradeResult::Failed(_) => Decimal::ZERO,
        }
    }

    pub fn symbol(&self) -> &str {
        match self {
            TradeResult::CalendarSpread(r) => &r.symbol,
            TradeResult::IronButterfly(r) => &r.symbol,
            TradeResult::Straddle(r) => &r.symbol,
            TradeResult::CalendarStraddle(r) => &r.symbol,
            TradeResult::Failed(f) => &f.symbol,
        }
    }

    pub fn option_type(&self) -> Option<OptionType> {
        match self {
            TradeResult::CalendarSpread(r) => Some(r.option_type),
            _ => None,
        }
    }

    pub fn strike(&self) -> Option<Strike> {
        match self {
            TradeResult::CalendarSpread(r) => Some(r.strike),
            TradeResult::IronButterfly(r) => Some(r.center_strike),
            TradeResult::Straddle(r) => Some(r.strike),
            TradeResult::CalendarStraddle(r) => Some(r.short_strike),
            TradeResult::Failed(_) => None,  // No strike for failed trades!
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

                        // Check if execution succeeded
                        if result.success {
                            TradeResult::CalendarSpread(result)
                        } else {
                            TradeResult::Failed(FailedTrade {
                                symbol: result.symbol,
                                earnings_date: result.earnings_date,
                                earnings_time: result.earnings_time,
                                trade_structure: structure,
                                reason: result.failure_reason.unwrap_or(cs_domain::FailureReason::PricingError("Unknown".to_string())),
                                phase: "execution".to_string(),
                                details: None,
                            })
                        }
                    }
                    Err(e) => {
                        TradeResult::Failed(FailedTrade {
                            symbol: event.symbol.clone(),
                            earnings_date: event.earnings_date,
                            earnings_time: event.earnings_time,
                            trade_structure: structure,
                            reason: cs_domain::FailureReason::PricingError(e.to_string()),
                            phase: "selection".to_string(),
                            details: Some(e.to_string()),
                        })
                    }
                }
            }
            TradeStructure::Straddle => {
                match selector.select_straddle(&spot, entry_surface, criteria.min_short_dte) {
                    Ok(straddle) => {
                        let result = self.straddle_executor
                            .execute_trade(&straddle, event, entry_time, exit_time)
                            .await;

                        if result.success {
                            TradeResult::Straddle(result)
                        } else {
                            TradeResult::Failed(FailedTrade {
                                symbol: result.symbol,
                                earnings_date: result.earnings_date,
                                earnings_time: result.earnings_time,
                                trade_structure: structure,
                                reason: result.failure_reason.unwrap_or(cs_domain::FailureReason::PricingError("Unknown".to_string())),
                                phase: "execution".to_string(),
                                details: None,
                            })
                        }
                    }
                    Err(e) => {
                        TradeResult::Failed(FailedTrade {
                            symbol: event.symbol.clone(),
                            earnings_date: event.earnings_date,
                            earnings_time: event.earnings_time,
                            trade_structure: structure,
                            reason: cs_domain::FailureReason::PricingError(e.to_string()),
                            phase: "selection".to_string(),
                            details: Some(e.to_string()),
                        })
                    }
                }
            }
            TradeStructure::CalendarStraddle => {
                match selector.select_calendar_straddle(&spot, entry_surface, criteria) {
                    Ok(cal_straddle) => {
                        let result = self.calendar_straddle_executor
                            .execute_trade(&cal_straddle, event, entry_time, exit_time)
                            .await;

                        if result.success {
                            TradeResult::CalendarStraddle(result)
                        } else {
                            TradeResult::Failed(FailedTrade {
                                symbol: result.symbol,
                                earnings_date: result.earnings_date,
                                earnings_time: result.earnings_time,
                                trade_structure: structure,
                                reason: result.failure_reason.unwrap_or(cs_domain::FailureReason::PricingError("Unknown".to_string())),
                                phase: "execution".to_string(),
                                details: None,
                            })
                        }
                    }
                    Err(e) => {
                        TradeResult::Failed(FailedTrade {
                            symbol: event.symbol.clone(),
                            earnings_date: event.earnings_date,
                            earnings_time: event.earnings_time,
                            trade_structure: structure,
                            reason: cs_domain::FailureReason::PricingError(e.to_string()),
                            phase: "selection".to_string(),
                            details: Some(e.to_string()),
                        })
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

                        if result.success {
                            TradeResult::IronButterfly(result)
                        } else {
                            TradeResult::Failed(FailedTrade {
                                symbol: result.symbol,
                                earnings_date: result.earnings_date,
                                earnings_time: result.earnings_time,
                                trade_structure: structure,
                                reason: result.failure_reason.unwrap_or(cs_domain::FailureReason::PricingError("Unknown".to_string())),
                                phase: "execution".to_string(),
                                details: None,
                            })
                        }
                    }
                    Err(e) => {
                        TradeResult::Failed(FailedTrade {
                            symbol: event.symbol.clone(),
                            earnings_date: event.earnings_date,
                            earnings_time: event.earnings_time,
                            trade_structure: structure,
                            reason: cs_domain::FailureReason::PricingError(e.to_string()),
                            phase: "selection".to_string(),
                            details: Some(e.to_string()),
                        })
                    }
                }
            }
        }
    }
}
