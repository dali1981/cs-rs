//! ExecutableTrade implementation for Straddle

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use cs_domain::{
    Straddle, StraddleResult, FailureReason, PricingSource, CONTRACT_MULTIPLIER,
};
use crate::straddle_pricer::{StraddlePricer, StraddlePricing};
use crate::trade_executor::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, ExecutionContext};

impl ExecutableTrade for Straddle {
    type Pricer = StraddlePricer;
    type Pricing = StraddlePricing;
    type Result = StraddleResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn validate_entry(
        pricing: &StraddlePricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // Validate minimum straddle price
        if pricing.total_price < config.min_entry_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Straddle price too small: {} < {}",
                pricing.total_price, config.min_entry_cost
            )));
        }

        // Validate entry IV
        if let Some(max_iv) = config.max_entry_iv {
            if let Some(iv) = pricing.call.iv {
                if iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "IV too high: {:.1}% > {:.1}%",
                        iv * 100.0,
                        max_iv * 100.0
                    )));
                }
            }
        }

        Ok(())
    }

    fn to_result(
        &self,
        entry_pricing: StraddlePricing,
        exit_pricing: StraddlePricing,
        ctx: &ExecutionContext,
    ) -> StraddleResult {
        // P&L = Exit value - Entry cost (profit when straddle appreciated)
        // Calculate per-share first, then multiply by contract multiplier
        let pnl_per_share = exit_pricing.total_price - entry_pricing.total_price;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.total_price != Decimal::ZERO {
            (pnl_per_share / entry_pricing.total_price) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Net Greeks (long call + long put) - keep per-share for hedging
        let (net_delta, net_gamma, net_theta, net_vega) = compute_net_greeks(&entry_pricing);

        // IV change (positive = good for long straddle)
        let (iv_entry, iv_exit, iv_change) = compute_iv_change(&entry_pricing, &exit_pricing);

        // Expected move at entry
        let expected_move_pct = if ctx.entry_spot > 0.0 {
            Some((entry_pricing.total_price.to_f64().unwrap_or(0.0) / ctx.entry_spot) * 100.0)
        } else {
            None
        };

        // Spot move
        let spot_move = ctx.exit_spot - ctx.entry_spot;
        let spot_move_pct = if ctx.entry_spot != 0.0 {
            (spot_move / ctx.entry_spot) * 100.0
        } else {
            0.0
        };

        // P&L attribution (pass total pnl, get results in dollars)
        let (delta_pnl, gamma_pnl, theta_pnl, vega_pnl, unexplained_pnl) =
            calculate_pnl_attribution(
                &entry_pricing,
                &exit_pricing,
                ctx.entry_spot,
                ctx.exit_spot,
                ctx.entry_time,
                ctx.exit_time,
                pnl,
            );

        StraddleResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            strike: self.strike(),
            expiration: self.expiration(),
            entry_time: ctx.entry_time,
            call_entry_price: entry_pricing.call.price * Decimal::from(CONTRACT_MULTIPLIER),
            put_entry_price: entry_pricing.put.price * Decimal::from(CONTRACT_MULTIPLIER),
            entry_debit: entry_pricing.total_price * Decimal::from(CONTRACT_MULTIPLIER),
            exit_time: ctx.exit_time,
            call_exit_price: exit_pricing.call.price * Decimal::from(CONTRACT_MULTIPLIER),
            put_exit_price: exit_pricing.put.price * Decimal::from(CONTRACT_MULTIPLIER),
            exit_credit: exit_pricing.total_price * Decimal::from(CONTRACT_MULTIPLIER),
            entry_surface_time: ctx.entry_surface_time,
            exit_surface_time: Some(ctx.exit_surface_time),
            exit_pricing_method: exit_pricing.source,
            pnl,
            pnl_pct,
            net_delta,
            net_gamma,
            net_theta,
            net_vega,
            iv_entry,
            iv_exit,
            iv_change,
            delta_pnl,
            gamma_pnl,
            theta_pnl,
            vega_pnl,
            unexplained_pnl,
            spot_at_entry: ctx.entry_spot,
            spot_at_exit: ctx.exit_spot,
            spot_move,
            spot_move_pct,
            expected_move_pct,
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
    ) -> StraddleResult {
        let failure_reason = match error {
            ExecutionError::NoSpotPrice => FailureReason::NoSpotPrice,
            ExecutionError::Repository(_) => FailureReason::NoOptionsData,
            ExecutionError::Pricing(_) => FailureReason::PricingError(error.to_string()),
            ExecutionError::InvalidSpread(_) => FailureReason::DegenerateSpread,
        };

        StraddleResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            strike: self.strike(),
            expiration: self.expiration(),
            entry_time: ctx.entry_time,
            call_entry_price: Decimal::ZERO,
            put_entry_price: Decimal::ZERO,
            entry_debit: Decimal::ZERO,
            exit_time: ctx.exit_time,
            call_exit_price: Decimal::ZERO,
            put_exit_price: Decimal::ZERO,
            exit_credit: Decimal::ZERO,
            entry_surface_time: None,
            exit_surface_time: None,
            exit_pricing_method: PricingSource::Market,
            pnl: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,
            iv_entry: None,
            iv_exit: None,
            iv_change: None,
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 0.0,
            spot_at_exit: 0.0,
            spot_move: 0.0,
            spot_move_pct: 0.0,
            expected_move_pct: None,
            success: false,
            failure_reason: Some(failure_reason),
            hedge_position: None,
            hedge_pnl: None,
            total_pnl_with_hedge: None,
            position_attribution: None,
        }
    }
}

