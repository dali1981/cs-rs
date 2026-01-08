//! Global arguments shared across all commands

use clap::Args;
use std::path::PathBuf;

/// Global CLI arguments
#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// Data directory
    #[arg(long, env = "FINQ_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(long, short)]
    pub verbose: bool,
}
