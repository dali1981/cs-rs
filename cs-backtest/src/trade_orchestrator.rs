//! Trade orchestrator - selects and executes trades
//!
//! Orchestrates:
//! 1. Strike/expiration selection via StrikeSelector
//! 2. Trade execution via generic execute_trade()
//! 3. Optional hedging for straddles

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_analytics::{IVSurface, PricingModel};
use cs_domain::{
    CalendarSpread, CalendarSpreadResult, CalendarStraddle, CalendarStraddleResult,
    EarningsEvent, EquityDataRepository, IronButterfly, IronButterflyResult,
    OptionsDataRepository, SpotPrice, Straddle, StraddleResult, Strike,
};
use cs_domain::strike_selection::{ExpirationCriteria, SelectionError, StrikeSelector};
use finq_core::OptionType;

use crate::execution::{execute_trade, ExecutableTrade, ExecutionConfig};
use crate::spread_pricer::SpreadPricer;
use crate::straddle_pricer::StraddlePricer;
use crate::calendar_straddle_pricer::CalendarStraddlePricer;
use crate::iron_butterfly_pricer::IronButterflyPricer;
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
    pub phase: String,
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
            TradeResult::Failed(_) => None,
        }
    }

    pub fn hedge_pnl(&self) -> Option<Decimal> {
        match self {
            TradeResult::Straddle(r) => r.hedge_pnl,
            _ => None,
        }
    }

    pub fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        match self {
            TradeResult::Straddle(r) => r.total_pnl_with_hedge,
            _ => None,
        }
    }

    pub fn has_hedge_data(&self) -> bool {
        match self {
            TradeResult::Straddle(r) => r.hedge_position.is_some(),
            _ => false,
        }
    }
}

/// Error type for orchestrator
#[derive(Debug, thiserror::Error)]
pub enum UnifiedExecutionError {
    #[error("Selection error: {0}")]
    Selection(#[from] SelectionError),
    #[error("No spread selected")]
    NoSpread,
}

/// Trade orchestrator - selects and executes trades
///
/// Lightweight orchestrator that:
/// - Uses StrikeSelector for strike/expiration selection
/// - Creates pricers on demand (no stored executor objects)
/// - Delegates execution to generic execute_trade()
/// - Handles straddle hedging when configured
pub struct TradeOrchestrator<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    pricing_model: PricingModel,
    max_entry_iv: Option<f64>,
    hedge_config: cs_domain::HedgeConfig,
    timing_strategy: Option<TimingStrategy>,
}

