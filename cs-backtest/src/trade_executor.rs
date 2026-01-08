//! Unified trade executor with hedging support
//!
//! This executor combines the capabilities of:
//! - RollingExecutor: Generic rolling execution for any trade type
//! - TradeOrchestrator: Delta hedging for trade results
//!
//! Key improvement: Rolling execution now supports hedging!

use chrono::{DateTime, NaiveDate, NaiveTime, Utc, Weekday};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;

use cs_domain::{
    EquityDataRepository, OptionsDataRepository, MarketTime,
    RollPolicy, RollPeriod, RollReason, RollingResult,
    TradeFactory, TradingCalendar,
    RollableTrade, TradeResult,
    EarningsEvent, EarningsTime,
    HedgeConfig, HedgeState, RealizedVolatilityMetrics,
    CompositeTrade, LegPosition,
};
use finq_core::OptionType;

use crate::execution::{ExecutableTrade, ExecutionConfig, execute_trade};
use crate::timing_strategy::TimingStrategy;

/// Tracks spot prices during hedging for realized volatility computation
struct RealizedVolatilityTracker {
    spot_history: Vec<(DateTime<Utc>, f64)>,
    entry_hv: Option<f64>,
    entry_iv: Option<f64>,
}

impl RealizedVolatilityTracker {
    fn new(entry_hv: Option<f64>, entry_iv: Option<f64>) -> Self {
        Self {
            spot_history: Vec::new(),
            entry_hv,
            entry_iv,
        }
    }

    /// Record a spot observation
    fn record(&mut self, timestamp: DateTime<Utc>, spot: f64) {
        self.spot_history.push((timestamp, spot));
    }

    /// Compute final metrics
    fn finalize(self, exit_iv: Option<f64>) -> RealizedVolatilityMetrics {
        RealizedVolatilityMetrics::from_spot_history(
            &self.spot_history,
            self.entry_hv,
            self.entry_iv,
            exit_iv,
        )
    }

    /// Get the spot history for attaching to HedgePosition
    fn into_spot_history(self) -> Vec<(DateTime<Utc>, f64)> {
        self.spot_history
    }
}

/// Unified executor for any trade type with optional hedging
///
/// Supports:
/// - Single trade execution
/// - Rolling execution
/// - Optional delta hedging for both modes
///
/// # Example
/// ```ignore
/// // Create executor with hedging
/// let executor = TradeExecutor::<Straddle>::new(...)
///     .with_hedging(hedge_config, timing_strategy);
///
/// // Execute rolling with hedging (NEW!)
/// let result = executor.execute_rolling("AAPL", ...).await;
/// ```
pub struct TradeExecutor<T>
where
    T: RollableTrade + ExecutableTrade + CompositeTrade,
{
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    pricer: T::Pricer,
    trade_factory: Arc<dyn TradeFactory>,
    config: ExecutionConfig,

    // Rolling support
    roll_policy: Option<RollPolicy>,

    // Hedging support (was missing from RollingExecutor!)
    hedge_config: Option<HedgeConfig>,
    timing_strategy: Option<TimingStrategy>,

    // Attribution support (optional)
    attribution_config: Option<cs_domain::AttributionConfig>,
}

