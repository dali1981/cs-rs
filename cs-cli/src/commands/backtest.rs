//! Backtest command handler

use anyhow::{Context, Result};
use async_trait::async_trait;
use console::style;

use crate::args::{BacktestArgs, GlobalArgs};
use cs_backtest::{DataSourceConfig, EarningsSourceConfig, RunBacktestCommand};
use crate::config::BacktestConfigBuilder;
use crate::factory::UseCaseFactory;
use crate::output::BacktestOutputHandler;
use super::CommandHandler;

/// Backtest command handler.
///
/// Responsible for parsing CLI args and TOML config into an explicit
/// `RunBacktestCommand` + infrastructure providers, then delegating to
/// `UseCaseFactory`. No business logic lives here.
pub struct BacktestCommand {
    args: BacktestArgs,
    global: GlobalArgs,
}

impl BacktestCommand {
    /// Create a new backtest command handler
    pub fn new(args: BacktestArgs, global: GlobalArgs) -> Self {
        Self { args, global }
    }

    /// Parse CLI args + TOML into explicit application command and infra config.
    ///
    /// Returns `(command, data_source, earnings_source)` separately so each can
    /// be wired to the factory independently. See ADR-0003.
    fn build_command(&self) -> Result<(RunBacktestCommand, DataSourceConfig, EarningsSourceConfig)> {
        BacktestConfigBuilder::from_args(&self.args)
            .with_global(&self.global)
            .with_config_files(&self.args.conf)?
            .build()
            .context("Failed to build backtest command")
    }
}

#[async_trait]
impl CommandHandler for BacktestCommand {
    async fn execute(&self) -> Result<()> {
        println!("{}", style("Running backtest...").bold());

        // 1. Parse CLI args + TOML → explicit command + infra config
        let (command, data_source, earnings_source) = self.build_command()?;

        // Log configuration
        let data_dir_source = if self.global.data_dir.is_some() {
            "CLI argument"
        } else if std::env::var("FINQ_DATA_DIR").is_ok() {
            "FINQ_DATA_DIR env"
        } else {
            "default (~/polygon/data)"
        };

        println!("  Data source: {:?}", data_source);
        println!("  Data directory: {} (from {})",
            style(data_source.data_dir().display()).cyan(),
            style(data_dir_source).dim());
        println!("  Earnings source: {}", style(&earnings_source).cyan());
        println!("  Strategy: {:?}", command.strategy.spread);
        println!("  Selection: {:?}", command.strategy.selection_strategy);
        println!("  Period: {} to {}", command.period.start_date, command.period.end_date);
        println!();

        // 2. Wire repositories and create use case via factory
        let use_case = UseCaseFactory::create_backtest(command, data_source, earnings_source)?;

        // 3. Execute
        let result = use_case
            .execute()
            .await
            .context("Backtest execution failed")?;

        // 4. Display results
        BacktestOutputHandler::display_unified(&result);

        // 5. Save if requested
        if let Some(ref output) = self.args.output {
            BacktestOutputHandler::save_unified(&result, output)?;
        }

        Ok(())
    }
}
