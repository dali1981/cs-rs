use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_domain::{
    EquityDataRepository, OptionsDataRepository, MarketTime,
    RollPolicy, RollPeriod, RollReason, RollingStraddleResult,
    Straddle, StraddleResult, TradingCalendar,
};

use crate::straddle_executor::StraddleExecutor;

/// Executor for rolling straddle strategies
///
/// Unlike single-trade executor, this:
/// - Enters new ATM position each roll period
/// - Tracks cumulative P&L across rolls
/// - Maintains rolling campaign from start to end date
pub struct RollingStraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    straddle_executor: StraddleExecutor<O, E>,
    equity_repo: Arc<E>,
    roll_policy: RollPolicy,
}

impl<O, E> RollingStraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(
        straddle_executor: StraddleExecutor<O, E>,
        equity_repo: Arc<E>,
        roll_policy: RollPolicy,
    ) -> Self {
        Self {
            straddle_executor,
            equity_repo,
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
    ) -> RollingStraddleResult {
        let mut rolls = Vec::new();
        let mut current_date = start_date;

        while current_date < end_date {
            // Find ATM straddle at current spot
            let straddle = match self.find_atm_straddle(symbol, current_date).await {
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

            let result = self.straddle_executor
                .execute_trade(&straddle, &earnings_event, entry_dt, exit_dt)
                .await;

            // Convert to RollPeriod
            let roll_period = self.to_roll_period(result, roll_reason);
            rolls.push(roll_period);

            // Move to next trading day after exit
            current_date = TradingCalendar::next_trading_day(exit_date);
        }

        // Aggregate results
        RollingStraddleResult::from_rolls(
            symbol.to_string(),
            start_date,
            end_date,
            self.roll_policy.description(),
            rolls,
        )
    }

    /// Find ATM straddle at given date
    async fn find_atm_straddle(
        &self,
        symbol: &str,
        date: NaiveDate,
    ) -> Result<Straddle, String> {
        use finq_core::OptionType;

        // Get spot price
        let dt = self.to_datetime(date, MarketTime { hour: 15, minute: 45 });
        let spot = self.equity_repo
            .get_spot_price(symbol, dt)
            .await
            .map_err(|e| format!("Failed to get spot: {}", e))?;

        // Find nearest strike
        let strike_value = spot.to_f64().round() as u32;
        let strike = cs_domain::Strike::new(Decimal::from(strike_value))
            .map_err(|e| format!("Invalid strike: {}", e))?;

        // Find next available expiration (simplified - should use expiration selection)
        let expiration = date + chrono::Duration::days(7);  // Assume 1-week expiry

        // Create option legs
        let call_leg = cs_domain::OptionLeg {
            symbol: symbol.to_string(),
            strike,
            expiration,
            option_type: OptionType::Call,
        };

        let put_leg = cs_domain::OptionLeg {
            symbol: symbol.to_string(),
            strike,
            expiration,
            option_type: OptionType::Put,
        };

        Straddle::new(call_leg, put_leg)
            .map_err(|e| format!("Failed to create straddle: {}", e))
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

            hedge_pnl: result.hedge_pnl,
            hedge_count: result.hedge_position.as_ref().map(|p| p.rehedge_count()).unwrap_or(0),
            transaction_cost: result.hedge_position.as_ref()
                .map(|p| p.total_cost)
                .unwrap_or(Decimal::ZERO),

            roll_reason,
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
