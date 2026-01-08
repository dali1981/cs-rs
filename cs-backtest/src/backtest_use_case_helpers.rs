//! Trade Execution Context - Encapsulated trade selection and execution
//!
//! This module provides a context-based approach to trade execution that:
//! - Encapsulates the common parameters into a single struct
//! - Extracts common data preparation (chain, surface, spot) into one method
//! - Provides a generic execute() method with closure-based trade selection

use chrono::{DateTime, Utc};
use cs_domain::*;
use cs_analytics::IVSurface;
use crate::execution::{execute_trade, ExecutionConfig, ExecutableTrade, TradePricer};
use crate::iv_surface_builder::build_iv_surface_minute_aligned;

/// Prepared market data for trade selection
pub struct PreparedData {
    pub spot: SpotPrice,
    pub surface: IVSurface,
}

/// Encapsulates all parameters needed for trade execution
///
/// Instead of passing many arguments to every function, create a context once
/// and reuse it for the prepare/select/execute workflow.
///
/// Strategy-specific parameters (option_type, wing_width, etc.) should be
/// captured in the selection closure.
pub struct TradeExecutionContext<'a> {
    pub options_repo: &'a dyn OptionsDataRepository,
    pub equity_repo: &'a dyn EquityDataRepository,
    pub event: &'a EarningsEvent,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub config: &'a ExecutionConfig,
}

impl<'a> TradeExecutionContext<'a> {
    /// Create a new execution context
    pub fn new(
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        config: &'a ExecutionConfig,
    ) -> Self {
        Self {
            options_repo,
            equity_repo,
            event,
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
            .get_option_bars_at_time(&self.event.symbol, self.entry_time)
            .await
            .ok()?;

        // Build IV surface from chain
        let surface = build_iv_surface_minute_aligned(
            &entry_chain,
            self.equity_repo,
            &self.event.symbol,
        ).await?;

        // Get spot price at entry time
        let spot = self.equity_repo
            .get_spot_price(&self.event.symbol, self.entry_time)
            .await
            .ok()?;

        Some(PreparedData { spot, surface })
    }

    /// Generic trade execution with closure-based selection
    ///
    /// This is the single entry point for all trade execution. Strategy-specific
    /// parameters should be captured in the selection closure.
    ///
    /// # Type Parameters
    /// - `T`: The trade type (must implement ExecutableTrade)
    /// - `P`: The pricer type (must implement TradePricer for T)
    /// - `F`: Selection function that creates T from prepared data
    ///
    /// # Arguments
    /// - `select`: Closure that selects/constructs the trade from market data
    /// - `pricer`: The pricer instance for this trade type
    ///
    /// # Example
    /// ```ignore
    /// // Calendar spread with option_type captured in closure
    /// let option_type = OptionType::Call;
    /// ctx.execute(
    ///     |data| selector.select_calendar_spread(&data.spot, &data.surface, option_type, criteria).ok(),
    ///     CalendarSpreadPricer::new(),
    /// ).await
    /// ```
    pub async fn execute<T, P, F>(&self, select: F, pricer: P) -> Option<T::Result>
    where
        T: ExecutableTrade<Pricer = P>,
        P: TradePricer<Trade = T>,
        F: FnOnce(&PreparedData) -> Option<T>,
    {
        let data = self.prepare().await?;
        let trade = select(&data)?;
        Some(execute_trade(
            &trade,
            &pricer,
            self.options_repo,
            self.equity_repo,
            self.config,
            self.event,
            self.entry_time,
            self.exit_time,
        ).await)
    }
}
