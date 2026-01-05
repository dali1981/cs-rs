use polars::prelude::*;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use cs_analytics::{IVSurface, PricingModel};
use cs_domain::CalendarStraddle;
use crate::spread_pricer::{SpreadPricer, LegPricing, PricingError};

/// Pricer for calendar straddle positions
///
/// A calendar straddle is a 4-leg position:
/// - Short near-term call + put (straddle)
/// - Long far-term call + put (straddle)
///
/// Uses market data when available, falls back to Black-Scholes model pricing.
pub struct CalendarStraddlePricer {
    spread_pricer: SpreadPricer,
}

/// Pricing result for a calendar straddle
#[derive(Debug, Clone)]
pub struct CalendarStraddlePricing {
    pub short_call: LegPricing,
    pub short_put: LegPricing,
    pub long_call: LegPricing,
    pub long_put: LegPricing,
    /// Net cost = (long_call + long_put) - (short_call + short_put)
    pub net_cost: Decimal,
}

impl CalendarStraddlePricer {
    pub fn new(spread_pricer: SpreadPricer) -> Self {
        Self { spread_pricer }
    }

    pub fn with_pricing_model(mut self, model: PricingModel) -> Self {
        self.spread_pricer = self.spread_pricer.with_pricing_model(model);
        self
    }

    /// Price calendar straddle - uses market data with model fallback
    pub fn price(
        &self,
        straddle: &CalendarStraddle,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
    ) -> Result<CalendarStraddlePricing, PricingError> {
        // Build IV surface for fallback interpolation
        let iv_surface = self.spread_pricer.build_iv_surface(
            chain_df,
            spot,
            timestamp,
            straddle.symbol(),
        );

        self.price_with_surface(straddle, chain_df, spot, timestamp, iv_surface.as_ref())
    }

    /// Price calendar straddle using a pre-built IV surface
    ///
    /// Use this when you have a minute-aligned IV surface built with per-option spot prices.
    pub fn price_with_surface(
        &self,
        straddle: &CalendarStraddle,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<CalendarStraddlePricing, PricingError> {
        // Create pricing provider
        let pricing_provider = self.spread_pricer.pricing_model().to_provider_with_rate(self.spread_pricer.risk_free_rate());

        // Price short call
        let short_call = self.spread_pricer.price_leg(
            &straddle.short_call.strike,
            straddle.short_call.expiration,
            straddle.short_call.option_type,
            chain_df,
            spot,
            timestamp,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        // Price short put
        let short_put = self.spread_pricer.price_leg(
            &straddle.short_put.strike,
            straddle.short_put.expiration,
            straddle.short_put.option_type,
            chain_df,
            spot,
            timestamp,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        // Price long call
        let long_call = self.spread_pricer.price_leg(
            &straddle.long_call.strike,
            straddle.long_call.expiration,
            straddle.long_call.option_type,
            chain_df,
            spot,
            timestamp,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        // Price long put
        let long_put = self.spread_pricer.price_leg(
            &straddle.long_put.strike,
            straddle.long_put.expiration,
            straddle.long_put.option_type,
            chain_df,
            spot,
            timestamp,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        // Net cost = (long_call + long_put) - (short_call + short_put)
        // Positive = debit (we pay to enter)
        let net_cost = (long_call.price + long_put.price) - (short_call.price + short_put.price);

        Ok(CalendarStraddlePricing {
            short_call,
            short_put,
            long_call,
            long_put,
            net_cost,
        })
    }
}

// TradePricer trait implementation for generic execution
impl crate::execution::TradePricer for CalendarStraddlePricer {
    type Trade = CalendarStraddle;
    type Pricing = CalendarStraddlePricing;

    fn price_with_surface(
        &self,
        trade: &CalendarStraddle,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<CalendarStraddlePricing, PricingError> {
        self.price_with_surface(trade, chain_df, spot, timestamp, iv_surface)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calendar_straddle_pricer_creation() {
        let spread_pricer = SpreadPricer::new();
        let _pricer = CalendarStraddlePricer::new(spread_pricer);
    }
}
