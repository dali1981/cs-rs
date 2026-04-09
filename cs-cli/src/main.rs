// cs-cli: Command-line interface for calendar spread backtesting

use anyhow::Result;
use clap::Parser;
use console::style;
use tracing_subscriber::EnvFilter;

// Keep existing modules for handler logic
mod cli_args;
mod config;
mod display;
mod parsing;

// New refactored modules
pub mod args;
pub mod cli;
pub mod commands;
pub mod factory;
pub mod mapping;
pub mod output;

use args::BacktestArgs;
#[cfg(feature = "experimental-cli")]
use args::{AnalyzeArgs, AtmIvArgs, CampaignArgs, EarningsAnalysisArgs, PriceArgs};
use cli::{Cli, Commands};

// Re-export for use in this file
use args::GlobalArgs;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging - supports RUST_LOG env var with --verbose as fallback
    let default_level = if cli.global.verbose { "debug" } else { "info" };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    println!(
        "{}",
        style("Calendar Spread Backtest - Rust Edition")
            .bold()
            .cyan()
    );
    println!();

    // Dispatch to the appropriate command handler
    match &cli.command {
        Commands::Backtest(args) => handle_backtest(args, cli.global).await,
        #[cfg(feature = "experimental-cli")]
        Commands::Analyze(args) => handle_analyze(args, cli.global).await,
        #[cfg(feature = "experimental-cli")]
        Commands::Price(args) => handle_price(args, cli.global).await,
        #[cfg(feature = "experimental-cli")]
        Commands::AtmIv(args) => handle_atm_iv(args, cli.global).await,
        #[cfg(feature = "experimental-cli")]
        Commands::EarningsAnalysis(args) => handle_earnings_analysis(args, cli.global).await,
        #[cfg(feature = "experimental-cli")]
        Commands::Campaign(args) => handle_campaign(args, cli.global).await,
    }
}

// ============================================================================
// Command Handlers - create handlers and execute
// ============================================================================

#[cfg(feature = "experimental-cli")]
use commands::{
    analyze::AnalyzeCommand, atm_iv::AtmIvCommand, campaign::CampaignCommand,
    earnings::EarningsAnalysisCommand, price::PriceCommand,
};
use commands::{backtest::BacktestCommand, CommandHandler};

async fn handle_backtest(args: &BacktestArgs, global: GlobalArgs) -> Result<()> {
    let command = BacktestCommand::new(args.clone(), global);
    command.execute().await
}

#[cfg(feature = "experimental-cli")]
async fn handle_analyze(args: &AnalyzeArgs, global: GlobalArgs) -> Result<()> {
    let command = AnalyzeCommand::new(args.clone(), global);
    command.execute().await
}

#[cfg(feature = "experimental-cli")]
async fn handle_price(args: &PriceArgs, global: GlobalArgs) -> Result<()> {
    let command = PriceCommand::new(args.clone(), global);
    command.execute().await
}

#[cfg(feature = "experimental-cli")]
async fn handle_atm_iv(args: &AtmIvArgs, global: GlobalArgs) -> Result<()> {
    let command = AtmIvCommand::new(args.clone(), global);
    command.execute().await
}

#[cfg(feature = "experimental-cli")]
async fn handle_earnings_analysis(args: &EarningsAnalysisArgs, global: GlobalArgs) -> Result<()> {
    let command = EarningsAnalysisCommand::new(args.clone(), global);
    command.execute().await
}

#[cfg(feature = "experimental-cli")]
async fn handle_campaign(args: &CampaignArgs, global: GlobalArgs) -> Result<()> {
    let command = CampaignCommand::new(args.clone(), global);
    command.execute().await
}
