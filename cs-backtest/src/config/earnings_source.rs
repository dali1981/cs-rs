use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Earnings calendar data provider (for use with earnings-rs library)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EarningsProvider {
    /// NASDAQ official API (most reliable)
    Nasdaq,
    /// TradingView scanner (default)
    #[default]
    TradingView,
    /// Yahoo Finance
    Yahoo,
}

impl fmt::Display for EarningsProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nasdaq => write!(f, "Nasdaq"),
            Self::TradingView => write!(f, "TradingView"),
            Self::Yahoo => write!(f, "Yahoo"),
        }
    }
}

impl EarningsProvider {
    /// Parse from string (case-insensitive)
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "nasdaq" => Ok(Self::Nasdaq),
            "tradingview" | "tv" => Ok(Self::TradingView),
            "yahoo" | "yf" => Ok(Self::Yahoo),
            _ => Err(format!(
                "Invalid earnings provider '{}'. Valid values: nasdaq, tradingview, yahoo",
                s
            )),
        }
    }

    /// Convert to earnings-rs DataSource
    #[cfg(feature = "full")]
    pub fn to_earnings_rs(&self) -> earnings_rs::DataSource {
        match self {
            Self::Nasdaq => earnings_rs::DataSource::Nasdaq,
            Self::TradingView => earnings_rs::DataSource::TradingView,
            Self::Yahoo => earnings_rs::DataSource::Yahoo,
        }
    }
}

/// Default earnings data directory
fn default_earnings_dir() -> PathBuf {
    std::env::var("EARNINGS_DATA_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("trading_project/nasdaq_earnings/data")
        })
}

/// Unified earnings source configuration
///
/// Encapsulates both WHERE earnings data comes from (file or provider directory)
/// and HOW to interpret it (provider type for directory-based sources).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum EarningsSourceConfig {
    /// Load from a custom file (Parquet or JSON, auto-detected by extension)
    File { path: PathBuf },

    /// Load from earnings-rs provider directory
    Provider {
        #[serde(default = "default_earnings_dir")]
        dir: PathBuf,
        #[serde(default)]
        source: EarningsProvider,
    },
}

impl Default for EarningsSourceConfig {
    fn default() -> Self {
        Self::Provider {
            dir: default_earnings_dir(),
            source: EarningsProvider::default(),
        }
    }
}

impl fmt::Display for EarningsSourceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { path } => write!(f, "File: {}", path.display()),
            Self::Provider { dir, source } => {
                write!(f, "{} (dir: {})", source, dir.display())
            }
        }
    }
}

impl EarningsSourceConfig {
    /// Create a File variant
    pub fn file(path: PathBuf) -> Self {
        Self::File { path }
    }

    /// Create a Provider variant with specified source and directory
    pub fn provider(source: EarningsProvider, dir: PathBuf) -> Self {
        Self::Provider { dir, source }
    }

    /// Create a Provider variant with specified source and default directory
    pub fn provider_default_dir(source: EarningsProvider) -> Self {
        Self::Provider {
            dir: default_earnings_dir(),
            source,
        }
    }

    /// Check if this is a file-based source
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File { .. })
    }

    /// Get the directory path (only for Provider variant)
    pub fn dir(&self) -> Option<&PathBuf> {
        match self {
            Self::Provider { dir, .. } => Some(dir),
            Self::File { .. } => None,
        }
    }

    /// Get the file path (only for File variant)
    pub fn file_path(&self) -> Option<&PathBuf> {
        match self {
            Self::File { path } => Some(path),
            Self::Provider { .. } => None,
        }
    }

    /// Get the provider (only for Provider variant)
    pub fn provider_source(&self) -> Option<EarningsProvider> {
        match self {
            Self::Provider { source, .. } => Some(*source),
            Self::File { .. } => None,
        }
    }
}
