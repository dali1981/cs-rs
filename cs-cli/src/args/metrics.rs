//! Metrics-related CLI arguments

use clap::Args;

/// Metrics configuration overrides
#[derive(Debug, Clone, Args, Default)]
pub struct MetricsArgs {
    /// Return denominator (premium, capital-required, max-loss)
    #[arg(long)]
    pub return_basis: Option<String>,
}
