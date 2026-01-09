//! ExecutableTrade implementation for Straddle

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use cs_domain::{
    Straddle, StraddleResult, PricingSource, CONTRACT_MULTIPLIER, EarningsEvent,
};
use crate::composite_pricer::{CompositePricer, CompositePricing};
use super::types::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, SimulationOutput};

impl ExecutableTrade for Straddle {
    type Pricer = CompositePricer;
    type Pricing = CompositePricing;
    type Result = StraddleResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn validate_entry(
        pricing: &CompositePricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // Validate minimum straddle price (net_cost is positive for debit)
        if pricing.net_cost < config.min_entry_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Straddle price too small: {} < {}",
                pricing.net_cost, config.min_entry_cost
            )));
        }

        // Validate entry IV (use average across legs)
        if let Some(max_iv) = config.max_entry_iv {
            if pricing.avg_iv > max_iv {
                return Err(ExecutionError::InvalidSpread(format!(
                    "IV too high: {:.1}% > {:.1}%",
                    pricing.avg_iv * 100.0,
                    max_iv * 100.0
                )));
            }
        }

        Ok(())
    }

    fn to_result(
        &self,
        entry_pricing: CompositePricing,
        exit_pricing: CompositePricing,
        output: &SimulationOutput,
        event: Option<&EarningsEvent>,
    ) -> StraddleResult {
        // Straddle legs: [0] = call (long), [1] = put (long)
        let call_entry = &entry_pricing.legs[0].0;
        let put_entry = &entry_pricing.legs[1].0;
        let call_exit = &exit_pricing.legs[0].0;
        let put_exit = &exit_pricing.legs[1].0;

        // P&L = Exit value - Entry cost (profit when straddle appreciated)
        let pnl_per_share = exit_pricing.net_cost - entry_pricing.net_cost;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.net_cost != Decimal::ZERO {
            (pnl_per_share / entry_pricing.net_cost) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Net Greeks from CompositePricing (already computed)
        let net_delta = Some(entry_pricing.net_delta * CONTRACT_MULTIPLIER as f64);
        let net_gamma = Some(entry_pricing.net_gamma * CONTRACT_MULTIPLIER as f64);
        let net_theta = Some(entry_pricing.net_theta * CONTRACT_MULTIPLIER as f64);
        let net_vega = Some(entry_pricing.net_vega * CONTRACT_MULTIPLIER as f64);

        // IV
        let iv_entry = if entry_pricing.avg_iv > 0.0 { Some(entry_pricing.avg_iv) } else { None };
        let iv_exit = if exit_pricing.avg_iv > 0.0 { Some(exit_pricing.avg_iv) } else { None };
        let iv_change = match (iv_entry, iv_exit) {
            (Some(entry), Some(exit)) => Some(((exit - entry) / entry) * 100.0),
            _ => None,
        };

        // Expected move at entry
        let expected_move_pct = if output.entry_spot > 0.0 {
            Some((entry_pricing.net_cost.to_f64().unwrap_or(0.0) / output.entry_spot) * 100.0)
        } else {
            None
        };

        // Spot move
        let spot_move = output.exit_spot - output.entry_spot;
        let spot_move_pct = if output.entry_spot != 0.0 {
            (spot_move / output.entry_spot) * 100.0
        } else {
            0.0
        };

        // P&L attribution using leg-by-leg approach
        let (delta_pnl, gamma_pnl, theta_pnl, vega_pnl, unexplained_pnl) =
            calculate_pnl_attribution(
                &entry_pricing,
                &exit_pricing,
                output.entry_spot,
                output.exit_spot,
                output.entry_time,
                output.exit_time,
                pnl,
            );

        StraddleResult {
            symbol: self.symbol().to_string(),
            earnings_date: event.map(|e| e.earnings_date),
            earnings_time: event.map(|e| e.earnings_time),
            strike: self.strike(),
            expiration: self.expiration(),
            entry_time: output.entry_time,
            call_entry_price: call_entry.price * Decimal::from(CONTRACT_MULTIPLIER),
            put_entry_price: put_entry.price * Decimal::from(CONTRACT_MULTIPLIER),
            entry_debit: entry_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            exit_time: output.exit_time,
            call_exit_price: call_exit.price * Decimal::from(CONTRACT_MULTIPLIER),
            put_exit_price: put_exit.price * Decimal::from(CONTRACT_MULTIPLIER),
            exit_credit: exit_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            entry_surface_time: output.entry_surface_time,
            exit_surface_time: Some(output.exit_surface_time),
            exit_pricing_method: PricingSource::Market,
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
            spot_at_entry: output.entry_spot,
            spot_at_exit: output.exit_spot,
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
        output: &SimulationOutput,
        event: Option<&EarningsEvent>,
        error: ExecutionError,
    ) -> StraddleResult {
        let failure_reason = super::helpers::error_to_failure_reason(&error);

        StraddleResult {
            symbol: self.symbol().to_string(),
            earnings_date: event.map(|e| e.earnings_date),
            earnings_time: event.map(|e| e.earnings_time),
            strike: self.strike(),
            expiration: self.expiration(),
            entry_time: output.entry_time,
            call_entry_price: Decimal::ZERO,
            put_entry_price: Decimal::ZERO,
            entry_debit: Decimal::ZERO,
            exit_time: output.exit_time,
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

/// Calculate P&L attribution using CompositePricing leg data
fn calculate_pnl_attribution(
    entry_pricing: &CompositePricing,
    exit_pricing: &CompositePricing,
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

    // Calculate P&L for each leg using position sign from CompositePricing
    let mut delta_sum = 0.0;
    let mut gamma_sum = 0.0;
    let mut theta_sum = 0.0;
    let mut vega_sum = 0.0;

    for ((entry_leg, position), (exit_leg, _)) in entry_pricing.legs.iter().zip(exit_pricing.legs.iter()) {
        let sign = position.sign();
        let leg_pnl = cs_domain::calculate_option_leg_pnl(
            entry_leg.greeks.as_ref(),
            entry_leg.iv,
            exit_leg.iv,
            spot_change,
            days_held,
            sign,
        );
        delta_sum += leg_pnl.delta;
        gamma_sum += leg_pnl.gamma;
        theta_sum += leg_pnl.theta;
        vega_sum += leg_pnl.vega;
    }

    // Scale to position level
    let multiplier = CONTRACT_MULTIPLIER as f64;
    let delta_pnl = delta_sum * multiplier;
    let gamma_pnl = gamma_sum * multiplier;
    let theta_pnl = theta_sum * multiplier;
    let vega_pnl = vega_sum * multiplier;

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
