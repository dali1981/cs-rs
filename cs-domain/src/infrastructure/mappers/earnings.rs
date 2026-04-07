//! Earnings data mappers — translate external provider formats to domain EarningsEvent.
//!
//! Single source of truth for all earnings time string parsing and
//! provider-specific event translation.

use crate::value_objects::EarningsTime;

/// Parse any known earnings time string into a canonical [`EarningsTime`].
///
/// Covers all provider formats in use:
/// - Parquet / JSON files: "BMO", "AMC", "BEFORE_MARKET", "AFTER_MARKET",
///   "BEFOREMARKET", "AFTERMARKET"
/// - Canonical names: "bmo", "amc", "before_market_open", "after_market_close"
/// - Legacy aliases: "pre-market", "post-market"
///
/// Unknown strings map to [`EarningsTime::Unknown`] — never an error.
pub fn parse_earnings_time(s: &str) -> EarningsTime {
    match s.to_lowercase().as_str() {
        "bmo" | "before_market_open" | "pre-market" | "before_market" | "beforemarket" => {
            EarningsTime::BeforeMarketOpen
        }
        "amc" | "after_market_close" | "post-market" | "after_market" | "aftermarket" => {
            EarningsTime::AfterMarketClose
        }
        _ => EarningsTime::Unknown,
    }
}

/// Map an `earnings_rs::ReportTime` to the canonical [`EarningsTime`].
#[cfg(feature = "earnings-rs")]
pub fn report_time_to_earnings_time(report_time: earnings_rs::ReportTime) -> EarningsTime {
    match report_time {
        earnings_rs::ReportTime::BeforeMarket => EarningsTime::BeforeMarketOpen,
        earnings_rs::ReportTime::AfterMarket => EarningsTime::AfterMarketClose,
        earnings_rs::ReportTime::DuringMarket | earnings_rs::ReportTime::NotSupplied => {
            EarningsTime::Unknown
        }
    }
}

/// Translate an `earnings_rs::EarningsEvent` into a domain [`EarningsEvent`].
///
/// Market cap is stored in millions in earnings-rs; we convert to whole dollars.
#[cfg(feature = "earnings-rs")]
impl IntoNormalized<EarningsEvent> for earnings_rs::EarningsEvent {
    fn into_normalized(self) -> Result<EarningsEvent, RepositoryError> {
        let earnings_time = report_time_to_earnings_time(self.report_time);

        let market_cap = self.market_cap_millions.and_then(|cap_millions| {
            let cap_millions_f64: f64 = cap_millions.try_into().ok()?;
            Some((cap_millions_f64 * 1_000_000.0) as u64)
        });

        let mut event = EarningsEvent::new(self.symbol, self.report_date, earnings_time);

        if !self.company_name.is_empty() {
            event = event.with_company_name(self.company_name);
        }

        if let Some(cap) = market_cap {
            event = event.with_market_cap(cap);
        }

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_earnings_time_bmo_variants() {
        for s in &["BMO", "bmo", "BEFORE_MARKET", "before_market", "BEFOREMARKET", "beforemarket", "before_market_open", "pre-market"] {
            assert_eq!(
                parse_earnings_time(s),
                EarningsTime::BeforeMarketOpen,
                "expected BeforeMarketOpen for '{s}'"
            );
        }
    }

    #[test]
    fn parse_earnings_time_amc_variants() {
        for s in &["AMC", "amc", "AFTER_MARKET", "after_market", "AFTERMARKET", "aftermarket", "after_market_close", "post-market"] {
            assert_eq!(
                parse_earnings_time(s),
                EarningsTime::AfterMarketClose,
                "expected AfterMarketClose for '{s}'"
            );
        }
    }

    #[test]
    fn parse_earnings_time_unknown_fallback() {
        for s in &["", "during", "???", "n/a", "DURING_MARKET"] {
            assert_eq!(
                parse_earnings_time(s),
                EarningsTime::Unknown,
                "expected Unknown for '{s}'"
            );
        }
    }

    #[cfg(feature = "earnings-rs")]
    mod earnings_rs_tests {
        use super::*;
        use chrono::NaiveDate;

        #[test]
        fn report_time_before_market_maps_to_bmo() {
            assert_eq!(
                report_time_to_earnings_time(earnings_rs::ReportTime::BeforeMarket),
                EarningsTime::BeforeMarketOpen
            );
        }

        #[test]
        fn report_time_after_market_maps_to_amc() {
            assert_eq!(
                report_time_to_earnings_time(earnings_rs::ReportTime::AfterMarket),
                EarningsTime::AfterMarketClose
            );
        }

        #[test]
        fn report_time_not_supplied_maps_to_unknown() {
            assert_eq!(
                report_time_to_earnings_time(earnings_rs::ReportTime::NotSupplied),
                EarningsTime::Unknown
            );
        }

        #[test]
        fn report_time_during_market_maps_to_unknown() {
            assert_eq!(
                report_time_to_earnings_time(earnings_rs::ReportTime::DuringMarket),
                EarningsTime::Unknown
            );
        }

        #[test]
        fn earnings_event_into_normalized_maps_all_fields() {
            use rust_decimal::Decimal;

            let ext = earnings_rs::EarningsEvent {
                symbol: "NVDA".into(),
                company_name: "NVIDIA Corporation".into(),
                report_date: NaiveDate::from_ymd_opt(2024, 11, 20).unwrap(),
                report_time: earnings_rs::ReportTime::AfterMarket,
                fiscal_quarter_ending: None,
                eps_forecast: None,
                eps_actual: None,
                last_year_eps: None,
                surprise_pct: None,
                market_cap_millions: Some(Decimal::from(3_000_000u64)),
                num_of_estimates: None,
            };

            let domain = ext.into_normalized().unwrap();

            assert_eq!(domain.symbol, "NVDA");
            assert_eq!(domain.company_name, Some("NVIDIA Corporation".into()));
            assert_eq!(domain.earnings_time, EarningsTime::AfterMarketClose);
            assert_eq!(domain.market_cap, Some(3_000_000_000_000u64));
            assert_eq!(domain.earnings_date, NaiveDate::from_ymd_opt(2024, 11, 20).unwrap());
        }

        #[test]
        fn earnings_event_into_normalized_empty_company_name_omitted() {
            let ext = earnings_rs::EarningsEvent {
                symbol: "AAPL".into(),
                company_name: "".into(),
                report_date: NaiveDate::from_ymd_opt(2025, 2, 1).unwrap(),
                report_time: earnings_rs::ReportTime::BeforeMarket,
                fiscal_quarter_ending: None,
                eps_forecast: None,
                eps_actual: None,
                last_year_eps: None,
                surprise_pct: None,
                market_cap_millions: None,
                num_of_estimates: None,
            };

            let domain = ext.into_normalized().unwrap();

            assert_eq!(domain.company_name, None);
            assert_eq!(domain.market_cap, None);
            assert_eq!(domain.earnings_time, EarningsTime::BeforeMarketOpen);
        }
    }
}
