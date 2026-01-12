//! Rule evaluator for entry filtering

use cs_domain::{RulesConfig, MarketRule, EarningsEvent, RuleError};
use cs_analytics::IVSurface;
use crate::backtest_use_case_helpers::PreparedData;

/// Evaluates entry rules at each stage of the backtest pipeline
#[derive(Clone)]
pub struct RuleEvaluator {
    config: RulesConfig,
}

impl RuleEvaluator {
    /// Create a new rule evaluator from config
    pub fn new(config: RulesConfig) -> Self {
        Self { config }
    }

    /// Check if any rules are configured
    pub fn has_rules(&self) -> bool {
        self.config.has_rules()
    }

    /// Check if there are market-level rules (need PreparedData)
    pub fn has_market_rules(&self) -> bool {
        self.config.has_market_rules()
    }

    /// Check if there are trade-level rules (need entry pricing)
    pub fn has_trade_rules(&self) -> bool {
        !self.config.trade.is_empty()
    }

    /// Evaluate all event-level rules (AND logic)
    ///
    /// Returns true if all rules pass, false if any rule fails.
    /// Returns true if no event rules are configured.
    pub fn eval_event_rules(&self, event: &EarningsEvent) -> bool {
        if self.config.event.is_empty() {
            return true;
        }

        for rule in &self.config.event {
            if !rule.eval(event) {
                tracing::debug!(
                    symbol = %event.symbol,
                    rule = rule.name(),
                    "Event rule failed"
                );
                return false;
            }
        }
        true
    }

    /// Evaluate all market-level rules (AND logic)
    ///
    /// Returns Ok(true) if all rules pass, Ok(false) if any rule fails.
    /// Returns Ok(true) if no market rules are configured.
    pub fn eval_market_rules(
        &self,
        event: &EarningsEvent,
        data: &PreparedData,
    ) -> Result<bool, RuleError> {
        if self.config.market.is_empty() {
            return Ok(true);
        }

        for rule in &self.config.market {
            match self.eval_market_rule(rule, data) {
                Ok(true) => continue,
                Ok(false) => {
                    tracing::debug!(
                        symbol = %event.symbol,
                        rule = rule.name(),
                        "Market rule failed"
                    );
                    return Ok(false);
                }
                Err(e) => {
                    tracing::debug!(
                        symbol = %event.symbol,
                        rule = rule.name(),
                        error = %e,
                        "Market rule evaluation error"
                    );
                    return Err(e);
                }
            }
        }
        Ok(true)
    }

    /// Evaluate market rules and return the first failed rule name (if any)
    ///
    /// Returns Ok(None) if all rules pass or if no market rules are configured.
    pub fn eval_market_rules_with_reason(
        &self,
        event: &EarningsEvent,
        data: &PreparedData,
    ) -> Result<Option<String>, RuleError> {
        if self.config.market.is_empty() {
            return Ok(None);
        }

        for rule in &self.config.market {
            match self.eval_market_rule(rule, data) {
                Ok(true) => continue,
                Ok(false) => {
                    tracing::debug!(
                        symbol = %event.symbol,
                        rule = rule.name(),
                        "Market rule failed"
                    );
                    return Ok(Some(rule.name().to_string()));
                }
                Err(e) => {
                    tracing::debug!(
                        symbol = %event.symbol,
                        rule = rule.name(),
                        error = %e,
                        "Market rule evaluation error"
                    );
                    return Err(e);
                }
            }
        }

        Ok(None)
    }

    /// Evaluate all trade-level rules (AND logic)
    ///
    /// Returns true if all rules pass, false if any rule fails.
    /// Returns true if no trade rules are configured.
    pub fn eval_trade_rules(&self, event: &EarningsEvent, entry_price: f64) -> bool {
        if self.config.trade.is_empty() {
            return true;
        }

        for rule in &self.config.trade {
            if !rule.eval_price(entry_price) {
                tracing::debug!(
                    symbol = %event.symbol,
                    rule = rule.name(),
                    entry_price = entry_price,
                    "Trade rule failed"
                );
                return false;
            }
        }
        true
    }