impl<T> TradeExecutor<T>
where
    T: RollableTrade + ExecutableTrade + CompositeTrade + Clone,
{
    /// Create a new trade executor
    pub fn new(
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        pricer: T::Pricer,
        trade_factory: Arc<dyn TradeFactory>,
        config: ExecutionConfig,
    ) -> Self {
        Self {
            options_repo,
            equity_repo,
            pricer,
            trade_factory,
            config,
            roll_policy: None,
            hedge_config: None,
            timing_strategy: None,
            attribution_config: None,
        }
    }

    /// Enable hedging (builder pattern)
    pub fn with_hedging(mut self, config: HedgeConfig, timing: TimingStrategy) -> Self {
        self.hedge_config = Some(config);
        self.timing_strategy = Some(timing);
        self
    }

    /// Enable P&L attribution (builder pattern)
    ///
    /// Attribution requires hedging to be enabled. If hedging is not enabled,
    /// attribution will have no effect.
    pub fn with_attribution(mut self, config: cs_domain::AttributionConfig) -> Self {
        self.attribution_config = Some(config);
        self
    }

    /// Set roll policy for rolling execution (builder pattern)
    pub fn with_roll_policy(mut self, policy: RollPolicy) -> Self {
        self.roll_policy = Some(policy);
        self
    }

    /// Execute a single trade with optional hedging
    ///
    /// Works for ANY trade implementing ExecutableTrade
    pub async fn execute(
        &self,
        trade: &T,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> <T as ExecutableTrade>::Result {
        // 1. Execute trade using generic executor
        let mut result = execute_trade(
            trade,
            &self.pricer,
            self.options_repo.as_ref(),
            self.equity_repo.as_ref(),
            &self.config,
            event,
            entry_time,
            exit_time,
        ).await;

        // 2. Apply hedging if enabled and trade succeeded
        if result.success() {
            if let (Some(ref hedge_config), Some(ref timing)) =
                (&self.hedge_config, &self.timing_strategy)
            {
                let rehedge_times = timing.rehedge_times(
                    entry_time,
                    exit_time,
                    &hedge_config.strategy,
                );

                if let Err(e) = self.apply_hedging(trade, &mut result, entry_time, exit_time, rehedge_times).await {
                    tracing::warn!("Hedging failed: {}", e);
                }
            }
        }

        result
    }

    /// Execute rolling strategy with optional hedging
    ///
    /// KEY IMPROVEMENT: This now supports hedging, which RollingExecutor couldn't do!
    pub async fn execute_rolling(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        entry_time: MarketTime,
        exit_time: MarketTime,
    ) -> RollingResult {
        let roll_policy = self.roll_policy.clone()
            .unwrap_or(RollPolicy::Weekly { roll_day: Weekday::Fri });

        let mut rolls = Vec::new();
        let mut current_date = start_date;

        // Ensure we start on a trading day
        if !TradingCalendar::is_trading_day(current_date) {
            current_date = TradingCalendar::next_trading_day(current_date);
        }

        while current_date < end_date {
            // Construct trade using trait method
            let entry_dt = self.to_datetime(current_date, entry_time);
            let min_expiration = current_date + chrono::Duration::days(1);

            let trade = match T::create(
                self.trade_factory.as_ref(),
                symbol,
                entry_dt,
                min_expiration,
            ).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("Failed to create trade at {}: {}", current_date, e);
                    current_date = TradingCalendar::next_trading_day(current_date);
                    continue;
                }
            };

            // Determine exit date based on roll policy
            let (exit_date, roll_reason) = self.determine_exit_date(
                current_date,
                end_date,
                trade.expiration(),
                &roll_policy,
            );

            let exit_dt = self.to_datetime(exit_date, exit_time);

            // Create dummy earnings event for rolling (no actual earnings)
            let event = EarningsEvent::new(
                symbol.to_string(),
                exit_dt.date_naive(),
                EarningsTime::AfterMarketClose,
            );

            // Execute WITH HEDGING (the key fix!)
            let result = self.execute(&trade, &event, entry_dt, exit_dt).await;

            // Convert to roll period (now includes hedge data!)
            let roll_period = self.to_roll_period(&trade, result, roll_reason);
            rolls.push(roll_period);

            // Move to next roll date
            current_date = TradingCalendar::next_trading_day(exit_date);
        }

        let trade_type = std::any::type_name::<T>()
            .split("::")
            .last()
            .unwrap_or("unknown")
            .to_lowercase();

        RollingResult::from_rolls(
            symbol.to_string(),
            start_date,
            end_date,
            roll_policy.description(),
            trade_type,
            rolls,
        )
    }

    /// Apply hedging to any trade result (UNIFIED for all delta computation modes)
    ///
    /// This method replaces the previous 6 mode-specific implementations with
    /// a single unified loop that uses the DeltaProvider strategy pattern.
    async fn apply_hedging(
        &self,
        trade: &T,
        result: &mut <T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
    ) -> Result<(), String> {
        let hedge_config = self.hedge_config.as_ref()
            .ok_or("Hedge config not set")?;

        let symbol = result.symbol();
        let entry_spot = result.spot_at_entry();
        let exit_spot = result.spot_at_exit();

        // Check if attribution is enabled
        let attribution_enabled = self.attribution_config
            .as_ref()
            .map(|c| c.enabled)
            .unwrap_or(false);

        // Create delta provider based on mode
        use cs_domain::DeltaComputation;
        use crate::delta_providers::*;

        // Helper macro to execute hedging with a specific provider
        macro_rules! hedge_with_provider {
            ($provider:expr) => {{
                let mut hedge_state = cs_domain::GenericHedgeState::new(
                    hedge_config.clone(),
                    $provider,
                    entry_spot,
                    attribution_enabled,
                );

                // === UNIFIED HEDGING LOOP (no more duplication!) ===
                for rehedge_time in rehedge_times {
                    if hedge_state.at_max_rehedges() {
                        break;
                    }

                    let spot = self.equity_repo
                        .get_spot_price(symbol, rehedge_time)
                        .await
                        .map_err(|e| e.to_string())?
                        .to_f64();

                    hedge_state.update(rehedge_time, spot).await?;
                }

                // Finalize
                let entry_iv = result.entry_iv().map(|iv| iv.primary);
                let exit_iv = result.exit_iv().map(|iv| iv.primary);
                hedge_state.finalize(exit_spot, entry_iv, exit_iv)
            }};
        }

        let hedge_position = match &hedge_config.delta_computation {
            DeltaComputation::GammaApproximation => {
                let delta = result.net_delta().unwrap_or(0.0);
                let gamma = result.net_gamma().unwrap_or(0.0);
                hedge_with_provider!(GammaApproximationProvider::new(delta, gamma, entry_spot))
            }
            DeltaComputation::EntryHV { window } => {
                let entry_hv = self.compute_hv_at_time(symbol, entry_time, *window).await?;
                let provider = EntryVolatilityProvider::new_entry_hv((*trade).clone(), entry_hv, 0.05);

                // Manually expand hedge_with_provider to call set_entry_hv
                let mut hedge_state = cs_domain::GenericHedgeState::new(
                    hedge_config.clone(),
                    provider,
                    entry_spot,
                    attribution_enabled,
                );
                hedge_state.set_entry_hv(entry_hv);  // Store for RV metrics

                for rehedge_time in &rehedge_times {
                    if hedge_state.at_max_rehedges() {
                        break;
                    }
                    let spot = self.equity_repo
                        .get_spot_price(symbol, *rehedge_time)
                        .await
                        .map_err(|e| e.to_string())?
                        .to_f64();
                    hedge_state.update(*rehedge_time, spot).await?;
                }

                let entry_iv = result.entry_iv().map(|iv| iv.primary);
                let exit_iv = result.exit_iv().map(|iv| iv.primary);
                hedge_state.finalize(exit_spot, entry_iv, exit_iv)
            }
            DeltaComputation::EntryIV { .. } => {
                let entry_iv = result.entry_iv()
                    .map(|iv| iv.primary)
                    .ok_or("No entry IV available")?;
                hedge_with_provider!(EntryVolatilityProvider::new_entry_iv((*trade).clone(), entry_iv, 0.05))
            }
            DeltaComputation::CurrentHV { window } => {
                hedge_with_provider!(CurrentHVProvider::new(
                    (*trade).clone(),
                    self.equity_repo.clone(),
                    symbol.to_string(),
                    *window,
                    0.05,
                ))
            }
            DeltaComputation::CurrentMarketIV { .. } => {
                hedge_with_provider!(CurrentMarketIVProvider::new(
                    (*trade).clone(),
                    self.options_repo.clone(),
                    self.equity_repo.clone(),
                    symbol.to_string(),
                    0.05,
                ))
            }
            DeltaComputation::HistoricalAverageIV { lookback_days, .. } => {
                hedge_with_provider!(HistoricalAverageIVProvider::new(
                    (*trade).clone(),
                    self.options_repo.clone(),
                    self.equity_repo.clone(),
                    symbol.to_string(),
                    *lookback_days,
                    0.05,
                ))
            }
        };

        // Compute attribution if enabled
        let attribution = if attribution_enabled && hedge_position.rehedge_count() > 0 {
            match self.compute_attribution(
                trade,
                &hedge_position,
                entry_time,
                exit_time,
                result.pnl(),
            )
            .await
            {
                Ok(attr) => Some(attr),
                Err(e) => {
                    tracing::warn!("Attribution failed for {}: {}", symbol, e);
                    None
                }
            }
        } else {
            None
        };

        // Apply results
        if hedge_position.rehedge_count() > 0 || attribution.is_some() {
            let hedge_pnl = hedge_position.calculate_pnl(exit_spot);
            let total_pnl = result.pnl() + hedge_pnl - hedge_position.total_cost;
            result.apply_hedge_results(hedge_position, hedge_pnl, total_pnl, attribution);
        }

        Ok(())
    }

    /// Compute P&L attribution from hedge history
    ///
    /// Collects daily snapshots and attributes P&L to Greeks components.
    async fn compute_attribution(
        &self,
        trade: &T,
        hedge_position: &cs_domain::HedgePosition,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        actual_pnl: Decimal,
    ) -> Result<cs_domain::PositionAttribution, String> {
        let attr_config = self.attribution_config.as_ref()
            .ok_or("Attribution config not set")?;

        let symbol = cs_domain::CompositeTrade::symbol(trade).to_string();
        let contract_multiplier = self.hedge_config
            .as_ref()
            .map(|c| c.contract_multiplier)
            .unwrap_or(100);

        // Create snapshot collector
        let mut collector = crate::attribution::SnapshotCollector::new(
            trade.clone(),
            self.options_repo.clone(),
            self.equity_repo.clone(),
            symbol,
            attr_config.clone(),
            contract_multiplier,
            0.05, // risk_free_rate
        );

        // Set hedge timeline from completed hedging
        collector.set_hedge_timeline(&hedge_position.hedges);

        // TODO: Set entry vol for EntryIV/EntryHV modes
        // This would require passing entry IV/HV from the hedging phase

        // Collect daily snapshots
        collector.collect(entry_time, exit_time).await?;

        // Build attribution
        collector
            .build_attribution(actual_pnl)
            .ok_or_else(|| "No snapshots collected for attribution".to_string())
    }


    /// Compute HV at a specific time using recent price history
    async fn compute_hv_at_time(
        &self,
        symbol: &str,
        at_time: DateTime<Utc>,
        window: u32,
    ) -> Result<f64, String> {
        use cs_analytics::realized_volatility;

        let end_date = at_time.date_naive();

        let bars = self.equity_repo
            .get_bars(symbol, end_date)
            .await
            .map_err(|e| format!("Failed to get bars: {}", e))?;

        let closes: Vec<f64> = bars.column("close")
            .map_err(|_| "No close column".to_string())?
            .f64()
            .map_err(|_| "Invalid close type".to_string())?
            .into_no_null_iter()
            .collect();

        realized_volatility(&closes, window as usize, 252.0)
            .ok_or_else(|| "Insufficient data for HV computation".to_string())
    }

    /// Determine when to exit based on roll policy and expiration
    fn determine_exit_date(
        &self,
        entry_date: NaiveDate,
        campaign_end: NaiveDate,
        expiration: NaiveDate,
        roll_policy: &RollPolicy,
    ) -> (NaiveDate, RollReason) {
        if campaign_end <= entry_date {
            return (entry_date, RollReason::EndOfCampaign);
        }

        let next_roll = roll_policy.next_roll_date(entry_date)
            .unwrap_or(campaign_end);

        let exit_date = next_roll.min(expiration).min(campaign_end);

        let reason = if exit_date >= campaign_end {
            RollReason::EndOfCampaign
        } else if exit_date >= expiration {
            RollReason::Expiry
        } else {
            RollReason::Scheduled
        };

        (exit_date, reason)
    }

    /// Convert execution result to roll period
    fn to_roll_period(
        &self,
        trade: &T,
        result: <T as ExecutableTrade>::Result,
        roll_reason: RollReason,
    ) -> RollPeriod {
        let iv_change = result.iv_change();

        RollPeriod {
            entry_date: result.entry_time().date_naive(),
            exit_date: result.exit_time().date_naive(),
            strike: trade.strike(),
            expiration: trade.expiration(),

            entry_debit: result.entry_cost(),
            exit_credit: result.exit_value(),
            pnl: result.pnl(),

            spot_at_entry: result.spot_at_entry(),
            spot_at_exit: result.spot_at_exit(),
            spot_move_pct: ((result.spot_at_exit() - result.spot_at_entry())
                / result.spot_at_entry() * 100.0),

            // Greeks from result
            net_delta: result.net_delta(),
            net_gamma: result.net_gamma(),
            net_theta: None,  // Could be added to TradeResult trait if needed
            net_vega: None,

            // IV now derived automatically from CompositeIV!
            iv_entry: result.entry_iv().map(|iv| iv.primary),
            iv_exit: result.exit_iv().map(|iv| iv.primary),
            iv_change: iv_change.map(|c| c.primary_change),

            // P&L attribution
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,

            // Hedging (NOW POPULATED!)
            hedge_pnl: result.hedge_pnl(),
            hedge_count: result.hedge_position()
                .map(|p| p.rehedge_count())
                .unwrap_or(0),
            transaction_cost: result.hedge_position()
                .map(|p| p.total_cost)
                .unwrap_or(Decimal::ZERO),

            roll_reason,
            position_attribution: None,

            // Extract realized vol metrics from hedge position (Phase 1c)
            realized_vol_metrics: result.hedge_position()
                .and_then(|hp| hp.realized_vol_metrics.clone()),

            // Extract capital metrics from hedge position (Phase 2b)
            hedge_capital: result.hedge_position().map(|hp| {
                use cs_domain::entities::rolling_result::HedgeCapitalMetrics;
                HedgeCapitalMetrics {
                    peak_long_shares: hp.peak_long_shares,
                    peak_short_shares: hp.peak_short_shares,
                    avg_hedge_price: hp.avg_hedge_price,
                    long_capital: hp.long_hedge_capital(),
                    short_margin: hp.short_hedge_margin(0.5),  // 50% margin
                }
            }),

            // Detailed hedge trades with per-trade metrics (includes unwind at exit)
            hedge_trade_details: result.hedge_position().map(|hp| {
                let gamma = result.net_gamma();
                let entry_spot = result.spot_at_entry();
                let exit_spot = result.spot_at_exit();
                let exit_time = result.exit_time();
                hp.build_trade_details(gamma, entry_spot, exit_spot, exit_time)
            }),
        }
    }

    /// Convert date + time to UTC DateTime
    fn to_datetime(&self, date: NaiveDate, time: MarketTime) -> DateTime<Utc> {
        use cs_domain::datetime::eastern_to_utc;

        let naive_time = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)
            .unwrap_or_else(|| NaiveTime::from_hms_opt(15, 45, 0).unwrap());

        eastern_to_utc(date, naive_time)
    }
}
