// IVSurface construction from option chain DataFrame
//
// This lives in cs-backtest (not cs-analytics) because it depends on polars
// for DataFrame handling. cs-analytics remains pure computational.

use chrono::{DateTime, NaiveDate, Utc};
use polars::prelude::*;
use rust_decimal::Decimal;

use cs_analytics::{bs_implied_volatility, BSConfig, IVPoint, IVSurface};
use cs_domain::{MarketTime, TradingDate, TradingTimestamp};

/// Build an IV surface from option chain DataFrame
///
/// The DataFrame should have columns: strike, expiration, close, option_type
pub fn build_iv_surface(
    chain_df: &DataFrame,
    spot_price: f64,
    pricing_time: DateTime<Utc>,
    symbol: &str,
) -> Option<IVSurface> {
    let bs_config = BSConfig::default();
    let market_close = MarketTime::new(16, 0);

    // Extract columns we need
    let strikes = chain_df.column("strike").ok()?.f64().ok()?;
    let expirations = chain_df.column("expiration").ok()?.date().ok()?;
    let closes = chain_df.column("close").ok()?.f64().ok()?;
    let option_types = chain_df.column("option_type").ok()?.str().ok()?;

    let spot_decimal = Decimal::try_from(spot_price).ok()?;
    let mut points = Vec::new();

    for i in 0..chain_df.height() {
        // Extract row data, skip if any value is missing
        let (strike_f64, exp_days, close, opt_type) = match (
            strikes.get(i),
            expirations.get(i),
            closes.get(i),
            option_types.get(i),
        ) {
            (Some(s), Some(e), Some(c), Some(t)) => (s, e, c, t),
            _ => continue,
        };

        // Skip invalid data
        if close <= 0.0 || strike_f64 <= 0.0 {
            continue;
        }

        // Convert expiration from Polars date (days since epoch) to NaiveDate
        let expiration = TradingDate::from_polars_date(exp_days).to_naive_date();
        let is_call = opt_type == "call";

        // Calculate time to maturity
        let ttm = calculate_ttm(pricing_time, expiration, &market_close);
        if ttm <= 0.0 {
            continue; // Skip expired options
        }

        // Calculate IV from market price
        let iv = match bs_implied_volatility(
            close,
            spot_price,
            strike_f64,
            ttm,
            is_call,
            &bs_config,
        ) {
            Some(v) => v,
            None => continue,
        };

        // Skip unreasonable IVs
        if iv < 0.01 || iv > 5.0 {
            continue;
        }

        let strike_decimal = match Decimal::try_from(strike_f64) {
            Ok(d) => d,
            Err(_) => continue,
        };

        points.push(IVPoint {
            strike: strike_decimal,
            expiration,
            iv,
            timestamp: pricing_time,
            underlying_price: spot_decimal,
            is_call,
            contract_ticker: format!(
                "{}{}{}{}",
                symbol,
                expiration.format("%y%m%d"),
                if is_call { "C" } else { "P" },
                strike_f64 as i64
            ),
        });
    }

    if points.is_empty() {
        return None;
    }

    Some(IVSurface::new(
        points,
        symbol.to_string(),
        pricing_time,
        spot_decimal,
    ))
}

fn calculate_ttm(from: DateTime<Utc>, to_date: NaiveDate, market_close: &MarketTime) -> f64 {
    let from_ts = TradingTimestamp::from_datetime_utc(from);
    let to_date_trading = TradingDate::from_naive_date(to_date);
    from_ts.time_to_expiry(&to_date_trading, market_close)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_chain() -> DataFrame {
        let strikes = Series::new("strike".into(), &[95.0, 100.0, 105.0, 100.0]);
        let exp_date = NaiveDate::from_ymd_opt(2025, 2, 21).unwrap();
        let exp_days = (exp_date - NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()).num_days() as i32;
        let expirations = Series::new("expiration".into(), &[exp_days, exp_days, exp_days, exp_days + 30]);
        let closes = Series::new("close".into(), &[6.0, 3.5, 1.5, 5.0]);
        let option_types = Series::new("option_type".into(), &["call", "call", "call", "call"]);

        DataFrame::new(vec![
            strikes.cast(&DataType::Float64).unwrap(),
            expirations.cast(&DataType::Date).unwrap(),
            closes.cast(&DataType::Float64).unwrap(),
            option_types,
        ]).unwrap()
    }

    #[test]
    fn test_builds_surface_from_chain() {
        let chain = create_test_chain();
        let pricing_time = NaiveDate::from_ymd_opt(2025, 1, 15)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap()
            .and_utc();

        let surface = build_iv_surface(&chain, 100.0, pricing_time, "TEST");

        assert!(surface.is_some());
        let surface = surface.unwrap();
        assert_eq!(surface.underlying(), "TEST");
        assert!(!surface.points().is_empty());
    }

    #[test]
    fn test_empty_chain_returns_none() {
        let chain = DataFrame::new(vec![
            Series::new("strike".into(), Vec::<f64>::new()),
            Series::new("expiration".into(), Vec::<i32>::new()).cast(&DataType::Date).unwrap(),
            Series::new("close".into(), Vec::<f64>::new()),
            Series::new("option_type".into(), Vec::<&str>::new()),
        ]).unwrap();

        let pricing_time = Utc::now();

        let surface = build_iv_surface(&chain, 100.0, pricing_time, "TEST");
        assert!(surface.is_none());
    }
}
