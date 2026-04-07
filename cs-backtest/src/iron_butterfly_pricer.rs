use chrono::{DateTime, Utc};
use polars::prelude::*;
use rust_decimal::Decimal;

use cs_analytics::IVSurface;
use cs_domain::IronButterfly;
use crate::spread_pricer::{SpreadPricer, LegPricing, PricingError};

/// Pricing result for an iron butterfly
#[derive(Debug, Clone)]
pub struct IronButterflyPricing {
    pub short_call: LegPricing,
    pub short_put: LegPricing,
    pub long_call: LegPricing,
    pub long_put: LegPricing,
    pub net_credit: Decimal,
}

/// Pricer for iron butterflies (reuses SpreadPricer infrastructure)
pub struct IronButterflyPricer {
    inner: SpreadPricer,
}

impl IronButterflyPricer {
    pub fn new(spread_pricer: SpreadPricer) -> Self {
        Self {
            inner: spread_pricer,
        }
    }

    pub fn price(
        &self,
        butterfly: &IronButterfly,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
    ) -> Result<IronButterflyPricing, PricingError> {
        // Build IV surface for fallback interpolation
        let iv_surface = self.inner.build_iv_surface(
            chain_df,
            spot_price,
            pricing_time,
            butterfly.symbol(),
        );

        self.price_with_surface(butterfly, chain_df, spot_price, pricing_time, iv_surface.as_ref())
    }

    /// Price iron butterfly using a pre-built IV surface
    ///
    /// Use this when you have a minute-aligned IV surface built with per-option spot prices.
    pub fn price_with_surface(
        &self,
        butterfly: &IronButterfly,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<IronButterflyPricing, PricingError> {
        // Create pricing provider
        let pricing_provider = self.inner.pricing_model().to_provider_with_rate(self.inner.risk_free_rate());

        // Price all 4 legs
        let short_call = self.inner.price_leg(
            butterfly.symbol(),
            &butterfly.short_call.strike,
            butterfly.short_call.expiration,
            butterfly.short_call.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let short_put = self.inner.price_leg(
            butterfly.symbol(),
            &butterfly.short_put.strike,
            butterfly.short_put.expiration,
            butterfly.short_put.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let long_call = self.inner.price_leg(
            butterfly.symbol(),
            &butterfly.long_call.strike,
            butterfly.long_call.expiration,
            butterfly.long_call.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let long_put = self.inner.price_leg(
            butterfly.symbol(),
            &butterfly.long_put.strike,
            butterfly.long_put.expiration,
            butterfly.long_put.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        // Net credit = (short call + short put) - (long call + long put)
        let net_credit = (short_call.price + short_put.price)
            - (long_call.price + long_put.price);

        Ok(IronButterflyPricing {
            short_call,
            short_put,
            long_call,
            long_put,
            net_credit,
        })
    }
}

// TradePricer trait implementation for generic execution
impl crate::execution::TradePricer for IronButterflyPricer {
    type Trade = IronButterfly;
    type Pricing = IronButterflyPricing;

    fn price_with_surface(
        &self,
        trade: &IronButterfly,
        chain: &[cs_domain::OptionBar],
        spot: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
        iv_surface: Option<&cs_analytics::IVSurface>,
    ) -> Result<IronButterflyPricing, PricingError> {
        let chain_df = crate::option_bar_adapter::to_dataframe(chain);
        self.price_with_surface(trade, &chain_df, spot, timestamp, iv_surface)
    }
}
