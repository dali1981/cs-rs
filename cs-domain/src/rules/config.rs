//! Rules configuration
//!
//! Follows the Rust-idiomatic pattern:
//! - Runtime config (RulesConfig) has real defaults via Default impl
//! - File config (FileRulesConfig) is partial with Option<T> fields
//! - Merge via apply_file() method

use serde::{Deserialize, Serialize};
use super::{EventRule, MarketRule, TradeRule};

/// Runtime rules configuration (source of truth for defaults)
///
/// This is the "full" config with real defaults. Use this type in the
/// application runtime. Load from files via `apply_file()`.
#[derive(Debug, Clone, Default)]
pub struct RulesConfig {
    /// Event-level rules (no market data needed)
    pub event: Vec<EventRule>,
    /// Market-level rules (need IV surface, spot, chain)
    pub market: Vec<MarketRule>,
    /// Trade-level rules (need execution result)
    pub trade: Vec<TradeRule>,
}

impl RulesConfig {
    /// Create empty config (no rules = all trades pass)
    pub fn none() -> Self {
        Self::default()
    }

    /// Check if any rules are configured
    pub fn has_rules(&self) -> bool {
        !self.event.is_empty() || !self.market.is_empty() || !self.trade.is_empty()
    }

    /// Check if there are event-level rules
    pub fn has_event_rules(&self) -> bool {
        !self.event.is_empty()
    }

    /// Check if there are market-level rules
    pub fn has_market_rules(&self) -> bool {
        !self.market.is_empty()
    }

    /// Check if there are trade-level rules
    pub fn has_trade_rules(&self) -> bool {
        !self.trade.is_empty()
    }

    /// Apply file config overrides
    ///
    /// File config fields that are Some replace the corresponding runtime
    /// config fields entirely (not merged at the rule level).
    pub fn apply_file(mut self, file: FileRulesConfig) -> Self {
        if let Some(event_rules) = file.event {
            self.event = event_rules;
        }
        if let Some(market_rules) = file.market {
            self.market = market_rules;
        }
        if let Some(trade_rules) = file.trade {
            self.trade = trade_rules;
        }
        self
    }

    /// Add an event rule
    pub fn with_event_rule(mut self, rule: EventRule) -> Self {
        self.event.push(rule);
        self
    }

    /// Add a market rule
    pub fn with_market_rule(mut self, rule: MarketRule) -> Self {
        self.market.push(rule);
        self
    }

    /// Add a trade rule
    pub fn with_trade_rule(mut self, rule: TradeRule) -> Self {
        self.trade.push(rule);
        self
    }
}

/// File config for rules - all fields optional (partial)
///
/// This is the type for TOML/JSON deserialization. Missing fields
/// mean "use defaults from RulesConfig::default()".
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct FileRulesConfig {
    /// Event-level rules (None = use default, which is empty)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<Vec<EventRule>>,
    /// Market-level rules (None = use default, which is empty)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market: Option<Vec<MarketRule>>,
    /// Trade-level rules (None = use default, which is empty)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade: Option<Vec<TradeRule>>,
}

impl FileRulesConfig {
    /// Check if any rules are specified in the file config
    pub fn has_rules(&self) -> bool {
        self.event.as_ref().map_or(false, |v| !v.is_empty())
            || self.market.as_ref().map_or(false, |v| !v.is_empty())
            || self.trade.as_ref().map_or(false, |v| !v.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_empty() {
        let config = RulesConfig::default();
        assert!(!config.has_rules());
        assert!(config.event.is_empty());
        assert!(config.market.is_empty());
        assert!(config.trade.is_empty());
    }

    #[test]
    fn test_apply_file_event_rules() {
        let config = RulesConfig::default();
        let file = FileRulesConfig {
            event: Some(vec![EventRule::MinMarketCap { threshold: 1_000_000_000 }]),
            market: None,
            trade: None,
        };

        let config = config.apply_file(file);
        assert!(config.has_event_rules());
        assert!(!config.has_market_rules());
    }

    #[test]
    fn test_apply_file_replaces_not_merges() {
        let config = RulesConfig::default()
            .with_event_rule(EventRule::MinMarketCap { threshold: 500_000_000 });

        let file = FileRulesConfig {
            event: Some(vec![EventRule::Symbols { include: vec!["AAPL".to_string()] }]),
            market: None,
            trade: None,
        };

        let config = config.apply_file(file);

        // File config replaced the event rules, not merged
        assert_eq!(config.event.len(), 1);
        assert!(matches!(config.event[0], EventRule::Symbols { .. }));
    }

    #[test]
    fn test_builder_pattern() {
        let config = RulesConfig::default()
            .with_event_rule(EventRule::MinMarketCap { threshold: 1_000_000_000 })
            .with_market_rule(MarketRule::MaxEntryIv { threshold: 1.5 })
            .with_trade_rule(TradeRule::EntryPriceRange { min: Some(0.50), max: None });

        assert!(config.has_event_rules());
        assert!(config.has_market_rules());
        assert!(config.has_trade_rules());
    }

    #[test]
    fn test_file_config_serde() {
        let toml = r#"
[[event]]
type = "min_market_cap"
threshold = 1000000000

[[market]]
type = "iv_slope"
short_dte = 7
long_dte = 20
threshold_pp = 0.05

[[market]]
type = "max_entry_iv"
threshold = 1.5
"#;

        let file: FileRulesConfig = toml::from_str(toml).unwrap();
        assert_eq!(file.event.as_ref().unwrap().len(), 1);
        assert_eq!(file.market.as_ref().unwrap().len(), 2);
        assert!(file.trade.is_none());
    }
}
