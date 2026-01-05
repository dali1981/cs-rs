//! ExecutableTrade implementation for CalendarStraddle

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use cs_domain::{
    CalendarStraddle, CalendarStraddleResult, FailureReason, CONTRACT_MULTIPLIER,
};
use crate::calendar_straddle_pricer::{CalendarStraddlePricer, CalendarStraddlePricing};
use super::types::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, ExecutionContext};

impl ExecutableTrade for CalendarStraddle {
    type Pricer = CalendarStraddlePricer;
    type Pricing = CalendarStraddlePricing;
    type Result = CalendarStraddleResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn validate_entry(
        pricing: &CalendarStraddlePricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // Validate max IV at entry
        if let Some(max_iv) = config.max_entry_iv {
            // Check short call IV
            if let Some(iv) = pricing.short_call.iv {
                if iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Short call IV too high: {:.1}% > {:.1}% (unreliable pricing)",
                        iv * 100.0,
                        max_iv * 100.0,
                    )));
                }
            }
            // Check short put IV
            if let Some(iv) = pricing.short_put.iv {
                if iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Short put IV too high: {:.1}% > {:.1}% (unreliable pricing)",
                        iv * 100.0,
                        max_iv * 100.0,
                    )));
                }
            }
            // Check long call IV
            if let Some(iv) = pricing.long_call.iv {
                if iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Long call IV too high: {:.1}% > {:.1}% (unreliable pricing)",
                        iv * 100.0,
                        max_iv * 100.0,
                    )));
                }
            }
            // Check long put IV
            if let Some(iv) = pricing.long_put.iv {
                if iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Long put IV too high: {:.1}% > {:.1}% (unreliable pricing)",
                        iv * 100.0,
                        max_iv * 100.0,
                    )));
                }
            }
        }

        // Validate: Calendar straddle must have positive entry cost (pay debit)
        if pricing.net_cost <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(format!(
                "Negative entry cost: {} (should pay debit)",
                pricing.net_cost,
            )));
        }

        // Validate: Entry cost must be reasonable
        if pricing.net_cost < config.min_entry_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Entry cost too small: {} < {}",
                pricing.net_cost,
                config.min_entry_cost,
            )));
        }

        Ok(())
    }

    fn to_result(
        &self,
        entry_pricing: CalendarStraddlePricing,
        exit_pricing: CalendarStraddlePricing,
        ctx: &ExecutionContext,
    ) -> CalendarStraddleResult {
        // Calculate P&L (per-share first, then multiply by contract multiplier)
        let pnl_per_share = exit_pricing.net_cost - entry_pricing.net_cost;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.net_cost != Decimal::ZERO {
            (pnl_per_share / entry_pricing.net_cost) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Calculate net greeks (long - short for each greek)
        let net_delta = match (
            entry_pricing.long_call.greeks,
            entry_pricing.long_put.greeks,
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
        ) {
            (Some(lc), Some(lp), Some(sc), Some(sp)) => {
                Some((lc.delta + lp.delta - sc.delta - sp.delta) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_gamma = match (
            entry_pricing.long_call.greeks,
            entry_pricing.long_put.greeks,
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
        ) {
            (Some(lc), Some(lp), Some(sc), Some(sp)) => {
                Some((lc.gamma + lp.gamma - sc.gamma - sp.gamma) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_theta = match (
            entry_pricing.long_call.greeks,
            entry_pricing.long_put.greeks,
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
        ) {
            (Some(lc), Some(lp), Some(sc), Some(sp)) => {
                Some((lc.theta + lp.theta - sc.theta - sp.theta) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_vega = match (
            entry_pricing.long_call.greeks,
            entry_pricing.long_put.greeks,
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
        ) {
            (Some(lc), Some(lp), Some(sc), Some(sp)) => {
                Some((lc.vega + lp.vega - sc.vega - sp.vega) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        // Average IV for short and long straddles
        let short_iv_entry = match (entry_pricing.short_call.iv, entry_pricing.short_put.iv) {
            (Some(c), Some(p)) => Some((c + p) / 2.0),
            (Some(c), None) => Some(c),
            (None, Some(p)) => Some(p),
            _ => None,
        };

        let long_iv_entry = match (entry_pricing.long_call.iv, entry_pricing.long_put.iv) {
            (Some(c), Some(p)) => Some((c + p) / 2.0),
            (Some(c), None) => Some(c),
            (None, Some(p)) => Some(p),
            _ => None,
        };

        let short_iv_exit = match (exit_pricing.short_call.iv, exit_pricing.short_put.iv) {
            (Some(c), Some(p)) => Some((c + p) / 2.0),
            (Some(c), None) => Some(c),
            (None, Some(p)) => Some(p),
            _ => None,
        };

        let long_iv_exit = match (exit_pricing.long_call.iv, exit_pricing.long_put.iv) {
            (Some(c), Some(p)) => Some((c + p) / 2.0),
            (Some(c), None) => Some(c),
            (None, Some(p)) => Some(p),
            _ => None,
        };

        let iv_ratio_entry = match (short_iv_entry, long_iv_entry) {
            (Some(short), Some(long)) if long > 0.0 => Some(short / long),
            _ => None,
        };

        // P&L attribution (simplified - would need per-leg attribution for full accuracy)
        let (delta_pnl, gamma_pnl, theta_pnl, vega_pnl, unexplained_pnl) = {
            // For now, leave attribution as None - this requires complex multi-leg attribution
            // TODO: Implement proper 4-leg attribution
            (None, None, None, None, None)
        };

        CalendarStraddleResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            short_strike: self.short_call.strike,
            long_strike: self.long_call.strike,
            short_expiry: self.short_call.expiration,
            long_expiry: self.long_call.expiration,
            entry_time: ctx.entry_time,
            short_call_entry: entry_pricing.short_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            short_put_entry: entry_pricing.short_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_call_entry: entry_pricing.long_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_put_entry: entry_pricing.long_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            entry_cost: entry_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            exit_time: ctx.exit_time,
            short_call_exit: exit_pricing.short_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            short_put_exit: exit_pricing.short_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_call_exit: exit_pricing.long_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_put_exit: exit_pricing.long_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            exit_value: exit_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            entry_surface_time: ctx.entry_surface_time,
            exit_surface_time: Some(ctx.exit_surface_time),
            pnl,
            pnl_pct,
            net_delta,
            net_gamma,
            net_theta,
            net_vega,
            short_iv_entry,
            long_iv_entry,
            short_iv_exit,
            long_iv_exit,
            iv_ratio_entry,
            delta_pnl,
            gamma_pnl,
            theta_pnl,
            vega_pnl,
            unexplained_pnl,
            spot_at_entry: ctx.entry_spot,
            spot_at_exit: ctx.exit_spot,
            success: true,
            failure_reason: None,
            hedge_position: None,
            hedge_pnl: None,
            total_pnl_with_hedge: None,
            position_attribution: None,
        }
    }

    fn to_failed_result(
        &self,
        ctx: &ExecutionContext,
        error: ExecutionError,
    ) -> CalendarStraddleResult {
        let failure_reason = match error {
            ExecutionError::NoSpotPrice => FailureReason::NoSpotPrice,
            ExecutionError::Repository(_) => FailureReason::NoOptionsData,
            ExecutionError::Pricing(_) => FailureReason::PricingError(error.to_string()),
            ExecutionError::InvalidSpread(_) => FailureReason::DegenerateSpread,
        };

        CalendarStraddleResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            short_strike: self.short_call.strike,
            long_strike: self.long_call.strike,
            short_expiry: self.short_call.expiration,
            long_expiry: self.long_call.expiration,
            entry_time: ctx.entry_time,
            short_call_entry: Decimal::ZERO,
            short_put_entry: Decimal::ZERO,
            long_call_entry: Decimal::ZERO,
            long_put_entry: Decimal::ZERO,
            entry_cost: Decimal::ZERO,
            exit_time: ctx.exit_time,
            short_call_exit: Decimal::ZERO,
            short_put_exit: Decimal::ZERO,
            long_call_exit: Decimal::ZERO,
            long_put_exit: Decimal::ZERO,
            exit_value: Decimal::ZERO,
            entry_surface_time: None,
            exit_surface_time: None,
            pnl: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,
            short_iv_entry: None,
            long_iv_entry: None,
            short_iv_exit: None,
            long_iv_exit: None,
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
            hedge_position: None,
            hedge_pnl: None,
            total_pnl_with_hedge: None,
            position_attribution: None,
        }
    }
}
