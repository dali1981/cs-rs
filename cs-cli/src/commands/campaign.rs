//! Campaign command handler

use anyhow::{Context, Result};
use async_trait::async_trait;
use console::style;

use cs_backtest::CampaignConfig;
use crate::args::{CampaignArgs, GlobalArgs};
use crate::config::CampaignConfigBuilder;
use crate::factory::UseCaseFactory;
use crate::output::CampaignOutputHandler;
use super::CommandHandler;

/// Campaign command handler
pub struct CampaignCommand {
    args: CampaignArgs,
    global: GlobalArgs,
}

impl CampaignCommand {
    /// Create a new campaign command handler
    pub fn new(args: CampaignArgs, global: GlobalArgs) -> Self {
        Self { args, global }
    }

    /// Build CampaignConfig using CampaignConfigBuilder
    fn build_config(&self) -> Result<CampaignConfig> {
        let config = CampaignConfigBuilder::from_args(&self.args)
            .with_global(&self.global)
            .build()?;

        Ok(config)
    }
}

#[async_trait]
impl CommandHandler for CampaignCommand {
    async fn execute(&self) -> Result<()> {
        println!("{}", style("Running campaign...").bold());

        // 1. Build config from args (includes dates, earnings, everything)
        let config = self.build_config()
            .context("Failed to build campaign config")?;

        // Log configuration
        let data_dir_source = if self.global.data_dir.is_some() {
            "CLI argument"
        } else if std::env::var("FINQ_DATA_DIR").is_ok() {
            "FINQ_DATA_DIR env"
        } else {
            "default (~/polygon/data)"
        };

        println!("  Data directory: {} (from {})",
            style(config.data_dir.display()).cyan(),
            style(data_dir_source).dim());

        // Display earnings source configuration
        println!("  Earnings source: {}", style(&config.earnings_source).cyan());

        println!("  Symbols: {}", config.symbols.join(", "));
        println!("  Strategy: {:?}", config.strategy);
        println!("  Direction: {:?}", config.trade_direction);
        println!("  Period: {} to {}", config.start_date, config.end_date);
        println!();

        // 2. Create use case via factory
        let use_case = UseCaseFactory::create_campaign(config)?;

        // 3. Execute
        let result = use_case
            .execute()
            .await
            .context("Campaign execution failed")?;

        // 4. Display results
        CampaignOutputHandler::display(&result);

        // 5. Save if requested
        if let Some(ref output) = self.args.output {
            CampaignOutputHandler::save(&result, output)?;
        }

        if let Some(ref output_dir) = self.args.output_dir {
            CampaignOutputHandler::save_detailed(&result, output_dir)?;
        }

        Ok(())
    }
}
