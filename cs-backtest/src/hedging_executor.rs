use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_domain::{
    EarningsEvent, StraddleResult, HedgeConfig, HedgePosition, HedgeAction,
    EquityDataRepository, OptionsDataRepository, Straddle,
};

use crate::straddle_executor::StraddleExecutor;

/// Executor wrapper that adds delta hedging to straddle positions
pub struct HedgingExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    inner_executor: StraddleExecutor<O, E>,
    equity_repo: Arc<E>,
    hedge_config: HedgeConfig,
}

impl<O, E> HedgingExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(
        inner_executor: StraddleExecutor<O, E>,
        equity_repo: Arc<E>,
        hedge_config: HedgeConfig,
    ) -> Self {
        Self {
            inner_executor,
            equity_repo,
            hedge_config,
        }
    }

    /// Execute trade with delta hedging
    pub async fn execute_with_hedging(
        &self,
        straddle: &Straddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
    ) -> StraddleResult {
        // 1. Execute base trade (entry and exit pricing)
        let mut base_result = self
            .inner_executor
            .execute_trade(straddle, event, entry_time, exit_time)
            .await;

        if !base_result.success || !self.hedge_config.is_enabled() {
            return base_result;
        }

        // 2. Initialize hedge position
        let mut hedge_position = HedgePosition::new();
        let mut current_delta = base_result.net_delta.unwrap_or(0.0);

        // 3. Process each rehedge time
        for rehedge_time in rehedge_times {
            // Skip if we've hit max rehedges
            if let Some(max) = self.hedge_config.max_rehedges {
                if hedge_position.rehedge_count() >= max {
                    break;
                }
            }

            // Get spot price at rehedge time
            let spot = match self
                .equity_repo
                .get_spot_price(straddle.symbol(), rehedge_time)
                .await
            {
                Ok(s) => s.to_f64(),
                Err(_) => continue, // Skip if no data
            };

            // Recompute delta using gamma approximation
            // delta_new ≈ delta_old + gamma × (spot_new - spot_old)
            let gamma = base_result.net_gamma.unwrap_or(0.0);
            let spot_change = spot - base_result.spot_at_entry;
            let new_delta = base_result.net_delta.unwrap_or(0.0) + gamma * spot_change;

            // Check if rehedge needed
            if !self.hedge_config.should_rehedge(new_delta, spot, gamma) {
                current_delta = new_delta;
                continue;
            }

            // Calculate shares needed to hedge
            let shares = self.hedge_config.shares_to_hedge(new_delta);
            if shares == 0 {
                continue;
            }

            // Calculate transaction cost
            let cost = self.hedge_config.transaction_cost_per_share
                * Decimal::from(shares.abs());

            // Record hedge action
            let delta_after = new_delta + (shares as f64 / self.hedge_config.contract_multiplier as f64);
            let action = HedgeAction {
                timestamp: rehedge_time,
                shares,
                spot_price: spot,
                delta_before: new_delta,
                delta_after,
                cost,
            };

            hedge_position.add_hedge(action);

            // Update current delta (now includes stock position)
            current_delta = delta_after;
        }

        // 4. Calculate hedge P&L at exit
        let hedge_pnl = hedge_position.calculate_pnl(base_result.spot_at_exit);
        let total_pnl = base_result.pnl + hedge_pnl - hedge_position.total_cost;

        // 5. Return enhanced result
        base_result.hedge_position = Some(hedge_position);
        base_result.hedge_pnl = Some(hedge_pnl);
        base_result.total_pnl_with_hedge = Some(total_pnl);

        base_result
    }
}
