use polars::prelude::*;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use cs_analytics::PricingModel;
use cs_domain::{Straddle, PricingSource};
use crate::spread_pricer::{SpreadPricer, LegPricing, PricingError};

/// Pricer for straddle positions
///
/// Uses market data when available, falls back to Black-Scholes model pricing.
pub struct StraddlePricer {
    spread_pricer: SpreadPricer,
}

pub struct StraddlePricing {
    pub call: LegPricing,
    pub put: LegPricing,
    pub total_price: Decimal,
    pub source: PricingSource,
}

impl StraddlePricer {
    pub fn new(spread_pricer: SpreadPricer) -> Self {
        Self { spread_pricer }
    }

    pub fn with_pricing_model(mut self, model: PricingModel) -> Self {
        self.spread_pricer = self.spread_pricer.with_pricing_model(model);
        self
    }

    /// Price straddle - uses market data with model fallback
    pub fn price(
        &self,
        straddle: &Straddle,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
    ) -> Result<StraddlePricing, PricingError> {
        // Build IV surface for fallback interpolation
        let iv_surface = self.spread_pricer.build_iv_surface(
            chain_df,
            spot,
            timestamp,
            straddle.symbol(),
        );

        // Create pricing provider
        let pricing_provider = self.spread_pricer.pricing_model().to_provider_with_rate(0.0);

        // Price call leg
        let call_pricing = self.spread_pricer.price_leg(
            &straddle.call_leg.strike,
            straddle.call_leg.expiration,
            straddle.call_leg.option_type,
            chain_df,
            spot,
            timestamp,
            iv_surface.as_ref(),
            pricing_provider.as_ref(),
        )?;

        // Price put leg
        let put_pricing = self.spread_pricer.price_leg(
            &straddle.put_leg.strike,
            straddle.put_leg.expiration,
            straddle.put_leg.option_type,
            chain_df,
            spot,
            timestamp,
            iv_surface.as_ref(),
            pricing_provider.as_ref(),
        )?;

        let total_price = call_pricing.price + put_pricing.price;

        // Both legs use the same pricing approach, so we can use Market as the source
        // (SpreadPricer uses market data when available)
        Ok(StraddlePricing {
            call: call_pricing,
            put: put_pricing,
            total_price,
            source: PricingSource::Market,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_straddle_pricer_creation() {
        let spread_pricer = SpreadPricer::new();
        let _pricer = StraddlePricer::new(spread_pricer);
    }
}
