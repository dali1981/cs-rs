// cs-backtest: Backtest execution engine
//
// BacktestUseCase, TradeExecutor, parallel processing.

pub mod config;
pub mod backtest_use_case;
pub mod trade_executor;

pub use config::{BacktestConfig, StrategyType};
pub use backtest_use_case::{BacktestUseCase, BacktestResult, SessionProgress};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder() {
        // TODO: Implement when modules are ready
        assert!(true);
    }
}
