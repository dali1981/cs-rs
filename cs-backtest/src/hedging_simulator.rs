//! Hedging Simulator - Trade simulation with integrated hedging
//!
//! This module provides a simulation function that integrates hedging directly
//! into the trade execution loop. Hedging happens DURING the trade lifetime:
//!
//! ```text
//! Timeline:
//! ─────────────────────────────────────────────────────────────────────►
//! │                                                                     │
//! ENTRY                        REHEDGES                              EXIT
//! │                                                                     │
//! ├── Get spot/surface         ├── Get spot                       ├── Get spot/surface
//! ├── Price entry              ├── Compute delta                  ├── Price exit
//! ├── Init hedge (Δ=0)         ├── Adjust hedge if needed         ├── Finalize hedge
//! │                            ├── Track P&L                       │
//! │                            │                                   │
//! t₀                          t₁, t₂, t₃, ...                      tₙ
//! ```
//!
//! # Usage
//!
//! ```ignore
//! // In any strategy's execute_trade:
//! let sim = simulate_with_hedging(
//!     &trade,
//!     &pricer,
//!     options_repo,
//!     equity_repo,
//!     entry_time,
//!     exit_time,
//!     exec_config.hedge_config.as_ref(),
//!     self.timing(),
//! ).await?;
//!
//! // sim.hedge_position contains the hedge P&L
//! ```

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use cs_domain::{
    CompositeTrade, EquityDataRepository, OptionsDataRepository,
    HedgeConfig, HedgePosition, DeltaComputation, GenericHedgeState,
};

use crate::delta_providers::{GammaApproximationProvider, EntryVolatilityProvider};
use crate::execution::{ExecutableTrade, TradePricer, ExecutionError};
use crate::iv_surface_builder::build_iv_surface_minute_aligned;
use crate::timing_strategy::TimingStrategy;

/// Output from simulation with integrated hedging
#[derive(Debug)]
pub struct HedgedSimulationOutput<P> {
    /// Pricing at entry
    pub entry_pricing: P,
    /// Pricing at exit
    pub exit_pricing: P,
    /// Spot price at entry
    pub entry_spot: f64,
    /// Spot price at exit
    pub exit_spot: f64,
    /// Entry timestamp
    pub entry_time: DateTime<Utc>,
    /// Exit timestamp
    pub exit_time: DateTime<Utc>,
    /// Entry surface timestamp (may differ from entry_time if data not available)
    pub entry_surface_time: Option<DateTime<Utc>>,
    /// Exit surface timestamp
    pub exit_surface_time: DateTime<Utc>,
    /// Hedge position with P&L (None if hedging disabled)
    pub hedge_position: Option<HedgePosition>,
}

/// Precomputed entry pricing inputs for hedged simulation
#[derive(Debug)]
pub struct EntryPricingContext<P> {
    pub pricing: P,
    pub spot: f64,
    pub surface_time: Option<DateTime<Utc>>,
}

impl<P> HedgedSimulationOutput<P> {
    /// Get hedge P&L (0 if no hedging)
    pub fn hedge_pnl(&self) -> Decimal {
        self.hedge_position
            .as_ref()
            .map(|pos| pos.calculate_pnl(self.exit_spot))
            .unwrap_or(Decimal::ZERO)
    }

    /// Get total hedge transaction costs
    pub fn hedge_costs(&self) -> Decimal {
        self.hedge_position
            .as_ref()
            .map(|pos| pos.total_cost)
            .unwrap_or(Decimal::ZERO)
    }

    /// Number of rehedges performed
    pub fn rehedge_count(&self) -> usize {
        self.hedge_position
            .as_ref()
            .map(|pos| pos.rehedge_count())
            .unwrap_or(0)
    }
}

