// cs-cli: Command-line interface for calendar spread backtesting

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cs")]
#[command(about = "Calendar Spread Backtest CLI")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Data directory
    #[arg(long, env = "FINQ_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run backtest
    Backtest {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long, default_value = "call")]
        option_type: String,
        #[arg(long, default_value = "atm")]
        strategy: String,
        #[arg(long)]
        symbols: Option<Vec<String>>,
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Analyze results from a run
    Analyze {
        #[arg(long)]
        run_dir: PathBuf,
    },

    /// Price a single spread (for debugging)
    Price {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        strike: f64,
        #[arg(long)]
        short_expiry: String,
        #[arg(long)]
        long_expiry: String,
        #[arg(long)]
        date: String,
    },
}

fn main() {
    println!("cs-cli v{}", env!("CARGO_PKG_VERSION"));
    println!("Calendar Spread Backtest - Rust Edition");
    println!();
    println!("🚧 Under construction - Phase 1 in progress");
    println!();
    println!("See README.md for current status and roadmap.");

    // Parse args to show help
    let _cli = Cli::parse();
}
