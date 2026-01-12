use std::path::PathBuf;
use serde::{Serialize, Deserialize};

fn default_finq_dir() -> PathBuf {
    std::env::var("FINQ_DATA_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("polygon/data")
        })
}

/// Market data provider configuration (infrastructure concern)
///
/// Specifies which provider to use for options and equity market data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase")]
pub enum DataSourceConfig {
    Finq { data_dir: PathBuf },
    Ib { data_dir: PathBuf },
}

impl Default for DataSourceConfig {
    fn default() -> Self {
        Self::Finq {
            data_dir: default_finq_dir(),
        }
    }
}

impl DataSourceConfig {
    pub fn data_dir(&self) -> &PathBuf {
        match self {
            Self::Finq { data_dir } => data_dir,
            Self::Ib { data_dir } => data_dir,
        }
    }
}
