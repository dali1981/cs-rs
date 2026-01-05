//! ExecutableTrade implementation for CalendarSpread

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use cs_domain::{
    CalendarSpread, CalendarSpreadResult, FailureReason, CONTRACT_MULTIPLIER,
};
use crate::spread_pricer::{SpreadPricer, SpreadPricing};
use super::types::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, ExecutionContext};

impl ExecutableTrade for CalendarSpread {
    type Pricer = SpreadPricer;
    type Pricing = SpreadPricing;
    type Result = CalendarSpreadResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn validate_entry(
        pricing: &SpreadPricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // Validate max IV at entry
        if let Some(max_iv) = config.max_entry_iv {
            if let Some(short_iv) = pricing.short_leg.iv {
                if short_iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Short leg IV too high: {:.1}% > {:.1}% (unreliable pricing)",
                        short_iv * 100.0,
                        max_iv * 100.0,
                    )));
                }
            }
            if let Some(long_iv) = pricing.long_leg.iv {
                if long_iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Long leg IV too high: {:.1}% > {:.1}% (unreliable pricing)",
                        long_iv * 100.0,
                        max_iv * 100.0,
                    )));
                }
            }
        }

        // Validate: Calendar spread must have positive entry cost (long > short)
        if pricing.net_cost <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(format!(
                "Negative entry cost: {} (short={}, long={})",
                pricing.net_cost, pricing.short_leg.price, pricing.long_leg.price,
            )));
        }

        // Validate: Entry cost must be reasonable
        if pricing.net_cost < config.min_entry_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Entry cost too small: {} < {} (short={}, long={})",
                pricing.net_cost,
                config.min_entry_cost,
                pricing.short_leg.price,
                pricing.long_leg.price,
            )));
        }

        Ok(())
    }

    fn to_result(
        &self,
        entry_pricing: SpreadPricing,
        exit_pricing: SpreadPricing,
        ctx: &ExecutionContext,
    ) -> CalendarSpreadResult {
        // Calculate P&L (per-share first, then multiply by contract multiplier)
        let pnl_per_share = exit_pricing.net_cost - entry_pricing.net_cost;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.net_cost != Decimal::ZERO {
            (pnl_per_share / entry_pricing.net_cost) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // P&L attribution using leg-by-leg approach
        let (delta_pnl, gamma_pnl, theta_pnl, vega_pnl, unexplained_pnl) = {
            let spot_change = ctx.exit_spot - ctx.entry_spot;
            let days_held = (ctx.exit_time - ctx.entry_time).num_hours() as f64 / 24.0;

            // Calculate P&L for short leg (negative sign because we're short)
            let short_pnl = cs_domain::calculate_option_leg_pnl(
                entry_pricing.short_leg.greeks.as_ref(),
                entry_pricing.short_leg.iv,
                exit_pricing.short_leg.iv,
                spot_change,
                days_held,
                -1.0, // Short position
            );

            // Calculate P&L for long leg (positive sign because we're long)
            let long_pnl = cs_domain::calculate_option_leg_pnl(
                entry_pricing.long_leg.greeks.as_ref(),
                entry_pricing.long_leg.iv,
                exit_pricing.long_leg.iv,
                spot_change,
                days_held,
                1.0, // Long position
            );

            // Sum the legs and scale to position level
            let multiplier = Decimal::from(CONTRACT_MULTIPLIER).to_f64().unwrap();
            let delta = (short_pnl.delta + long_pnl.delta) * multiplier;
            let gamma = (short_pnl.gamma + long_pnl.gamma) * multiplier;
            let theta = (short_pnl.theta + long_pnl.theta) * multiplier;
            let vega = (short_pnl.vega + long_pnl.vega) * multiplier;

            let explained = delta + gamma + theta + vega;
            let unexplained = pnl.to_f64().unwrap_or(0.0) - explained;

            (
                Some(Decimal::try_from(delta).unwrap_or_default()),
                Some(Decimal::try_from(gamma).unwrap_or_default()),
                Some(Decimal::try_from(theta).unwrap_or_default()),
                Some(Decimal::try_from(vega).unwrap_or_default()),
                Some(Decimal::try_from(unexplained).unwrap_or_default()),
            )
        };

        CalendarSpreadResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            strike: self.strike(),
            long_strike: if self.short_leg.strike != self.long_leg.strike {
                Some(self.long_leg.strike)
            } else {
                None
            },
            option_type: self.option_type(),
            short_expiry: self.short_expiry(),
            long_expiry: self.long_expiry(),
            entry_time: ctx.entry_time,
            short_entry_price: entry_pricing.short_leg.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_entry_price: entry_pricing.long_leg.price * Decimal::from(CONTRACT_MULTIPLIER),
            entry_cost: entry_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            exit_time: ctx.exit_time,
            short_exit_price: exit_pricing.short_leg.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_exit_price: exit_pricing.long_leg.price * Decimal::from(CONTRACT_MULTIPLIER),
            exit_value: exit_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            entry_surface_time: ctx.entry_surface_time,
            exit_surface_time: Some(ctx.exit_surface_time),
            pnl,
            pnl_per_contract: pnl,
            pnl_pct,
            short_delta: entry_pricing.short_leg.greeks.map(|g| g.delta),
            short_gamma: entry_pricing.short_leg.greeks.map(|g| g.gamma),
            short_theta: entry_pricing.short_leg.greeks.map(|g| g.theta),
            short_vega: entry_pricing.short_leg.greeks.map(|g| g.vega),
            long_delta: entry_pricing.long_leg.greeks.map(|g| g.delta),
            long_gamma: entry_pricing.long_leg.greeks.map(|g| g.gamma),
            long_theta: entry_pricing.long_leg.greeks.map(|g| g.theta),
            long_vega: entry_pricing.long_leg.greeks.map(|g| g.vega),
            iv_short_entry: entry_pricing.short_leg.iv,
            iv_long_entry: entry_pricing.long_leg.iv,
            iv_short_exit: exit_pricing.short_leg.iv,
            iv_long_exit: exit_pricing.long_leg.iv,
            iv_ratio_entry: match (entry_pricing.short_leg.iv, entry_pricing.long_leg.iv) {
                (Some(short_iv), Some(long_iv)) if long_iv > 0.0 => Some(short_iv / long_iv),
                _ => None,
            },
            delta_pnl,
            gamma_pnl,
            theta_pnl,
            vega_pnl,
            unexplained_pnl,
            spot_at_entry: ctx.entry_spot,
            spot_at_exit: ctx.exit_spot,
            success: true,
            failure_reason: None,
        }
    }

    fn to_failed_result(
        &self,
        ctx: &ExecutionContext,
        error: ExecutionError,
    ) -> CalendarSpreadResult {
        let failure_reason = match error {
            ExecutionError::NoSpotPrice => FailureReason::NoSpotPrice,
            ExecutionError::Repository(_) => FailureReason::NoOptionsData,
            ExecutionError::Pricing(_) => FailureReason::PricingError(error.to_string()),
            ExecutionError::InvalidSpread(_) => FailureReason::DegenerateSpread,
        };

        CalendarSpreadResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            strike: self.strike(),
            long_strike: if self.short_leg.strike != self.long_leg.strike {
                Some(self.long_leg.strike)
            } else {
                None
            },
            option_type: self.option_type(),
            short_expiry: self.short_expiry(),
            long_expiry: self.long_expiry(),
            entry_time: ctx.entry_time,
            short_entry_price: Decimal::ZERO,
            long_entry_price: Decimal::ZERO,
            entry_cost: Decimal::ZERO,
            exit_time: ctx.exit_time,
            short_exit_price: Decimal::ZERO,
            long_exit_price: Decimal::ZERO,
            exit_value: Decimal::ZERO,
            entry_surface_time: None,
            exit_surface_time: None,
            pnl: Decimal::ZERO,
            pnl_per_contract: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
            short_delta: None,
            short_gamma: None,
            short_theta: None,
            short_vega: None,
            long_delta: None,
            long_gamma: None,
            long_theta: None,
            long_vega: None,
            iv_short_entry: None,
            iv_long_entry: None,
            iv_short_exit: None,
            iv_long_exit: None,
            iv_ratio_entry: None,
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 0.0,
            spot_at_exit: 0.0,
            success: false,
            failure_reason: Some(failure_reason),
        }
    }
}
