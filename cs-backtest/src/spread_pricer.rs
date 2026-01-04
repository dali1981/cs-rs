use chrono::{DateTime, NaiveDate, Utc};
use polars::prelude::*;
use rust_decimal::Decimal;
use finq_core::OptionType;
use tracing::warn;

use cs_analytics::{
    bs_price, bs_greeks, bs_implied_volatility, BSConfig, Greeks, IVSurface, IVPoint,
    PricingModel, PricingIVProvider,
};
use cs_domain::{CalendarSpread, Strike, TradingDate, TradingTimestamp, MarketTime};

/// Error type for pricing operations
#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    #[error("No option data available for {symbol} on {date}")]
    NoData { symbol: String, date: String },
    #[error("Missing column: {0}")]
    MissingColumn(String),
    #[error("Polars error: {0}")]
    Polars(String),
    #[error("No price found for contract {0}")]
    NoPriceFound(String),
    #[error("Invalid IV: {0}")]
    InvalidIV(String),
}

/// Pricing result for a single option leg
#[derive(Debug, Clone)]
pub struct LegPricing {
    pub price: Decimal,
    pub iv: Option<f64>,
    pub greeks: Option<Greeks>,
}

/// Pricing result for a calendar spread
#[derive(Debug, Clone)]
pub struct SpreadPricing {
    pub short_leg: LegPricing,
    pub long_leg: LegPricing,
    pub net_cost: Decimal,  // Long - Short (for calendar spread)
}

/// Service for pricing options using Black-Scholes
pub struct SpreadPricer {
    bs_config: BSConfig,
    market_close: MarketTime,
    pricing_model: PricingModel,
}

impl SpreadPricer {
    pub fn new() -> Self {
        Self {
            bs_config: BSConfig::default(),
            market_close: MarketTime::new(16, 0), // Default 4 PM
            pricing_model: PricingModel::default(),         // Sticky moneyness
        }
    }

    pub fn with_market_close(mut self, market_close: MarketTime) -> Self {
        self.market_close = market_close;
        self
    }

    /// Set the pricing IV interpolation model
    ///
    /// - `StickyStrike`: IV indexed by absolute strike K (default)
    /// - `StickyMoneyness`: IV indexed by K/S (floats with spot)
    /// - `StickyDelta`: IV indexed by delta (iterative, most accurate floating smile)
    pub fn with_pricing_model(mut self, pricing_model: PricingModel) -> Self {
        self.pricing_model = pricing_model;
        self
    }

    /// Get the current pricing model
    pub fn pricing_model(&self) -> PricingModel {
        self.pricing_model
    }

    /// Price a calendar spread using option chain data
    pub fn price_spread(
        &self,
        spread: &CalendarSpread,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
    ) -> Result<SpreadPricing, PricingError> {
        // Build IV surface for fallback interpolation
        let iv_surface = self.build_iv_surface(
            chain_df,
            spot_price,
            pricing_time,
            spread.symbol(),
        );

        self.price_spread_with_surface(spread, chain_df, spot_price, pricing_time, iv_surface.as_ref())
    }

