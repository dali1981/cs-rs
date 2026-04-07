//! Test infrastructure for the cs-domain crate.
//!
//! Provides fluent builders for constructing domain objects in tests,
//! preventing direct struct initialization and insulating tests from
//! field-name / variant changes. See ADR-0005.
//!
//! # Placement rationale
//! Builders live here (not in production module paths) to avoid polluting the
//! production API. They are `pub` so integration tests in other crates can
//! import them as `cs_domain::testing::EarningsEventBuilder`.

use chrono::NaiveDate;

use crate::entities::EarningsEvent;
use crate::value_objects::{EarningsTime, TradeDirection, IronButterflyConfig, MultiLegStrategyConfig};
use crate::strike_selection::OptionStrategy;
use crate::expiration::ExpirationPolicy;
use crate::campaign::{TradingCampaign, PeriodPolicy};

// ── EarningsEventBuilder ──────────────────────────────────────────────────────

/// Builder for [`EarningsEvent`] — use in tests instead of direct struct init.
///
/// Default values:
/// - `symbol`: "TEST"
/// - `earnings_date`: 2024-01-15
/// - `earnings_time`: `EarningsTime::AfterMarketClose`
/// - All optional fields: `None`
///
/// # Invariants enforced by `build()`
/// - `symbol` must not be empty
pub struct EarningsEventBuilder {
    symbol: String,
    earnings_date: NaiveDate,
    earnings_time: EarningsTime,
    market_cap: Option<u64>,
    company_name: Option<String>,
}

impl Default for EarningsEventBuilder {
    fn default() -> Self {
        Self::new("TEST")
    }
}

impl EarningsEventBuilder {
    pub fn new(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            earnings_date: NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
            earnings_time: EarningsTime::AfterMarketClose,
            market_cap: None,
            company_name: None,
        }
    }

    pub fn symbol(mut self, symbol: &str) -> Self {
        self.symbol = symbol.to_string();
        self
    }

    pub fn earnings_date(mut self, date: NaiveDate) -> Self {
        self.earnings_date = date;
        self
    }

    pub fn earnings_time(mut self, time: EarningsTime) -> Self {
        self.earnings_time = time;
        self
    }

    pub fn market_cap(mut self, cap: u64) -> Self {
        self.market_cap = Some(cap);
        self
    }

    pub fn market_cap_opt(mut self, cap: Option<u64>) -> Self {
        self.market_cap = cap;
        self
    }

    pub fn company_name(mut self, name: &str) -> Self {
        self.company_name = Some(name.to_string());
        self
    }

    /// Build the `EarningsEvent`.
    ///
    /// # Panics
    /// Panics if `symbol` is empty.
    pub fn build(self) -> EarningsEvent {
        assert!(!self.symbol.is_empty(), "EarningsEventBuilder: symbol must not be empty");
        EarningsEvent {
            symbol: self.symbol,
            earnings_date: self.earnings_date,
            earnings_time: self.earnings_time,
            company_name: self.company_name,
            eps_forecast: None,
            market_cap: self.market_cap,
        }
    }
}

// ── TradingCampaignBuilder ────────────────────────────────────────────────────

/// Builder for [`TradingCampaign`] — use in tests instead of direct struct init.
///
/// Default values:
/// - `symbol`: "TEST"
/// - `strategy`: `OptionStrategy::CalendarSpread`
/// - `start_date`: 2025-01-01
/// - `end_date`: 2025-12-31
/// - `period_policy`: `PeriodPolicy::cross_earnings()`
/// - `expiration_policy`: `ExpirationPolicy::FirstAfter { min_date: 2025-01-01 }`
/// - `iron_butterfly_config`: `None`
/// - `multi_leg_strategy_config`: `None`
/// - `trade_direction`: `TradeDirection::Short`
///
/// # Invariants enforced by `build()`
/// - `symbol` must not be empty
/// - `end_date` must be >= `start_date`
pub struct TradingCampaignBuilder {
    symbol: String,
    strategy: OptionStrategy,
    start_date: NaiveDate,
    end_date: NaiveDate,
    period_policy: PeriodPolicy,
    expiration_policy: ExpirationPolicy,
    iron_butterfly_config: Option<IronButterflyConfig>,
    multi_leg_strategy_config: Option<MultiLegStrategyConfig>,
    trade_direction: TradeDirection,
}

impl Default for TradingCampaignBuilder {
    fn default() -> Self {
        Self::new("TEST")
    }
}

impl TradingCampaignBuilder {
    pub fn new(symbol: &str) -> Self {
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        Self {
            symbol: symbol.to_string(),
            strategy: OptionStrategy::CalendarSpread,
            start_date: start,
            end_date: NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            period_policy: PeriodPolicy::cross_earnings(),
            expiration_policy: ExpirationPolicy::FirstAfter { min_date: start },
            iron_butterfly_config: None,
            multi_leg_strategy_config: None,
            trade_direction: TradeDirection::Short,
        }
    }

