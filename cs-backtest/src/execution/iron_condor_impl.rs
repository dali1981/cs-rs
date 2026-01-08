//! ExecutableTrade implementation for IronCondor

use rust_decimal::Decimal;
use cs_domain::{IronCondor, IronCondorResult, CONTRACT_MULTIPLIER};
use crate::multi_leg_pricer::{IronCondorPricer, IronCondorPricing};
use super::types::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, ExecutionContext};

impl ExecutableTrade for IronCondor {
    type Pricer = IronCondorPricer;
    type Pricing = IronCondorPricing;
    type Result = IronCondorResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn validate_entry(
        pricing: &IronCondorPricing,
        config: &ExecutionConfig,
    ) -> Result<(), ExecutionError> {
        // Validate max IV at entry
        if let Some(max_iv) = config.max_entry_iv {
            for (leg_name, leg_iv) in [
                ("near_call", pricing.near_call.iv),
                ("near_put", pricing.near_put.iv),
                ("far_upper_call", pricing.far_upper_call.iv),
                ("far_lower_put", pricing.far_lower_put.iv),
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

        // Validate: Iron condor must receive credit
        if pricing.net_credit <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(
                "Iron condor must receive credit".to_string(),
            ));
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

    fn to_failed_result(
        &self,
        ctx: &ExecutionContext,
        error: ExecutionError,
    ) -> IronCondorResult {
        IronCondorResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            near_call_strike: self.near_call.strike,
            near_put_strike: self.near_put.strike,
            far_call_strike: self.far_upper_call.strike,
            far_put_strike: self.far_lower_put.strike,
            expiration: self.near_call.expiration,
            entry_time: ctx.entry_time,
            entry_credit: Decimal::ZERO,
            exit_time: ctx.exit_time,
            exit_cost: Decimal::ZERO,
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
        entry_pricing: IronCondorPricing,
        exit_pricing: IronCondorPricing,
        ctx: &ExecutionContext,
    ) -> IronCondorResult {
        // P&L for credit spread: entry_credit - exit_cost
        let pnl_per_share = entry_pricing.net_credit - exit_pricing.net_credit;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.net_credit != Decimal::ZERO {
            (pnl_per_share / entry_pricing.net_credit) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Calculate net greeks (short near - long far)
        let net_delta = match (
            entry_pricing.near_call.greeks,
            entry_pricing.near_put.greeks,
            entry_pricing.far_upper_call.greeks,
            entry_pricing.far_lower_put.greeks,
        ) {
            (Some(nc), Some(np), Some(fuc), Some(flp)) => {
                Some((nc.delta + np.delta - fuc.delta - flp.delta) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_gamma = match (
            entry_pricing.near_call.greeks,
            entry_pricing.near_put.greeks,
            entry_pricing.far_upper_call.greeks,
            entry_pricing.far_lower_put.greeks,
        ) {
            (Some(nc), Some(np), Some(fuc), Some(flp)) => {
                Some((nc.gamma + np.gamma - fuc.gamma - flp.gamma) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_theta = match (
            entry_pricing.near_call.greeks,
            entry_pricing.near_put.greeks,
            entry_pricing.far_upper_call.greeks,
            entry_pricing.far_lower_put.greeks,
        ) {
            (Some(nc), Some(np), Some(fuc), Some(flp)) => {
                Some((nc.theta + np.theta - fuc.theta - flp.theta) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let net_vega = match (
            entry_pricing.near_call.greeks,
            entry_pricing.near_put.greeks,
            entry_pricing.far_upper_call.greeks,
            entry_pricing.far_lower_put.greeks,
        ) {
            (Some(nc), Some(np), Some(fuc), Some(flp)) => {
                Some((nc.vega + np.vega - fuc.vega - flp.vega) * CONTRACT_MULTIPLIER as f64)
            }
            _ => None,
        };

        let iv_entry = {
            let ivs: Vec<f64> = [
                entry_pricing.near_call.iv,
                entry_pricing.near_put.iv,
                entry_pricing.far_upper_call.iv,
                entry_pricing.far_lower_put.iv,
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
                exit_pricing.near_call.iv,
                exit_pricing.near_put.iv,
                exit_pricing.far_upper_call.iv,
                exit_pricing.far_lower_put.iv,
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

        IronCondorResult {
            symbol: self.symbol().to_string(),
            earnings_date: ctx.earnings_event.earnings_date,
            earnings_time: ctx.earnings_event.earnings_time,
            near_call_strike: self.near_call.strike,
            near_put_strike: self.near_put.strike,
            far_call_strike: self.far_upper_call.strike,
            far_put_strike: self.far_lower_put.strike,
            expiration: self.near_call.expiration,
            entry_time: ctx.entry_time,
            entry_credit: entry_pricing.net_credit,
            exit_time: ctx.exit_time,
            exit_cost: exit_pricing.net_credit,
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