/// Simulate a trade with integrated hedging
///
/// This is THE simulation function - hedging happens during execution,
/// not as post-processing. The flow is:
///
/// 1. ENTRY: Get spot, build surface, price entry, initialize hedge
/// 2. HEDGING LOOP: For each rehedge time, get spot, update hedge state
/// 3. EXIT: Get spot, build surface, price exit, finalize hedge
///
/// # Type Parameters
/// * `T` - Trade type implementing ExecutableTrade and CompositeTrade
/// * `Pr` - Pricer type implementing TradePricer
///
/// # Arguments
/// * `trade` - The trade to simulate
/// * `pricer` - Pricer for the trade type
/// * `options_repo` - Options data repository
/// * `equity_repo` - Equity data repository
/// * `entry_time` - Entry timestamp
/// * `exit_time` - Exit timestamp
/// * `hedge_config` - Hedging configuration (None = no hedging)
/// * `timing` - Timing strategy (provides rehedge schedule)
///
/// # Returns
/// `HedgedSimulationOutput` with pricing and hedge position
pub async fn simulate_with_hedging<T, Pr>(
    trade: &T,
    pricer: &Pr,
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    hedge_config: Option<&HedgeConfig>,
    timing: &TimingStrategy,
) -> Result<HedgedSimulationOutput<Pr::Pricing>, ExecutionError>
where
    T: ExecutableTrade<Pricer = Pr> + CompositeTrade + Clone + Send + Sync,
    Pr: TradePricer<Trade = T>,
    Pr::Pricing: HasDelta + HasGamma + HasIV + Clone,
{
    // Use fully qualified syntax to disambiguate - prefer ExecutableTrade::symbol
    let symbol = ExecutableTrade::symbol(trade);

    // =========================================================================
    // PHASE 1: ENTRY
    // =========================================================================

    // Get spot at entry
    let entry_spot = equity_repo
        .get_spot_price(symbol, entry_time)
        .await?;
    let entry_spot_f64 = entry_spot.to_f64();

    // Get option chain at entry
    let entry_chain = options_repo
        .get_option_bars_at_time(symbol, entry_time)
        .await?;

    // Build IV surface
    let entry_surface = build_iv_surface_minute_aligned(
        &entry_chain,
        equity_repo,
        symbol,
    ).await;
    let entry_surface_time = entry_surface.as_ref().map(|s| s.as_of_time());

    // Price at entry
    let entry_pricing = pricer.price_with_surface(
        trade,
        &entry_chain,
        entry_spot_f64,
        entry_time,
        entry_surface.as_ref(),
    )?;
    let entry_context = EntryPricingContext {
        pricing: entry_pricing,
        spot: entry_spot_f64,
        surface_time: entry_surface_time,
    };

    simulate_with_hedging_prepriced(
        trade,
        pricer,
        options_repo,
        equity_repo,
        entry_time,
        exit_time,
        hedge_config,
        timing,
        entry_context,
    ).await
}

