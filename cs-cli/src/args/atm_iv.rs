//! ATM IV time series generation command arguments

use clap::Args;
use std::path::PathBuf;

/// Arguments for the atm-iv command
#[derive(Debug, Clone, Args)]
pub struct AtmIvArgs {
    /// Symbol(s) to analyze (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub symbols: Vec<String>,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    pub start: String,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    pub end: String,

    /// Target maturities in days (default: 7,14,21,30,60,90)
    #[arg(long, value_delimiter = ',')]
    pub maturities: Option<Vec<u32>>,

    /// Maturity tolerance in days (default: 7)
    #[arg(long)]
    pub tolerance: Option<u32>,

    /// Output directory for parquet files and plots
    #[arg(long)]
    pub output: PathBuf,

    /// Generate plots
    #[arg(long)]
    pub plot: bool,

    /// Use EOD pricing instead of minute-aligned (default: minute-aligned)
    #[arg(long)]
    pub eod_pricing: bool,

    /// Use constant-maturity IV interpolation (variance-space interpolation to exact DTEs)
    #[arg(long, alias = "cm")]
    pub constant_maturity: bool,

    /// Minimum DTE for expiration inclusion (default: 3)
    #[arg(long, default_value = "3")]
    pub min_dte: i64,

    /// Include historical volatility computation
    #[arg(long)]
    pub with_hv: bool,

    /// HV windows in days (default: 10,20,30,60)
    #[arg(long, value_delimiter = ',')]
    pub hv_windows: Option<Vec<usize>>,
}
