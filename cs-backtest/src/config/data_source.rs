use std::path::PathBuf;
use serde::{Serialize, Deserialize};

/// Data source configuration (infrastructure concern)
///
/// Specifies where to load market data and earnings from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceConfig {
    pub data_dir: PathBuf,
    pub earnings_dir: PathBuf,
    pub earnings_file: Option<PathBuf>,
}

impl Default for DataSourceConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("data"),
            earnings_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("trading_project/nasdaq_earnings/data"),
            earnings_file: None,
        }
    }
}
