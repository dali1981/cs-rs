//! Analyze results command arguments

use clap::Args;
use std::path::PathBuf;

/// Arguments for the analyze command
#[derive(Debug, Clone, Args)]
pub struct AnalyzeArgs {
    /// Directory containing the backtest results
    #[arg(long)]
    pub run_dir: PathBuf,
}
