//! Earnings analysis command arguments

use clap::Args;
use std::path::PathBuf;

/// Arguments for the earnings-analysis command
#[derive(Debug, Clone, Args)]
pub struct EarningsAnalysisArgs {
    /// Symbol(s) to analyze (comma-separated)
    #[arg(long, value_delimiter = ',', required = true)]
    pub symbols: Vec<String>,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    pub start: String,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    pub end: String,

    /// Earnings data directory
    #[arg(long, env = "EARNINGS_DATA_DIR")]
    pub earnings_dir: Option<PathBuf>,

    /// Output format (parquet, csv, json)
    #[arg(long, default_value = "parquet")]
    pub format: String,

    /// Output file path (optional, defaults to ./earnings_analysis_<symbol>.parquet)
    #[arg(long)]
    pub output: Option<PathBuf>,
}
