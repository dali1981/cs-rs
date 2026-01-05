//! Unified trade executor with hedging support
//!
//! This executor combines the capabilities of:
//! - RollingExecutor: Generic rolling execution for any trade type
//! - TradeOrchestrator: Delta hedging for trade results
//!
//! Key improvement: Rolling execution now supports hedging!

use chrono::{DateTime, NaiveDate, NaiveTime, Utc, Weekday};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_domain::{
    EquityDataRepository, OptionsDataRepository, MarketTime,
    RollPolicy, RollPeriod, RollReason, RollingResult,
    TradeFactory, TradingCalendar,
    RollableTrade, TradeResult,
    EarningsEvent, EarningsTime,
    HedgeConfig, HedgeState,
};

use crate::execution::{ExecutableTrade, ExecutionConfig, execute_trade};
use crate::timing_strategy::TimingStrategy;

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
    T: RollableTrade + ExecutableTrade,
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
    T: RollableTrade + ExecutableTrade,
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

                if let Err(e) = self.apply_hedging(&mut result, entry_time, exit_time, rehedge_times).await {
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
    /// Copied from TradeOrchestrator - now shared by all execution paths
    async fn apply_hedging(
        &self,
        result: &mut <T as ExecutableTrade>::Result,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
    ) -> Result<(), String> {
        let hedge_config = self.hedge_config.as_ref()
            .ok_or("Hedge config not set")?;

        // Get net delta/gamma from the trade result (trade-agnostic)
        let net_delta = result.net_delta().unwrap_or(0.0);
        let net_gamma = result.net_gamma().unwrap_or(0.0);
        let entry_spot = result.spot_at_entry();
        let exit_spot = result.spot_at_exit();
        let symbol = result.symbol();

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

            hedge_state.update(rehedge_time, spot.to_f64());
        }

        // Finalize hedge position
        let hedge_position = hedge_state.finalize(exit_spot);

        // Apply hedge results if any rehedges occurred
        if hedge_position.rehedge_count() > 0 {
            let hedge_pnl = hedge_position.calculate_pnl(exit_spot);
            let total_pnl = result.pnl() + hedge_pnl - hedge_position.total_cost;

            result.apply_hedge_results(hedge_position, hedge_pnl, total_pnl, None);
        }

        Ok(())
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

            // IV from result
            iv_entry: result.iv_entry(),
            iv_exit: result.iv_exit(),
            iv_change: result.iv_change(),

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
