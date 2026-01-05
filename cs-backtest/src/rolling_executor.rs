//! Generic rolling executor for any trade type
//!
//! This module provides RollingExecutor<T> which can roll ANY trade type
//! that implements both RollableTrade and ExecutableTrade traits.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_domain::{
    EquityDataRepository, OptionsDataRepository, MarketTime,
    RollPolicy, RollPeriod, RollReason, RollingResult,
    TradeFactory, TradingCalendar,
    RollableTrade, TradeResult,
    EarningsEvent, EarningsTime,
};

use crate::execution::{ExecutableTrade, ExecutionConfig, execute_trade};

/// Generic executor for rolling any trade type
///
/// This executor can roll straddles, calendar spreads, iron butterflies,
/// or any other trade type that implements RollableTrade + ExecutableTrade.
///
/// # Example
/// ```ignore
/// // Rolling straddles
/// let executor = RollingExecutor::<Straddle>::new(...);
/// let result = executor.execute_rolling("AAPL", ...).await;
///
/// // Rolling calendar spreads (same code!)
/// let executor = RollingExecutor::<CalendarSpread>::new(...);
/// let result = executor.execute_rolling("AAPL", ...).await;
/// ```
pub struct RollingExecutor<T>
where
    T: RollableTrade + ExecutableTrade,
{
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    pricer: T::Pricer,
    trade_factory: Arc<dyn TradeFactory>,
    roll_policy: RollPolicy,
    config: ExecutionConfig,
}

impl<T> RollingExecutor<T>
where
    T: RollableTrade + ExecutableTrade,
{
    pub fn new(
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        pricer: T::Pricer,
        trade_factory: Arc<dyn TradeFactory>,
        roll_policy: RollPolicy,
        config: ExecutionConfig,
    ) -> Self {
        Self {
            options_repo,
            equity_repo,
            pricer,
            trade_factory,
            roll_policy,
            config,
        }
    }

    /// Execute rolling strategy for any trade type
    ///
    /// Continuously rolls trades from start_date to end_date
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
            )
            .await
            {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Failed to create trade at {}: {}", current_date, e);
                    current_date = TradingCalendar::next_trading_day(current_date);
                    continue;
                }
            };

            // Determine exit date
            let (exit_date, roll_reason) = self.determine_exit_date(
                current_date,
                end_date,
                trade.expiration(),
            );

            let exit_dt = self.to_datetime(exit_date, exit_time);

            // Execute trade - fully generic, no unsafe!
            let result = self.execute_trade(&trade, entry_dt, exit_dt).await;

            // Convert to RollPeriod
            let roll_period = self.to_roll_period(&trade, result, roll_reason);
            rolls.push(roll_period);

            // Next roll
            current_date = TradingCalendar::next_trading_day(exit_date);
        }

        // Get trade type name from first trade or use generic
        let trade_type = if rolls.is_empty() {
            std::any::type_name::<T>()
                .split("::")
                .last()
                .unwrap_or("unknown")
                .to_lowercase()
        } else {
            std::any::type_name::<T>()
                .split("::")
                .last()
                .unwrap_or("unknown")
                .to_lowercase()
        };

        RollingResult::from_rolls(
            symbol.to_string(),
            start_date,
            end_date,
            self.roll_policy.description(),
            trade_type,
            rolls,
        )
    }

    /// Execute trade - generic, type-safe, no unsafe!
    async fn execute_trade(
        &self,
        trade: &T,
        entry: DateTime<Utc>,
        exit: DateTime<Utc>,
    ) -> <T as ExecutableTrade>::Result {
        // Create dummy earnings event (not used for rolling)
        let earnings_event = EarningsEvent::new(
            ExecutableTrade::symbol(trade).to_string(),
            exit.date_naive(),
            EarningsTime::AfterMarketClose,
        );

        // Generic execution - works for ANY trade type!
        execute_trade(
            trade,
            &self.pricer,
            self.options_repo.as_ref(),
            self.equity_repo.as_ref(),
            &self.config,
            &earnings_event,
            entry,
            exit,
        )
        .await
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
            None => campaign_end, // No rolling, hold until end
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

    /// Convert trade result to RollPeriod
    fn to_roll_period(
        &self,
        trade: &T,
        result: <T as ExecutableTrade>::Result,
        roll_reason: RollReason,
    ) -> RollPeriod
    where
        <T as ExecutableTrade>::Result: TradeResult,
    {
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
                / result.spot_at_entry()
                * 100.0),

            // These fields would need to be added to TradeResult trait
            // For now, use defaults
            iv_entry: None,
            iv_exit: None,
            iv_change: None,

            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,

            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,

            hedge_pnl: result.hedge_pnl(),
            hedge_count: 0, // Would need to add to TradeResult
            transaction_cost: Decimal::ZERO,

            roll_reason,

            position_attribution: None, // Would need to add to TradeResult
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
