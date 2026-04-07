//! Application-layer mappers — translate config DTOs into application commands.
//!
//! Per ADR-0003, mapping from infrastructure DTOs (BacktestConfig) to application
//! commands (RunBacktestCommand) belongs here, not in the config layer.

pub mod backtest_command_mapper;
pub use backtest_command_mapper::map_config_to_command;
