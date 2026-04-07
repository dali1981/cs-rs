//! Event-level rules (no market data needed)

use serde::{Deserialize, Serialize};
use crate::EarningsEvent;

/// Event-level rules that filter by earnings event metadata
///
/// These rules are evaluated first, before any market data is loaded,
/// making them very cheap to evaluate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventRule {
    /// Minimum market capitalization
    MinMarketCap {
        /// Threshold in dollars (e.g., 1_000_000_000 for $1B)
        threshold: u64,
    },
    /// Symbol whitelist
    Symbols {
        /// List of allowed symbols (case-insensitive)
        include: Vec<String>,
    },
}

impl EventRule {
    /// Human-readable name for logging
    pub fn name(&self) -> &'static str {
        match self {
            Self::MinMarketCap { .. } => "min_market_cap",
            Self::Symbols { .. } => "symbols",
        }
    }

    /// Evaluate the rule against an earnings event
    pub fn eval(&self, event: &EarningsEvent) -> bool {
        match self {
            Self::MinMarketCap { threshold } => {
                event.market_cap.map_or(false, |cap| cap >= *threshold)
            }
            Self::Symbols { include } => {
                include.iter().any(|s| s.eq_ignore_ascii_case(&event.symbol))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::EarningsEventBuilder;

    fn mock_event(symbol: &str, market_cap: Option<u64>) -> EarningsEvent {
        EarningsEventBuilder::new(symbol)
            .market_cap_opt(market_cap)
            .build()
    }

    #[test]
    fn test_min_market_cap_passes() {
        let rule = EventRule::MinMarketCap { threshold: 1_000_000_000 };
        let event = mock_event("AAPL", Some(2_000_000_000));
        assert!(rule.eval(&event));
    }

    #[test]
    fn test_min_market_cap_fails() {
        let rule = EventRule::MinMarketCap { threshold: 1_000_000_000 };
        let event = mock_event("SMALL", Some(500_000_000));
        assert!(!rule.eval(&event));
    }

    #[test]
    fn test_min_market_cap_missing_fails() {
        let rule = EventRule::MinMarketCap { threshold: 1_000_000_000 };
        let event = mock_event("UNKNOWN", None);
        assert!(!rule.eval(&event));
    }

    #[test]
    fn test_symbols_passes() {
        let rule = EventRule::Symbols {
            include: vec!["AAPL".to_string(), "MSFT".to_string()],
        };
        let event = mock_event("AAPL", None);
        assert!(rule.eval(&event));
    }

    #[test]
    fn test_symbols_case_insensitive() {
        let rule = EventRule::Symbols {
            include: vec!["aapl".to_string()],
        };
        let event = mock_event("AAPL", None);
        assert!(rule.eval(&event));
    }

    #[test]
    fn test_symbols_fails() {
        let rule = EventRule::Symbols {
            include: vec!["AAPL".to_string(), "MSFT".to_string()],
        };
        let event = mock_event("GOOGL", None);
        assert!(!rule.eval(&event));
    }
}
