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

    /// Timing strategy: PreEarnings, PostEarnings, CrossEarnings
    #[arg(long)]
    pub timing_strategy: Option<String>,

    /// Entry days before event (for PreEarnings/CrossEarnings)
    #[arg(long)]
    pub entry_days_before: Option<u16>,

    /// Exit days before event (for PreEarnings)
    #[arg(long)]
    pub exit_days_before: Option<u16>,

    /// Days after event to enter (for PostEarnings)
    #[arg(long)]
    pub entry_offset: Option<i16>,

    /// Holding days (for PostEarnings/HoldingPeriod)
    #[arg(long)]
    pub holding_days: Option<u16>,

    /// Exit days after event (for CrossEarnings)
    #[arg(long)]
    pub exit_days_after: Option<u16>,
}