    /// Evaluate trade rules and return the first failed rule name (if any)
    pub fn eval_trade_rules_with_reason(
        &self,
        event: &EarningsEvent,
        entry_price: f64,
    ) -> Option<String> {
        if self.config.trade.is_empty() {
            return None;
        }

        for rule in &self.config.trade {
            if !rule.eval_price(entry_price) {
                tracing::debug!(
                    symbol = %event.symbol,
                    rule = rule.name(),
                    entry_price = entry_price,
                    "Trade rule failed"
                );
                return Some(rule.name().to_string());
            }
        }

        None
    }

    /// Evaluate a single market rule
    fn eval_market_rule(
        &self,
        rule: &MarketRule,
        data: &PreparedData,
    ) -> Result<bool, RuleError> {
        match rule {
            MarketRule::IvSlope { short_dte, long_dte, threshold_pp } => {
                let iv_short = get_atm_iv_at_dte(&data.surface, *short_dte)
                    .ok_or(RuleError::MissingDteData { rule: "iv_slope", dte: *short_dte })?;
                let iv_long = get_atm_iv_at_dte(&data.surface, *long_dte)
                    .ok_or(RuleError::MissingDteData { rule: "iv_slope", dte: *long_dte })?;

                tracing::trace!(
                    short_dte = short_dte,
                    iv_short = iv_short,
                    long_dte = long_dte,
                    iv_long = iv_long,
                    threshold = *threshold_pp,
                    "Evaluating IV slope rule"
                );

                let passes = iv_short > iv_long + *threshold_pp;
                if !passes {
                    tracing::debug!(
                        short_dte = short_dte,
                        iv_short = iv_short,
                        long_dte = long_dte,
                        iv_long = iv_long,
                        threshold = *threshold_pp,
                        "IV slope rule failed"
                    );
                }
                Ok(passes)
            }

            MarketRule::MaxEntryIv { threshold } => {
                let atm_iv = get_front_month_atm_iv(&data.surface)
                    .ok_or(RuleError::MissingData { rule: "max_entry_iv", field: "ATM IV" })?;
                let passes = atm_iv <= *threshold;
                if !passes {
                    tracing::debug!(
                        atm_iv = atm_iv,
                        threshold = *threshold,
                        "Max entry IV rule failed"
                    );
                }
                Ok(passes)
            }

            MarketRule::MinIvRatio { short_dte, long_dte, threshold } => {
                let iv_short = get_atm_iv_at_dte(&data.surface, *short_dte)
                    .ok_or(RuleError::MissingDteData { rule: "min_iv_ratio", dte: *short_dte })?;
                let iv_long = get_atm_iv_at_dte(&data.surface, *long_dte)
                    .ok_or(RuleError::MissingDteData { rule: "min_iv_ratio", dte: *long_dte })?;

                if iv_long <= 0.0 {
                    return Ok(false); // Can't compute ratio with zero denominator
                }

                let ratio = iv_short / iv_long;
                let passes = ratio >= *threshold;
                if !passes {
                    tracing::debug!(
                        short_dte = short_dte,
                        iv_short = iv_short,
                        long_dte = long_dte,
                        iv_long = iv_long,
                        ratio = ratio,
                        threshold = *threshold,
                        "IV ratio rule failed"
                    );
                }
                Ok(passes)
            }

            MarketRule::IvVsHv { hv_window_days: _, min_ratio: _ } => {
                // HV computation not yet implemented
                // For now, pass if HV rule is specified but we can't evaluate
                tracing::warn!("IV vs HV rule not yet implemented, passing by default");
                Ok(true)
            }

            MarketRule::MinNotional { threshold: _ } => {
                // Notional computation requires option volume data
                // For now, pass if notional rule is specified
                tracing::warn!("Min notional rule not yet implemented, passing by default");
                Ok(true)
            }
        }
    }
}

