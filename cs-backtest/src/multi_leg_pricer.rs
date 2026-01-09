//! Pricers for multi-leg volatility strategies
//!
//! Provides pricing for Strangle, Butterfly, Condor, and IronCondor strategies.
//! All reuse SpreadPricer infrastructure for consistency.

use chrono::DateTime;
use chrono::Utc;
use polars::prelude::*;
use rust_decimal::Decimal;

use cs_analytics::IVSurface;
use cs_domain::{Strangle, Butterfly, Condor, IronCondor, TradingContext, LegContext, TradeType};
use crate::spread_pricer::{SpreadPricer, LegPricing, PricingError};
use crate::execution::TradePricer;

// ============================================================================
// Strangle Pricer (2 legs: OTM call + OTM put)
// ============================================================================

/// Pricing result for a strangle
#[derive(Debug, Clone)]
pub struct StranglePricing {
    pub call: LegPricing,
    pub put: LegPricing,
    pub entry_debit: Decimal,
}

impl StranglePricing {
    /// Create a TradingContext for cost calculations
    pub fn to_trading_context(
        &self,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> TradingContext {
        // Strangle: long call + long put
        let legs = vec![
            LegContext::long(self.call.price, self.call.iv),
            LegContext::long(self.put.price, self.put.iv),
        ];
        TradingContext::new(legs, symbol.to_string(), spot, time, trade_type)
    }
}

/// Pricer for strangles
pub struct StranglePricer {
    inner: SpreadPricer,
}

impl StranglePricer {
    pub fn new(spread_pricer: SpreadPricer) -> Self {
        Self { inner: spread_pricer }
    }

    pub fn price(
        &self,
        strangle: &Strangle,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
    ) -> Result<StranglePricing, PricingError> {
        let iv_surface = self.inner.build_iv_surface(
            chain_df,
            spot_price,
            pricing_time,
            strangle.symbol(),
        );

        self.price_with_surface(strangle, chain_df, spot_price, pricing_time, iv_surface.as_ref())
    }

