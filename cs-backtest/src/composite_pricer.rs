//! Generic pricer for any CompositeTrade

use chrono::{DateTime, Utc};
use polars::prelude::DataFrame;
use rust_decimal::Decimal;
use cs_analytics::IVSurface;
use cs_domain::trade::{CompositeTrade, LegPosition};

use crate::spread_pricer::{SpreadPricer, PricingError, LegPricing};

/// Pricing result for a composite trade
#[derive(Debug, Clone)]
pub struct CompositePricing {
    /// Individual leg pricings with their positions
    pub legs: Vec<(LegPricing, LegPosition)>,
    /// Net cost (positive = debit, negative = credit)
    pub net_cost: Decimal,
    /// Net Greeks
    pub net_delta: f64,
    pub net_gamma: f64,
    pub net_theta: f64,
    pub net_vega: f64,
    /// Average IV (simple average across legs)
    pub avg_iv: f64,
}

impl CompositePricing {
    /// Construct CompositePricing from leg pricings and positions
    pub fn from_legs(legs: Vec<(LegPricing, LegPosition)>) -> Self {
        let mut net_cost = Decimal::ZERO;
        let mut net_delta = 0.0;
        let mut net_gamma = 0.0;
        let mut net_theta = 0.0;
        let mut net_vega = 0.0;
        let mut iv_sum = 0.0;
        let mut iv_count = 0;

        for (pricing, position) in &legs {
            let sign = position.sign_decimal();
            let sign_f64 = position.sign();

            // Long = pay (positive), Short = receive (negative)
            net_cost += pricing.price * sign;

            if let Some(greeks) = &pricing.greeks {
                net_delta += greeks.delta * sign_f64;
                net_gamma += greeks.gamma * sign_f64;
                net_theta += greeks.theta * sign_f64;
                net_vega += greeks.vega * sign_f64;
            }

            if let Some(iv) = pricing.iv {
                iv_sum += iv;
                iv_count += 1;
            }
        }

        Self {
            legs,
            net_cost,
            net_delta,
            net_gamma,
            net_theta,
            net_vega,
            avg_iv: if iv_count > 0 { iv_sum / iv_count as f64 } else { 0.0 },
        }
    }

    /// Get pricing for a specific leg index
    pub fn leg(&self, index: usize) -> Option<&LegPricing> {
        self.legs.get(index).map(|(p, _)| p)
    }

    /// Average IV across all legs (recomputed from legs for consistency)
    pub fn avg_iv_from_legs(&self) -> Option<f64> {
        let ivs: Vec<f64> = self.legs.iter()
            .filter_map(|(p, _)| p.iv)
            .collect();

        if ivs.is_empty() {
            None
        } else {
            Some(ivs.iter().sum::<f64>() / ivs.len() as f64)
        }
    }

    /// IV grouped by expiration (for calendars)
    pub fn iv_by_expiration(&self) -> std::collections::BTreeMap<chrono::NaiveDate, f64> {
        use std::collections::BTreeMap;

        let mut by_expiry: BTreeMap<chrono::NaiveDate, Vec<f64>> = BTreeMap::new();

        for (pricing, _) in &self.legs {
            if let Some(iv) = pricing.iv {
                by_expiry.entry(pricing.expiration)
                    .or_default()
                    .push(iv);
            }
        }

        by_expiry.into_iter()
            .map(|(exp, ivs)| (exp, ivs.iter().sum::<f64>() / ivs.len() as f64))
            .collect()
    }

    /// Detect if this is a calendar structure (multiple expirations)
    pub fn is_calendar(&self) -> bool {
        use std::collections::HashSet;
        let expirations: HashSet<chrono::NaiveDate> = self.legs.iter()
            .map(|(p, _)| p.expiration)
            .collect();
        expirations.len() > 1
    }

