use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_domain::{
    EquityDataRepository, OptionsDataRepository, MarketTime,
    RollPolicy, RollPeriod, RollReason, RollingResult,
    Straddle, StraddleResult, TradingCalendar, TradeFactory,
};

use crate::trade_orchestrator::TradeOrchestrator;

/// Executor for rolling straddle strategies with optional hedging
///
/// Unlike single-trade executor, this:
/// - Enters new ATM position each roll period
/// - Tracks cumulative P&L across rolls
/// - Maintains rolling campaign from start to end date
/// - Supports delta hedging if configured in TradeOrchestrator
/// - Uses TradeFactory to construct straddles with REAL expirations from market data
pub struct RollingStraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    unified_executor: TradeOrchestrator<O, E>,
    trade_factory: Arc<dyn TradeFactory>,
    roll_policy: RollPolicy,
}

impl<O, E> RollingStraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(
        unified_executor: TradeOrchestrator<O, E>,
        trade_factory: Arc<dyn TradeFactory>,
        roll_policy: RollPolicy,
    ) -> Self {
        Self {
            unified_executor,
            trade_factory,
            roll_policy,
        }
    }

    /// Execute rolling straddle strategy
    ///
    /// Continuously rolls ATM straddles from start_date to end_date
    /// according to the roll policy.
    pub async fn execute_rolling(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        entry_time: MarketTime,
        exit_time: MarketTime,
    ) -> RollingResult {
        let mut rolls = Vec::new();
        let mut current_date = start_date;

        while current_date < end_date {
            // Find ATM straddle at current spot using entry_time to avoid look-ahead bias
            let straddle = match self.find_atm_straddle(symbol, current_date, entry_time).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to find ATM straddle at {}: {}", current_date, e);
                    // Try next day
                    current_date = current_date + chrono::Duration::days(1);
                    continue;
                }
            };

            // Determine exit date for this roll
            let (exit_date, roll_reason) = self.determine_exit_date(
                current_date,
                end_date,
                straddle.expiration(),
            );

            // Execute this leg
            let entry_dt = self.to_datetime(current_date, entry_time);
            let exit_dt = self.to_datetime(exit_date, exit_time);

            // Use default earnings event (not used for rolling, just a placeholder)
            let earnings_event = cs_domain::EarningsEvent::new(
                symbol.to_string(),
                end_date,
                cs_domain::EarningsTime::AfterMarketClose,
            );

            let result = self.unified_executor
                .execute_straddle(&straddle, &earnings_event, entry_dt, exit_dt)
                .await;

            // Convert to RollPeriod
            let roll_period = self.to_roll_period(result, roll_reason);
            rolls.push(roll_period);

            // Move to next trading day after exit
            current_date = TradingCalendar::next_trading_day(exit_date);
        }

        // Aggregate results
        RollingResult::from_rolls(
            symbol.to_string(),
            start_date,
            end_date,
            self.roll_policy.description(),
            "straddle".to_string(),
            rolls,
        )
    }

    /// Find ATM straddle at given date using real market data
    ///
    /// This method delegates to the TradeFactory which:
    /// 1. Queries the option chain from the repository
    /// 2. Builds an IV surface with available strikes and expirations
    /// 3. Selects the ATM strike (closest to spot)
    /// 4. Selects the first valid expiration after min_expiration
    ///
    /// Unlike the old implementation, this uses REAL expiration dates from
    /// the options chain, not hardcoded date arithmetic.
    ///
    /// IMPORTANT: Uses entry_time to avoid look-ahead bias. Strike selection
    /// must use the same time as trade entry to ensure delta-neutral entry.
    async fn find_atm_straddle(
        &self,
        symbol: &str,
        date: NaiveDate,
        entry_time: MarketTime,
    ) -> Result<Straddle, String> {
        // Use entry_time for strike selection to avoid look-ahead bias
        let dt = self.to_datetime(date, entry_time);

        // Require options to expire at least 1 day after entry
        // This ensures we don't select same-day expirations
        let min_expiration = date + chrono::Duration::days(1);

        // Delegate to factory - uses REAL expirations from market data
        self.trade_factory
            .create_atm_straddle(symbol, dt, min_expiration)
            .await
            .map_err(|e| format!("Trade factory error: {}", e))
    }

    /// Determine when to exit this roll period
    fn determine_exit_date(
        &self,
        entry_date: NaiveDate,
        campaign_end: NaiveDate,
        option_expiration: NaiveDate,
    ) -> (NaiveDate, RollReason) {
        // Check if campaign ends before next roll
        if campaign_end <= entry_date {
            return (entry_date, RollReason::EndOfCampaign);
        }

        // Get next roll date based on policy
        let next_roll = match self.roll_policy.next_roll_date(entry_date) {
            Some(date) => date,
            None => campaign_end,  // No rolling, hold until end
        };

        // Choose earliest of: next_roll, option_expiration, campaign_end
        let exit_date = next_roll.min(option_expiration).min(campaign_end);

        // Determine reason
        let reason = if exit_date >= campaign_end {
            RollReason::EndOfCampaign
        } else if exit_date >= option_expiration {
            RollReason::Expiry
        } else {
            RollReason::Scheduled
        };

        (exit_date, reason)
    }

    /// Convert StraddleResult to RollPeriod
    fn to_roll_period(&self, result: StraddleResult, roll_reason: RollReason) -> RollPeriod {
        RollPeriod {
            entry_date: result.entry_time.date_naive(),
            exit_date: result.exit_time.date_naive(),
            strike: result.strike.value(),  // Already a Decimal
            expiration: result.expiration,

            entry_debit: result.entry_debit,
            exit_credit: result.exit_credit,
            pnl: result.pnl,

            spot_at_entry: result.spot_at_entry,
            spot_at_exit: result.spot_at_exit,
            spot_move_pct: result.spot_move_pct,

            iv_entry: result.iv_entry,
            iv_exit: result.iv_exit,
            iv_change: result.iv_change,

            net_delta: result.net_delta,
            net_gamma: result.net_gamma,
            net_theta: result.net_theta,
            net_vega: result.net_vega,

            // P&L Attribution
            delta_pnl: result.delta_pnl,
            gamma_pnl: result.gamma_pnl,
            theta_pnl: result.theta_pnl,
            vega_pnl: result.vega_pnl,
            unexplained_pnl: result.unexplained_pnl,

            hedge_pnl: result.hedge_pnl,
            hedge_count: result.hedge_position.as_ref().map(|p| p.rehedge_count()).unwrap_or(0),
            transaction_cost: result.hedge_position.as_ref()
                .map(|p| p.total_cost)
                .unwrap_or(Decimal::ZERO),

            roll_reason,

            position_attribution: result.position_attribution,
        }
    }

    /// Helper to convert date + time to DateTime<Utc>
    fn to_datetime(&self, date: NaiveDate, time: MarketTime) -> DateTime<Utc> {
        use chrono::NaiveTime;
        use cs_domain::datetime::eastern_to_utc;

        let naive_time = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)
            .unwrap_or_else(|| NaiveTime::from_hms_opt(15, 45, 0).unwrap());

        // Properly convert Eastern time to UTC (handles EST/EDT automatically)
        eastern_to_utc(date, naive_time)
    }
}