impl<O, E> TradeOrchestrator<O, E>
where
    O: OptionsDataRepository + 'static,
    E: EquityDataRepository + 'static,
{
    pub fn new(options_repo: Arc<O>, equity_repo: Arc<E>) -> Self {
        Self {
            options_repo,
            equity_repo,
            pricing_model: PricingModel::default(),
            max_entry_iv: None,
            hedge_config: cs_domain::HedgeConfig::default(),
            timing_strategy: None,
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

    pub fn with_hedge_config(mut self, hedge_config: cs_domain::HedgeConfig) -> Self {
        self.hedge_config = hedge_config;
        self
    }

    pub fn with_timing_strategy(mut self, timing_strategy: TimingStrategy) -> Self {
        self.timing_strategy = Some(timing_strategy);
        self
    }

    /// Execute any trade type with selection
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

        match structure {
            TradeStructure::CalendarSpread(option_type) => {
                self.execute_calendar_spread(
                    event, entry_time, exit_time, &spot, entry_surface,
                    selector, option_type, criteria, structure,
                ).await
            }
            TradeStructure::Straddle => {
                self.execute_straddle_with_selection(
                    event, entry_time, exit_time, &spot, entry_surface,
                    selector, structure,
                ).await
            }
            TradeStructure::CalendarStraddle => {
                self.execute_calendar_straddle(
                    event, entry_time, exit_time, &spot, entry_surface,
                    selector, criteria, structure,
                ).await
            }
            TradeStructure::IronButterfly { wing_width } => {
                self.execute_iron_butterfly(
                    event, entry_time, exit_time, &spot, entry_surface,
                    selector, wing_width, criteria, structure,
                ).await
            }
        }
    }

    async fn execute_calendar_spread(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        spot: &SpotPrice,
        entry_surface: &IVSurface,
        selector: &dyn StrikeSelector,
        option_type: OptionType,
        criteria: &ExpirationCriteria,
        structure: TradeStructure,
    ) -> TradeResult {
        // 1. Select trade
        let spread = match selector.select_calendar_spread(spot, entry_surface, option_type, criteria) {
            Ok(s) => s,
            Err(e) => return self.selection_failed(event, structure, e),
        };

        // 2. Create pricer on demand
        let pricer = SpreadPricer::new().with_pricing_model(self.pricing_model);

        // 3. Execute using generic function
        let config = ExecutionConfig::for_calendar_spread(self.max_entry_iv);
        let result = execute_trade(
            &spread,
            &pricer,
            self.options_repo.as_ref(),
            self.equity_repo.as_ref(),
            &config,
            event,
            entry_time,
            exit_time,
        ).await;

        // 4. Wrap result
        if result.success {
            TradeResult::CalendarSpread(result)
        } else {
            self.execution_failed(event, structure, result.failure_reason)
        }
    }

    async fn execute_straddle_with_selection(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        spot: &SpotPrice,
        entry_surface: &IVSurface,
        selector: &dyn StrikeSelector,
        structure: TradeStructure,
    ) -> TradeResult {
        // 1. Select trade
        let min_expiration = exit_time.date_naive();
        let straddle = match selector.select_straddle(spot, entry_surface, min_expiration) {
            Ok(s) => s,
            Err(e) => return self.selection_failed(event, structure, e),
        };

        // 2. Execute (with hedging if enabled)
        let result = self.execute_straddle(&straddle, event, entry_time, exit_time).await;

        // 3. Wrap result
        if result.success {
            TradeResult::Straddle(result)
        } else {
            self.execution_failed(event, structure, result.failure_reason)
        }
    }

    async fn execute_calendar_straddle(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        spot: &SpotPrice,
        entry_surface: &IVSurface,
        selector: &dyn StrikeSelector,
        criteria: &ExpirationCriteria,
        structure: TradeStructure,
    ) -> TradeResult {
        // 1. Select trade
        let cal_straddle = match selector.select_calendar_straddle(spot, entry_surface, criteria) {
            Ok(s) => s,
            Err(e) => return self.selection_failed(event, structure, e),
        };

        // 2. Execute trade
        let result = self.execute_calendar_straddle_direct(&cal_straddle, event, entry_time, exit_time).await;

        // 3. Wrap result
        if result.success {
            TradeResult::CalendarStraddle(result)
        } else {
            self.execution_failed(event, structure, result.failure_reason)
        }
    }

    async fn execute_iron_butterfly(
        &self,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        spot: &SpotPrice,
        entry_surface: &IVSurface,
        selector: &dyn StrikeSelector,
        wing_width: Decimal,
        criteria: &ExpirationCriteria,
        structure: TradeStructure,
    ) -> TradeResult {
        // 1. Select trade
        let butterfly = match selector.select_iron_butterfly(
            spot, entry_surface, wing_width,
            criteria.min_short_dte, criteria.max_short_dte,
        ) {
            Ok(b) => b,
            Err(e) => return self.selection_failed(event, structure, e),
        };

        // 2. Execute trade
        let result = self.execute_iron_butterfly_direct(&butterfly, event, entry_time, exit_time).await;

        // 3. Wrap result
        if result.success {
            TradeResult::IronButterfly(result)
        } else {
            self.execution_failed(event, structure, result.failure_reason)
        }
    }

    /// Execute a pre-built straddle directly (for rolling strategies)
    pub async fn execute_straddle(
        &self,
        straddle: &Straddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> StraddleResult {
        // Create pricer on demand
        let spread_pricer = SpreadPricer::new().with_pricing_model(self.pricing_model);
        let pricer = StraddlePricer::new(spread_pricer);
        let config = ExecutionConfig::for_straddle(self.max_entry_iv);

        // Execute using generic function
        let mut result = execute_trade(
            straddle,
            &pricer,
            self.options_repo.as_ref(),
            self.equity_repo.as_ref(),
            &config,
            event,
            entry_time,
            exit_time,
        ).await;

        // Apply hedging if enabled
        if result.success && self.hedge_config.is_enabled() && self.timing_strategy.is_some() {
            let rehedge_times = self.timing_strategy.as_ref().unwrap()
                .rehedge_times(entry_time, exit_time, &self.hedge_config.strategy);

            if let Err(e) = self.apply_hedging(&mut result, entry_time, exit_time, rehedge_times).await {
                eprintln!("Hedging failed: {}", e);
                result.success = false;
                result.failure_reason = Some(cs_domain::FailureReason::PricingError(
                    format!("Hedging failed: {}", e)
                ));
            }
        }

        result
    }

    /// Execute a calendar straddle using generic execution
    async fn execute_calendar_straddle_direct(
        &self,
        cal_straddle: &CalendarStraddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> CalendarStraddleResult {
        // Create pricer on demand
        let spread_pricer = SpreadPricer::new().with_pricing_model(self.pricing_model);
        let pricer = CalendarStraddlePricer::new(spread_pricer);
        let config = ExecutionConfig::for_calendar_spread(self.max_entry_iv);

        // Execute using generic function
        let mut result = execute_trade(
            cal_straddle,
            &pricer,
            self.options_repo.as_ref(),
            self.equity_repo.as_ref(),
            &config,
            event,
            entry_time,
            exit_time,
        ).await;

        // Apply hedging if enabled (calendar straddles can be hedged like straddles)
        if result.success && self.hedge_config.is_enabled() && self.timing_strategy.is_some() {
            let rehedge_times = self.timing_strategy.as_ref().unwrap()
                .rehedge_times(entry_time, exit_time, &self.hedge_config.strategy);

            if let Err(e) = self.apply_hedging(&mut result, entry_time, exit_time, rehedge_times).await {
                eprintln!("Hedging failed: {}", e);
                result.success = false;
                result.failure_reason = Some(cs_domain::FailureReason::PricingError(
                    format!("Hedging failed: {}", e)
                ));
            }
        }

        result
    }

    /// Execute an iron butterfly using generic execution
    async fn execute_iron_butterfly_direct(
        &self,
        butterfly: &IronButterfly,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> IronButterflyResult {
        // Create pricer on demand
        let spread_pricer = SpreadPricer::new().with_pricing_model(self.pricing_model);
        let pricer = IronButterflyPricer::new(spread_pricer);
        let config = ExecutionConfig::for_iron_butterfly(self.max_entry_iv);

        // Execute using generic function
        let mut result = execute_trade(
            butterfly,
            &pricer,
            self.options_repo.as_ref(),
            self.equity_repo.as_ref(),
            &config,
            event,
            entry_time,
            exit_time,
        ).await;

        // Apply hedging if enabled (iron butterflies can be hedged)
        if result.success && self.hedge_config.is_enabled() && self.timing_strategy.is_some() {
            let rehedge_times = self.timing_strategy.as_ref().unwrap()
                .rehedge_times(entry_time, exit_time, &self.hedge_config.strategy);

            if let Err(e) = self.apply_hedging(&mut result, entry_time, exit_time, rehedge_times).await {
                eprintln!("Hedging failed: {}", e);
                result.success = false;
                result.failure_reason = Some(cs_domain::FailureReason::PricingError(
                    format!("Hedging failed: {}", e)
                ));
            }
        }

        result
    }

    fn selection_failed(&self, event: &EarningsEvent, structure: TradeStructure, error: SelectionError) -> TradeResult {
        TradeResult::Failed(FailedTrade {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            trade_structure: structure,
            reason: cs_domain::FailureReason::PricingError(error.to_string()),
            phase: "selection".to_string(),
            details: Some(error.to_string()),
        })
    }

    fn execution_failed(&self, event: &EarningsEvent, structure: TradeStructure, reason: Option<cs_domain::FailureReason>) -> TradeResult {
        TradeResult::Failed(FailedTrade {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            trade_structure: structure,
            reason: reason.unwrap_or(cs_domain::FailureReason::PricingError("Unknown".to_string())),
            phase: "execution".to_string(),
            details: None,
        })
    }

    /// Apply delta hedging to any trade result (trade-agnostic)
    ///
    /// This method:
    /// 1. Initializes hedge state based on net delta/gamma at entry
    /// 2. Rehedges at specified times based on delta drift
    /// 3. Calculates hedge P&L and total P&L including hedge
    /// 4. Stores results in the trade result via apply_hedge_results()
    async fn apply_hedging<T: cs_domain::trade::TradeResult>(
        &self,
        result: &mut T,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
    ) -> Result<(), String> {
        use cs_domain::HedgeState;
        use rust_decimal::prelude::ToPrimitive;

        // Get net delta/gamma from the trade result (trade-agnostic)
        let net_delta = result.net_delta().unwrap_or(0.0);
        let net_gamma = result.net_gamma().unwrap_or(0.0);
        let entry_spot = result.spot_at_entry();
        let exit_spot = result.spot_at_exit();
        let symbol = result.symbol();

        let mut hedge_state = HedgeState::new(
            self.hedge_config.clone(),
            net_delta,
            net_gamma,
            entry_spot,
        );

        let _hedge_shares_timeline: Vec<(DateTime<Utc>, i32)> = vec![(entry_time, 0)];

        for rehedge_time in rehedge_times {
            if hedge_state.at_max_rehedges() {
                break;
            }

            let spot = self.equity_repo.get_spot_price(symbol, rehedge_time)
                .await
                .map_err(|e| format!("Failed to get spot price at {}: {}", rehedge_time, e))?;

            hedge_state.update(rehedge_time, spot.to_f64());
        }

        let hedge_position = hedge_state.finalize(exit_spot);

        if hedge_position.rehedge_count() > 0 {
            let hedge_pnl = hedge_position.calculate_pnl(exit_spot);
            let total_pnl = result.pnl() + hedge_pnl - hedge_position.total_cost;

            // Use the trait method to apply hedge results (trade-agnostic)
            result.apply_hedge_results(
                hedge_position,
                hedge_pnl,
                total_pnl,
                None, // Attribution can be added later as a trade-specific feature
            );
        }

        Ok(())
    }

    async fn collect_daily_snapshots(
        &self,
        straddle: &Straddle,
        hedge_shares_timeline: &[(DateTime<Utc>, i32)],
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Result<Vec<(cs_domain::PositionSnapshot, cs_domain::PositionSnapshot)>, String> {
        use cs_domain::TradingCalendar;

        let trading_days: Vec<_> = TradingCalendar::trading_days_between(
            entry_time.date_naive(),
            exit_time.date_naive(),
        ).collect();

        if trading_days.is_empty() {
            return Ok(Vec::new());
        }

        let mut snapshots = Vec::new();
        let mut prev_snapshot = self.create_snapshot(straddle, entry_time, hedge_shares_timeline).await
            .map_err(|e| format!("Failed to create entry snapshot: {}", e))?;

        for day in trading_days {
            let close_time = cs_domain::datetime::eastern_to_utc(
                day,
                chrono::NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            );

            let actual_close = if day == exit_time.date_naive() { exit_time } else { close_time };

            let close_snapshot = match self.create_snapshot(straddle, actual_close, hedge_shares_timeline).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Warning: Failed to create EOD snapshot for {}: {}", day, e);
                    continue;
                }
            };

            snapshots.push((prev_snapshot, close_snapshot.clone()));
            prev_snapshot = close_snapshot;
        }

        Ok(snapshots)
    }

    async fn create_snapshot(
        &self,
        straddle: &Straddle,
        timestamp: DateTime<Utc>,
        hedge_shares_timeline: &[(DateTime<Utc>, i32)],
    ) -> Result<cs_domain::PositionSnapshot, String> {
        use cs_analytics::bs_greeks;
        use cs_domain::{PositionGreeks, PositionSnapshot, CONTRACT_MULTIPLIER};
        use rust_decimal::prelude::ToPrimitive;

        let spot = self.equity_repo.get_spot_price(straddle.symbol(), timestamp)
            .await
            .map_err(|e| format!("Failed to get spot price: {}", e))?;

        let chain = self.options_repo.get_option_bars_at_time(straddle.symbol(), timestamp)
            .await
            .map_err(|e| format!("Failed to get option chain: {}", e))?;

        let iv_surface = crate::iv_surface_builder::build_iv_surface_minute_aligned(
            &chain,
            self.equity_repo.as_ref(),
            straddle.symbol(),
        ).await;

        let strike_decimal = straddle.strike().value();
        let strike_f64 = strike_decimal.to_f64().unwrap_or(0.0);
        let risk_free_rate = 0.05;
        let pricing_provider = self.pricing_model.to_provider_with_rate(risk_free_rate);

        let iv = if let Some(ref surface) = iv_surface {
            let call_iv = pricing_provider.get_iv(surface, strike_decimal, straddle.expiration(), true);
            let put_iv = pricing_provider.get_iv(surface, strike_decimal, straddle.expiration(), false);

            match (call_iv, put_iv) {
                (Some(c), Some(p)) => (c + p) / 2.0,
                (Some(c), None) => c,
                (None, Some(p)) => p,
                (None, None) => 0.30,
            }
        } else {
            0.30
        };

        let tte_days = (straddle.expiration() - timestamp.date_naive()).num_days() as f64;
        let tte = tte_days / 365.0;

        let call_greeks = bs_greeks(spot.to_f64(), strike_f64, tte, iv, true, risk_free_rate);
        let put_greeks = bs_greeks(spot.to_f64(), strike_f64, tte, iv, false, risk_free_rate);
        let position_greeks = PositionGreeks::straddle(&call_greeks, &put_greeks, CONTRACT_MULTIPLIER);

        let hedge_shares = hedge_shares_timeline
            .iter()
            .rev()
            .find(|(t, _)| *t <= timestamp)
            .map(|(_, shares)| *shares)
            .unwrap_or(0);

        Ok(PositionSnapshot::new(timestamp, spot.to_f64(), iv, position_greeks, hedge_shares))
    }
}
