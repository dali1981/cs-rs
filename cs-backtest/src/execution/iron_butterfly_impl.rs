//! ExecutableTrade implementation for IronButterfly

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use cs_domain::{
    IronButterfly, IronButterflyResult, FailureReason, CONTRACT_MULTIPLIER,
};
use crate::iron_butterfly_pricer::{IronButterflyPricer, IronButterflyPricing};
use super::types::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, ExecutionContext};

impl ExecutableTrade for IronButterfly {
    type Pricer = IronButterflyPricer;
    type Pricing = IronButterflyPricing;
    type Result = IronButterflyResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn validate_entry(
        pricing: &IronButterflyPricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // Validate max IV at entry
        if let Some(max_iv) = config.max_entry_iv {
            // Check all leg IVs
            for (leg_name, leg_iv) in [
                ("short call", pricing.short_call.iv),
                ("short put", pricing.short_put.iv),
                ("long call", pricing.long_call.iv),
                ("long put", pricing.long_put.iv),
            ] {
                if let Some(iv) = leg_iv {
                    if iv > max_iv {
                        return Err(ExecutionError::InvalidSpread(format!(
                            "{} IV too high: {:.1}% > {:.1}% (unreliable pricing)",
                            leg_name,
                            iv * 100.0,
                            max_iv * 100.0,
                        )));
                    }
                }
            }
        }

        // Validate: Iron butterfly must receive credit (negative entry cost)
        if pricing.net_credit <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(format!(
                "Invalid iron butterfly: net_credit={} (should be positive credit)",
                pricing.net_credit,
            )));
        }

        // Validate: Entry credit must be reasonable
        if pricing.net_credit < config.min_entry_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Entry credit too small: {} < {}",
                pricing.net_credit,
                config.min_entry_cost,
            )));
        }

        Ok(())
    }

    fn to_result(
        &self,
        entry_pricing: IronButterflyPricing,
        exit_pricing: IronButterflyPricing,
        ctx: &ExecutionContext,
    ) -> IronButterflyResult {
        // P&L calculation for credit spread:
        // Entry: receive credit (positive)
        // Exit: pay to close (cost)
        // P&L = entry_credit - exit_cost
        let pnl_per_share = entry_pricing.net_credit - exit_pricing.net_credit;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.net_credit != Decimal::ZERO {
            (pnl_per_share / entry_pricing.net_credit) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Calculate max loss (wing width - entry credit)
        let wing_width = (self.long_call.strike.value() - self.short_call.strike.value()) * Decimal::from(CONTRACT_MULTIPLIER);
        let max_loss = wing_width - (entry_pricing.net_credit * Decimal::from(CONTRACT_MULTIPLIER));

        // Calculate net greeks (short - long for iron butterfly)
        let net_delta = match (
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
            entry_pricing.long_call.greeks,
            entry_pricing.long_put.greeks,
        ) {
            (Some(sc), Some(sp), Some(lc), Some(lp)) => {
                Some((sc.delta + sp.delta - lc.delta - lp.delta) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_gamma = match (
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
            entry_pricing.long_call.greeks,
            entry_pricing.long_put.greeks,
        ) {
            (Some(sc), Some(sp), Some(lc), Some(lp)) => {
                Some((sc.gamma + sp.gamma - lc.gamma - lp.gamma) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_theta = match (
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
            entry_pricing.long_call.greeks,
            entry_pricing.long_put.greeks,
        ) {
            (Some(sc), Some(sp), Some(lc), Some(lp)) => {
                Some((sc.theta + sp.theta - lc.theta - lp.theta) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_vega = match (
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
            entry_pricing.long_call.greeks,
            entry_pricing.long_put.greeks,
        ) {
            (Some(sc), Some(sp), Some(lc), Some(lp)) => {
                Some((sc.vega + sp.vega - lc.vega - lp.vega) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        // Average IV across all legs
        let iv_entry = {
            let ivs: Vec<f64> = [
                entry_pricing.short_call.iv,
                entry_pricing.short_put.iv,
                entry_pricing.long_call.iv,
                entry_pricing.long_put.iv,
            ]
            .iter()
            .filter_map(|&x| x)
            .collect();

            if ivs.is_empty() {
                None
            } else {
                Some(ivs.iter().sum::<f64>() / ivs.len() as f64)
            }
        };

        let iv_exit = {
            let ivs: Vec<f64> = [
                exit_pricing.short_call.iv,
                exit_pricing.short_put.iv,
                exit_pricing.long_call.iv,
                exit_pricing.long_put.iv,
            ]
            .iter()
            .filter_map(|&x| x)
            .collect();

            if ivs.is_empty() {
                None
            } else {
                Some(ivs.iter().sum::<f64>() / ivs.len() as f64)
            }
        };

        let iv_crush = match (iv_entry, iv_exit) {
            (Some(entry), Some(exit)) => Some(entry - exit),
            _ => None,
        };

        // P&L attribution using leg-by-leg approach
        let (delta_pnl, gamma_pnl, theta_pnl, vega_pnl, unexplained_pnl) = {
            let spot_change = ctx.exit_spot - ctx.entry_spot;
            let days_held = (ctx.exit_time - ctx.entry_time).num_hours() as f64 / 24.0;

            // Calculate P&L for all 4 legs
            let short_call_pnl = cs_domain::calculate_option_leg_pnl(
                entry_pricing.short_call.greeks.as_ref(),
                entry_pricing.short_call.iv,
                exit_pricing.short_call.iv,
                spot_change,
                days_held,
                -1.0, // Short position
            );

            let short_put_pnl = cs_domain::calculate_option_leg_pnl(
                entry_pricing.short_put.greeks.as_ref(),
                entry_pricing.short_put.iv,
                exit_pricing.short_put.iv,
                spot_change,
                days_held,
                -1.0, // Short position
            );

            let long_call_pnl = cs_domain::calculate_option_leg_pnl(
                entry_pricing.long_call.greeks.as_ref(),
                entry_pricing.long_call.iv,
                exit_pricing.long_call.iv,
                spot_change,
                days_held,
                1.0, // Long position
            );

            let long_put_pnl = cs_domain::calculate_option_leg_pnl(
                entry_pricing.long_put.greeks.as_ref(),
                entry_pricing.long_put.iv,
                exit_pricing.long_put.iv,
                spot_change,
                days_held,
                1.0, // Long position
            );

            // Sum all legs and scale to position level
            let multiplier = Decimal::from(CONTRACT_MULTIPLIER).to_f64().unwrap();
            let delta = (short_call_pnl.delta + short_put_pnl.delta + long_call_pnl.delta + long_put_pnl.delta) * multiplier;
            let gamma = (short_call_pnl.gamma + short_put_pnl.gamma + long_call_pnl.gamma + long_put_pnl.gamma) * multiplier;
            let theta = (short_call_pnl.theta + short_put_pnl.theta + long_call_pnl.theta + long_put_pnl.theta) * multiplier;
            let vega = (short_call_pnl.vega + short_put_pnl.vega + long_call_pnl.vega + long_put_pnl.vega) * multiplier;

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

        // Breakeven calculation
        let center_strike_f64 = self.center_strike().value().to_f64().unwrap_or(0.0);
        let credit_per_share = entry_pricing.net_credit.to_f64().unwrap_or(0.0);
        let breakeven_up = center_strike_f64 + credit_per_share;
        let breakeven_down = center_strike_f64 - credit_per_share;
        let within_breakeven = ctx.exit_spot >= breakeven_down && ctx.exit_spot <= breakeven_up;

        let spot_move = ctx.exit_spot - ctx.entry_spot;
        let spot_move_pct = if ctx.entry_spot != 0.0 {
            (spot_move / ctx.entry_spot) * 100.0
        } else {
            0.0
        };

        IronButterflyResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            center_strike: self.center_strike(),
            upper_strike: self.upper_strike(),
            lower_strike: self.lower_strike(),
            expiration: self.expiration(),
            wing_width: self.long_call.strike.value() - self.short_call.strike.value(),
            entry_time: ctx.entry_time,
            short_call_entry: entry_pricing.short_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            short_put_entry: entry_pricing.short_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_call_entry: entry_pricing.long_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_put_entry: entry_pricing.long_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            entry_credit: entry_pricing.net_credit * Decimal::from(CONTRACT_MULTIPLIER),
            exit_time: ctx.exit_time,
            short_call_exit: exit_pricing.short_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            short_put_exit: exit_pricing.short_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_call_exit: exit_pricing.long_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_put_exit: exit_pricing.long_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            exit_cost: exit_pricing.net_credit * Decimal::from(CONTRACT_MULTIPLIER),
            entry_surface_time: ctx.entry_surface_time,
            exit_surface_time: Some(ctx.exit_surface_time),
            pnl,
            pnl_pct,
            max_loss,
            net_delta,
            net_gamma,
            net_theta,
            net_vega,
            iv_entry,
            iv_exit,
            iv_crush,
            delta_pnl,
            gamma_pnl,
            theta_pnl,
            vega_pnl,
            unexplained_pnl,
            spot_at_entry: ctx.entry_spot,
            spot_at_exit: ctx.exit_spot,
            spot_move,
            spot_move_pct,
            breakeven_up,
            breakeven_down,
            within_breakeven,
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
    ) -> IronButterflyResult {
        let failure_reason = super::helpers::error_to_failure_reason(&error);

        IronButterflyResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            center_strike: self.center_strike(),
            upper_strike: self.upper_strike(),
            lower_strike: self.lower_strike(),
            expiration: self.expiration(),
            wing_width: self.long_call.strike.value() - self.short_call.strike.value(),
            entry_time: ctx.entry_time,
            short_call_entry: Decimal::ZERO,
            short_put_entry: Decimal::ZERO,
            long_call_entry: Decimal::ZERO,
            long_put_entry: Decimal::ZERO,
            entry_credit: Decimal::ZERO,
            exit_time: ctx.exit_time,
            short_call_exit: Decimal::ZERO,
            short_put_exit: Decimal::ZERO,
            long_call_exit: Decimal::ZERO,
            long_put_exit: Decimal::ZERO,
            exit_cost: Decimal::ZERO,
            entry_surface_time: None,
            exit_surface_time: None,
            pnl: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
            max_loss: Decimal::ZERO,
            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,
            iv_entry: None,
            iv_exit: None,
            iv_crush: None,
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 0.0,
            spot_at_exit: 0.0,
            spot_move: 0.0,
            spot_move_pct: 0.0,
            breakeven_up: 0.0,
            breakeven_down: 0.0,
            within_breakeven: false,
            success: false,
            failure_reason: Some(failure_reason),
            hedge_position: None,
            hedge_pnl: None,
            total_pnl_with_hedge: None,
            position_attribution: None,
        }
    }
}
