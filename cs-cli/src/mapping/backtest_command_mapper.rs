//! Maps `BacktestConfig` (TOML DTO) → `RunBacktestCommand` (application command).
//!
//! This is the explicit application-layer mapping described in ADR-0003.
//! Infrastructure fields (`data_source`, `earnings_source`, deprecated `data_dir`)
//! are intentionally excluded — they are passed separately to the factory.

use cs_backtest::{
    BacktestConfig, RunBacktestCommand,
    BacktestPeriod, StrategySpec, ExecutionSpec, FilterSet, RiskConfig,
};

/// Translate a `BacktestConfig` TOML DTO into a `RunBacktestCommand`.
///
/// Only business-intent fields are transferred, grouped into bounded sub-structures.
/// Infrastructure fields (`data_source`, `earnings_source`, `data_dir`) are omitted;
/// the caller is responsible for wiring those to the factory separately.
pub fn map_config_to_command(cfg: &BacktestConfig) -> RunBacktestCommand {
    RunBacktestCommand {
        period: BacktestPeriod {
            start_date: cfg.start_date,
            end_date: cfg.end_date,
        },
        strategy: StrategySpec {
            spread: cfg.spread,
            selection_strategy: cfg.selection_strategy,
            selection: cfg.selection.clone(),
            timing: cfg.timing,
            timing_strategy: cfg.timing_strategy.clone(),
            entry_days_before: cfg.entry_days_before,
            exit_days_before: cfg.exit_days_before,
            entry_offset: cfg.entry_offset,
            holding_days: cfg.holding_days,
            exit_days_after: cfg.exit_days_after,
            wing_width: cfg.wing_width,
            straddle_entry_days: cfg.straddle_entry_days,
            straddle_exit_days: cfg.straddle_exit_days,
            min_straddle_dte: cfg.min_straddle_dte,
            post_earnings_holding_days: cfg.post_earnings_holding_days,
        },
        execution: ExecutionSpec {
            parallel: cfg.parallel,
            pricing_model: cfg.pricing_model,
            vol_model: cfg.vol_model,
            target_delta: cfg.target_delta,
            delta_range: cfg.delta_range,
            delta_scan_steps: cfg.delta_scan_steps,
            strike_match_mode: cfg.strike_match_mode,
        },
        filters: FilterSet {
            symbols: cfg.symbols.clone(),
            min_market_cap: cfg.min_market_cap,
            max_entry_iv: cfg.max_entry_iv,
            min_notional: cfg.min_notional,
            min_entry_price: cfg.min_entry_price,
            max_entry_price: cfg.max_entry_price,
            rules: cfg.rules.clone(),
        },
        risk: RiskConfig {
            return_basis: cfg.return_basis,
            margin: cfg.margin.clone(),
            hedge_config: cfg.hedge_config.clone(),
            attribution_config: cfg.attribution_config.clone(),
            trading_costs: cfg.trading_costs.clone(),
        },
    }
}
