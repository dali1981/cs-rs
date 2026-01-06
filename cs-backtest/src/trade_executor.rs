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
}

impl<T> TradeExecutor<T>
where
    T: RollableTrade + ExecutableTrade + CompositeTrade,
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
        }
    }

    /// Enable hedging (builder pattern)
    pub fn with_hedging(mut self, config: HedgeConfig, timing: TimingStrategy) -> Self {
        self.hedge_config = Some(config);
        self.timing_strategy = Some(timing);
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

    /// Apply hedging to any trade result (trade-agnostic)
    ///
    /// Dispatches to appropriate delta computation method based on config
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

        // Initialize RV tracker if enabled
        let mut rv_tracker = if hedge_config.track_realized_vol {
            let entry_iv = result.entry_iv().map(|iv| iv.primary);
            Some(RealizedVolatilityTracker::new(None, entry_iv))
        } else {
            None
        };

        // Initialize snapshot collector
        let snapshot_collector = crate::attribution::SnapshotCollector::new(
            self.equity_repo.clone(),
            self.options_repo.clone(),
        );

        // Dispatch based on delta computation mode
        use cs_domain::DeltaComputation;
        let (hedge_position, snapshots) = match &hedge_config.delta_computation {
            DeltaComputation::GammaApproximation => {
                // Gamma approximation doesn't recompute Greeks, so no attribution
                let pos = self.hedge_with_gamma_approximation(
                    result,
                    entry_time,
                    exit_time,
                    rehedge_times,
                    &mut rv_tracker,
                ).await?;
                (pos, Vec::new())
            }
            DeltaComputation::EntryHV { window } => {
                self.hedge_with_entry_hv(
                    trade,
                    result,
                    entry_time,
                    exit_time,
                    rehedge_times,
                    *window,
                    &mut rv_tracker,
                    &snapshot_collector,
                ).await?
            }
            DeltaComputation::EntryIV { .. } => {
                self.hedge_with_entry_iv(
                    trade,
                    result,
                    entry_time,
                    exit_time,
                    rehedge_times,
                    &mut rv_tracker,
                    &snapshot_collector,
                ).await?
            }
            DeltaComputation::CurrentHV { window } => {
                self.hedge_with_current_hv(
                    trade,
                    result,
                    entry_time,
                    exit_time,
                    rehedge_times,
                    *window,
                    &mut rv_tracker,
                    &snapshot_collector,
                ).await?
            }
            DeltaComputation::CurrentMarketIV { .. } => {
                let pos = self.hedge_with_current_market_iv(
                    trade,
                    result,
                    entry_time,
                    exit_time,
                    rehedge_times,
                    &mut rv_tracker,
                ).await?;
                (pos, Vec::new())  // TODO: Implement when CurrentMarketIV is complete
            }
            DeltaComputation::HistoricalAverageIV { lookback_days, .. } => {
                let pos = self.hedge_with_historical_average_iv(
                    trade,
                    result,
                    entry_time,
                    exit_time,
                    rehedge_times,
                    *lookback_days,
                    &mut rv_tracker,
                ).await?;
                (pos, Vec::new())  // TODO: Implement when HistoricalAverageIV is complete
            }
        };

        // Build attribution from snapshots if we have them
        let attribution = if !snapshots.is_empty() {
            Some(self.build_attribution(snapshots, result.pnl()))
        } else {
            None
        };

        // Apply hedge results if any rehedges occurred
        if hedge_position.rehedge_count() > 0 {
            let exit_spot = result.spot_at_exit();
            let hedge_pnl = hedge_position.calculate_pnl(exit_spot);
            let total_pnl = result.pnl() + hedge_pnl - hedge_position.total_cost;

            result.apply_hedge_results(hedge_position, hedge_pnl, total_pnl, attribution);
        }

        Ok(())
    }

    /// Build position attribution from snapshots
    ///
    /// Pairs up consecutive snapshots and computes daily attribution,
    /// then aggregates into total attribution.
    fn build_attribution(
        &self,
        snapshots: Vec<cs_domain::PositionSnapshot>,
        actual_pnl: Decimal,
    ) -> cs_domain::PositionAttribution {
        use cs_domain::DailyAttribution;

        // Pair up consecutive snapshots (start-of-day, end-of-day)
        let daily_attributions: Vec<DailyAttribution> = snapshots
            .windows(2)
            .map(|pair| DailyAttribution::compute(&pair[0], &pair[1]))
            .collect();

        cs_domain::PositionAttribution::from_daily(daily_attributions, actual_pnl)
    }

    /// Hedge using gamma approximation (original/default behavior)
    async fn hedge_with_gamma_approximation(
        &self,
        result: &<T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
        rv_tracker: &mut Option<RealizedVolatilityTracker>,
    ) -> Result<cs_domain::HedgePosition, String> {
        let hedge_config = self.hedge_config.as_ref().unwrap();
        let net_delta = result.net_delta().unwrap_or(0.0);
        let net_gamma = result.net_gamma().unwrap_or(0.0);
        let entry_spot = result.spot_at_entry();
        let exit_spot = result.spot_at_exit();
        let symbol = result.symbol();

        // Record entry spot for RV tracking
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(entry_time, entry_spot);
        }

        let mut hedge_state = HedgeState::new(
            hedge_config.clone(),
            net_delta,
            net_gamma,
            entry_spot,
        );

        // Rehedge at specified times
        for rehedge_time in rehedge_times {
            if hedge_state.at_max_rehedges() {
                break;
            }

            let spot = self.equity_repo
                .get_spot_price(symbol, rehedge_time)
                .await
                .map_err(|e| format!("Failed to get spot at {}: {}", rehedge_time, e))?;

            let spot_f64 = spot.to_f64();

            // Track for RV computation
            if let Some(ref mut tracker) = rv_tracker {
                tracker.record(rehedge_time, spot_f64);
            }

            hedge_state.update(rehedge_time, spot_f64);
        }

        // Record exit spot
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(exit_time, exit_spot);
        }

        // Finalize hedge position
        let mut hedge_position = hedge_state.finalize(exit_spot);

        // Attach RV metrics and spot history if tracking is enabled
        if let Some(tracker) = rv_tracker.take() {
            let exit_iv = result.exit_iv().map(|iv| iv.primary);

            // Store spot history before finalizing
            hedge_position.spot_history = tracker.spot_history.clone();

            // Compute and attach metrics
            hedge_position.realized_vol_metrics = Some(tracker.finalize(exit_iv));
        }

        Ok(hedge_position)
    }

    /// Compute position delta from leg structure
    ///
    /// Recomputes delta for composite trades by summing leg deltas using Black-Scholes.
    /// Works for any trade implementing CompositeTrade (straddles, spreads, butterflies).
    fn compute_position_delta(
        trade: &T,
        spot: f64,
        volatility: f64,
        at_time: DateTime<Utc>,
        risk_free_rate: f64,
        contract_multiplier: i32,
    ) -> f64 {
        use cs_analytics::bs_delta;

        trade.legs().iter().map(|(leg, position)| {
            let tte = (leg.expiration - at_time.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 {
                return 0.0;
            }

            let is_call = leg.option_type == OptionType::Call;
            let strike_f64 = leg.strike.value().to_f64().unwrap_or(0.0);

            let leg_delta = bs_delta(
                spot,
                strike_f64,
                tte,
                volatility,
                is_call,
                risk_free_rate,
            );

            // Apply position sign (long = +1, short = -1) and contract multiplier
            leg_delta * position.sign() * contract_multiplier as f64
        }).sum()
    }

    /// Hedge using Entry HV (Historical Volatility at trade entry)
    async fn hedge_with_entry_hv(
        &self,
        trade: &T,
        result: &<T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
        hv_window: u32,
        rv_tracker: &mut Option<RealizedVolatilityTracker>,
        snapshot_collector: &crate::attribution::SnapshotCollector,
    ) -> Result<(cs_domain::HedgePosition, Vec<cs_domain::PositionSnapshot>), String> {
        use cs_analytics::realized_volatility;

        let hedge_config = self.hedge_config.as_ref().unwrap();
        let symbol = result.symbol();
        let entry_spot = result.spot_at_entry();
        let exit_spot = result.spot_at_exit();

        // Compute HV at entry
        let entry_hv = self.compute_hv_at_time(symbol, entry_time, hv_window).await?;

        // Initialize position tracking
        let mut position = cs_domain::HedgePosition::new();
        let mut current_shares = 0i32;
        let mut snapshots = Vec::new();

        // Record entry spot for RV tracking
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(entry_time, entry_spot);
        }

        // Capture entry snapshot
        let entry_snapshot = snapshot_collector
            .capture_snapshot(trade, entry_time, entry_hv, current_shares, hedge_config.contract_multiplier)
            .await?;
        snapshots.push(entry_snapshot);

        // Rehedge at specified times
        for rehedge_time in rehedge_times {
            if let Some(max) = hedge_config.max_rehedges {
                if position.rehedge_count() >= max {
                    break;
                }
            }

            let spot = self.equity_repo
                .get_spot_price(symbol, rehedge_time)
                .await
                .map_err(|e| format!("Failed to get spot at {}: {}", rehedge_time, e))?;

            let spot_f64 = spot.to_f64();

            // Track for RV computation
            if let Some(ref mut tracker) = rv_tracker {
                tracker.record(rehedge_time, spot_f64);
            }

            // Recompute position delta using entry HV
            let position_delta = Self::compute_position_delta(
                trade,
                spot_f64,
                entry_hv,
                rehedge_time,
                0.05, // risk-free rate
                hedge_config.contract_multiplier,
            );

            // Calculate net delta (position delta + stock hedge delta)
            let stock_delta = current_shares as f64 / hedge_config.contract_multiplier as f64;
            let net_delta = position_delta + stock_delta;

            // Capture snapshot before rehedge
            let snapshot = snapshot_collector
                .capture_snapshot(trade, rehedge_time, entry_hv, current_shares, hedge_config.contract_multiplier)
                .await?;
            snapshots.push(snapshot);

            // Check if rehedge is needed
            if !hedge_config.should_rehedge(net_delta, spot_f64, 0.0) {
                continue;
            }

            // Calculate shares needed to neutralize
            let shares = hedge_config.shares_to_hedge(net_delta);
            if shares == 0 {
                continue;
            }

            let delta_before = net_delta;
            current_shares += shares;
            let stock_delta_after = current_shares as f64 / hedge_config.contract_multiplier as f64;
            let delta_after = position_delta + stock_delta_after;

            let action = cs_domain::HedgeAction {
                timestamp: rehedge_time,
                shares,
                spot_price: spot_f64,
                delta_before,
                delta_after,
                cost: hedge_config.transaction_cost_per_share * Decimal::from(shares.abs()),
            };

            position.add_hedge(action);
        }

        // Record exit spot
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(exit_time, exit_spot);
        }

        // Capture exit snapshot
        let exit_iv = result.exit_iv().map(|iv| iv.primary);
        let exit_snapshot = snapshot_collector
            .capture_snapshot(trade, exit_time, entry_hv, current_shares, hedge_config.contract_multiplier)
            .await?;
        snapshots.push(exit_snapshot);

        // Finalize position
        position.unrealized_pnl = position.calculate_pnl(exit_spot);

        // Attach RV metrics if tracking is enabled
        if let Some(tracker) = rv_tracker.take() {
            // Update tracker with entry_hv before finalizing
            let tracker_with_hv = RealizedVolatilityTracker {
                spot_history: tracker.spot_history.clone(),
                entry_hv: Some(entry_hv),
                entry_iv: tracker.entry_iv,
            };

            // Store spot history
            position.spot_history = tracker.spot_history.clone();

            // Compute and attach metrics
            position.realized_vol_metrics = Some(tracker_with_hv.finalize(exit_iv));
        }

        Ok((position, snapshots))
    }

    /// Hedge using Entry IV (Implied Volatility at trade entry)
    async fn hedge_with_entry_iv(
        &self,
        trade: &T,
        result: &<T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
        rv_tracker: &mut Option<RealizedVolatilityTracker>,
        snapshot_collector: &crate::attribution::SnapshotCollector,
    ) -> Result<(cs_domain::HedgePosition, Vec<cs_domain::PositionSnapshot>), String> {
        let hedge_config = self.hedge_config.as_ref().unwrap();
        let symbol = result.symbol();
        let entry_spot = result.spot_at_entry();
        let exit_spot = result.spot_at_exit();

        // Get IV at entry from result
        let entry_iv = result.entry_iv()
            .ok_or("Entry IV not available")?
            .primary;

        // Initialize position tracking
        let mut position = cs_domain::HedgePosition::new();
        let mut current_shares = 0i32;
        let mut snapshots = Vec::new();

        // Record entry spot for RV tracking
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(entry_time, entry_spot);
        }

        // Capture entry snapshot
        let entry_snapshot = snapshot_collector
            .capture_snapshot(trade, entry_time, entry_iv, current_shares, hedge_config.contract_multiplier)
            .await?;
        snapshots.push(entry_snapshot);

        // Rehedge at specified times
        for rehedge_time in rehedge_times {
            if let Some(max) = hedge_config.max_rehedges {
                if position.rehedge_count() >= max {
                    break;
                }
            }

            let spot = self.equity_repo
                .get_spot_price(symbol, rehedge_time)
                .await
                .map_err(|e| format!("Failed to get spot at {}: {}", rehedge_time, e))?;

            let spot_f64 = spot.to_f64();

            // Track for RV computation
            if let Some(ref mut tracker) = rv_tracker {
                tracker.record(rehedge_time, spot_f64);
            }

            // Recompute position delta using entry IV
            let position_delta = Self::compute_position_delta(
                trade,
                spot_f64,
                entry_iv,
                rehedge_time,
                0.05, // risk-free rate
                hedge_config.contract_multiplier,
            );

            // Calculate net delta (position delta + stock hedge delta)
            let stock_delta = current_shares as f64 / hedge_config.contract_multiplier as f64;
            let net_delta = position_delta + stock_delta;

            // Capture snapshot before rehedge
            let snapshot = snapshot_collector
                .capture_snapshot(trade, rehedge_time, entry_iv, current_shares, hedge_config.contract_multiplier)
                .await?;
            snapshots.push(snapshot);

            // Check if rehedge is needed
            if !hedge_config.should_rehedge(net_delta, spot_f64, 0.0) {
                continue;
            }

            // Calculate shares needed to neutralize
            let shares = hedge_config.shares_to_hedge(net_delta);
            if shares == 0 {
                continue;
            }

            let delta_before = net_delta;
            current_shares += shares;
            let stock_delta_after = current_shares as f64 / hedge_config.contract_multiplier as f64;
            let delta_after = position_delta + stock_delta_after;

            let action = cs_domain::HedgeAction {
                timestamp: rehedge_time,
                shares,
                spot_price: spot_f64,
                delta_before,
                delta_after,
                cost: hedge_config.transaction_cost_per_share * Decimal::from(shares.abs()),
            };

            position.add_hedge(action);
        }

        // Record exit spot
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(exit_time, exit_spot);
        }

        // Capture exit snapshot
        let exit_iv = result.exit_iv().map(|iv| iv.primary).unwrap_or(entry_iv);
        let exit_snapshot = snapshot_collector
            .capture_snapshot(trade, exit_time, exit_iv, current_shares, hedge_config.contract_multiplier)
            .await?;
        snapshots.push(exit_snapshot);

        // Finalize position
        position.unrealized_pnl = position.calculate_pnl(exit_spot);

        // Attach RV metrics if tracking is enabled
        if let Some(tracker) = rv_tracker.take() {
            let exit_iv_opt = result.exit_iv().map(|iv| iv.primary);

            // Store spot history
            position.spot_history = tracker.spot_history.clone();

            // Compute and attach metrics
            position.realized_vol_metrics = Some(tracker.finalize(exit_iv_opt));
        }

        Ok((position, snapshots))
    }

    /// Hedge using Current HV (recompute HV at each rehedge)
    async fn hedge_with_current_hv(
        &self,
        trade: &T,
        result: &<T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
        hv_window: u32,
        rv_tracker: &mut Option<RealizedVolatilityTracker>,
        snapshot_collector: &crate::attribution::SnapshotCollector,
    ) -> Result<(cs_domain::HedgePosition, Vec<cs_domain::PositionSnapshot>), String> {
        let hedge_config = self.hedge_config.as_ref().unwrap();
        let symbol = result.symbol();
        let entry_spot = result.spot_at_entry();
        let exit_spot = result.spot_at_exit();

        // Compute HV at entry for RV metrics and first snapshot
        let entry_hv = self.compute_hv_at_time(symbol, entry_time, hv_window).await?;

        // Initialize position tracking
        let mut position = cs_domain::HedgePosition::new();
        let mut current_shares = 0i32;
        let mut snapshots = Vec::new();

        // Record entry spot for RV tracking
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(entry_time, entry_spot);
        }

        // Capture entry snapshot with entry HV
        let entry_snapshot = snapshot_collector
            .capture_snapshot(trade, entry_time, entry_hv, current_shares, hedge_config.contract_multiplier)
            .await?;
        snapshots.push(entry_snapshot);

        // Rehedge at specified times
        for rehedge_time in rehedge_times {
            if let Some(max) = hedge_config.max_rehedges {
                if position.rehedge_count() >= max {
                    break;
                }
            }

            let spot = self.equity_repo
                .get_spot_price(symbol, rehedge_time)
                .await
                .map_err(|e| format!("Failed to get spot at {}: {}", rehedge_time, e))?;

            let spot_f64 = spot.to_f64();

            // Track for RV computation
            if let Some(ref mut tracker) = rv_tracker {
                tracker.record(rehedge_time, spot_f64);
            }

            // Recompute HV at current time
            let current_hv = self.compute_hv_at_time(symbol, rehedge_time, hv_window).await?;

            // Recompute position delta using current HV
            let position_delta = Self::compute_position_delta(
                trade,
                spot_f64,
                current_hv,
                rehedge_time,
                0.05,
                hedge_config.contract_multiplier,
            );

            // Calculate net delta
            let stock_delta = current_shares as f64 / hedge_config.contract_multiplier as f64;
            let net_delta = position_delta + stock_delta;

            // Capture snapshot with current HV (before rehedge)
            let snapshot = snapshot_collector
                .capture_snapshot(trade, rehedge_time, current_hv, current_shares, hedge_config.contract_multiplier)
                .await?;
            snapshots.push(snapshot);

            // Check if rehedge is needed
            if !hedge_config.should_rehedge(net_delta, spot_f64, 0.0) {
                continue;
            }

            // Calculate shares needed
            let shares = hedge_config.shares_to_hedge(net_delta);
            if shares == 0 {
                continue;
            }

            let delta_before = net_delta;
            current_shares += shares;
            let stock_delta_after = current_shares as f64 / hedge_config.contract_multiplier as f64;
            let delta_after = position_delta + stock_delta_after;

            let action = cs_domain::HedgeAction {
                timestamp: rehedge_time,
                shares,
                spot_price: spot_f64,
                delta_before,
                delta_after,
                cost: hedge_config.transaction_cost_per_share * Decimal::from(shares.abs()),
            };

            position.add_hedge(action);
        }

        // Record exit spot
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(exit_time, exit_spot);
        }

        // Compute HV at exit and capture exit snapshot
        let exit_hv = self.compute_hv_at_time(symbol, exit_time, hv_window).await?;
        let exit_snapshot = snapshot_collector
            .capture_snapshot(trade, exit_time, exit_hv, current_shares, hedge_config.contract_multiplier)
            .await?;
        snapshots.push(exit_snapshot);

        // Finalize position
        position.unrealized_pnl = position.calculate_pnl(exit_spot);

        // Attach RV metrics if tracking is enabled
        if let Some(tracker) = rv_tracker.take() {
            let exit_iv = result.exit_iv().map(|iv| iv.primary);

            let tracker_with_hv = RealizedVolatilityTracker {
                spot_history: tracker.spot_history.clone(),
                entry_hv: Some(entry_hv),
                entry_iv: tracker.entry_iv,
            };

            position.spot_history = tracker.spot_history.clone();
            position.realized_vol_metrics = Some(tracker_with_hv.finalize(exit_iv));
        }

        Ok((position, snapshots))
    }

    /// Hedge using Current Market IV (build IV surface at each rehedge)
    async fn hedge_with_current_market_iv(
        &self,
        trade: &T,
        result: &<T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
        rv_tracker: &mut Option<RealizedVolatilityTracker>,
    ) -> Result<cs_domain::HedgePosition, String> {
        use crate::iv_surface_builder::build_iv_surface;
        use cs_analytics::bs_delta;

        let hedge_config = self.hedge_config.as_ref().unwrap();
        let symbol = result.symbol();
        let entry_spot = result.spot_at_entry();
        let exit_spot = result.spot_at_exit();

        // Initialize position tracking
        let mut position = cs_domain::HedgePosition::new();
        let mut current_shares = 0i32;

        // Record entry spot for RV tracking
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(entry_time, entry_spot);
        }

        // Rehedge at specified times
        for rehedge_time in rehedge_times {
            if let Some(max) = hedge_config.max_rehedges {
                if position.rehedge_count() >= max {
                    break;
                }
            }

            let spot = self.equity_repo
                .get_spot_price(symbol, rehedge_time)
                .await
                .map_err(|e| format!("Failed to get spot at {}: {}", rehedge_time, e))?;

            let spot_f64 = spot.to_f64();

            // Track for RV computation
            if let Some(ref mut tracker) = rv_tracker {
                tracker.record(rehedge_time, spot_f64);
            }

            // Build IV surface at current time
            let chain_df = self.options_repo
                .get_option_bars(symbol, rehedge_time.date_naive())
                .await
                .map_err(|e| format!("Failed to get option chain at {}: {}", rehedge_time, e))?;

            let iv_surface = build_iv_surface(&chain_df, spot_f64, rehedge_time, symbol)
                .ok_or_else(|| format!("Failed to build IV surface at {}", rehedge_time))?;

            // Compute position delta using current market IVs
            let mut position_delta = 0.0;
            for (leg, leg_position) in trade.legs() {
                let tte = (leg.expiration - rehedge_time.date_naive()).num_days() as f64 / 365.0;
                if tte <= 0.0 {
                    continue;
                }

                let strike_f64 = leg.strike.value().to_f64().unwrap_or(0.0);
                let is_call = leg.option_type == finq_core::OptionType::Call;

                // Get IV from surface for this specific leg
                let leg_iv = iv_surface
                    .get_iv(leg.strike.value(), leg.expiration, is_call)
                    .unwrap_or(0.30); // Fallback to 30% if not found

                let leg_delta = bs_delta(spot_f64, strike_f64, tte, leg_iv, is_call, 0.05);
                position_delta += leg_delta * leg_position.sign() * hedge_config.contract_multiplier as f64;
            }

            // Calculate net delta
            let stock_delta = current_shares as f64 / hedge_config.contract_multiplier as f64;
            let net_delta = position_delta + stock_delta;

            // Check if rehedge is needed
            if !hedge_config.should_rehedge(net_delta, spot_f64, 0.0) {
                continue;
            }

            // Calculate shares needed
            let shares = hedge_config.shares_to_hedge(net_delta);
            if shares == 0 {
                continue;
            }

            let delta_before = net_delta;
            current_shares += shares;
            let stock_delta_after = current_shares as f64 / hedge_config.contract_multiplier as f64;
            let delta_after = position_delta + stock_delta_after;

            let action = cs_domain::HedgeAction {
                timestamp: rehedge_time,
                shares,
                spot_price: spot_f64,
                delta_before,
                delta_after,
                cost: hedge_config.transaction_cost_per_share * Decimal::from(shares.abs()),
            };

            position.add_hedge(action);
        }

        // Record exit spot
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(exit_time, exit_spot);
        }

        // Finalize position
        position.unrealized_pnl = position.calculate_pnl(exit_spot);

        // Attach RV metrics if tracking is enabled
        if let Some(tracker) = rv_tracker.take() {
            let exit_iv = result.exit_iv().map(|iv| iv.primary);
            position.spot_history = tracker.spot_history.clone();
            position.realized_vol_metrics = Some(tracker.finalize(exit_iv));
        }

        Ok(position)
    }

    /// Hedge using Historical Average IV
    async fn hedge_with_historical_average_iv(
        &self,
        trade: &T,
        result: &<T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
        lookback_days: u32,
        rv_tracker: &mut Option<RealizedVolatilityTracker>,
    ) -> Result<cs_domain::HedgePosition, String> {
        use crate::iv_surface_builder::build_iv_surface;
        use cs_analytics::bs_delta;

        let hedge_config = self.hedge_config.as_ref().unwrap();
        let symbol = result.symbol();
        let entry_spot = result.spot_at_entry();
        let exit_spot = result.spot_at_exit();

        // Initialize position tracking
        let mut position = cs_domain::HedgePosition::new();
        let mut current_shares = 0i32;

        // Record entry spot for RV tracking
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(entry_time, entry_spot);
        }

        // Rehedge at specified times
        for rehedge_time in rehedge_times {
            if let Some(max) = hedge_config.max_rehedges {
                if position.rehedge_count() >= max {
                    break;
                }
            }

            let spot = self.equity_repo
                .get_spot_price(symbol, rehedge_time)
                .await
                .map_err(|e| format!("Failed to get spot at {}: {}", rehedge_time, e))?;

            let spot_f64 = spot.to_f64();

            // Track for RV computation
            if let Some(ref mut tracker) = rv_tracker {
                tracker.record(rehedge_time, spot_f64);
            }

            // Compute historical average IV for each leg
            let mut position_delta = 0.0;
            for (leg, leg_position) in trade.legs() {
                let tte = (leg.expiration - rehedge_time.date_naive()).num_days() as f64 / 365.0;
                if tte <= 0.0 {
                    continue;
                }

                // Compute average IV for this leg over lookback window
                let avg_iv = self.compute_historical_average_iv_for_leg(
                    symbol,
                    leg.strike.value(),
                    leg.expiration,
                    leg.option_type == finq_core::OptionType::Call,
                    rehedge_time.date_naive(),
                    lookback_days,
                )
                .await
                .unwrap_or(0.30); // Fallback to 30% if computation fails

                let strike_f64 = leg.strike.value().to_f64().unwrap_or(0.0);
                let is_call = leg.option_type == finq_core::OptionType::Call;

                let leg_delta = bs_delta(spot_f64, strike_f64, tte, avg_iv, is_call, 0.05);
                position_delta += leg_delta * leg_position.sign() * hedge_config.contract_multiplier as f64;
            }

            // Calculate net delta
            let stock_delta = current_shares as f64 / hedge_config.contract_multiplier as f64;
            let net_delta = position_delta + stock_delta;

            // Check if rehedge is needed
            if !hedge_config.should_rehedge(net_delta, spot_f64, 0.0) {
                continue;
            }

            // Calculate shares needed
            let shares = hedge_config.shares_to_hedge(net_delta);
            if shares == 0 {
                continue;
            }

            let delta_before = net_delta;
            current_shares += shares;
            let stock_delta_after = current_shares as f64 / hedge_config.contract_multiplier as f64;
            let delta_after = position_delta + stock_delta_after;

            let action = cs_domain::HedgeAction {
                timestamp: rehedge_time,
                shares,
                spot_price: spot_f64,
                delta_before,
                delta_after,
                cost: hedge_config.transaction_cost_per_share * Decimal::from(shares.abs()),
            };

            position.add_hedge(action);
        }

        // Record exit spot
        if let Some(ref mut tracker) = rv_tracker {
            tracker.record(exit_time, exit_spot);
        }

        // Finalize position
        position.unrealized_pnl = position.calculate_pnl(exit_spot);

        // Attach RV metrics if tracking is enabled
        if let Some(tracker) = rv_tracker.take() {
            let exit_iv = result.exit_iv().map(|iv| iv.primary);
            position.spot_history = tracker.spot_history.clone();
            position.realized_vol_metrics = Some(tracker.finalize(exit_iv));
        }

        Ok(position)
    }

    /// Compute historical average IV for a specific leg over lookback window
    async fn compute_historical_average_iv_for_leg(
        &self,
        symbol: &str,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
        current_date: NaiveDate,
        lookback_days: u32,
    ) -> Result<f64, String> {
        use crate::iv_surface_builder::build_iv_surface;
        use cs_domain::TradingCalendar;

        let end_date = current_date;
        let start_date = current_date - chrono::Duration::days(lookback_days as i64);

        let trading_days: Vec<NaiveDate> = TradingCalendar::trading_days_between(start_date, end_date)
            .collect();

        if trading_days.is_empty() {
            return Err("No trading days in lookback window".to_string());
        }

        let mut ivs = Vec::new();

        // Sample IVs from historical days (limit to avoid performance issues)
        let sample_size = trading_days.len().min(10); // Sample at most 10 days
        let step = if trading_days.len() > sample_size {
            trading_days.len() / sample_size
        } else {
            1
        };

        for (idx, date) in trading_days.iter().enumerate() {
            if idx % step != 0 {
                continue; // Skip non-sampled days
            }

            // Get option chain for this historical day
            let chain_df = match self.options_repo.get_option_bars(symbol, *date).await {
                Ok(df) => df,
                Err(_) => continue, // Skip days with missing data
            };

            // Get spot price for this day (use close price from bars)
            let bars_df = match self.equity_repo.get_bars(symbol, *date).await {
                Ok(df) => df,
                Err(_) => continue,
            };

            // Extract close price as spot (last bar's close)
            let spot = if let Ok(close_series) = bars_df.column("close") {
                if let Ok(close_values) = close_series.f64() {
                    if let Some(last_close) = close_values.last() {
                        last_close
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            } else {
                continue;
            };

            // Build IV surface
            let pricing_time = cs_domain::eastern_to_utc(
                *date,
                cs_domain::MarketTime::DEFAULT_ENTRY.to_naive_time(),
            );
            let iv_surface = match build_iv_surface(&chain_df, spot, pricing_time, symbol) {
                Some(surface) => surface,
                None => continue,
            };

            // Get IV for this specific leg
            if let Some(iv) = iv_surface.get_iv(strike, expiration, is_call) {
                ivs.push(iv);
            }
        }

        if ivs.is_empty() {
            return Err("Could not compute historical IV - no valid data points".to_string());
        }

        // Return average IV
        Ok(ivs.iter().sum::<f64>() / ivs.len() as f64)
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