/// Simulate with precomputed entry pricing (skips entry pricing pass).
pub async fn simulate_with_hedging_prepriced<T, Pr>(
    trade: &T,
    pricer: &Pr,
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    hedge_config: Option<&HedgeConfig>,
    timing: &TimingStrategy,
    entry: EntryPricingContext<Pr::Pricing>,
) -> Result<HedgedSimulationOutput<Pr::Pricing>, ExecutionError>
where
    T: ExecutableTrade<Pricer = Pr> + CompositeTrade + Clone + Send + Sync,
    Pr: TradePricer<Trade = T>,
    Pr::Pricing: HasDelta + HasGamma + HasIV + Clone,
{
    let symbol = ExecutableTrade::symbol(trade);
    let EntryPricingContext {
        pricing: entry_pricing,
        spot: entry_spot_f64,
        surface_time: entry_surface_time,
    } = entry;

    // =========================================================================
    // PHASE 2: HEDGING LOOP (if configured)
    // =========================================================================

    let hedge_position = if let Some(config) = hedge_config {
        if config.is_enabled() {
            // Get entry Greeks from pricing for hedge initialization
            let entry_delta = get_pricing_delta(&entry_pricing);
            let entry_gamma = get_pricing_gamma(&entry_pricing);
            let entry_iv = get_pricing_iv(&entry_pricing);

            tracing::debug!(
                symbol = %symbol,
                entry_delta = entry_delta,
                entry_gamma = entry_gamma,
                entry_iv = ?entry_iv,
                delta_mode = ?config.delta_computation,
                "Initializing hedge state"
            );

            // Create delta provider based on mode
            let hedge_result = match &config.delta_computation {
                DeltaComputation::GammaApproximation => {
                    let provider = GammaApproximationProvider::new(
                        entry_delta,
                        entry_gamma,
                        entry_spot_f64,
                    );
                    run_hedge_loop(
                        config,
                        provider,
                        equity_repo,
                        symbol,
                        entry_time,
                        exit_time,
                        entry_spot_f64,
                        timing,
                        entry_iv,
                    ).await
                }

                DeltaComputation::EntryIV { .. } => {
                    let iv = entry_iv.unwrap_or(0.30); // Default 30% if not available
                    let provider = EntryVolatilityProvider::new_entry_iv(
                        trade.clone(),
                        iv,
                        0.05, // risk-free rate
                    );
                    run_hedge_loop(
                        config,
                        provider,
                        equity_repo,
                        symbol,
                        entry_time,
                        exit_time,
                        entry_spot_f64,
                        timing,
                        entry_iv,
                    ).await
                }

                DeltaComputation::EntryHV { window } => {
                    // Compute HV at entry
                    let entry_hv = compute_hv_at_time(
                        equity_repo,
                        symbol,
                        entry_time,
                        *window,
                    ).await.unwrap_or(0.25); // Default 25% if computation fails

                    let provider = EntryVolatilityProvider::new_entry_hv(
                        trade.clone(),
                        entry_hv,
                        0.05,
                    );
                    run_hedge_loop(
                        config,
                        provider,
                        equity_repo,
                        symbol,
                        entry_time,
                        exit_time,
                        entry_spot_f64,
                        timing,
                        entry_iv,
                    ).await
                }

                // CurrentHV, CurrentMarketIV, HistoricalAverageIV - fall back to gamma approx
                _ => {
                    tracing::warn!(
                        symbol = %symbol,
                        mode = ?config.delta_computation,
                        "Delta mode requires Arc repos, falling back to GammaApproximation"
                    );
                    let provider = GammaApproximationProvider::new(
                        entry_delta,
                        entry_gamma,
                        entry_spot_f64,
                    );
                    run_hedge_loop(
                        config,
                        provider,
                        equity_repo,
                        symbol,
                        entry_time,
                        exit_time,
                        entry_spot_f64,
                        timing,
                        entry_iv,
                    ).await
                }
            };

            match hedge_result {
                Ok(pos) => Some(pos),
                Err(e) => {
                    tracing::warn!(
                        symbol = %symbol,
                        error = %e,
                        "Hedging failed, continuing without hedge"
                    );
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // =========================================================================
    // PHASE 3: EXIT
    // =========================================================================

    if exit_time == entry_time {
        let entry_pricing_clone = entry_pricing.clone();
        return Ok(HedgedSimulationOutput {
            entry_pricing: entry_pricing_clone,
            exit_pricing: entry_pricing,
            entry_spot: entry_spot_f64,
            exit_spot: entry_spot_f64,
            entry_time,
            exit_time,
            entry_surface_time,
            exit_surface_time: entry_surface_time.unwrap_or(entry_time),
            hedge_position,
        });
    }

    // Get spot at exit
    let exit_spot = equity_repo
        .get_spot_price(symbol, exit_time)
        .await?;
    let exit_spot_f64 = exit_spot.to_f64();

    // Get option chain at exit (with tolerance for timing)
    let (exit_chain, exit_surface_time) = options_repo
        .get_option_bars_at_or_after_time(symbol, exit_time, 30)
        .await?;

    // Build exit IV surface
    let exit_surface = build_iv_surface_minute_aligned(
        &exit_chain,
        equity_repo,
        symbol,
    ).await;

    // Price at exit
    let exit_pricing = pricer.price_with_surface(
        trade,
        &exit_chain,
        exit_spot_f64,
        exit_time,
        exit_surface.as_ref(),
    )?;

    // Log summary
    if let Some(ref pos) = hedge_position {
        let hedge_pnl = pos.calculate_pnl(exit_spot_f64);
        tracing::info!(
            symbol = %symbol,
            rehedges = pos.rehedge_count(),
            hedge_pnl = %hedge_pnl,
            total_cost = %pos.total_cost,
            "Simulation with hedging complete"
        );
    }

    Ok(HedgedSimulationOutput {
        entry_pricing,
        exit_pricing,
        entry_spot: entry_spot_f64,
        exit_spot: exit_spot_f64,
        entry_time,
        exit_time,
        entry_surface_time,
        exit_surface_time,
        hedge_position,
    })
}

/// Run the hedging loop with a specific delta provider
async fn run_hedge_loop<P: cs_domain::DeltaProvider>(
    config: &HedgeConfig,
    provider: P,
    equity_repo: &dyn EquityDataRepository,
    symbol: &str,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    entry_spot: f64,
    timing: &TimingStrategy,
    entry_iv: Option<f64>,
) -> Result<HedgePosition, String> {
    // Initialize hedge state
    let mut state = GenericHedgeState::new(
        config.clone(),
        provider,
        entry_spot,
        false, // attribution not yet supported
    );

    // Generate rehedge schedule
    let rehedge_times = timing.rehedge_times(entry_time, exit_time, &config.strategy);

    tracing::debug!(
        symbol = %symbol,
        rehedge_count = rehedge_times.len(),
        "Running hedge loop"
    );

    // Iterate through rehedge times
    for rehedge_time in rehedge_times {
        if state.at_max_rehedges() {
            tracing::debug!(symbol = %symbol, "Max rehedges reached");
            break;
        }

        // Get spot at rehedge time
        let spot = match equity_repo.get_spot_price(symbol, rehedge_time).await {
            Ok(s) => s.to_f64(),
            Err(e) => {
                tracing::warn!(
                    symbol = %symbol,
                    time = %rehedge_time,
                    error = %e,
                    "Failed to get spot for rehedge, skipping"
                );
                continue;
            }
        };

        // Update hedge state
        if let Err(e) = state.update(rehedge_time, spot).await {
            tracing::warn!(
                symbol = %symbol,
                time = %rehedge_time,
                error = %e,
                "Hedge update failed, skipping"
            );
        }
    }

    // Finalize with exit spot
    let exit_spot = equity_repo
        .get_spot_price(symbol, exit_time)
        .await
        .map(|s| s.to_f64())
        .unwrap_or(entry_spot);

    // TODO: Get exit IV for RV metrics
    let exit_iv: Option<f64> = None;

    Ok(state.finalize(exit_spot, entry_iv, exit_iv))
}

/// Compute historical volatility at a specific time using intraday minute bars
async fn compute_hv_at_time(
    equity_repo: &dyn EquityDataRepository,
    symbol: &str,
    time: DateTime<Utc>,
    window: u32,
) -> Result<f64, String> {
    use cs_analytics::realized_volatility;

    let date = time.date_naive();

    let bars = equity_repo
        .get_bars(symbol, date)
        .await
        .map_err(|e| format!("Failed to get bars: {}", e))?;

    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();

    realized_volatility(&closes, window as usize, 252.0)
        .ok_or_else(|| "Insufficient data for HV computation".to_string())
}

// =============================================================================
// Helper functions to extract Greeks from pricing
// =============================================================================

/// Extract delta from pricing (works with any pricing type that has Greeks)
fn get_pricing_delta<P>(pricing: &P) -> f64
where
    P: HasDelta,
{
    pricing.net_delta()
}

/// Extract gamma from pricing
fn get_pricing_gamma<P>(pricing: &P) -> f64
where
    P: HasGamma,
{
    pricing.net_gamma()
}

/// Extract IV from pricing
fn get_pricing_iv<P>(pricing: &P) -> Option<f64>
where
    P: HasIV,
{
    pricing.primary_iv()
}

/// Trait for types that have delta
pub trait HasDelta {
    fn net_delta(&self) -> f64;
}

/// Trait for types that have gamma
pub trait HasGamma {
    fn net_gamma(&self) -> f64;
}

/// Trait for types that have IV
pub trait HasIV {
    fn primary_iv(&self) -> Option<f64>;
}

// Implement for common pricing types
use crate::straddle_pricer::StraddlePricing;
use crate::composite_pricer::CompositePricing;

impl HasDelta for StraddlePricing {
    fn net_delta(&self) -> f64 {
        self.call.greeks.as_ref().map(|g| g.delta).unwrap_or(0.0)
            + self.put.greeks.as_ref().map(|g| g.delta).unwrap_or(0.0)
    }
}

impl HasGamma for StraddlePricing {
    fn net_gamma(&self) -> f64 {
        self.call.greeks.as_ref().map(|g| g.gamma).unwrap_or(0.0)
            + self.put.greeks.as_ref().map(|g| g.gamma).unwrap_or(0.0)
    }
}

impl HasIV for StraddlePricing {
    fn primary_iv(&self) -> Option<f64> {
        // Average of call and put IV (both are Option<f64>)
        match (self.call.iv, self.put.iv) {
            (Some(call_iv), Some(put_iv)) if call_iv > 0.0 && put_iv > 0.0 => {
                Some((call_iv + put_iv) / 2.0)
            }
            (Some(call_iv), _) if call_iv > 0.0 => Some(call_iv),
            (_, Some(put_iv)) if put_iv > 0.0 => Some(put_iv),
            _ => None,
        }
    }
}

impl HasDelta for CompositePricing {
    fn net_delta(&self) -> f64 {
        // CompositePricing.legs is Vec<(LegPricing, LegPosition)>
        // leg.0 = LegPricing, leg.1 = LegPosition
        self.legs.iter()
            .map(|(pricing, position)| {
                let delta = pricing.greeks.as_ref().map(|g| g.delta).unwrap_or(0.0);
                match position {
                    cs_domain::LegPosition::Long => delta,
                    cs_domain::LegPosition::Short => -delta,
                }
            })
            .sum()
    }
}

impl HasGamma for CompositePricing {
    fn net_gamma(&self) -> f64 {
        self.legs.iter()
            .map(|(pricing, position)| {
                let gamma = pricing.greeks.as_ref().map(|g| g.gamma).unwrap_or(0.0);
                match position {
                    cs_domain::LegPosition::Long => gamma,
                    cs_domain::LegPosition::Short => -gamma,
                }
            })
            .sum()
    }
}

impl HasIV for CompositePricing {
    fn primary_iv(&self) -> Option<f64> {
        // Average IV across all legs
        // LegPricing.iv is Option<f64>
        let ivs: Vec<f64> = self.legs.iter()
            .filter_map(|(pricing, _)| pricing.iv)
            .filter(|iv| *iv > 0.0)
            .collect();

        if ivs.is_empty() {
            None
        } else {
            Some(ivs.iter().sum::<f64>() / ivs.len() as f64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hedged_output_defaults() {
        let output: HedgedSimulationOutput<()> = HedgedSimulationOutput {
            entry_pricing: (),
            exit_pricing: (),
            entry_spot: 100.0,
            exit_spot: 105.0,
            entry_time: Utc::now(),
            exit_time: Utc::now(),
            entry_surface_time: None,
            exit_surface_time: Utc::now(),
            hedge_position: None,
        };

        assert_eq!(output.hedge_pnl(), Decimal::ZERO);
        assert_eq!(output.hedge_costs(), Decimal::ZERO);
        assert_eq!(output.rehedge_count(), 0);
    }
}
