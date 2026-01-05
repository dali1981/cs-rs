use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_domain::{
    EarningsEvent, StraddleResult, HedgeConfig, HedgeState,
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

        // 2. Initialize hedge state from option position at entry
        let mut hedge_state = HedgeState::new(
            self.hedge_config.clone(),
            base_result.net_delta.unwrap_or(0.0),
            base_result.net_gamma.unwrap_or(0.0),
            base_result.spot_at_entry,
        );

        // 3. Process each rehedge time - state machine handles all logic
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

        // 4. Finalize hedge state and calculate P&L
        let hedge_position = hedge_state.finalize(base_result.spot_at_exit);
        let hedge_pnl = hedge_position.calculate_pnl(base_result.spot_at_exit);
        let total_pnl = base_result.pnl + hedge_pnl - hedge_position.total_cost;

        // 5. Return enhanced result
        base_result.hedge_position = Some(hedge_position);
        base_result.hedge_pnl = Some(hedge_pnl);
        base_result.total_pnl_with_hedge = Some(total_pnl);

        base_result
    }
}