    /// For calendars: short IV / long IV ratio
    pub fn iv_ratio(&self) -> Option<f64> {
        if !self.is_calendar() {
            return None;
        }

        let by_exp = self.iv_by_expiration();
        let expirations: Vec<_> = by_exp.keys().collect();

        if expirations.len() != 2 {
            return None;  // Not a simple calendar
        }

        let short_exp = expirations[0];  // Earlier = short
        let long_exp = expirations[1];   // Later = long

        let short_iv = by_exp.get(short_exp)?;
        let long_iv = by_exp.get(long_exp)?;

        Some(short_iv / long_iv)
    }

    /// Primary IV metric (for display)
    /// - Non-calendar: average IV
    /// - Calendar: short leg IV (earnings-affected)
    pub fn primary_iv(&self) -> Option<f64> {
        if self.is_calendar() {
            let by_exp = self.iv_by_expiration();
            by_exp.values().next().copied()  // Earliest expiration
        } else {
            self.avg_iv_from_legs()
        }
    }
}

/// Generic pricer for any composite trade
pub struct CompositePricer {
    inner: SpreadPricer,
}

impl CompositePricer {
    /// Create a new CompositePricer wrapping a SpreadPricer
    pub fn new(inner: SpreadPricer) -> Self {
        Self { inner }
    }

    /// Price any composite trade
    pub fn price<T: CompositeTrade>(
        &self,
        trade: &T,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<CompositePricing, PricingError> {
        let mut leg_pricings = Vec::with_capacity(trade.leg_count());

        // Create pricing provider based on configured pricing model
        let pricing_provider = self.inner.pricing_model().to_provider_with_rate(
            self.inner.risk_free_rate()
        );

        for (leg, position) in trade.legs() {
            let pricing = self.inner.price_leg(
                &leg.strike,
                leg.expiration,
                leg.option_type,
                chain_df,
                spot,
                timestamp,
                iv_surface,
                pricing_provider.as_ref(),
            )?;

            leg_pricings.push((pricing, position));
        }

        Ok(CompositePricing::from_legs(leg_pricings))
    }

    /// Expose the inner SpreadPricer for advanced use cases
    pub fn inner(&self) -> &SpreadPricer {
        &self.inner
    }
}

impl Default for CompositePricer {
    fn default() -> Self {
        Self::new(SpreadPricer::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_domain::trade::LegPosition;

    #[test]
    fn test_composite_pricing_from_legs() {
        use cs_analytics::Greeks;
        use chrono::NaiveDate;

        let exp_date = NaiveDate::from_ymd_opt(2025, 3, 21).unwrap();

        // Create mock leg pricings
        let leg1 = LegPricing {
            price: Decimal::from(100),
            iv: Some(0.25),
            greeks: Some(Greeks {
                delta: 0.5,
                gamma: 0.01,
                theta: -0.05,
                vega: 0.2,
                rho: 0.1,
            }),
            expiration: exp_date,
        };

        let leg2 = LegPricing {
            price: Decimal::from(50),
            iv: Some(0.30),
            greeks: Some(Greeks {
                delta: 0.3,
                gamma: 0.02,
                theta: -0.03,
                vega: 0.15,
                rho: 0.05,
            }),
            expiration: exp_date,
        };

        // Long leg1, Short leg2
        let legs = vec![
            (leg1, LegPosition::Long),
            (leg2, LegPosition::Short),
        ];

        let pricing = CompositePricing::from_legs(legs);

        // Verify net cost: 100 * 1 + 50 * (-1) = 50
        assert_eq!(pricing.net_cost, Decimal::from(50));

        // Verify net delta: 0.5 * 1 + 0.3 * (-1) = 0.2
        assert_eq!(pricing.net_delta, 0.2);

        // Verify net gamma: 0.01 * 1 + 0.02 * (-1) = -0.01
        assert_eq!(pricing.net_gamma, -0.01);

        // Verify average IV: (0.25 + 0.30) / 2 = 0.275
        assert_eq!(pricing.avg_iv, 0.275);
    }
}
