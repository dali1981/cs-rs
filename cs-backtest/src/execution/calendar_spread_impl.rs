//! ExecutableTrade implementation for CalendarSpread

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use cs_domain::{
    CalendarSpread, CalendarSpreadResult, CONTRACT_MULTIPLIER, EarningsEvent,
};
use crate::composite_pricer::{CalendarSpreadPricer, CompositePricing};
use super::types::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, SimulationOutput};

impl ExecutableTrade for CalendarSpread {
    type Pricer = CalendarSpreadPricer;
    type Pricing = CompositePricing;
    type Result = CalendarSpreadResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn validate_entry(
        pricing: &CompositePricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // CalendarSpread legs: [0]=short_leg, [1]=long_leg
        let short_leg = &pricing.legs[0].0;
        let long_leg = &pricing.legs[1].0;

        // Validate max IV at entry
        if let Some(max_iv) = config.max_entry_iv {
            if let Some(short_iv) = short_leg.iv {
                if short_iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Short leg IV too high: {:.1}% > {:.1}% (unreliable pricing)",
                        short_iv * 100.0,
                        max_iv * 100.0,
                    )));
                }
            }
            if let Some(long_iv) = long_leg.iv {
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
                pricing.net_cost, short_leg.price, long_leg.price,
            )));
        }

        // Validate: Entry cost must be reasonable
        if pricing.net_cost < config.min_entry_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Entry cost too small: {} < {} (short={}, long={})",
                pricing.net_cost,
                config.min_entry_cost,
                short_leg.price,
                long_leg.price,
            )));
        }

        Ok(())
    }

    fn to_result(
        &self,
        entry_pricing: CompositePricing,
        exit_pricing: CompositePricing,
        output: &SimulationOutput,
        event: &EarningsEvent,
    ) -> CalendarSpreadResult {
        // CalendarSpread legs: [0]=short_leg, [1]=long_leg
        let short_entry = &entry_pricing.legs[0].0;
        let long_entry = &entry_pricing.legs[1].0;
        let short_exit = &exit_pricing.legs[0].0;
        let long_exit = &exit_pricing.legs[1].0;

        // Calculate P&L (per-share first, then multiply by contract multiplier)
        let pnl_per_share = exit_pricing.net_cost - entry_pricing.net_cost;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.net_cost != Decimal::ZERO {
            (pnl_per_share / entry_pricing.net_cost) * Decimal::from(100)
        } else {
            Decimal::ZERO
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

        CalendarSpreadResult {
            symbol: self.symbol().to_string(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            strike: self.strike(),
            long_strike: if self.short_leg.strike != self.long_leg.strike {
                Some(self.long_leg.strike)
            } else {
                None
            },
            option_type: self.option_type(),
            short_expiry: self.short_expiry(),
            long_expiry: self.long_expiry(),
            entry_time: output.entry_time,
            short_entry_price: short_entry.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_entry_price: long_entry.price * Decimal::from(CONTRACT_MULTIPLIER),
            entry_cost: entry_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            exit_time: output.exit_time,
            short_exit_price: short_exit.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_exit_price: long_exit.price * Decimal::from(CONTRACT_MULTIPLIER),
            exit_value: exit_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            entry_surface_time: output.entry_surface_time,
            exit_surface_time: Some(output.exit_surface_time),
            pnl,
            pnl_per_contract: pnl,
            pnl_pct,
            short_delta: short_entry.greeks.map(|g| g.delta),
            short_gamma: short_entry.greeks.map(|g| g.gamma),
            short_theta: short_entry.greeks.map(|g| g.theta),
            short_vega: short_entry.greeks.map(|g| g.vega),
            long_delta: long_entry.greeks.map(|g| g.delta),
            long_gamma: long_entry.greeks.map(|g| g.gamma),
            long_theta: long_entry.greeks.map(|g| g.theta),
            long_vega: long_entry.greeks.map(|g| g.vega),
            iv_short_entry: short_entry.iv,
            iv_long_entry: long_entry.iv,
            iv_short_exit: short_exit.iv,
            iv_long_exit: long_exit.iv,
            iv_ratio_entry: match (short_entry.iv, long_entry.iv) {
                (Some(short_iv), Some(long_iv)) if long_iv > 0.0 => Some(short_iv / long_iv),
                _ => None,
            },
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
        }
    }

    fn to_failed_result(
        &self,
        output: &SimulationOutput,
        event: &EarningsEvent,
        error: ExecutionError,
    ) -> CalendarSpreadResult {
        let failure_reason = super::helpers::error_to_failure_reason(&error);

        CalendarSpreadResult {
            symbol: self.symbol().to_string(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            strike: self.strike(),
            long_strike: if self.short_leg.strike != self.long_leg.strike {
                Some(self.long_leg.strike)
            } else {
                None
            },
            option_type: self.option_type(),
            short_expiry: self.short_expiry(),
            long_expiry: self.long_expiry(),
            entry_time: output.entry_time,
            short_entry_price: Decimal::ZERO,
            long_entry_price: Decimal::ZERO,
            entry_cost: Decimal::ZERO,
            exit_time: output.exit_time,
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