    pub fn price_with_surface(
        &self,
        strangle: &Strangle,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<StranglePricing, PricingError> {
        let pricing_provider = self.inner.pricing_model().to_provider_with_rate(self.inner.risk_free_rate());

        let call = self.inner.price_leg(
            &strangle.call_leg.strike,
            strangle.call_leg.expiration,
            strangle.call_leg.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let put = self.inner.price_leg(
            &strangle.put_leg.strike,
            strangle.put_leg.expiration,
            strangle.put_leg.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let entry_debit = call.price + put.price;

        Ok(StranglePricing { call, put, entry_debit })
    }
}

// ============================================================================
// Butterfly Pricer (4 legs: 2x short ATM ± 2x long OTM)
// ============================================================================

/// Pricing result for a butterfly
#[derive(Debug, Clone)]
pub struct ButterflyPricing {
    pub short_call: LegPricing,
    pub short_put: LegPricing,
    pub long_upper_call: LegPricing,
    pub long_lower_put: LegPricing,
    pub entry_debit: Decimal,
}

impl ButterflyPricing {
    /// Create a TradingContext for cost calculations
    pub fn to_trading_context(
        &self,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> TradingContext {
        // Butterfly: short straddle + long wings
        let legs = vec![
            LegContext::short(self.short_call.price, self.short_call.iv),
            LegContext::short(self.short_put.price, self.short_put.iv),
            LegContext::long(self.long_upper_call.price, self.long_upper_call.iv),
            LegContext::long(self.long_lower_put.price, self.long_lower_put.iv),
        ];
        TradingContext::new(legs, symbol.to_string(), spot, time, trade_type)
    }
}

/// Pricer for butterflies
pub struct ButterflyPricer {
    inner: SpreadPricer,
}

impl ButterflyPricer {
    pub fn new(spread_pricer: SpreadPricer) -> Self {
        Self { inner: spread_pricer }
    }

    pub fn price(
        &self,
        butterfly: &Butterfly,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
    ) -> Result<ButterflyPricing, PricingError> {
        let iv_surface = self.inner.build_iv_surface(
            chain_df,
            spot_price,
            pricing_time,
            butterfly.symbol(),
        );

        self.price_with_surface(butterfly, chain_df, spot_price, pricing_time, iv_surface.as_ref())
    }

    pub fn price_with_surface(
        &self,
        butterfly: &Butterfly,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<ButterflyPricing, PricingError> {
        let pricing_provider = self.inner.pricing_model().to_provider_with_rate(self.inner.risk_free_rate());

        let short_call = self.inner.price_leg(
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
            &butterfly.short_put.strike,
            butterfly.short_put.expiration,
            butterfly.short_put.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let long_upper_call = self.inner.price_leg(
            &butterfly.long_upper_call.strike,
            butterfly.long_upper_call.expiration,
            butterfly.long_upper_call.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let long_lower_put = self.inner.price_leg(
            &butterfly.long_lower_put.strike,
            butterfly.long_lower_put.expiration,
            butterfly.long_lower_put.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        // Butterfly is a debit spread: pay for long wings, receive for short ATM
        let entry_debit = (long_upper_call.price + long_lower_put.price)
            - (short_call.price + short_put.price);

        Ok(ButterflyPricing {
            short_call,
            short_put,
            long_upper_call,
            long_lower_put,
            entry_debit,
        })
    }
}

// ============================================================================
// Condor Pricer (4 legs: 2x short near ± 2x long far)
// ============================================================================

/// Pricing result for a condor
#[derive(Debug, Clone)]
pub struct CondorPricing {
    pub near_call: LegPricing,
    pub near_put: LegPricing,
    pub far_upper_call: LegPricing,
    pub far_lower_put: LegPricing,
    pub entry_debit: Decimal,
}

impl CondorPricing {
    /// Create a TradingContext for cost calculations
    pub fn to_trading_context(
        &self,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> TradingContext {
        // Condor: short near + long far
        let legs = vec![
            LegContext::short(self.near_call.price, self.near_call.iv),
            LegContext::short(self.near_put.price, self.near_put.iv),
            LegContext::long(self.far_upper_call.price, self.far_upper_call.iv),
            LegContext::long(self.far_lower_put.price, self.far_lower_put.iv),
        ];
        TradingContext::new(legs, symbol.to_string(), spot, time, trade_type)
    }
}

/// Pricer for condors
pub struct CondorPricer {
    inner: SpreadPricer,
}

impl CondorPricer {
    pub fn new(spread_pricer: SpreadPricer) -> Self {
        Self { inner: spread_pricer }
    }

    pub fn price(
        &self,
        condor: &Condor,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
    ) -> Result<CondorPricing, PricingError> {
        let iv_surface = self.inner.build_iv_surface(
            chain_df,
            spot_price,
            pricing_time,
            condor.symbol(),
        );

        self.price_with_surface(condor, chain_df, spot_price, pricing_time, iv_surface.as_ref())
    }

    pub fn price_with_surface(
        &self,
        condor: &Condor,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<CondorPricing, PricingError> {
        let pricing_provider = self.inner.pricing_model().to_provider_with_rate(self.inner.risk_free_rate());

        let near_call = self.inner.price_leg(
            &condor.near_call.strike,
            condor.near_call.expiration,
            condor.near_call.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let near_put = self.inner.price_leg(
            &condor.near_put.strike,
            condor.near_put.expiration,
            condor.near_put.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let far_upper_call = self.inner.price_leg(
            &condor.far_upper_call.strike,
            condor.far_upper_call.expiration,
            condor.far_upper_call.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let far_lower_put = self.inner.price_leg(
            &condor.far_lower_put.strike,
            condor.far_lower_put.expiration,
            condor.far_lower_put.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        // Condor is a debit spread
        let entry_debit = (far_upper_call.price + far_lower_put.price)
            - (near_call.price + near_put.price);

        Ok(CondorPricing {
            near_call,
            near_put,
            far_upper_call,
            far_lower_put,
            entry_debit,
        })
    }
}

// ============================================================================
// Iron Condor Pricer (4 legs: 2x short near ± 2x long far, credit spread)
// ============================================================================

/// Pricing result for an iron condor
#[derive(Debug, Clone)]
pub struct IronCondorPricing {
    pub near_call: LegPricing,
    pub near_put: LegPricing,
    pub far_upper_call: LegPricing,
    pub far_lower_put: LegPricing,
    pub net_credit: Decimal,
}

impl IronCondorPricing {
    /// Create a TradingContext for cost calculations
    pub fn to_trading_context(
        &self,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> TradingContext {
        // Iron Condor: short near + long far (credit spread)
        let legs = vec![
            LegContext::short(self.near_call.price, self.near_call.iv),
            LegContext::short(self.near_put.price, self.near_put.iv),
            LegContext::long(self.far_upper_call.price, self.far_upper_call.iv),
            LegContext::long(self.far_lower_put.price, self.far_lower_put.iv),
        ];
        TradingContext::new(legs, symbol.to_string(), spot, time, trade_type)
    }
}

/// Pricer for iron condors
pub struct IronCondorPricer {
    inner: SpreadPricer,
}

impl IronCondorPricer {
    pub fn new(spread_pricer: SpreadPricer) -> Self {
        Self { inner: spread_pricer }
    }

    pub fn price(
        &self,
        condor: &IronCondor,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
    ) -> Result<IronCondorPricing, PricingError> {
        let iv_surface = self.inner.build_iv_surface(
            chain_df,
            spot_price,
            pricing_time,
            condor.symbol(),
        );

        self.price_with_surface(condor, chain_df, spot_price, pricing_time, iv_surface.as_ref())
    }

    pub fn price_with_surface(
        &self,
        condor: &IronCondor,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<IronCondorPricing, PricingError> {
        let pricing_provider = self.inner.pricing_model().to_provider_with_rate(self.inner.risk_free_rate());

        let near_call = self.inner.price_leg(
            &condor.near_call.strike,
            condor.near_call.expiration,
            condor.near_call.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let near_put = self.inner.price_leg(
            &condor.near_put.strike,
            condor.near_put.expiration,
            condor.near_put.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let far_upper_call = self.inner.price_leg(
            &condor.far_upper_call.strike,
            condor.far_upper_call.expiration,
            condor.far_upper_call.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        let far_lower_put = self.inner.price_leg(
            &condor.far_lower_put.strike,
            condor.far_lower_put.expiration,
            condor.far_lower_put.option_type,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface,
            pricing_provider.as_ref(),
        )?;

        // Iron condor is a credit spread: short near (receive) - long far (pay)
        let net_credit = (near_call.price + near_put.price)
            - (far_upper_call.price + far_lower_put.price);

        Ok(IronCondorPricing {
            near_call,
            near_put,
            far_upper_call,
            far_lower_put,
            net_credit,
        })
    }
}

// ============================================================================
// TradePricer implementations
// ============================================================================

impl TradePricer for StranglePricer {
    type Trade = Strangle;
    type Pricing = StranglePricing;

    fn price_with_surface(
        &self,
        trade: &Strangle,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<StranglePricing, PricingError> {
        Self::price_with_surface(self, trade, chain_df, spot, timestamp, iv_surface)
    }
}

impl TradePricer for ButterflyPricer {
    type Trade = Butterfly;
    type Pricing = ButterflyPricing;

    fn price_with_surface(
        &self,
        trade: &Butterfly,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<ButterflyPricing, PricingError> {
        Self::price_with_surface(self, trade, chain_df, spot, timestamp, iv_surface)
    }
}

impl TradePricer for CondorPricer {
    type Trade = Condor;
    type Pricing = CondorPricing;

    fn price_with_surface(
        &self,
        trade: &Condor,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<CondorPricing, PricingError> {
        Self::price_with_surface(self, trade, chain_df, spot, timestamp, iv_surface)
    }
}

impl TradePricer for IronCondorPricer {
    type Trade = IronCondor;
    type Pricing = IronCondorPricing;

    fn price_with_surface(
        &self,
        trade: &IronCondor,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<IronCondorPricing, PricingError> {
        Self::price_with_surface(self, trade, chain_df, spot, timestamp, iv_surface)
    }
}
