//! ExecutableTrade implementation for CalendarStraddle

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use cs_domain::{
    CalendarStraddle, CalendarStraddleResult, CONTRACT_MULTIPLIER, EarningsEvent,
};
use crate::composite_pricer::{CalendarStraddleCompositePricer, CompositePricing};
use super::types::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, SimulationOutput};

impl ExecutableTrade for CalendarStraddle {
    type Pricer = CalendarStraddleCompositePricer;
    type Pricing = CompositePricing;
    type Result = CalendarStraddleResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn trade_type() -> cs_domain::TradeType {
        cs_domain::TradeType::CalendarStraddle
    }

    fn validate_entry(
        pricing: &CompositePricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // CalendarStraddle legs: [0]=short_call, [1]=short_put, [2]=long_call, [3]=long_put
        let short_call = &pricing.legs[0].0;
        let short_put = &pricing.legs[1].0;
        let long_call = &pricing.legs[2].0;
        let long_put = &pricing.legs[3].0;

        // Validate max IV at entry
        if let Some(max_iv) = config.max_entry_iv {
            for (leg_name, leg_iv) in [
                ("short call", short_call.iv),
                ("short put", short_put.iv),
                ("long call", long_call.iv),
                ("long put", long_put.iv),
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
        entry_pricing: CompositePricing,
        exit_pricing: CompositePricing,
        output: &SimulationOutput,
        event: Option<&EarningsEvent>,
    ) -> CalendarStraddleResult {
        // CalendarStraddle legs: [0]=short_call, [1]=short_put, [2]=long_call, [3]=long_put
        let short_call_entry = &entry_pricing.legs[0].0;
        let short_put_entry = &entry_pricing.legs[1].0;
        let long_call_entry = &entry_pricing.legs[2].0;
        let long_put_entry = &entry_pricing.legs[3].0;

        let short_call_exit = &exit_pricing.legs[0].0;
        let short_put_exit = &exit_pricing.legs[1].0;
        let long_call_exit = &exit_pricing.legs[2].0;
        let long_put_exit = &exit_pricing.legs[3].0;

        // Calculate GROSS P&L (trading costs applied separately via ApplyCosts trait)
        let pnl_per_share = exit_pricing.net_cost - entry_pricing.net_cost;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.net_cost.abs() > Decimal::ZERO {
            (pnl / (entry_pricing.net_cost.abs() * Decimal::from(CONTRACT_MULTIPLIER))) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Net Greeks from CompositePricing (already computed with signs)
        let net_delta = Some(entry_pricing.net_delta * CONTRACT_MULTIPLIER as f64);
        let net_gamma = Some(entry_pricing.net_gamma * CONTRACT_MULTIPLIER as f64);
        let net_theta = Some(entry_pricing.net_theta * CONTRACT_MULTIPLIER as f64);
        let net_vega = Some(entry_pricing.net_vega * CONTRACT_MULTIPLIER as f64);

        // Average IV for short and long straddles
        let short_iv_entry = match (short_call_entry.iv, short_put_entry.iv) {
            (Some(c), Some(p)) => Some((c + p) / 2.0),
            (Some(c), None) => Some(c),
            (None, Some(p)) => Some(p),
            _ => None,
        };

        let long_iv_entry = match (long_call_entry.iv, long_put_entry.iv) {
            (Some(c), Some(p)) => Some((c + p) / 2.0),
            (Some(c), None) => Some(c),
            (None, Some(p)) => Some(p),
            _ => None,
        };

        let short_iv_exit = match (short_call_exit.iv, short_put_exit.iv) {
            (Some(c), Some(p)) => Some((c + p) / 2.0),
            (Some(c), None) => Some(c),
            (None, Some(p)) => Some(p),
            _ => None,
        };

        let long_iv_exit = match (long_call_exit.iv, long_put_exit.iv) {
            (Some(c), Some(p)) => Some((c + p) / 2.0),
            (Some(c), None) => Some(c),
            (None, Some(p)) => Some(p),
            _ => None,
        };

        let iv_ratio_entry = match (short_iv_entry, long_iv_entry) {
            (Some(short), Some(long)) if long > 0.0 => Some(short / long),
            _ => None,
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

        CalendarStraddleResult {
            symbol: self.symbol().to_string(),
            earnings_date: event.map(|e| e.earnings_date),
            earnings_time: event.map(|e| e.earnings_time),
            short_strike: self.short_call.strike,
            long_strike: self.long_call.strike,
            short_expiry: self.short_call.expiration,
            long_expiry: self.long_call.expiration,
            entry_time: output.entry_time,
            short_call_entry: short_call_entry.price * Decimal::from(CONTRACT_MULTIPLIER),
            short_put_entry: short_put_entry.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_call_entry: long_call_entry.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_put_entry: long_put_entry.price * Decimal::from(CONTRACT_MULTIPLIER),
            entry_cost: entry_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            exit_time: output.exit_time,
            short_call_exit: short_call_exit.price * Decimal::from(CONTRACT_MULTIPLIER),
            short_put_exit: short_put_exit.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_call_exit: long_call_exit.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_put_exit: long_put_exit.price * Decimal::from(CONTRACT_MULTIPLIER),
            exit_value: exit_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            entry_surface_time: output.entry_surface_time,
            exit_surface_time: Some(output.exit_surface_time),
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
            spot_at_entry: output.entry_spot,
            spot_at_exit: output.exit_spot,
            success: true,
            failure_reason: None,
            hedge_position: None,
            hedge_pnl: None,
            total_pnl_with_hedge: None,
            position_attribution: None,
            cost_summary: None,  // Costs applied separately via ApplyCosts trait
        }
    }

    fn to_failed_result(
        &self,
        output: &SimulationOutput,
        event: Option<&EarningsEvent>,
        error: ExecutionError,
    ) -> CalendarStraddleResult {
        let failure_reason = super::helpers::error_to_failure_reason(&error);

        CalendarStraddleResult {
            symbol: self.symbol().to_string(),
            earnings_date: event.map(|e| e.earnings_date),
            earnings_time: event.map(|e| e.earnings_time),
            short_strike: self.short_call.strike,
            long_strike: self.long_call.strike,
            short_expiry: self.short_call.expiration,
            long_expiry: self.long_call.expiration,
            entry_time: output.entry_time,
            short_call_entry: Decimal::ZERO,
            short_put_entry: Decimal::ZERO,
            long_call_entry: Decimal::ZERO,
            long_put_entry: Decimal::ZERO,
            entry_cost: Decimal::ZERO,
            exit_time: output.exit_time,
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
            cost_summary: None,
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