/// Compute net Greeks for long straddle (long call + long put)
fn compute_net_greeks(
    pricing: &StraddlePricing,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    match (pricing.call.greeks, pricing.put.greeks) {
        (Some(call_g), Some(put_g)) => {
            // Long both legs: add greeks
            let net_delta = call_g.delta + put_g.delta; // ~0 for ATM straddle
            let net_gamma = call_g.gamma + put_g.gamma; // Positive (long gamma)
            let net_theta = call_g.theta + put_g.theta; // Negative (time decay)
            let net_vega = call_g.vega + put_g.vega; // Positive (want IV expansion)

            (
                Some(net_delta),
                Some(net_gamma),
                Some(net_theta),
                Some(net_vega),
            )
        }
        _ => (None, None, None, None),
    }
}

/// Compute IV change between entry and exit
fn compute_iv_change(
    entry_pricing: &StraddlePricing,
    exit_pricing: &StraddlePricing,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    // Use average of call and put IV
    let iv_entry = match (entry_pricing.call.iv, entry_pricing.put.iv) {
        (Some(c), Some(p)) => Some((c + p) / 2.0),
        (Some(c), None) => Some(c),
        (None, Some(p)) => Some(p),
        _ => None,
    };

    let iv_exit = match (exit_pricing.call.iv, exit_pricing.put.iv) {
        (Some(c), Some(p)) => Some((c + p) / 2.0),
        (Some(c), None) => Some(c),
        (None, Some(p)) => Some(p),
        _ => None,
    };

    let iv_change = match (iv_entry, iv_exit) {
        (Some(entry), Some(exit)) => Some(exit - entry),
        _ => None,
    };

    (iv_entry, iv_exit, iv_change)
}

/// Calculate P&L attribution using leg-by-leg approach
fn calculate_pnl_attribution(
    entry_pricing: &StraddlePricing,
    exit_pricing: &StraddlePricing,
    entry_spot: f64,
    exit_spot: f64,
    entry_time: chrono::DateTime<chrono::Utc>,
    exit_time: chrono::DateTime<chrono::Utc>,
    total_pnl: Decimal,
) -> (
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
) {
    let spot_change = exit_spot - entry_spot;
    let days_held = (exit_time - entry_time).num_hours() as f64 / 24.0;

    // Calculate P&L for call leg (long position, sign = +1.0)
    let call_pnl = cs_domain::calculate_option_leg_pnl(
        entry_pricing.call.greeks.as_ref(),
        entry_pricing.call.iv,
        exit_pricing.call.iv,
        spot_change,
        days_held,
        1.0, // Long call
    );

    // Calculate P&L for put leg (long position, sign = +1.0)
    let put_pnl = cs_domain::calculate_option_leg_pnl(
        entry_pricing.put.greeks.as_ref(),
        entry_pricing.put.iv,
        exit_pricing.put.iv,
        spot_change,
        days_held,
        1.0, // Long put
    );

    // Sum the legs and scale to position level
    let multiplier = CONTRACT_MULTIPLIER as f64;
    let delta_pnl = (call_pnl.delta + put_pnl.delta) * multiplier;
    let gamma_pnl = (call_pnl.gamma + put_pnl.gamma) * multiplier;
    let theta_pnl = (call_pnl.theta + put_pnl.theta) * multiplier;
    let vega_pnl = (call_pnl.vega + put_pnl.vega) * multiplier;

    let explained = delta_pnl + gamma_pnl + theta_pnl + vega_pnl;
    let unexplained = total_pnl.to_f64().unwrap_or(0.0) - explained;

    (
        Some(Decimal::try_from(delta_pnl).unwrap_or_default()),
        Some(Decimal::try_from(gamma_pnl).unwrap_or_default()),
        Some(Decimal::try_from(theta_pnl).unwrap_or_default()),
        Some(Decimal::try_from(vega_pnl).unwrap_or_default()),
        Some(Decimal::try_from(unexplained).unwrap_or_default()),
    )
}
