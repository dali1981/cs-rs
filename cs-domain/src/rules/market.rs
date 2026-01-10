//! Market-level rules (need IV surface, spot, chain data)

use serde::{Deserialize, Serialize};

/// Market-level rules that require IV surface and market data
///
/// These rules are evaluated after market data is loaded but before
/// trade execution, allowing early filtering of unpromising opportunities.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MarketRule {
    /// IV term structure slope: iv_short > iv_long + threshold_pp
    ///
    /// Used to detect elevated near-term IV relative to longer-term IV,
    /// which may indicate a volatility selling opportunity.
    IvSlope {
        /// Short-term DTE for IV comparison (e.g., 7)
        short_dte: u16,
        /// Long-term DTE for IV comparison (e.g., 20)
        long_dte: u16,
        /// Threshold in percentage points (e.g., 0.05 for 5pp)
        threshold_pp: f64,
    },

    /// Maximum ATM IV at entry
    ///
    /// Filters out trades where IV is unreliably high (pricing issues)
    /// or where risk/reward is unfavorable.
    MaxEntryIv {
        /// Maximum IV threshold (e.g., 1.5 for 150%)
        threshold: f64,
    },

    /// Minimum IV ratio (short_iv / long_iv)
    ///
    /// For calendar spreads, ensures sufficient IV curve steepness.
    MinIvRatio {
        /// Short-term DTE for ratio (default: 7)
        #[serde(default = "default_short_dte")]
        short_dte: u16,
        /// Long-term DTE for ratio (default: 30)
        #[serde(default = "default_long_dte")]
        long_dte: u16,
        /// Minimum ratio threshold (e.g., 1.2)
        threshold: f64,
    },

    /// IV vs Historical Volatility comparison
    ///
    /// Ensures IV is sufficiently elevated relative to realized vol,
    /// indicating potential mean reversion opportunity.
    IvVsHv {
        /// Historical volatility lookback window in days (default: 20)
        #[serde(default = "default_hv_window")]
        hv_window_days: u16,
        /// Minimum IV/HV ratio (e.g., 1.1 means IV >= 1.1 * HV)
        min_ratio: f64,
    },

    /// Minimum daily option notional
    ///
    /// Filters for sufficient liquidity based on option trading volume.
    MinNotional {
        /// Minimum notional in dollars (e.g., 100_000 for $100k)
        threshold: f64,
    },
}

fn default_short_dte() -> u16 {
    7
}

fn default_long_dte() -> u16 {
    30
}

fn default_hv_window() -> u16 {
    20
}

impl MarketRule {
    /// Human-readable name for logging
    pub fn name(&self) -> &'static str {
        match self {
            Self::IvSlope { .. } => "iv_slope",
            Self::MaxEntryIv { .. } => "max_entry_iv",
            Self::MinIvRatio { .. } => "min_iv_ratio",
            Self::IvVsHv { .. } => "iv_vs_hv",
            Self::MinNotional { .. } => "min_notional",
        }
    }

    /// Get short DTE if applicable
    pub fn short_dte(&self) -> Option<u16> {
        match self {
            Self::IvSlope { short_dte, .. } => Some(*short_dte),
            Self::MinIvRatio { short_dte, .. } => Some(*short_dte),
            _ => None,
        }
    }

    /// Get long DTE if applicable
    pub fn long_dte(&self) -> Option<u16> {
        match self {
            Self::IvSlope { long_dte, .. } => Some(*long_dte),
            Self::MinIvRatio { long_dte, .. } => Some(*long_dte),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iv_slope_name() {
        let rule = MarketRule::IvSlope {
            short_dte: 7,
            long_dte: 20,
            threshold_pp: 0.05,
        };
        assert_eq!(rule.name(), "iv_slope");
    }

    #[test]
    fn test_serde_roundtrip() {
        let rule = MarketRule::IvSlope {
            short_dte: 7,
            long_dte: 20,
            threshold_pp: 0.05,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: MarketRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule.name(), parsed.name());
    }

    #[test]
    fn test_min_iv_ratio_defaults() {
        let json = r#"{"type": "min_iv_ratio", "threshold": 1.2}"#;
        let rule: MarketRule = serde_json::from_str(json).unwrap();
        match rule {
            MarketRule::MinIvRatio { short_dte, long_dte, threshold } => {
                assert_eq!(short_dte, 7);
                assert_eq!(long_dte, 30);
                assert_eq!(threshold, 1.2);
            }
            _ => panic!("Expected MinIvRatio"),
        }
    }
}
