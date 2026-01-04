use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;

use cs_analytics::PricingModel;
use cs_domain::{
    IronButterfly, IronButterflyResult, EarningsEvent, FailureReason,
    EquityDataRepository, OptionsDataRepository, MarketTime,
};
use crate::iv_surface_builder::build_iv_surface_minute_aligned;
use crate::iron_butterfly_pricer::{IronButterflyPricer, IronButterflyPricing};
use crate::spread_pricer::{SpreadPricer, LegPricing};
use crate::trade_executor::ExecutionError;

/// Executor for iron butterfly trades
pub struct IronButterflyExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    pricer: IronButterflyPricer,
    max_entry_iv: Option<f64>,
}

impl<O, E> IronButterflyExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(options_repo: Arc<O>, equity_repo: Arc<E>) -> Self {
        let spread_pricer = SpreadPricer::new();
        Self {
            options_repo,
            equity_repo,
            pricer: IronButterflyPricer::new(spread_pricer),
            max_entry_iv: None,
        }
    }

    pub fn with_market_close(mut self, market_close: MarketTime) -> Self {
        let spread_pricer = SpreadPricer::new().with_market_close(market_close);
        self.pricer = IronButterflyPricer::new(spread_pricer);
        self
    }

    pub fn with_pricing_model(mut self, pricing_model: PricingModel) -> Self {
        let spread_pricer = SpreadPricer::new().with_pricing_model(pricing_model);
        self.pricer = IronButterflyPricer::new(spread_pricer);
        self
    }

    pub fn with_max_entry_iv(mut self, max_entry_iv: Option<f64>) -> Self {
        self.max_entry_iv = max_entry_iv;
        self
    }

    /// Execute a complete iron butterfly trade (entry + exit)
    pub async fn execute_trade(
        &self,
        butterfly: &IronButterfly,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> IronButterflyResult {
        match self.try_execute_trade(butterfly, event, entry_time, exit_time).await {
            Ok(result) => result,
            Err(e) => self.create_failed_result(butterfly, event, entry_time, exit_time, e),
        }
    }

    async fn try_execute_trade(
        &self,
        butterfly: &IronButterfly,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Result<IronButterflyResult, ExecutionError> {
        // Get spot prices
        let entry_spot = self.equity_repo
            .get_spot_price(butterfly.symbol(), entry_time)
            .await?;

        let exit_spot = self.equity_repo
            .get_spot_price(butterfly.symbol(), exit_time)
            .await?;

        // Get option chain data with timestamps for minute-aligned IV computation
        let entry_chain = self.options_repo
            .get_option_bars_at_time(butterfly.symbol(), entry_time)
            .await?;

        let (exit_chain, exit_surface_time) = self.options_repo
            .get_option_bars_at_or_after_time(butterfly.symbol(), exit_time, 30)
            .await?;

        // Build IV surface with per-option spot prices (minute-aligned)
        let entry_surface = build_iv_surface_minute_aligned(
            &entry_chain,
            self.equity_repo.as_ref(),
            butterfly.symbol(),
        ).await;

        // Capture entry surface timestamp
        let entry_surface_time = entry_surface.as_ref().map(|s| s.as_of_time());

        // Price at entry using pre-built minute-aligned surface
        let entry_pricing = self.pricer.price_with_surface(
            butterfly,
            &entry_chain,
            entry_spot.to_f64(),
            entry_time,
            entry_surface.as_ref(),
        )?;

        // Validate minimum credit
        let min_credit = Decimal::new(50, 2); // $0.50 minimum
        if entry_pricing.net_credit < min_credit {
            return Err(ExecutionError::InvalidSpread(format!(
                "Credit too small: {} < {}",
                entry_pricing.net_credit,
                min_credit
            )));
        }

        // Validate IV at entry (use short call IV as reference)
        if let Some(max_iv) = self.max_entry_iv {
            if let Some(iv) = entry_pricing.short_call.iv {
                if iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "IV too high: {:.1}% > {:.1}%",
                        iv * 100.0,
                        max_iv * 100.0
                    )));
                }
            }
        }

        // Build IV surface for exit with per-option spot prices
        let exit_surface = build_iv_surface_minute_aligned(
            &exit_chain,
            self.equity_repo.as_ref(),
            butterfly.symbol(),
        ).await;

        // Price at exit using pre-built minute-aligned surface
        let exit_pricing = self.pricer.price_with_surface(
            butterfly,
            &exit_chain,
            exit_spot.to_f64(),
            exit_time,
            exit_surface.as_ref(),
        )?;

        // P&L = entry_credit - exit_cost (profit when options expire worthless)
        let pnl = entry_pricing.net_credit - exit_pricing.net_credit;
        let pnl_pct = if entry_pricing.net_credit != Decimal::ZERO {
            (pnl / entry_pricing.net_credit) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Calculate max loss
        let max_loss = butterfly.wing_width() - entry_pricing.net_credit;

        // Calculate breakeven levels
        let center_f64: f64 = butterfly.center_strike().into();
        let credit_f64: f64 = entry_pricing.net_credit.try_into().unwrap_or(0.0);
        let breakeven_up = center_f64 + credit_f64;
        let breakeven_down = center_f64 - credit_f64;

        let exit_spot_f64 = exit_spot.to_f64();
        let within_breakeven = exit_spot_f64 >= breakeven_down && exit_spot_f64 <= breakeven_up;

        // Net greeks
        let (net_delta, net_gamma, net_theta, net_vega) = compute_net_greeks(&entry_pricing);

        // IV crush (use short call as reference)
        let (iv_entry, iv_exit, iv_crush) = match (
            entry_pricing.short_call.iv,
            exit_pricing.short_call.iv,
        ) {
            (Some(entry), Some(exit)) => (Some(entry), Some(exit), Some(entry - exit)),
            _ => (None, None, None),
        };

        // P&L attribution
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

        // Spot move
        let spot_move = exit_spot.to_f64() - entry_spot.to_f64();
        let spot_move_pct = if entry_spot.to_f64() != 0.0 {
            (spot_move / entry_spot.to_f64()) * 100.0
        } else {
            0.0
        };

        Ok(IronButterflyResult {
            symbol: butterfly.symbol().to_string(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            center_strike: butterfly.center_strike(),
            upper_strike: butterfly.upper_strike(),
            lower_strike: butterfly.lower_strike(),
            expiration: butterfly.expiration(),
            wing_width: butterfly.wing_width(),
            entry_time,
            short_call_entry: entry_pricing.short_call.price,
            short_put_entry: entry_pricing.short_put.price,
            long_call_entry: entry_pricing.long_call.price,
            long_put_entry: entry_pricing.long_put.price,
            entry_credit: entry_pricing.net_credit,
            exit_time,
            short_call_exit: exit_pricing.short_call.price,
            short_put_exit: exit_pricing.short_put.price,
            long_call_exit: exit_pricing.long_call.price,
            long_put_exit: exit_pricing.long_put.price,
            exit_cost: exit_pricing.net_credit,
            entry_surface_time,
            exit_surface_time: Some(exit_surface_time),
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
            spot_at_entry: entry_spot.to_f64(),
            spot_at_exit: exit_spot.to_f64(),
            spot_move,
            spot_move_pct,
            breakeven_up,
            breakeven_down,
            within_breakeven,
            success: true,
            failure_reason: None,
        })
    }

    fn create_failed_result(
        &self,
        butterfly: &IronButterfly,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        error: ExecutionError,
    ) -> IronButterflyResult {
        let failure_reason = match error {
            ExecutionError::NoSpotPrice => FailureReason::NoSpotPrice,
            ExecutionError::Repository(_) => FailureReason::NoOptionsData,
            ExecutionError::Pricing(_) => FailureReason::PricingError(error.to_string()),
            ExecutionError::InvalidSpread(_) => FailureReason::DegenerateSpread,
        };

        IronButterflyResult {
            symbol: butterfly.symbol().to_string(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            center_strike: butterfly.center_strike(),
            upper_strike: butterfly.upper_strike(),
            lower_strike: butterfly.lower_strike(),
            expiration: butterfly.expiration(),
            wing_width: butterfly.wing_width(),
            entry_time,
            short_call_entry: Decimal::ZERO,
            short_put_entry: Decimal::ZERO,
            long_call_entry: Decimal::ZERO,
            long_put_entry: Decimal::ZERO,
            entry_credit: Decimal::ZERO,
            exit_time,
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
        }
    }
}

fn compute_net_greeks(pricing: &IronButterflyPricing) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    match (
        pricing.short_call.greeks,
        pricing.short_put.greeks,
        pricing.long_call.greeks,
        pricing.long_put.greeks,
    ) {
        (Some(sc), Some(sp), Some(lc), Some(lp)) => {
            // Net greeks for short iron butterfly
            // Short positions: negate greeks
            let net_delta = -sc.delta - sp.delta + lc.delta + lp.delta;
            let net_gamma = -sc.gamma - sp.gamma + lc.gamma + lp.gamma;
            let net_theta = -sc.theta - sp.theta + lc.theta + lp.theta;
            let net_vega = -sc.vega - sp.vega + lc.vega + lp.vega;

            (Some(net_delta), Some(net_gamma), Some(net_theta), Some(net_vega))
        }
        _ => (None, None, None, None),
    }
}

