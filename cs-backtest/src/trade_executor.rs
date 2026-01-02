use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_analytics::PricingModel;
use cs_domain::{
    CalendarSpread, CalendarSpreadResult, EarningsEvent, FailureReason,
    EquityDataRepository, OptionsDataRepository, RepositoryError, MarketTime,
};
use crate::spread_pricer::{SpreadPricer, PricingError};

/// Error type for trade execution
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
    #[error("Pricing error: {0}")]
    Pricing(#[from] PricingError),
    #[error("No spot price available")]
    NoSpotPrice,
    #[error("Invalid spread: {0}")]
    InvalidSpread(String),
}

/// Executes individual trades (entry and exit)
pub struct TradeExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    pricer: SpreadPricer,
    max_entry_iv: Option<f64>,
}

impl<O, E> TradeExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(options_repo: Arc<O>, equity_repo: Arc<E>) -> Self {
        Self {
            options_repo,
            equity_repo,
            pricer: SpreadPricer::new(),
            max_entry_iv: None,
        }
    }

    pub fn with_market_close(mut self, market_close: MarketTime) -> Self {
        self.pricer = self.pricer.with_market_close(market_close);
        self
    }

    /// Set the pricing IV interpolation model
    ///
    /// - `StickyStrike`: IV indexed by absolute strike K (default)
    /// - `StickyMoneyness`: IV indexed by K/S (floats with spot)
    /// - `StickyDelta`: IV indexed by delta (iterative, most accurate floating smile)
    pub fn with_pricing_model(mut self, pricing_model: PricingModel) -> Self {
        self.pricer = self.pricer.with_pricing_model(pricing_model);
        self
    }

    /// Get the current pricing model
    pub fn pricing_model(&self) -> PricingModel {
        self.pricer.pricing_model()
    }

    /// Set maximum allowed IV at entry (filters trades with unreliable pricing)
    pub fn with_max_entry_iv(mut self, max_entry_iv: Option<f64>) -> Self {
        self.max_entry_iv = max_entry_iv;
        self
    }

    /// Execute a complete trade (entry + exit)
    pub async fn execute_trade(
        &self,
        spread: &CalendarSpread,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> CalendarSpreadResult {
        // Try to execute, catch errors and convert to failed result
        match self.try_execute_trade(spread, event, entry_time, exit_time).await {
            Ok(result) => result,
            Err(e) => self.create_failed_result(spread, event, entry_time, exit_time, e),
        }
    }

    async fn try_execute_trade(
        &self,
        spread: &CalendarSpread,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Result<CalendarSpreadResult, ExecutionError> {
        // Get entry spot price
        let entry_spot = self.equity_repo
            .get_spot_price(spread.symbol(), entry_time)
            .await?;

        // Get exit spot price
        let exit_spot = self.equity_repo
            .get_spot_price(spread.symbol(), exit_time)
            .await?;

        // Get option chain data for entry
        let entry_chain = self.options_repo
            .get_option_bars(spread.symbol(), entry_time.date_naive())
            .await?;

        // Get option chain data for exit
        let exit_chain = self.options_repo
            .get_option_bars(spread.symbol(), exit_time.date_naive())
            .await?;

        // Price at entry
        let entry_pricing = self.pricer.price_spread(
            spread,
            &entry_chain,
            entry_spot.to_f64(),
            entry_time,
        )?;

        // Validate: Filter trades with extreme IV at entry (unreliable pricing/greeks)
        if let Some(max_iv) = self.max_entry_iv {
            if let Some(short_iv) = entry_pricing.short_leg.iv {
                if short_iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "Short leg IV too high: {:.1}% > {:.1}% (unreliable pricing)",
                        short_iv * 100.0,
                        max_iv * 100.0,
                    )));
                }
            }
            if let Some(long_iv) = entry_pricing.long_leg.iv {
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
        if entry_pricing.net_cost <= Decimal::ZERO {
            return Err(ExecutionError::InvalidSpread(format!(
                "Negative entry cost: {} (short={}, long={})",
                entry_pricing.net_cost,
                entry_pricing.short_leg.price,
                entry_pricing.long_leg.price,
            )));
        }

        // Validate: Entry cost must be reasonable (avoid division by near-zero in pnl_pct)
        let min_entry_cost = Decimal::new(5, 2); // $0.05
        if entry_pricing.net_cost < min_entry_cost {
            return Err(ExecutionError::InvalidSpread(format!(
                "Entry cost too small: {} < {} (short={}, long={})",
                entry_pricing.net_cost,
                min_entry_cost,
                entry_pricing.short_leg.price,
                entry_pricing.long_leg.price,
            )));
        }

        // Price at exit
        let exit_pricing = self.pricer.price_spread(
            spread,
            &exit_chain,
            exit_spot.to_f64(),
            exit_time,
        )?;

        // Calculate P&L
        let pnl = exit_pricing.net_cost - entry_pricing.net_cost;
        let pnl_pct = if entry_pricing.net_cost != Decimal::ZERO {
            (pnl / entry_pricing.net_cost) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // P&L attribution
        let (delta_pnl, gamma_pnl, theta_pnl, vega_pnl, unexplained_pnl) =
            if let (Some(entry_greeks_short), Some(entry_greeks_long)) =
                (entry_pricing.short_leg.greeks, entry_pricing.long_leg.greeks)
            {
                let spot_change = exit_spot.to_f64() - entry_spot.to_f64();

                // Calculate IV changes for BOTH legs
                let short_iv_change = match (
                    exit_pricing.short_leg.iv,
                    entry_pricing.short_leg.iv,
                ) {
                    (Some(exit_iv), Some(entry_iv)) => exit_iv - entry_iv,
                    _ => 0.0,
                };

                let long_iv_change = match (
                    exit_pricing.long_leg.iv,
                    entry_pricing.long_leg.iv,
                ) {
                    (Some(exit_iv), Some(entry_iv)) => exit_iv - entry_iv,
                    _ => 0.0,
                };

                let days_held = (exit_time - entry_time).num_hours() as f64 / 24.0;

                // Use the corrected spread attribution function
                let attribution = cs_domain::calculate_spread_pnl_attribution(
                    &entry_greeks_short,
                    &entry_greeks_long,
                    spot_change,
                    short_iv_change,
                    long_iv_change,
                    days_held,
                    pnl,
                );

                (
                    Some(attribution.delta),
                    Some(attribution.gamma),
                    Some(attribution.theta),
                    Some(attribution.vega),
                    Some(attribution.unexplained),
                )
            } else {
                (None, None, None, None, None)
            };

        Ok(CalendarSpreadResult {
            symbol: spread.symbol().to_string(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            strike: spread.strike(),
            long_strike: if spread.short_leg.strike != spread.long_leg.strike {
                Some(spread.long_leg.strike)
            } else {
                None
            },
            option_type: spread.option_type(),
            short_expiry: spread.short_expiry(),
            long_expiry: spread.long_expiry(),
            entry_time,
            short_entry_price: entry_pricing.short_leg.price,
            long_entry_price: entry_pricing.long_leg.price,
            entry_cost: entry_pricing.net_cost,
            exit_time,
            short_exit_price: exit_pricing.short_leg.price,
            long_exit_price: exit_pricing.long_leg.price,
            exit_value: exit_pricing.net_cost,
            pnl,
            pnl_per_contract: pnl,
            pnl_pct,
            short_delta: entry_pricing.short_leg.greeks.map(|g| g.delta),
            short_gamma: entry_pricing.short_leg.greeks.map(|g| g.gamma),
            short_theta: entry_pricing.short_leg.greeks.map(|g| g.theta),
            short_vega: entry_pricing.short_leg.greeks.map(|g| g.vega),
            long_delta: entry_pricing.long_leg.greeks.map(|g| g.delta),
            long_gamma: entry_pricing.long_leg.greeks.map(|g| g.gamma),
            long_theta: entry_pricing.long_leg.greeks.map(|g| g.theta),
            long_vega: entry_pricing.long_leg.greeks.map(|g| g.vega),
            iv_short_entry: entry_pricing.short_leg.iv,
            iv_long_entry: entry_pricing.long_leg.iv,
            iv_short_exit: exit_pricing.short_leg.iv,
            iv_long_exit: exit_pricing.long_leg.iv,
            iv_ratio_entry: match (entry_pricing.short_leg.iv, entry_pricing.long_leg.iv) {
                (Some(short_iv), Some(long_iv)) if long_iv > 0.0 => Some(short_iv / long_iv),
                _ => None,
            },
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

    fn create_failed_result(
        &self,
        spread: &CalendarSpread,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        error: ExecutionError,
    ) -> CalendarSpreadResult {
        let failure_reason = match error {
            ExecutionError::NoSpotPrice => FailureReason::NoSpotPrice,
            ExecutionError::Repository(_) => FailureReason::NoOptionsData,
            ExecutionError::Pricing(_) => FailureReason::PricingError(error.to_string()),
            ExecutionError::InvalidSpread(_) => FailureReason::DegenerateSpread,
        };

        CalendarSpreadResult {
            symbol: spread.symbol().to_string(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            strike: spread.strike(),
            long_strike: if spread.short_leg.strike != spread.long_leg.strike {
                Some(spread.long_leg.strike)
            } else {
                None
            },
            option_type: spread.option_type(),
            short_expiry: spread.short_expiry(),
            long_expiry: spread.long_expiry(),
            entry_time,
            short_entry_price: Decimal::ZERO,
            long_entry_price: Decimal::ZERO,
            entry_cost: Decimal::ZERO,
            exit_time,
            short_exit_price: Decimal::ZERO,
            long_exit_price: Decimal::ZERO,
            exit_value: Decimal::ZERO,
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
        }
    }
}
