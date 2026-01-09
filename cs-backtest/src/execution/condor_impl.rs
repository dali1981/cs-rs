//! ExecutableTrade implementation for Condor

use rust_decimal::Decimal;
use cs_domain::{Condor, CondorResult, CONTRACT_MULTIPLIER, EarningsEvent};
use crate::multi_leg_pricer::{CondorPricer, CondorPricing};
use super::types::ExecutionError;
use super::traits::ExecutableTrade;
use super::types::{ExecutionConfig, SimulationOutput};

impl ExecutableTrade for Condor {
    type Pricer = CondorPricer;
    type Pricing = CondorPricing;
    type Result = CondorResult;

    fn symbol(&self) -> &str {
        self.symbol()
    }

    fn trade_type() -> cs_domain::TradeType {
        cs_domain::TradeType::Condor
    }

    fn validate_entry(
        pricing: &CondorPricing,
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

        // Validate: Condor must have positive entry cost (debit)
        if pricing.entry_debit <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(
                "Condor entry cost must be positive (debit)".to_string(),
            ));
        }

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
        output: &SimulationOutput,
        event: Option<&EarningsEvent>,
        error: ExecutionError,
    ) -> CondorResult {
        CondorResult {
            symbol: self.symbol().to_string(),
            earnings_date: event.map(|e| e.earnings_date),
            earnings_time: event.map(|e| e.earnings_time),
            near_call_strike: self.near_call.strike,
            near_put_strike: self.near_put.strike,
            far_call_strike: self.far_upper_call.strike,
            far_put_strike: self.far_lower_put.strike,
            expiration: self.near_call.expiration,
            entry_time: output.entry_time,
            entry_debit: Decimal::ZERO,
            exit_time: output.exit_time,
            exit_credit: Decimal::ZERO,
            pnl: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,
            iv_entry: None,
            iv_exit: None,
            spot_at_entry: output.entry_spot,
            spot_at_exit: output.exit_spot,
            success: false,
            failure_reason: Some(cs_domain::FailureReason::PricingError(error.to_string())),
            hedge_pnl: None,
            total_pnl_with_hedge: None,
            position_attribution: None,
            cost_summary: None,
        }
    }

    fn to_result(
        &self,
        entry_pricing: CondorPricing,
        exit_pricing: CondorPricing,
        output: &SimulationOutput,
        event: Option<&EarningsEvent>,
    ) -> CondorResult {
        let pnl_per_share = exit_pricing.entry_debit - entry_pricing.entry_debit;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.entry_debit != Decimal::ZERO {
            (pnl_per_share / entry_pricing.entry_debit) * Decimal::from(100)
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

        CondorResult {
            symbol: self.symbol().to_string(),
            earnings_date: event.map(|e| e.earnings_date),
            earnings_time: event.map(|e| e.earnings_time),
            near_call_strike: self.near_call.strike,
            near_put_strike: self.near_put.strike,
            far_call_strike: self.far_upper_call.strike,
            far_put_strike: self.far_lower_put.strike,
            expiration: self.near_call.expiration,
            entry_time: output.entry_time,
            entry_debit: entry_pricing.entry_debit,
            exit_time: output.exit_time,
            exit_credit: exit_pricing.entry_debit,
            pnl,
            pnl_pct,
            net_delta,
            net_gamma,
            net_theta,
            net_vega,
            iv_entry,
            iv_exit,
            spot_at_entry: output.entry_spot,
            spot_at_exit: output.exit_spot,
            success: true,
            failure_reason: None,
            hedge_pnl: None,
            total_pnl_with_hedge: None,
            position_attribution: None,
            cost_summary: None,  // Costs applied separately via ApplyCosts trait
        }
    }
}
