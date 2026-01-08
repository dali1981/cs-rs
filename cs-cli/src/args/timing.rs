//! Timing-related arguments

use clap::Args;

/// Timing configuration arguments
#[derive(Debug, Clone, Args)]
pub struct TimingArgs {
    /// Entry time in HH:MM format (default: 09:35)
    #[arg(long)]
    pub entry_time: Option<String>,

    /// Exit time in HH:MM format (default: 15:55)
    #[arg(long)]
    pub exit_time: Option<String>,
}
