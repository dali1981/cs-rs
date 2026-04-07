//! Tests for `map_config_to_command()` — the application-layer mapper from
//! the TOML DTO to the application command type.
//!
//! Moved from cs-backtest/tests/command_mapper.rs (DAL-74): mapping belongs
//! in the application layer (ADR-0003), so its tests live here.
//!
//! Verifies that:
//! 1. The mapper is deterministic: same config → same command fields every time.
//! 2. Infrastructure fields (data_source, earnings_source, data_dir) are NOT
//!    present in the resulting RunBacktestCommand.
//! 3. All business-intent fields are faithfully transferred.

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

/// The mapper is pure: calling it twice on the same config produces the same
/// command. Validates that no randomness or mutable state leaks into the mapping.
#[test]
fn config_mapping_is_deterministic() {
    let cfg = sample_config();
    let cmd1 = map_config_to_command(&cfg);
    let cmd2 = map_config_to_command(&cfg);

    assert_eq!(cmd1.start_date, cmd2.start_date);
    assert_eq!(cmd1.end_date,   cmd2.end_date);
    assert_eq!(cmd1.spread as u8, cmd2.spread as u8);
    assert_eq!(cmd1.selection_strategy as u8, cmd2.selection_strategy as u8);
    assert_eq!(cmd1.symbols,        cmd2.symbols);
    assert_eq!(cmd1.min_market_cap, cmd2.min_market_cap);
    assert_eq!(cmd1.straddle_entry_days, cmd2.straddle_entry_days);
    assert_eq!(cmd1.straddle_exit_days,  cmd2.straddle_exit_days);
}

/// Business-intent fields are faithfully transferred from config to command.
#[test]
fn map_config_transfers_business_fields() {
    let cfg = sample_config();
    let cmd = map_config_to_command(&cfg);

    assert_eq!(cmd.start_date, NaiveDate::from_ymd_opt(2024, 8, 14).unwrap());
    assert_eq!(cmd.end_date,   NaiveDate::from_ymd_opt(2024, 8, 28).unwrap());
    assert!(matches!(cmd.spread, SpreadType::Straddle));
    assert_eq!(cmd.symbols, Some(vec!["NVDA".to_string()]));
    assert_eq!(cmd.min_market_cap, Some(100_000_000_000));
    assert_eq!(cmd.straddle_entry_days, 6);
    assert_eq!(cmd.straddle_exit_days,  2);
    assert!(cmd.parallel);
}

/// Infrastructure fields must NOT be present on RunBacktestCommand.
/// Verified structurally: the type simply does not have these fields.
///
/// Fields absent from RunBacktestCommand (compile-time enforcement):
///   - data_source   (infrastructure: where market data lives)
///   - earnings_source (infrastructure: where earnings calendar lives)
///   - data_dir       (deprecated, infrastructure)
#[test]
fn mapped_command_has_no_infra_fields() {
    let cfg = sample_config();
    let cmd = map_config_to_command(&cfg);

    // Infrastructure config extracted independently — not embedded in command
    let data_source = cfg.data_source.clone();
    let earnings_source = cfg.earnings_source.clone();

    assert_eq!(cmd.start_date, cfg.start_date);
    assert_eq!(cmd.end_date,   cfg.end_date);
    assert!(matches!(data_source, DataSourceConfig::Finq { .. }));
    let _ = earnings_source;
}

/// Validates the explicit wiring invariant: RunBacktestCommand, DataSourceConfig,
/// and EarningsSourceConfig are separate values. map_config_to_command produces
/// only the command — infrastructure wiring is the caller's responsibility.
#[test]
fn command_executes_without_bundle() {
    use cs_backtest::DataSourceConfig;
    use std::path::PathBuf;

    let cfg = sample_config();
    let command = map_config_to_command(&cfg);
    let data_source = cfg.data_source.clone();
    let earnings_source = cfg.earnings_source.clone();

    assert_eq!(command.start_date, cfg.start_date);
    assert!(matches!(command.spread, SpreadType::Straddle));
    assert!(matches!(data_source, DataSourceConfig::Finq { .. }));
    let _ = earnings_source;
    let _ = PathBuf::new();
}
