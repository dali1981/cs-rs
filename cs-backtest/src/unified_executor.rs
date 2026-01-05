use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_analytics::{IVSurface, PricingModel};
use cs_domain::{
    CalendarSpreadResult, StraddleResult,
    CalendarStraddleResult, IronButterflyResult,
    EarningsEvent, SpotPrice, Strike, Straddle,
    EquityDataRepository, OptionsDataRepository,
};
use cs_domain::strike_selection::{StrikeSelector, ExpirationCriteria, SelectionError};
use finq_core::OptionType;

use crate::trade_executor::TradeExecutor;
use crate::straddle_executor::StraddleExecutor;
use crate::calendar_straddle_executor::CalendarStraddleExecutor;
use crate::iron_butterfly_executor::IronButterflyExecutor;
use crate::hedging_executor::HedgingExecutor;
use crate::timing_strategy::TimingStrategy;

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

    /// Get hedge P&L if hedging was applied
    pub fn hedge_pnl(&self) -> Option<Decimal> {
        match self {
            TradeResult::Straddle(r) => r.hedge_pnl,
            _ => None,
        }
    }

    /// Get total P&L including hedge if hedging was applied
    pub fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        match self {
            TradeResult::Straddle(r) => r.total_pnl_with_hedge,
            _ => None,
        }
    }

    /// Check if this trade has hedging data
    pub fn has_hedge_data(&self) -> bool {
        match self {
            TradeResult::Straddle(r) => r.hedge_position.is_some(),
            _ => false,
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
    equity_repo: Arc<E>,
    hedge_config: cs_domain::HedgeConfig,
    timing_strategy: Option<TimingStrategy>,
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
            butterfly_executor: IronButterflyExecutor::new(options_repo, equity_repo.clone()),
            equity_repo,
            hedge_config: cs_domain::HedgeConfig::default(),
            timing_strategy: None,
        }
    }

    pub fn with_hedge_config(mut self, hedge_config: cs_domain::HedgeConfig) -> Self {
        self.hedge_config = hedge_config;
        self
    }

    pub fn with_timing_strategy(mut self, timing_strategy: TimingStrategy) -> Self {
        self.timing_strategy = Some(timing_strategy);
        self
    }

    /// Apply delta hedging to a straddle result
    async fn apply_hedging(
        &self,
        result: &mut StraddleResult,
        straddle: &cs_domain::Straddle,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
    ) {
        use cs_domain::HedgeState;

        // Initialize hedge state from option position at entry
        let mut hedge_state = HedgeState::new(
            self.hedge_config.clone(),
            result.net_delta.unwrap_or(0.0),
            result.net_gamma.unwrap_or(0.0),
            result.spot_at_entry,
        );

        // Process each rehedge time - state machine handles all logic
        for rehedge_time in rehedge_times {
            // Check max rehedges limit
            if hedge_state.at_max_rehedges() {
                break;
            }

            // Get spot price at rehedge time
            if let Ok(spot) = self.equity_repo.get_spot_price(straddle.symbol(), rehedge_time).await {
                // Update state - will hedge if needed
                hedge_state.update(rehedge_time, spot.to_f64());
            }
        }

        // Finalize hedge state and calculate P&L
        let hedge_position = hedge_state.finalize(result.spot_at_exit);

        // Calculate hedge P&L if any hedges were performed
        if hedge_position.rehedge_count() > 0 {
            let hedge_pnl = hedge_position.calculate_pnl(result.spot_at_exit);
            let total_pnl = result.pnl + hedge_pnl - hedge_position.total_cost;

            result.hedge_position = Some(hedge_position);
            result.hedge_pnl = Some(hedge_pnl);
            result.total_pnl_with_hedge = Some(total_pnl);
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
                // Use exit date as minimum expiration - options must expire AFTER we exit
                let min_expiration = exit_time.date_naive();

                match selector.select_straddle(&spot, entry_surface, min_expiration) {
                    Ok(straddle) => {
                        // Execute trade (with hedging if enabled)
                        let result = if self.hedge_config.is_enabled() && self.timing_strategy.is_some() {
                            // Generate rehedge times
                            let rehedge_times = self.timing_strategy.as_ref().unwrap()
                                .rehedge_times(entry_time, exit_time, &self.hedge_config.strategy);

                            // Execute base trade first
                            let mut base_result = self.straddle_executor
                                .execute_trade(&straddle, event, entry_time, exit_time)
                                .await;

                            // If successful, apply hedging
                            if base_result.success {
                                self.apply_hedging(&mut base_result, &straddle, entry_time, exit_time, rehedge_times).await;
                            }

                            base_result
                        } else {
                            // Execute without hedging
                            self.straddle_executor
                                .execute_trade(&straddle, event, entry_time, exit_time)
                                .await
                        };

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

    /// Execute a pre-built straddle directly (for rolling strategies)
    ///
    /// This method bypasses the selection process and executes a pre-constructed
    /// straddle trade. If hedging is configured, it will be applied automatically.
    pub async fn execute_straddle(
        &self,
        straddle: &Straddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> StraddleResult {
        // Execute trade (with hedging if enabled)
        let result = if self.hedge_config.is_enabled() && self.timing_strategy.is_some() {
            // Generate rehedge times
            let rehedge_times = self.timing_strategy.as_ref().unwrap()
                .rehedge_times(entry_time, exit_time, &self.hedge_config.strategy);

            // Execute base trade first
            let mut base_result = self.straddle_executor
                .execute_trade(straddle, event, entry_time, exit_time)
                .await;

            // If successful, apply hedging
            if base_result.success {
                self.apply_hedging(&mut base_result, straddle, entry_time, exit_time, rehedge_times).await;
            }

            base_result
        } else {
            // Execute without hedging
            self.straddle_executor
                .execute_trade(straddle, event, entry_time, exit_time)
                .await
        };

        result
    }
}
