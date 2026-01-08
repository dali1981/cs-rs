//! Price a single spread command arguments

use clap::Args;

/// Arguments for the price command
#[derive(Debug, Clone, Args)]
pub struct PriceArgs {
    /// Symbol
    #[arg(long)]
    pub symbol: String,

    /// Strike price
    #[arg(long)]
    pub strike: f64,

    /// Short leg expiration date (YYYY-MM-DD)
    #[arg(long)]
    pub short_expiry: String,

    /// Long leg expiration date (YYYY-MM-DD)
    #[arg(long)]
    pub long_expiry: String,

    /// Pricing date (YYYY-MM-DD)
    #[arg(long)]
    pub date: String,
}