/// Get ATM IV for a specific target DTE
///
/// Finds the closest expiration to the target DTE and returns its ATM IV.
fn get_atm_iv_at_dte(surface: &IVSurface, target_dte: u16) -> Option<f64> {
    let target_days = target_dte as i32;
    let call_iv = surface.get_iv_by_moneyness_ttm(1.0, target_days, true);
    let put_iv = surface.get_iv_by_moneyness_ttm(1.0, target_days, false);

    match (call_iv, put_iv) {
        (Some(call), Some(put)) => Some((call + put) / 2.0),
        (Some(call), None) => Some(call),
        (None, Some(put)) => Some(put),
        (None, None) => None,
    }
}

/// Get ATM IV for a specific expiration date
fn get_atm_iv_for_expiration(surface: &IVSurface, expiration: chrono::NaiveDate) -> Option<f64> {
    let spot = surface.spot_price();
    if spot.is_zero() {
        return None;
    }

    // Find points for this expiration near ATM
    let atm_tolerance = 0.05; // 5% moneyness tolerance

    let atm_points: Vec<_> = surface.points().iter()
        .filter(|p| p.expiration == expiration)
        .filter(|p| p.is_atm(atm_tolerance))
        .collect();

    if atm_points.is_empty() {
        // Fall back to closest to ATM
        let closest = surface.points().iter()
            .filter(|p| p.expiration == expiration)
            .min_by(|a, b| {
                let a_dist = (a.moneyness() - 1.0).abs();
                let b_dist = (b.moneyness() - 1.0).abs();
                a_dist.partial_cmp(&b_dist).unwrap_or(std::cmp::Ordering::Equal)
            })?;
        return Some(closest.iv);
    }

    // Average IV of ATM points
    let sum: f64 = atm_points.iter().map(|p| p.iv).sum();
    Some(sum / atm_points.len() as f64)
}

/// Get front-month ATM IV
fn get_front_month_atm_iv(surface: &IVSurface) -> Option<f64> {
    let as_of = surface.as_of_time().date_naive();

    // Find closest future expiration
    let front_month = surface.expirations().into_iter()
        .filter(|exp| *exp > as_of)
        .min()?;

    get_atm_iv_for_expiration(surface, front_month)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use cs_domain::EarningsTime;

    fn mock_event(symbol: &str, market_cap: Option<u64>) -> EarningsEvent {
        EarningsEvent {
            symbol: symbol.to_string(),
            date: NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
            time: EarningsTime::AfterClose,
            market_cap,
        }
    }

    #[test]
    fn test_empty_rules_pass() {
        let evaluator = RuleEvaluator::new(RulesConfig::default());
        let event = mock_event("AAPL", Some(1_000_000_000));

        assert!(evaluator.eval_event_rules(&event));
        assert!(!evaluator.has_rules());
    }

    #[test]
    fn test_event_rule_market_cap_passes() {
        let config = RulesConfig::default()
            .with_event_rule(EventRule::MinMarketCap { threshold: 1_000_000_000 });
        let evaluator = RuleEvaluator::new(config);
        let event = mock_event("AAPL", Some(2_000_000_000));

        assert!(evaluator.eval_event_rules(&event));
    }

    #[test]
    fn test_event_rule_market_cap_fails() {
        let config = RulesConfig::default()
            .with_event_rule(EventRule::MinMarketCap { threshold: 1_000_000_000 });
        let evaluator = RuleEvaluator::new(config);
        let event = mock_event("SMALL", Some(500_000_000));

        assert!(!evaluator.eval_event_rules(&event));
    }

    #[test]
    fn test_trade_rule_price_range() {
        let config = RulesConfig::default()
            .with_trade_rule(TradeRule::EntryPriceRange { min: Some(0.50), max: Some(50.0) });
        let evaluator = RuleEvaluator::new(config);
        let event = mock_event("AAPL", None);

        assert!(evaluator.eval_trade_rules(&event, 10.0));
        assert!(!evaluator.eval_trade_rules(&event, 0.25));
        assert!(!evaluator.eval_trade_rules(&event, 75.0));
    }
}