/// Calculate P&L attribution for a single leg (DEPRECATED - use domain function)
///
/// This is kept for backwards compatibility but should use cs_domain::calculate_option_leg_pnl
#[deprecated(note = "Use cs_domain::calculate_option_leg_pnl instead")]
fn calculate_leg_attribution(
    entry_leg: &LegPricing,
    exit_leg: &LegPricing,
    spot_change: f64,
    days_held: f64,
    leg_sign: f64, // +1 for long, -1 for short
) -> (f64, f64, f64, f64) {
    let pnl = cs_domain::calculate_option_leg_pnl(
        entry_leg.greeks.as_ref(),
        entry_leg.iv,
        exit_leg.iv,
        spot_change,
        days_held,
        leg_sign,
    );
    (pnl.delta, pnl.gamma, pnl.theta, pnl.vega)
}

fn calculate_pnl_attribution(
    entry_pricing: &IronButterflyPricing,
    exit_pricing: &IronButterflyPricing,
    entry_spot: f64,
    exit_spot: f64,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    total_pnl: Decimal,
) -> (Option<Decimal>, Option<Decimal>, Option<Decimal>, Option<Decimal>, Option<Decimal>) {
    let spot_change = exit_spot - entry_spot;
    let days_held = (exit_time - entry_time).num_hours() as f64 / 24.0;

    // Calculate attribution for each leg
    // Short legs: we SOLD them (negative position, sign = -1)
    // Long legs: we BOUGHT them (positive position, sign = +1)
    let (sc_delta, sc_gamma, sc_theta, sc_vega) = calculate_leg_attribution(
        &entry_pricing.short_call,
        &exit_pricing.short_call,
        spot_change,
        days_held,
        -1.0, // Short position
    );

    let (sp_delta, sp_gamma, sp_theta, sp_vega) = calculate_leg_attribution(
        &entry_pricing.short_put,
        &exit_pricing.short_put,
        spot_change,
        days_held,
        -1.0, // Short position
    );

    let (lc_delta, lc_gamma, lc_theta, lc_vega) = calculate_leg_attribution(
        &entry_pricing.long_call,
        &exit_pricing.long_call,
        spot_change,
        days_held,
        1.0, // Long position
    );

    let (lp_delta, lp_gamma, lp_theta, lp_vega) = calculate_leg_attribution(
        &entry_pricing.long_put,
        &exit_pricing.long_put,
        spot_change,
        days_held,
        1.0, // Long position
    );

    // Sum across all legs
    let delta_pnl = sc_delta + sp_delta + lc_delta + lp_delta;
    let gamma_pnl = sc_gamma + sp_gamma + lc_gamma + lp_gamma;
    let theta_pnl = sc_theta + sp_theta + lc_theta + lp_theta;
    let vega_pnl = sc_vega + sp_vega + lc_vega + lp_vega;

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
