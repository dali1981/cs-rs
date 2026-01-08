//! ExecutableTrade implementation for Butterfly

use rust_decimal::Decimal;
use cs_domain::{Butterfly, ButterflyResult, CONTRACT_MULTIPLIER};
use crate::multi_leg_pricer::{ButterflyPricer, ButterflyPricing};
use super::types::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, ExecutionContext};

impl ExecutableTrade for Butterfly {
    type Pricer = ButterflyPricer;
    type Pricing = ButterflyPricing;
    type Result = ButterflyResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn validate_entry(
        pricing: &ButterflyPricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // Validate max IV at entry
        if let Some(max_iv) = config.max_entry_iv {
            for (leg_name, leg_iv) in [
                ("short_call", pricing.short_call.iv),
                ("short_put", pricing.short_put.iv),
                ("long_upper_call", pricing.long_upper_call.iv),
                ("long_lower_put", pricing.long_lower_put.iv),
            ] {
                if let Some(iv) = leg_iv {
                    if iv > max_iv {
                        return Err(ExecutionError::InvalidSpread(format!(
                            "{} IV too high: {:.1}%",
                            leg_name,
                            iv * 100.0,
                        )));
                    }
                }
            }
        }

        // Validate: Butterfly must have positive entry cost (debit)
        if pricing.entry_debit <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(
                "Butterfly entry cost must be positive (debit)".to_string(),
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
    ) -> ButterflyResult {
        ButterflyResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            short_strike: self.short_call.strike,
            upper_strike: self.long_upper_call.strike,
            lower_strike: self.long_lower_put.strike,
            expiration: self.short_call.expiration,
            entry_time: ctx.entry_time,
            entry_debit: Decimal::ZERO,
            exit_time: ctx.exit_time,
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
        entry_pricing: ButterflyPricing,
        exit_pricing: ButterflyPricing,
        ctx: &ExecutionContext,
    ) -> ButterflyResult {
        let pnl_per_share = exit_pricing.entry_debit - entry_pricing.entry_debit;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.entry_debit != Decimal::ZERO {
            (pnl_per_share / entry_pricing.entry_debit) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Calculate net greeks
        let net_delta = match (
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
            entry_pricing.long_upper_call.greeks,
            entry_pricing.long_lower_put.greeks,
        ) {
            (Some(sc), Some(sp), Some(luc), Some(llp)) => {
                Some((sc.delta + sp.delta - luc.delta - llp.delta) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_gamma = match (
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
            entry_pricing.long_upper_call.greeks,
            entry_pricing.long_lower_put.greeks,
        ) {
            (Some(sc), Some(sp), Some(luc), Some(llp)) => {
                Some((sc.gamma + sp.gamma - luc.gamma - llp.gamma) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_theta = match (
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
            entry_pricing.long_upper_call.greeks,
            entry_pricing.long_lower_put.greeks,
        ) {
            (Some(sc), Some(sp), Some(luc), Some(llp)) => {
                Some((sc.theta + sp.theta - luc.theta - llp.theta) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_vega = match (
            entry_pricing.short_call.greeks,
            entry_pricing.short_put.greeks,
            entry_pricing.long_upper_call.greeks,
            entry_pricing.long_lower_put.greeks,
        ) {
            (Some(sc), Some(sp), Some(luc), Some(llp)) => {
                Some((sc.vega + sp.vega - luc.vega - llp.vega) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        // Average IV
        let iv_entry = {
            let ivs: Vec<f64> = [
                entry_pricing.short_call.iv,
                entry_pricing.short_put.iv,
                entry_pricing.long_upper_call.iv,
                entry_pricing.long_lower_put.iv,
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
                exit_pricing.long_upper_call.iv,
                exit_pricing.long_lower_put.iv,
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

        ButterflyResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            short_strike: self.short_call.strike,
            upper_strike: self.long_upper_call.strike,
            lower_strike: self.long_lower_put.strike,
            expiration: self.short_call.expiration,
            entry_time: ctx.entry_time,
            entry_debit: entry_pricing.entry_debit,
            exit_time: ctx.exit_time,
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
