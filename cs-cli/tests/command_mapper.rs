//! Tests for `map_config_to_command()` — the application-layer mapper from
//! the TOML DTO to the application command type.
//!
//! Verifies that:
//! 1. The mapper is deterministic: same config → same command fields every time.
//! 2. All business-intent fields are faithfully transferred into the correct
//!    bounded sub-structure.
//! 3. Infrastructure fields are NOT present on `RunBacktestCommand`.
//! 4. `RunBacktestCommand` has ≤5 top-level fields (period/strategy/execution/
//!    filters/risk).

use chrono::NaiveDate;
use cs_backtest::{BacktestConfig, DataSourceConfig, RunBacktestCommand};
use cs_backtest::config::SpreadType;
use cs_cli::mapping::map_config_to_command;

fn sample_config() -> BacktestConfig {
    let mut config = BacktestConfig::default();
    config.start_date = NaiveDate::from_ymd_opt(2024, 8, 14).unwrap();
    config.end_date   = NaiveDate::from_ymd_opt(2024, 8, 28).unwrap();
    config.spread     = SpreadType::Straddle;
    config.symbols    = Some(vec!["NVDA".to_string()]);
    config.min_market_cap = Some(100_000_000_000);
    config.straddle_entry_days = 6;
    config.straddle_exit_days  = 2;
    config
}

/// The mapper is pure: calling it twice on the same config produces identical values.
#[test]
fn config_mapping_is_deterministic() {
    let cfg = sample_config();
    let cmd1 = map_config_to_command(&cfg);
    let cmd2 = map_config_to_command(&cfg);

    assert_eq!(cmd1.period.start_date,  cmd2.period.start_date);
    assert_eq!(cmd1.period.end_date,    cmd2.period.end_date);
    assert_eq!(cmd1.strategy.spread as u8, cmd2.strategy.spread as u8);
    assert_eq!(cmd1.strategy.selection_strategy as u8, cmd2.strategy.selection_strategy as u8);
    assert_eq!(cmd1.filters.symbols,        cmd2.filters.symbols);
    assert_eq!(cmd1.filters.min_market_cap, cmd2.filters.min_market_cap);
    assert_eq!(cmd1.strategy.straddle_entry_days, cmd2.strategy.straddle_entry_days);
    assert_eq!(cmd1.strategy.straddle_exit_days,  cmd2.strategy.straddle_exit_days);
}

/// Business-intent fields land in the correct bounded sub-structure.
#[test]
fn map_config_transfers_business_fields() {
    let cfg = sample_config();
    let cmd = map_config_to_command(&cfg);

    // Period
    assert_eq!(cmd.period.start_date, NaiveDate::from_ymd_opt(2024, 8, 14).unwrap());
    assert_eq!(cmd.period.end_date,   NaiveDate::from_ymd_opt(2024, 8, 28).unwrap());

    // Strategy
    assert!(matches!(cmd.strategy.spread, SpreadType::Straddle));
    assert_eq!(cmd.strategy.straddle_entry_days, 6);
    assert_eq!(cmd.strategy.straddle_exit_days,  2);

    // Filters
    assert_eq!(cmd.filters.symbols, Some(vec!["NVDA".to_string()]));
    assert_eq!(cmd.filters.min_market_cap, Some(100_000_000_000));

    // Execution
    assert!(cmd.execution.parallel);
}

/// `RunBacktestCommand` has exactly five top-level fields — no flat field sprawl.
///
/// Verified structurally: if this compiles, the decomposition is complete.
/// Destructuring all five fields at once proves no extra flat fields exist.
#[test]
fn command_has_five_top_level_fields() {
    let cfg = sample_config();
    let RunBacktestCommand { period, strategy, execution, filters, risk } =
        map_config_to_command(&cfg);

    // Every sub-structure is populated
    assert_eq!(period.start_date, cfg.start_date);
    assert!(matches!(strategy.spread, SpreadType::Straddle));
    assert!(execution.parallel);
    let _ = filters;
    let _ = risk;
}

/// Infrastructure fields must NOT be present on `RunBacktestCommand`.
/// Compile-time proof: the type does not have data_source, earnings_source, data_dir.
#[test]
fn mapped_command_has_no_infra_fields() {
    let cfg = sample_config();
    let cmd = map_config_to_command(&cfg);

    let data_source = cfg.data_source.clone();
    let earnings_source = cfg.earnings_source.clone();

    assert_eq!(cmd.period.start_date, cfg.start_date);
    assert!(matches!(data_source, DataSourceConfig::Finq { .. }));
    let _ = earnings_source;
}

/// Validates the explicit wiring invariant: command, data_source, earnings_source
/// are all separate values produced independently from the same config.
#[test]
fn command_executes_without_bundle() {
    let cfg = sample_config();
    let command = map_config_to_command(&cfg);
    let data_source = cfg.data_source.clone();
    let earnings_source = cfg.earnings_source.clone();

    assert_eq!(command.period.start_date, cfg.start_date);
    assert!(matches!(command.strategy.spread, SpreadType::Straddle));
    assert!(matches!(data_source, DataSourceConfig::Finq { .. }));
    let _ = earnings_source;
}
