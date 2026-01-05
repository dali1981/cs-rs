use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;

use cs_analytics::PricingModel;
use cs_domain::{
    CalendarStraddle, CalendarStraddleResult, EarningsEvent, FailureReason,
    EquityDataRepository, OptionsDataRepository,
    CONTRACT_MULTIPLIER,
};
use crate::iv_surface_builder::build_iv_surface_minute_aligned;
use crate::calendar_straddle_pricer::{CalendarStraddlePricer, CalendarStraddlePricing};
use crate::spread_pricer::SpreadPricer;
use crate::trade_executor::ExecutionError;

/// Executor for calendar straddle trades
///
/// Structure:
/// - Short near-term straddle (short call + short put at near expiration)
/// - Long far-term straddle (long call + long put at far expiration)
///
/// Entry: Sell near-term straddle + Buy far-term straddle (net debit)
/// Exit: Close all 4 legs
/// P&L = Exit value - Entry cost
///
/// Uses minute data when available, falls back to PricingModel for
/// options that may not have market prices.
pub struct CalendarStraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    pricer: CalendarStraddlePricer,
    max_entry_iv: Option<f64>,
}

impl<O, E> CalendarStraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(options_repo: Arc<O>, equity_repo: Arc<E>) -> Self {
        let spread_pricer = SpreadPricer::new();
        Self {
            options_repo,
            equity_repo,
            pricer: CalendarStraddlePricer::new(spread_pricer),
            max_entry_iv: None,
        }
    }

    pub fn with_pricing_model(mut self, model: PricingModel) -> Self {
        self.pricer = self.pricer.with_pricing_model(model);
        self
    }

    pub fn with_max_entry_iv(mut self, max_iv: Option<f64>) -> Self {
        self.max_entry_iv = max_iv;
        self
    }

    /// Execute a complete calendar straddle trade (entry + exit)
    pub async fn execute_trade(
        &self,
        straddle: &CalendarStraddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> CalendarStraddleResult {
        match self.try_execute_trade(straddle, event, entry_time, exit_time).await {
            Ok(result) => result,
            Err(e) => self.create_failed_result(straddle, event, entry_time, exit_time, e),
        }
    }

    async fn try_execute_trade(
        &self,
        straddle: &CalendarStraddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Result<CalendarStraddleResult, ExecutionError> {
        // Get spot prices
        let entry_spot = self.equity_repo
            .get_spot_price(straddle.symbol(), entry_time)
            .await?;

        let exit_spot = self.equity_repo
            .get_spot_price(straddle.symbol(), exit_time)
            .await?;

        // Get option chain data with timestamps for minute-aligned IV computation
        let entry_chain = self.get_option_chain(straddle.symbol(), entry_time).await?;
        let (exit_chain, exit_surface_time) = self.options_repo
            .get_option_bars_at_or_after_time(straddle.symbol(), exit_time, 30)
            .await?;

        // Build IV surface with per-option spot prices (minute-aligned)
        let entry_surface = build_iv_surface_minute_aligned(
            &entry_chain,
            self.equity_repo.as_ref(),
            straddle.symbol(),
        ).await;

        // Capture entry surface timestamp
        let entry_surface_time = entry_surface.as_ref().map(|s| s.as_of_time());

        // Price at entry using pre-built minute-aligned surface
        let entry_pricing = self.pricer.price_with_surface(
            straddle,
            &entry_chain,
            entry_spot.to_f64(),
            entry_time,
            entry_surface.as_ref(),
        )?;

        // Validate minimum position cost
        let min_cost = Decimal::new(10, 2); // $0.10 minimum net cost
        if entry_pricing.net_cost.abs() < min_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Calendar straddle net cost too small: {} < {}",
                entry_pricing.net_cost, min_cost
            )));
        }

        // Validate entry IV on the short leg (near-term)
        if let Some(max_iv) = self.max_entry_iv {
            let short_iv = compute_short_iv(&entry_pricing);
            if let Some(iv) = short_iv {
                if iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Short IV too high: {:.1}% > {:.1}%",
                        iv * 100.0, max_iv * 100.0
                    )));
                }
            }
        }

        // Build IV surface for exit with per-option spot prices
        let exit_surface = build_iv_surface_minute_aligned(
            &exit_chain,
            self.equity_repo.as_ref(),
            straddle.symbol(),
        ).await;

        // Price at exit using pre-built minute-aligned surface
        let exit_pricing = self.pricer.price_with_surface(
            straddle,
            &exit_chain,
            exit_spot.to_f64(),
            exit_time,
            exit_surface.as_ref(),
        )?;

        // P&L = Exit value - Entry cost
        // For calendar straddle: we pay debit at entry (positive cost), receive at exit
        // If exit_value > entry_cost, profit (per-share first, then multiply by contract multiplier)
        let pnl_per_share = exit_pricing.net_cost - entry_pricing.net_cost;
        let pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
        let pnl_pct = if entry_pricing.net_cost != Decimal::ZERO {
            (pnl_per_share / entry_pricing.net_cost.abs()) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Net Greeks at entry (short=-1, long=+1) - keep per-share
        let (net_delta, net_gamma, net_theta, net_vega) = compute_net_greeks(&entry_pricing);

        // IV tracking
        let short_iv_entry = compute_short_iv(&entry_pricing);
        let long_iv_entry = compute_long_iv(&entry_pricing);
        let short_iv_exit = compute_short_iv(&exit_pricing);
        let long_iv_exit = compute_long_iv(&exit_pricing);

        let iv_ratio_entry = match (short_iv_entry, long_iv_entry) {
            (Some(short), Some(long)) if long > 0.0 => Some(short / long),
            _ => None,
        };

        // P&L attribution (pass total pnl)
        let (delta_pnl, gamma_pnl, theta_pnl, vega_pnl, unexplained_pnl) =
            calculate_pnl_attribution(
                &entry_pricing,
                &exit_pricing,
                entry_spot.to_f64(),
                exit_spot.to_f64(),
                entry_time,
                exit_time,
                pnl,
            );

        Ok(CalendarStraddleResult {
            symbol: straddle.symbol().to_string(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            short_strike: straddle.short_strike(),
            long_strike: straddle.long_strike(),
            short_expiry: straddle.short_expiry(),
            long_expiry: straddle.long_expiry(),
            entry_time,
            short_call_entry: entry_pricing.short_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            short_put_entry: entry_pricing.short_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_call_entry: entry_pricing.long_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_put_entry: entry_pricing.long_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            entry_cost: entry_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            exit_time,
            short_call_exit: exit_pricing.short_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            short_put_exit: exit_pricing.short_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_call_exit: exit_pricing.long_call.price * Decimal::from(CONTRACT_MULTIPLIER),
            long_put_exit: exit_pricing.long_put.price * Decimal::from(CONTRACT_MULTIPLIER),
            exit_value: exit_pricing.net_cost * Decimal::from(CONTRACT_MULTIPLIER),
            entry_surface_time,
            exit_surface_time: Some(exit_surface_time),
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
            spot_at_entry: entry_spot.to_f64(),
            spot_at_exit: exit_spot.to_f64(),
            success: true,
            failure_reason: None,
        })
    }

    /// Get option chain with timestamps for minute-aligned IV computation
    async fn get_option_chain(
        &self,
        symbol: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<polars::prelude::DataFrame, ExecutionError> {
        self.options_repo
            .get_option_bars_at_time(symbol, timestamp)
            .await
            .map_err(ExecutionError::Repository)
    }

    fn create_failed_result(
        &self,
        straddle: &CalendarStraddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        error: ExecutionError,
    ) -> CalendarStraddleResult {
        let failure_reason = match error {
            ExecutionError::NoSpotPrice => FailureReason::NoSpotPrice,
            ExecutionError::Repository(_) => FailureReason::NoOptionsData,
            ExecutionError::Pricing(_) => FailureReason::PricingError(error.to_string()),
            ExecutionError::InvalidSpread(_) => FailureReason::DegenerateSpread,
        };

        CalendarStraddleResult {
            symbol: straddle.symbol().to_string(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            short_strike: straddle.short_strike(),
            long_strike: straddle.long_strike(),
            short_expiry: straddle.short_expiry(),
            long_expiry: straddle.long_expiry(),
            entry_time,
            short_call_entry: Decimal::ZERO,
            short_put_entry: Decimal::ZERO,
            long_call_entry: Decimal::ZERO,
            long_put_entry: Decimal::ZERO,
            entry_cost: Decimal::ZERO,
            exit_time,
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
        }
    }
}

/// Compute short (near-term) IV as average of short call and put IV
fn compute_short_iv(pricing: &CalendarStraddlePricing) -> Option<f64> {
    match (pricing.short_call.iv, pricing.short_put.iv) {
        (Some(c), Some(p)) => Some((c + p) / 2.0),
        (Some(c), None) => Some(c),
        (None, Some(p)) => Some(p),
        _ => None,
    }
}

/// Compute long (far-term) IV as average of long call and put IV
fn compute_long_iv(pricing: &CalendarStraddlePricing) -> Option<f64> {
    match (pricing.long_call.iv, pricing.long_put.iv) {
        (Some(c), Some(p)) => Some((c + p) / 2.0),
        (Some(c), None) => Some(c),
        (None, Some(p)) => Some(p),
        _ => None,
    }
}

/// Compute net Greeks for calendar straddle
///
/// Position signs:
/// - Short call: -1
/// - Short put: -1
/// - Long call: +1
/// - Long put: +1
fn compute_net_greeks(pricing: &CalendarStraddlePricing) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    let short_call_g = pricing.short_call.greeks;
    let short_put_g = pricing.short_put.greeks;
    let long_call_g = pricing.long_call.greeks;
    let long_put_g = pricing.long_put.greeks;

    match (short_call_g, short_put_g, long_call_g, long_put_g) {
        (Some(sc), Some(sp), Some(lc), Some(lp)) => {
            // Net = -short + long
            let net_delta = -sc.delta - sp.delta + lc.delta + lp.delta;
            let net_gamma = -sc.gamma - sp.gamma + lc.gamma + lp.gamma;
            let net_theta = -sc.theta - sp.theta + lc.theta + lp.theta;
            let net_vega = -sc.vega - sp.vega + lc.vega + lp.vega;

            (Some(net_delta), Some(net_gamma), Some(net_theta), Some(net_vega))
        }
        _ => (None, None, None, None),
    }
}

/// Calculate P&L attribution using leg-by-leg approach
fn calculate_pnl_attribution(
    entry_pricing: &CalendarStraddlePricing,
    exit_pricing: &CalendarStraddlePricing,
    entry_spot: f64,
    exit_spot: f64,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    total_pnl: Decimal,
) -> (Option<Decimal>, Option<Decimal>, Option<Decimal>, Option<Decimal>, Option<Decimal>) {
    let spot_change = exit_spot - entry_spot;
    let days_held = (exit_time - entry_time).num_hours() as f64 / 24.0;

    // Short call (sign = -1)
    let short_call_pnl = cs_domain::calculate_option_leg_pnl(
        entry_pricing.short_call.greeks.as_ref(),
        entry_pricing.short_call.iv,
        exit_pricing.short_call.iv,
        spot_change,
        days_held,
        -1.0,
    );

    // Short put (sign = -1)
    let short_put_pnl = cs_domain::calculate_option_leg_pnl(
        entry_pricing.short_put.greeks.as_ref(),
        entry_pricing.short_put.iv,
        exit_pricing.short_put.iv,
        spot_change,
        days_held,
        -1.0,
    );

    // Long call (sign = +1)
    let long_call_pnl = cs_domain::calculate_option_leg_pnl(
        entry_pricing.long_call.greeks.as_ref(),
        entry_pricing.long_call.iv,
        exit_pricing.long_call.iv,
        spot_change,
        days_held,
        1.0,
    );

    // Long put (sign = +1)
    let long_put_pnl = cs_domain::calculate_option_leg_pnl(
        entry_pricing.long_put.greeks.as_ref(),
        entry_pricing.long_put.iv,
        exit_pricing.long_put.iv,
        spot_change,
        days_held,
        1.0,
    );

    // Sum all legs and scale to position level
    let multiplier = CONTRACT_MULTIPLIER as f64;
    let delta_pnl = (short_call_pnl.delta + short_put_pnl.delta + long_call_pnl.delta + long_put_pnl.delta) * multiplier;
    let gamma_pnl = (short_call_pnl.gamma + short_put_pnl.gamma + long_call_pnl.gamma + long_put_pnl.gamma) * multiplier;
    let theta_pnl = (short_call_pnl.theta + short_put_pnl.theta + long_call_pnl.theta + long_put_pnl.theta) * multiplier;
    let vega_pnl = (short_call_pnl.vega + short_put_pnl.vega + long_call_pnl.vega + long_put_pnl.vega) * multiplier;

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
