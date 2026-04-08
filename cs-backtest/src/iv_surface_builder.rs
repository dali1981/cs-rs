// IVSurface construction from option chain data
//
// Accepts domain `OptionBar` slices — no polars dependency on function signatures.
// This lives in cs-backtest (not cs-analytics) because it depends on equity repo
// for per-minute spot price lookups. cs-analytics remains pure computational.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use tracing::{debug, trace, warn};

use cs_analytics::{bs_implied_volatility, BSConfig, IVPoint, IVSurface};
use cs_domain::{CallPut, MarketTime, OptionBar, TradingDate, TradingTimestamp};
use cs_domain::repositories::EquityDataRepository;
use crate::iv_validation::validate_iv_for_surface;

/// Build an IV surface from an option chain slice.
///
/// Uses a single spot price for all options (no per-option spot lookup).
/// For more accurate surfaces, prefer `build_iv_surface_minute_aligned`.
pub fn build_iv_surface(
    chain: &[OptionBar],
    spot_price: f64,
    pricing_time: DateTime<Utc>,
    symbol: &str,
) -> Option<IVSurface> {
    let bs_config = BSConfig::default();
    let market_close = MarketTime::new(16, 0);
    let spot_decimal = Decimal::try_from(spot_price).ok()?;
    let mut points = Vec::new();

    for bar in chain {
        let close = match bar.close.filter(|&c| c > 0.0) {
            Some(c) => c,
            None => continue,
        };
        if bar.strike <= 0.0 {
            continue;
        }

        let ttm = calculate_ttm(pricing_time, bar.expiration, &market_close);
        if ttm <= 0.0 {
            continue;
        }

        let is_call = matches!(bar.option_type, CallPut::Call);

        let iv = match bs_implied_volatility(
            close,
            spot_price,
            bar.strike,
            ttm,
            is_call,
            &bs_config,
        ) {
            Some(v) => v,
            None => continue,
        };

        if !validate_iv_for_surface(iv) {
            continue;
        }

        let strike_decimal = match Decimal::try_from(bar.strike) {
            Ok(d) => d,
            Err(_) => continue,
        };

        points.push(IVPoint {
            strike: strike_decimal,
            expiration: bar.expiration,
            iv,
            timestamp: pricing_time,
            underlying_price: spot_decimal,
            is_call,
            contract_ticker: format!(
                "{}{}{}{}",
                symbol,
                bar.expiration.format("%y%m%d"),
                if is_call { "C" } else { "P" },
                bar.strike as i64
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

/// Build an IV surface with per-option spot price lookup (minute-aligned).
///
/// For each option bar, looks up the spot price at that option's specific timestamp,
/// ensuring correct IV computation. This is the preferred method for accurate surfaces.
///
/// The `chain` slice must include bars with `timestamp` set.
pub async fn build_iv_surface_minute_aligned<R: EquityDataRepository + ?Sized>(
    chain: &[OptionBar],
    equity_repo: &R,
    symbol: &str,
) -> Option<IVSurface> {
    let bs_config = BSConfig::default();
    let market_close = MarketTime::new(16, 0);

    let total_options = chain.len();
    debug!("Building IV surface for {} with {} options in chain", symbol, total_options);

    let mut points = Vec::new();
    let mut latest_timestamp: Option<DateTime<Utc>> = None;
    let mut latest_spot: Option<Decimal> = None;

    // Track failure reasons
    let mut skipped_invalid_prices = 0;
    let mut skipped_missing_timestamp = 0;
    let mut skipped_no_spot = 0;
    let mut skipped_decimal_conversion = 0;
    let mut skipped_expired = 0;
    let mut skipped_iv_calc_failed = 0;
    let mut skipped_iv_validation = 0;

    for bar in chain {
        let close = match bar.close.filter(|&c| c > 0.0) {
            Some(c) => c,
            None => {
                skipped_invalid_prices += 1;
                trace!("Bar: invalid close price");
                continue;
            }
        };

        if bar.strike <= 0.0 {
            skipped_invalid_prices += 1;
            continue;
        }

        let opt_timestamp = match bar.timestamp {
            Some(ts) => ts,
            None => {
                skipped_missing_timestamp += 1;
                continue;
            }
        };

        // Look up spot price at this option's trade timestamp
        let spot_price = match equity_repo.get_spot_price(symbol, opt_timestamp).await {
            Ok(sp) => sp,
            Err(e) => {
                skipped_no_spot += 1;
                if skipped_no_spot <= 3 {
                    debug!("No spot price at {} for {} - {}", opt_timestamp, symbol, e);
                }
                continue;
            }
        };
        let spot_f64 = spot_price.to_f64();
        let spot_decimal = match Decimal::try_from(spot_f64) {
            Ok(d) => d,
            Err(e) => {
                skipped_decimal_conversion += 1;
                debug!("Decimal conversion failed for spot {} - {}", spot_f64, e);
                continue;
            }
        };

        // Track latest timestamp and its spot for surface-level metadata
        if latest_timestamp.is_none() || opt_timestamp > latest_timestamp.unwrap() {
            latest_timestamp = Some(opt_timestamp);
            latest_spot = Some(spot_decimal);
        }

        let is_call = matches!(bar.option_type, CallPut::Call);

        let ttm = calculate_ttm(opt_timestamp, bar.expiration, &market_close);
        if ttm <= 0.0 {
            skipped_expired += 1;
            trace!("Bar: expired (ttm={})", ttm);
            continue;
        }

        let iv = match bs_implied_volatility(
            close,
            spot_f64,
            bar.strike,
            ttm,
            is_call,
            &bs_config,
        ) {
            Some(v) => v,
            None => {
                skipped_iv_calc_failed += 1;
                if skipped_iv_calc_failed <= 3 {
                    debug!(
                        "IV calculation failed (spot={}, strike={}, close={}, ttm={}, is_call={})",
                        spot_f64, bar.strike, close, ttm, is_call
                    );
                }
                continue;
            }
        };

        if !validate_iv_for_surface(iv) {
            skipped_iv_validation += 1;
            if skipped_iv_validation <= 3 {
                debug!("IV {} failed validation (bounds: 0.01-5.0)", iv);
            }
            continue;
        }

        let strike_decimal = match Decimal::try_from(bar.strike) {
            Ok(d) => d,
            Err(e) => {
                skipped_decimal_conversion += 1;
                debug!("Decimal conversion failed for strike {} - {}", bar.strike, e);
                continue;
            }
        };

        points.push(IVPoint {
            strike: strike_decimal,
            expiration: bar.expiration,
            iv,
            timestamp: opt_timestamp,
            underlying_price: spot_decimal,
            is_call,
            contract_ticker: format!(
                "{}{}{}{}",
                symbol,
                bar.expiration.format("%y%m%d"),
                if is_call { "C" } else { "P" },
                bar.strike as i64
            ),
        });
    }

    debug!(
        "IV surface for {}: processed {}, collected {} valid points. \
         Skipped: {} invalid prices, {} missing timestamp, {} no spot, \
         {} decimal conv, {} expired, {} IV calc failed, {} IV validation",
        symbol,
        total_options,
        points.len(),
        skipped_invalid_prices,
        skipped_missing_timestamp,
        skipped_no_spot,
        skipped_decimal_conversion,
        skipped_expired,
        skipped_iv_calc_failed,
        skipped_iv_validation
    );

    if points.is_empty() {
        warn!(
            "No valid IV points for {} (all {} options skipped). \
             Breakdown: invalid_prices={}, missing_timestamp={}, no_spot={}, \
             decimal_conv={}, expired={}, iv_calc_failed={}, iv_validation={}",
            symbol,
            total_options,
            skipped_invalid_prices,
            skipped_missing_timestamp,
            skipped_no_spot,
            skipped_decimal_conversion,
            skipped_expired,
            skipped_iv_calc_failed,
            skipped_iv_validation
        );
        return None;
    }

    let surface_spot = latest_spot?;
    let surface_time = latest_timestamp?;

    Some(IVSurface::new(
        points,
        symbol.to_string(),
        surface_time,
        surface_spot,
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
    use chrono::Utc;
    fn make_bar(strike: f64, expiration: NaiveDate, is_call: bool, close: f64) -> OptionBar {
        OptionBar {
            strike,
            expiration,
            option_type: if is_call { CallPut::Call } else { CallPut::Put },
            close: Some(close),
            timestamp: None,
        }
    }

    #[test]
    fn test_builds_surface_from_chain() {
        let exp = NaiveDate::from_ymd_opt(2025, 2, 21).unwrap();
        let chain = vec![
            make_bar(95.0, exp, true, 6.0),
            make_bar(100.0, exp, true, 3.5),
            make_bar(105.0, exp, true, 1.5),
            make_bar(100.0, NaiveDate::from_ymd_opt(2025, 3, 21).unwrap(), true, 5.0),
        ];

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
        let chain: Vec<OptionBar> = vec![];
        let pricing_time = Utc::now();
        let surface = build_iv_surface(&chain, 100.0, pricing_time, "TEST");
        assert!(surface.is_none());
    }
}