    /// Price a calendar spread using a pre-built IV surface
    ///
    /// Use this when you have a minute-aligned IV surface built with per-option spot prices.
    pub fn price_spread_with_surface(
        &self,
        spread: &CalendarSpread,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<SpreadPricing, PricingError> {
        // Create pricing provider based on configured pricing model
        let pricing_provider = self.pricing_model.to_provider_with_rate(self.bs_config.risk_free_rate);

        let short_pricing = self.price_leg(
            &spread.short_leg.strike,
            spread.short_leg.expiration,
            spread.short_leg.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let long_pricing = self.price_leg(
            &spread.long_leg.strike,
            spread.long_leg.expiration,
            spread.long_leg.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        // Calendar spread: pay (long - short)
        let net_cost = long_pricing.price - short_pricing.price;

        Ok(SpreadPricing {
            short_leg: short_pricing,
            long_leg: long_pricing,
            net_cost,
        })
    }

    /// Price a single option leg (public for use by iron butterfly pricer)
    pub fn price_leg(
        &self,
        strike: &Strike,
        expiration: NaiveDate,
        option_type: OptionType,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
        pricing_provider: &dyn PricingIVProvider,
    ) -> Result<LegPricing, PricingError> {
        // Filter to matching strike, expiration, and option type
        let expiration_polars = TradingDate::from_naive_date(expiration).to_polars_date();
        let strike_f64: f64 = (*strike).into();

        let opt_type_str = match option_type {
            OptionType::Call => "call",
            OptionType::Put => "put",
        };

        let filtered = chain_df
            .clone()
            .lazy()
            .filter(
                col("strike").eq(lit(strike_f64))
                    .and(col("expiration").eq(lit(expiration_polars)))
                    .and(col("option_type").eq(lit(opt_type_str)))
            )
            .collect()
            .map_err(|e| PricingError::Polars(e.to_string()))?;

        if filtered.is_empty() {
            // No market data for this option type
            // First try put-call parity if the opposite type has market data
            let ttm = self.calculate_ttm(pricing_time, expiration);

            if let Some(parity_result) = self.try_put_call_parity(
                strike_f64,
                expiration,
                option_type,
                chain_df,
                spot_price,
                ttm,
            ) {
                return Ok(parity_result);
            }

            // Fall back to IV surface interpolation
            let estimated_iv = iv_surface
                .and_then(|surface| {
                    let result = pricing_provider.get_iv(
                        surface,
                        strike.value(),
                        expiration,
                        option_type == OptionType::Call,
                    );

                    if result.is_none() {
                        // Log surface diagnostics for debugging
                        let opt_type_str = if option_type == OptionType::Call { "call" } else { "put" };
                        let matching_type: Vec<_> = surface.points()
                            .iter()
                            .filter(|p| p.is_call == (option_type == OptionType::Call))
                            .collect();

                        // Log all points in surface for diagnosis
                        let all_points: Vec<String> = surface.points()
                            .iter()
                            .map(|p| format!(
                                "{} K={} exp={} iv={:.2}",
                                if p.is_call { "C" } else { "P" },
                                p.strike,
                                p.expiration,
                                p.iv
                            ))
                            .collect();

                        warn!(
                            surface_points = surface.points().len(),
                            matching_type_points = matching_type.len(),
                            option_type = opt_type_str,
                            target_strike = strike_f64,
                            target_expiration = %expiration,
                            all_points = ?all_points,
                            "IV surface interpolation failed"
                        );

                        if !matching_type.is_empty() {
                            let strikes: Vec<f64> = matching_type.iter()
                                .map(|p| p.strike.try_into().unwrap_or(0.0))
                                .collect();
                            let exps: Vec<String> = matching_type.iter()
                                .map(|p| p.expiration.to_string())
                                .collect();
                            warn!(
                                available_strikes = ?strikes,
                                available_expirations = ?exps,
                                "Available surface data for option type"
                            );
                        }
                    }

                    result
                })
                .ok_or_else(|| PricingError::InvalidIV(format!(
                    "Cannot determine IV for {} strike {}, expiration {} - no market data, put-call parity failed, and interpolation failed",
                    if option_type == OptionType::Call { "call" } else { "put" },
                    strike_f64,
                    expiration
                )))?;

            let price = bs_price(
                spot_price,
                strike_f64,
                ttm,
                estimated_iv,
                option_type == OptionType::Call,
                self.bs_config.risk_free_rate,
            );

            let greeks = bs_greeks(
                spot_price,
                strike_f64,
                ttm,
                estimated_iv,
                option_type == OptionType::Call,
                self.bs_config.risk_free_rate,
            );

            return Ok(LegPricing {
                price: Decimal::try_from(price).unwrap_or_default(),
                iv: Some(estimated_iv),
                greeks: Some(greeks),
            });
        }

        // Use mid price from market data
        let close_col = filtered.column("close")
            .map_err(|_| PricingError::MissingColumn("close".to_string()))?
            .f64()
            .map_err(|e| PricingError::Polars(e.to_string()))?;

        let market_price = close_col.get(0)
            .ok_or_else(|| PricingError::NoPriceFound(format!("{} {} {}", strike_f64, expiration, opt_type_str)))?;

        // Calculate IV from market price
        let ttm = self.calculate_ttm(pricing_time, expiration);
        let iv = bs_implied_volatility(
            market_price,
            spot_price,
            strike_f64,
            ttm,
            option_type == OptionType::Call,
            &self.bs_config,
        );

        let greeks = if let Some(vol) = iv {
            Some(bs_greeks(
                spot_price,
                strike_f64,
                ttm,
                vol,
                option_type == OptionType::Call,
                self.bs_config.risk_free_rate,
            ))
        } else {
            None
        };

        Ok(LegPricing {
            price: Decimal::try_from(market_price).unwrap_or_default(),
            iv,
            greeks,
        })
    }

    /// Try to derive option price using put-call parity when market data is missing.
    ///
    /// Put-call parity: C - P = S - K × e^(-rT)
    /// - If we need call price but have put: C = P + S - K × e^(-rT)
    /// - If we need put price but have call: P = C - S + K × e^(-rT)
    fn try_put_call_parity(
        &self,
        strike: f64,
        expiration: NaiveDate,
        needed_type: OptionType,
        chain_df: &DataFrame,
        spot_price: f64,
        ttm: f64,
    ) -> Option<LegPricing> {
        // Look for the opposite option type at the same strike/expiration
        let opposite_type = match needed_type {
            OptionType::Call => "put",
            OptionType::Put => "call",
        };

        let expiration_polars = TradingDate::from_naive_date(expiration).to_polars_date();

        let filtered = chain_df
            .clone()
            .lazy()
            .filter(
                col("strike").eq(lit(strike))
                    .and(col("expiration").eq(lit(expiration_polars)))
                    .and(col("option_type").eq(lit(opposite_type)))
            )
            .collect()
            .ok()?;

        if filtered.is_empty() {
            return None;
        }

        // Get the opposite option's market price
        let opposite_price = filtered
            .column("close")
            .ok()?
            .f64()
            .ok()?
            .get(0)?;

        if opposite_price <= 0.0 {
            return None;
        }

        // Calculate discount factor
        let discount = (-self.bs_config.risk_free_rate * ttm).exp();
        let pv_strike = strike * discount;

        // Apply put-call parity
        let derived_price = match needed_type {
            OptionType::Call => {
                // C = P + S - K × e^(-rT)
                opposite_price + spot_price - pv_strike
            }
            OptionType::Put => {
                // P = C - S + K × e^(-rT)
                opposite_price - spot_price + pv_strike
            }
        };

        // Price must be non-negative
        if derived_price <= 0.0 {
            return None;
        }

        // Compute IV from the derived price
        let iv = bs_implied_volatility(
            derived_price,
            spot_price,
            strike,
            ttm,
            needed_type == OptionType::Call,
            &self.bs_config,
        );

        // Compute greeks if we have valid IV
        let greeks = iv.map(|vol| {
            bs_greeks(
                spot_price,
                strike,
                ttm,
                vol,
                needed_type == OptionType::Call,
                self.bs_config.risk_free_rate,
            )
        });

        Some(LegPricing {
            price: Decimal::try_from(derived_price).unwrap_or_default(),
            iv,
            greeks,
        })
    }

    fn calculate_ttm(&self, from: DateTime<Utc>, to_date: NaiveDate) -> f64 {
        let from_ts = TradingTimestamp::from_datetime_utc(from);
        let to_date_trading = TradingDate::from_naive_date(to_date);
        from_ts.time_to_expiry(&to_date_trading, &self.market_close)
    }

    /// Build an IV surface from option chain data for interpolation
    ///
    /// This method is public so callers can pre-build the surface and pass it
    /// to strategies that need delta-space analysis.
    pub fn build_iv_surface(
        &self,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        symbol: &str,
    ) -> Option<IVSurface> {
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
            let ttm = self.calculate_ttm(pricing_time, expiration);
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
                &self.bs_config,
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
                contract_ticker: format!("{}{}{}{}",
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
}

impl Default for SpreadPricer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_spread_pricer_default_pricing_model() {
        let pricer = SpreadPricer::new();
        assert_eq!(pricer.pricing_model(), PricingModel::StickyMoneyness);
    }

    #[test]
    fn test_spread_pricer_with_pricing_model() {
        let pricer = SpreadPricer::new()
            .with_pricing_model(PricingModel::StickyDelta);
        assert_eq!(pricer.pricing_model(), PricingModel::StickyDelta);

        let pricer = SpreadPricer::new()
            .with_pricing_model(PricingModel::StickyMoneyness);
        assert_eq!(pricer.pricing_model(), PricingModel::StickyMoneyness);
    }

    #[test]
    fn test_spread_pricer_builder_chain() {
        let pricer = SpreadPricer::new()
            .with_market_close(MarketTime::new(15, 45))
            .with_pricing_model(PricingModel::StickyDelta);

        assert_eq!(pricer.pricing_model(), PricingModel::StickyDelta);
    }

    #[test]
    fn test_spread_pricer_calculate_ttm() {
        let pricer = SpreadPricer::new();

        let from = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()
            .and_hms_opt(9, 30, 0).unwrap()
            .and_utc();
        let to = NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();

        let ttm = pricer.calculate_ttm(from, to);

        // Should be approximately 14 days / 365.25 ≈ 0.038
        assert!(ttm > 0.03 && ttm < 0.05);
    }

    #[test]
    fn test_spread_pricer_ttm_at_expiration() {
        let pricer = SpreadPricer::new();

        let date = NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
        let from = date.and_hms_opt(16, 0, 0).unwrap().and_utc();

        let ttm = pricer.calculate_ttm(from, date);

        assert!(ttm.abs() < 1e-6); // Should be very close to 0
    }
}
