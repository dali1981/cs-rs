//! ExecutableTrade implementation for Strangle

use rust_decimal::Decimal;
use cs_domain::{Strangle, StrangleResult, CONTRACT_MULTIPLIER};
use crate::multi_leg_pricer::{StranglePricer, StranglePricing};
use super::types::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, ExecutionContext};

impl ExecutableTrade for Strangle {
    type Pricer = StranglePricer;
    type Pricing = StranglePricing;
    type Result = StrangleResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn validate_entry(
        pricing: &StranglePricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // Validate max IV at entry
        if let Some(max_iv) = config.max_entry_iv {
            for (leg_name, leg_iv) in [
                ("call", pricing.call.iv),
                ("put", pricing.put.iv),
            ] {
                if let Some(iv) = leg_iv {
                    if iv > max_iv {
                        return Err(ExecutionError::InvalidSpread(format!(
                            "{} IV too high: {:.1}% > {:.1}%",
                            leg_name,
                            iv * 100.0,
                            max_iv * 100.0,
                        )));
                    }
                }
            }
        }

        // Validate: Strangle must have positive entry cost (debit)
        if pricing.entry_debit <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(
                "Strangle entry cost must be positive (debit)".to_string(),
            ));
        }

        // Validate: Entry cost must be reasonable
        if pricing.entry_debit < config.min_entry_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Entry debit too small: {} < {}",
                pricing.entry_debit,
                config.min_entry_cost,
            )));
        }

        Ok(())
    }

    fn to_failed_result(
        &self,
        ctx: &ExecutionContext,
        error: ExecutionError,
    ) -> StrangleResult {
        StrangleResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            call_strike: self.call_leg.strike,
            put_strike: self.put_leg.strike,
            expiration: self.call_leg.expiration,
            entry_time: ctx.entry_time,
            call_entry_price: Decimal::ZERO,
            put_entry_price: Decimal::ZERO,
            entry_debit: Decimal::ZERO,
            exit_time: ctx.exit_time,
            call_exit_price: Decimal::ZERO,
            put_exit_price: Decimal::ZERO,
            exit_credit: Decimal::ZERO,
            pnl: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,
            iv_entry: None,
            iv_exit: None,
            spot_at_entry: ctx.entry_spot,
            spot_at_exit: ctx.exit_spot,
            success: false,
            failure_reason: Some(cs_domain::FailureReason::PricingError(error.to_string())),
            hedge_pnl: None,
            total_pnl_with_hedge: None,
            position_attribution: None,
        }
    }

    fn to_result(
        &self,
        entry_pricing: StranglePricing,
        exit_pricing: StranglePricing,
        ctx: &ExecutionContext,
    ) -> StrangleResult {
        // P&L for long strangle (debit spread):
        // Entry: pay debit
        // Exit: receive credit (or pay more to close)
        // P&L = exit_credit - entry_debit
        let pnl_per_share = exit_pricing.entry_debit - entry_pricing.entry_debit;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.entry_debit != Decimal::ZERO {
            (pnl_per_share / entry_pricing.entry_debit) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Calculate net greeks (long call + long put)
        let net_delta = match (entry_pricing.call.greeks, entry_pricing.put.greeks) {
            (Some(call_g), Some(put_g)) => {
                Some((call_g.delta + put_g.delta) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_gamma = match (entry_pricing.call.greeks, entry_pricing.put.greeks) {
            (Some(call_g), Some(put_g)) => {
                Some((call_g.gamma + put_g.gamma) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_theta = match (entry_pricing.call.greeks, entry_pricing.put.greeks) {
            (Some(call_g), Some(put_g)) => {
                Some((call_g.theta + put_g.theta) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_vega = match (entry_pricing.call.greeks, entry_pricing.put.greeks) {
            (Some(call_g), Some(put_g)) => {
                Some((call_g.vega + put_g.vega) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        // Average IV across both legs
        let iv_entry = {
            let ivs: Vec<f64> = [entry_pricing.call.iv, entry_pricing.put.iv]
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
            let ivs: Vec<f64> = [exit_pricing.call.iv, exit_pricing.put.iv]
                .iter()
                .filter_map(|&x| x)
                .collect();
            if ivs.is_empty() {
                None
            } else {
                Some(ivs.iter().sum::<f64>() / ivs.len() as f64)
            }
        };

        StrangleResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            call_strike: self.call_leg.strike,
            put_strike: self.put_leg.strike,
            expiration: self.call_leg.expiration,
            entry_time: ctx.entry_time,
            call_entry_price: entry_pricing.call.price,
            put_entry_price: entry_pricing.put.price,
            entry_debit: entry_pricing.entry_debit,
            exit_time: ctx.exit_time,
            call_exit_price: exit_pricing.call.price,
            put_exit_price: exit_pricing.put.price,
            exit_credit: exit_pricing.entry_debit,
            pnl,
            pnl_pct,
            net_delta,
            net_gamma,
            net_theta,
            net_vega,
            iv_entry,
            iv_exit,
            spot_at_entry: ctx.entry_spot,
            spot_at_exit: ctx.exit_spot,
            success: true,
            failure_reason: None,
            hedge_pnl: None,
            total_pnl_with_hedge: None,
            position_attribution: None,
        }
    }
}
