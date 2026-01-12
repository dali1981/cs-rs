//! Backtest command handler

use anyhow::{Context, Result};
use async_trait::async_trait;
use console::style;

use cs_backtest::BacktestConfig;
use crate::args::{BacktestArgs, GlobalArgs};
use crate::config::BacktestConfigBuilder;
use crate::factory::UseCaseFactory;
use crate::output::BacktestOutputHandler;
use super::CommandHandler;

/// Backtest command handler
pub struct BacktestCommand {
    args: BacktestArgs,
    global: GlobalArgs,
}

impl BacktestCommand {
    /// Create a new backtest command handler
    pub fn new(args: BacktestArgs, global: GlobalArgs) -> Self {
        Self { args, global }
    }

    /// Build BacktestConfig using BacktestConfigBuilder
    fn build_config(&self) -> Result<BacktestConfig> {
        // Use builder pattern to construct config (includes dates, earnings file, etc.)
        let config = BacktestConfigBuilder::from_args(&self.args)
            .with_global(&self.global)
            .with_config_files(&self.args.conf)?
            .build()?;

        Ok(config)
    }
}

#[async_trait]
impl CommandHandler for BacktestCommand {
    async fn execute(&self) -> Result<()> {
        println!("{}", style("Running backtest...").bold());

        // 1. Build config from args (includes dates, earnings, everything)
        let config = self.build_config()
            .context("Failed to build backtest config")?;

        // Log configuration
        let data_dir_source = if self.global.data_dir.is_some() {
            "CLI argument"
        } else if std::env::var("FINQ_DATA_DIR").is_ok() {
            "FINQ_DATA_DIR env"
        } else {
            "default (~/polygon/data)"
        };

        println!("  Data source: {:?}", config.data_source);
        println!("  Data directory: {} (from {})",
            style(config.data_source.data_dir().display()).cyan(),
            style(data_dir_source).dim());

        if let Some(earnings_file) = &config.earnings_file {
            println!("  Earnings file: {}", style(earnings_file.display()).cyan());
        } else {
            println!("  Earnings directory: {}", style(config.earnings_dir.display()).cyan());
        }

        println!("  Strategy: {:?}", config.spread);
        println!("  Selection: {:?}", config.selection_strategy);
        println!("  Period: {} to {}", config.start_date, config.end_date);
        println!();

        // 2. Create use case via factory
        let use_case = UseCaseFactory::create_backtest(config)?;

        // 3. Execute (use case handles strategy dispatch internally)
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
