//! Output handlers for different commands

pub mod earnings_output;

pub use earnings_output::{save_earnings_parquet, save_earnings_csv, save_earnings_json};
