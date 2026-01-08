//! Trade Simulator - Encapsulated trade simulation workflow
//!
//! This module provides a simulation-based approach to trade execution that:
//! - Encapsulates common simulation parameters into a single struct
//! - Extracts common data preparation (chain, surface, spot) into one method
//! - Provides a generic run() method that returns raw simulation data
//!
//! The simulator is agnostic of business context (earnings events, hedging).
//! It returns raw simulation data that the caller enriches as needed.

use chrono::{DateTime, Utc};
use cs_domain::*;
use cs_analytics::IVSurface;
use crate::execution::{ExecutionConfig, ExecutableTrade, TradePricer, SimulationOutput, ExecutionError};
use crate::iv_surface_builder::build_iv_surface_minute_aligned;

/// Prepared market data for trade selection
pub struct PreparedData {
    pub spot: SpotPrice,
    pub surface: IVSurface,
}

/// Raw simulation output before enrichment
///
/// Contains the pricing data and simulation metadata. The caller can:
/// - Construct a result with `T::to_result()` if they have an earnings event
/// - Use the raw data for rolling/non-earnings scenarios
/// - Apply hedging before or after result construction
pub struct RawSimulationOutput<P> {
    pub entry_pricing: P,
    pub exit_pricing: P,
    pub output: SimulationOutput,
}

/// Encapsulates all parameters needed for trade simulation
///
/// Instead of passing many arguments to every function, create a simulator once
/// and reuse it for the prepare/select/run workflow.
///
/// The simulator only needs the symbol and market data - it doesn't care about
/// business context (earnings events, hedging). The caller handles enrichment.
///
/// Strategy-specific parameters (option_type, wing_width, etc.) should be
/// captured in the selection closure.
pub struct TradeSimulator<'a> {
    pub options_repo: &'a dyn OptionsDataRepository,
    pub equity_repo: &'a dyn EquityDataRepository,
    pub symbol: &'a str,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub config: &'a ExecutionConfig,
}

impl<'a> TradeSimulator<'a> {
    /// Create a new trade simulator
    pub fn new(
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        symbol: &'a str,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        config: &'a ExecutionConfig,
    ) -> Self {
        Self {
            options_repo,
            equity_repo,
            symbol,
            entry_time,
            exit_time,
            config,
        }
    }

    /// Prepare market data for trade selection
    ///
    /// Fetches option chain, builds IV surface, and gets spot price.
    /// This is the common setup needed by all trade types.
    pub async fn prepare(&self) -> Option<PreparedData> {
        // Get option chain at entry time
        let entry_chain = self.options_repo
            .get_option_bars_at_time(self.symbol, self.entry_time)
            .await
            .ok()?;

        // Build IV surface from chain
        let surface = build_iv_surface_minute_aligned(
            &entry_chain,
            self.equity_repo,
            self.symbol,
        ).await?;

        // Get spot price at entry time
        let spot = self.equity_repo
            .get_spot_price(self.symbol, self.entry_time)
            .await
            .ok()?;

        Some(PreparedData { spot, surface })
    }

    /// Run simulation and return raw data
    ///
    /// Returns raw simulation output that can be enriched by the caller.
    /// This is the core simulation: price entry, validate, price exit.
    ///
    /// # Returns
    /// - `Ok(RawSimulationOutput)` - Entry/exit pricing and simulation metadata
    /// - `Err(ExecutionError)` - If simulation failed (data missing, validation failed, etc.)
    pub async fn run<T>(
        &self,
        trade: &T,
        pricer: &T::Pricer,
    ) -> Result<RawSimulationOutput<T::Pricing>, ExecutionError>
    where
        T: ExecutableTrade,
    {
        // 1. Get spot prices
        let entry_spot = self.equity_repo
            .get_spot_price(trade.symbol(), self.entry_time)
            .await?;
        let exit_spot = self.equity_repo
            .get_spot_price(trade.symbol(), self.exit_time)
            .await?;

        // 2. Get option chains
        let entry_chain = self.options_repo
            .get_option_bars_at_time(trade.symbol(), self.entry_time)
            .await?;
        let (exit_chain, exit_surface_time) = self.options_repo
            .get_option_bars_at_or_after_time(trade.symbol(), self.exit_time, 30)
            .await?;

        // 3. Build IV surfaces with per-option spot prices (minute-aligned)
        let entry_surface = build_iv_surface_minute_aligned(
            &entry_chain,
            self.equity_repo,
            trade.symbol(),
        )
        .await;
        let entry_surface_time = entry_surface.as_ref().map(|s| s.as_of_time());

        let exit_surface = build_iv_surface_minute_aligned(
            &exit_chain,
            self.equity_repo,
            trade.symbol(),
        )
        .await;

        // 4. Price at entry
        let entry_pricing = pricer.price_with_surface(
            trade,
            &entry_chain,
            entry_spot.to_f64(),
            self.entry_time,
            entry_surface.as_ref(),
        )?;

        // 5. Validate entry
        T::validate_entry(&entry_pricing, self.config)?;

        // 6. Price at exit
        let exit_pricing = pricer.price_with_surface(
            trade,
            &exit_chain,
            exit_spot.to_f64(),
            self.exit_time,
            exit_surface.as_ref(),
        )?;

        // 7. Return raw simulation output
        let output = SimulationOutput::new(
            self.entry_time,
            self.exit_time,
            entry_spot.to_f64(),
            exit_spot.to_f64(),
            entry_surface_time,
            exit_surface_time,
        );

        Ok(RawSimulationOutput {
            entry_pricing,
            exit_pricing,
            output,
        })
    }

    /// Helper to create a failed simulation output
    pub fn failed_output(&self) -> SimulationOutput {
        SimulationOutput::new(
            self.entry_time,
            self.exit_time,
            0.0,
            0.0,
            None,
            self.entry_time, // dummy
        )
    }
}
