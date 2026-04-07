//! Backtest command handler

use anyhow::{Context, Result};
use async_trait::async_trait;
use console::style;

use crate::args::{BacktestArgs, GlobalArgs};
use crate::config::{BacktestConfigBuilder, BacktestCommandBundle};
use crate::factory::UseCaseFactory;
use crate::output::BacktestOutputHandler;
use super::CommandHandler;

/// Backtest command handler.
///
/// Responsible for parsing CLI args and TOML config into a `BacktestCommandBundle`,
/// then delegating to `UseCaseFactory`. No business logic lives here.
pub struct BacktestCommand {
    args: BacktestArgs,
    global: GlobalArgs,
}

impl BacktestCommand {
    /// Create a new backtest command handler
    pub fn new(args: BacktestArgs, global: GlobalArgs) -> Self {
        Self { args, global }
    }

    /// Parse CLI args + TOML into a `BacktestCommandBundle`.
    ///
    /// The bundle separates the business-intent command from infrastructure
    /// config (data source, earnings source). See ADR-0003.
    fn build_bundle(&self) -> Result<BacktestCommandBundle> {
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

        // 1. Parse CLI args + TOML → explicit application command bundle
        let bundle = self.build_bundle()?;

        // Log configuration (read from bundle before it is consumed by the factory)
        let data_dir_source = if self.global.data_dir.is_some() {
            "CLI argument"
        } else if std::env::var("FINQ_DATA_DIR").is_ok() {
            "FINQ_DATA_DIR env"
        } else {
            "default (~/polygon/data)"
        };

        println!("  Data source: {:?}", bundle.data_source);
        println!("  Data directory: {} (from {})",
            style(bundle.data_source.data_dir().display()).cyan(),
            style(data_dir_source).dim());
        println!("  Earnings source: {}", style(&bundle.earnings_source).cyan());
        println!("  Strategy: {:?}", bundle.command.spread);
        println!("  Selection: {:?}", bundle.command.selection_strategy);
        println!("  Period: {} to {}", bundle.command.start_date, bundle.command.end_date);
        println!();

        // 2. Wire repositories and create use case via factory
        let use_case = UseCaseFactory::create_backtest(bundle)?;

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
