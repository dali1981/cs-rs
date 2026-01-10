//! Trade-level rules (need execution result)

use serde::{Deserialize, Serialize};

/// Trade-level rules that require trade execution result
///
/// These rules are evaluated after trade execution to filter based on
/// actual pricing. While later in the pipeline, they catch cases where
/// theoretical filtering would be inaccurate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TradeRule {
    /// Entry price must be within specified range
    ///
    /// Filters trades with prices outside acceptable bounds:
    /// - min: avoid very cheap trades (may indicate pricing issues)
    /// - max: cap maximum loss exposure
    EntryPriceRange {
        /// Minimum entry price (None = no minimum)
        #[serde(default)]
        min: Option<f64>,
        /// Maximum entry price (None = no maximum)
        #[serde(default)]
        max: Option<f64>,
    },
}

impl TradeRule {
    /// Human-readable name for logging
    pub fn name(&self) -> &'static str {
        match self {
            Self::EntryPriceRange { .. } => "entry_price_range",
        }
    }

    /// Evaluate the rule against an entry price
    pub fn eval_price(&self, entry_price: f64) -> bool {
        match self {
            Self::EntryPriceRange { min, max } => {
                let min_ok = min.map_or(true, |m| entry_price >= m);
                let max_ok = max.map_or(true, |m| entry_price <= m);
                min_ok && max_ok
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_price_range_passes() {
        let rule = TradeRule::EntryPriceRange {
            min: Some(0.50),
            max: Some(50.0),
        };
        assert!(rule.eval_price(10.0));
        assert!(rule.eval_price(0.50));
        assert!(rule.eval_price(50.0));
    }

    #[test]
    fn test_entry_price_range_fails_min() {
        let rule = TradeRule::EntryPriceRange {
            min: Some(0.50),
            max: None,
        };
        assert!(!rule.eval_price(0.25));
    }

    #[test]
    fn test_entry_price_range_fails_max() {
        let rule = TradeRule::EntryPriceRange {
            min: None,
            max: Some(50.0),
        };
        assert!(!rule.eval_price(75.0));
    }

    #[test]
    fn test_entry_price_no_bounds() {
        let rule = TradeRule::EntryPriceRange {
            min: None,
            max: None,
        };
        assert!(rule.eval_price(0.01));
        assert!(rule.eval_price(1000.0));
    }
}