    pub fn symbol(mut self, symbol: &str) -> Self {
        self.symbol = symbol.to_string();
        self
    }

    pub fn strategy(mut self, strategy: OptionStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    pub fn start_date(mut self, date: NaiveDate) -> Self {
        self.start_date = date;
        self
    }

    pub fn end_date(mut self, date: NaiveDate) -> Self {
        self.end_date = date;
        self
    }

    pub fn period_policy(mut self, policy: PeriodPolicy) -> Self {
        self.period_policy = policy;
        self
    }

    pub fn expiration_policy(mut self, policy: ExpirationPolicy) -> Self {
        self.expiration_policy = policy;
        self
    }

    pub fn iron_butterfly_config(mut self, config: IronButterflyConfig) -> Self {
        self.iron_butterfly_config = Some(config);
        self
    }

    pub fn multi_leg_strategy_config(mut self, config: MultiLegStrategyConfig) -> Self {
        self.multi_leg_strategy_config = Some(config);
        self
    }

    pub fn trade_direction(mut self, direction: TradeDirection) -> Self {
        self.trade_direction = direction;
        self
    }

    /// Build the `TradingCampaign`.
    ///
    /// # Panics
    /// Panics if `symbol` is empty or `end_date` < `start_date`.
    pub fn build(self) -> TradingCampaign {
        assert!(!self.symbol.is_empty(), "TradingCampaignBuilder: symbol must not be empty");
        assert!(
            self.end_date >= self.start_date,
            "TradingCampaignBuilder: end_date ({}) must be >= start_date ({})",
            self.end_date, self.start_date,
        );
        TradingCampaign {
            symbol: self.symbol,
            strategy: self.strategy,
            start_date: self.start_date,
            end_date: self.end_date,
            period_policy: self.period_policy,
            expiration_policy: self.expiration_policy,
            iron_butterfly_config: self.iron_butterfly_config,
            multi_leg_strategy_config: self.multi_leg_strategy_config,
            trade_direction: self.trade_direction,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earnings_event_builder_default_symbol() {
        let event = EarningsEventBuilder::default().build();
        assert_eq!(event.symbol, "TEST");
        assert_eq!(event.earnings_time, EarningsTime::AfterMarketClose);
        assert!(event.market_cap.is_none());
    }

    #[test]
    fn earnings_event_builder_fluent_setters() {
        let date = NaiveDate::from_ymd_opt(2025, 3, 15).unwrap();
        let event = EarningsEventBuilder::new("AAPL")
            .earnings_date(date)
            .earnings_time(EarningsTime::BeforeMarketOpen)
            .market_cap(500_000_000_000)
            .company_name("Apple Inc.")
            .build();

        assert_eq!(event.symbol, "AAPL");
        assert_eq!(event.earnings_date, date);
        assert_eq!(event.earnings_time, EarningsTime::BeforeMarketOpen);
        assert_eq!(event.market_cap, Some(500_000_000_000));
        assert_eq!(event.company_name, Some("Apple Inc.".to_string()));
    }

    #[test]
    fn trading_campaign_builder_default_symbol() {
        let campaign = TradingCampaignBuilder::default().build();
        assert_eq!(campaign.symbol, "TEST");
        assert_eq!(campaign.strategy, OptionStrategy::CalendarSpread);
        assert!(campaign.iron_butterfly_config.is_none());
        assert!(campaign.multi_leg_strategy_config.is_none());
        assert_eq!(campaign.trade_direction, TradeDirection::Short);
    }

    #[test]
    fn trading_campaign_builder_overrides() {
        let campaign = TradingCampaignBuilder::new("MSFT")
            .strategy(OptionStrategy::Straddle)
            .build();

        assert_eq!(campaign.symbol, "MSFT");
        assert_eq!(campaign.strategy, OptionStrategy::Straddle);
    }

    #[test]
    #[should_panic(expected = "symbol must not be empty")]
    fn earnings_event_builder_rejects_empty_symbol() {
        EarningsEventBuilder::new("").build();
    }

    #[test]
    #[should_panic(expected = "symbol must not be empty")]
    fn trading_campaign_builder_rejects_empty_symbol() {
        TradingCampaignBuilder::new("").build();
    }

    #[test]
    #[should_panic(expected = "end_date")]
    fn trading_campaign_builder_rejects_inverted_dates() {
        TradingCampaignBuilder::new("TEST")
            .start_date(NaiveDate::from_ymd_opt(2025, 12, 31).unwrap())
            .end_date(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap())
            .build();
    }
}
