//! Trade Execution Context - Encapsulated trade selection and execution
//!
//! This module provides a context-based approach to trade execution that:
//! - Encapsulates the 9 common parameters into a single struct
//! - Extracts common data preparation (chain, surface, spot) into one method
//! - Provides a generic execute() method with closure-based trade selection

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use cs_domain::*;
use cs_domain::strike_selection::{StrikeSelector, ExpirationCriteria};
use cs_analytics::IVSurface;
use crate::execution::{execute_trade, ExecutionConfig, ExecutableTrade, TradePricer};
use crate::spread_pricer::SpreadPricer;
use crate::straddle_pricer::StraddlePricer;
use crate::iron_butterfly_pricer::IronButterflyPricer;
use crate::calendar_straddle_pricer::CalendarStraddlePricer;
use crate::iv_surface_builder::build_iv_surface_minute_aligned;
use finq_core::OptionType;

/// Prepared market data for trade selection
pub struct PreparedData {
    pub spot: SpotPrice,
    pub surface: IVSurface,
}

/// Encapsulates all parameters needed for trade execution
///
/// Instead of passing 9 arguments to every function, create a context once
/// and reuse it for the prepare/select/execute workflow.
pub struct TradeExecutionContext<'a> {
    pub options_repo: &'a dyn OptionsDataRepository,
    pub equity_repo: &'a dyn EquityDataRepository,
    pub selector: &'a dyn StrikeSelector,
    pub criteria: &'a ExpirationCriteria,
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
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        config: &'a ExecutionConfig,
    ) -> Self {
        Self {
            options_repo,
            equity_repo,
            selector,
            criteria,
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
    /// # Type Parameters
    /// - `T`: The trade type (must implement ExecutableTrade)
    /// - `P`: The pricer type (must implement TradePricer for T)
    /// - `F`: Selection function that creates T from prepared data
    ///
    /// # Arguments
    /// - `select`: Closure that selects/constructs the trade from market data
    /// - `pricer`: The pricer instance for this trade type
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

    // ========================================================================
    // Convenience methods for specific trade types
    // These provide a cleaner API while using the generic execute() internally
    // ========================================================================

    /// Execute a calendar spread trade
    pub async fn execute_calendar_spread(&self, option_type: OptionType) -> Option<CalendarSpreadResult> {
        let selector = self.selector;
        let criteria = self.criteria;

        self.execute(
            |data| selector.select_calendar_spread(&data.spot, &data.surface, option_type, criteria).ok(),
            SpreadPricer::new(),
        ).await
    }

    /// Execute a straddle trade
    pub async fn execute_straddle(&self) -> Option<StraddleResult> {
        let selector = self.selector;
        let criteria = self.criteria;
        let entry_date = self.entry_time.date_naive();
        let min_expiration = (entry_date + chrono::Duration::days(criteria.min_short_dte as i64))
            .max(entry_date);

        self.execute(
            |data| selector.select_straddle(&data.spot, &data.surface, min_expiration).ok(),
            StraddlePricer::new(SpreadPricer::new()),
        ).await
    }

    /// Execute an iron butterfly trade
    pub async fn execute_iron_butterfly(&self, wing_width: Decimal) -> Option<IronButterflyResult> {
        let selector = self.selector;
        let criteria = self.criteria;

        self.execute(
            |data| selector.select_iron_butterfly(
                &data.spot,
                &data.surface,
                wing_width,
                criteria.min_short_dte,
                criteria.max_short_dte,
            ).ok(),
            IronButterflyPricer::new(SpreadPricer::new()),
        ).await
    }

    /// Execute a calendar straddle trade
    pub async fn execute_calendar_straddle(&self) -> Option<CalendarStraddleResult> {
        let selector = self.selector;
        let criteria = self.criteria;

        self.execute(
            |data| selector.select_calendar_straddle(&data.spot, &data.surface, criteria).ok(),
            CalendarStraddlePricer::new(SpreadPricer::new()),
        ).await
    }
}

// ============================================================================
// Standalone functions for backwards compatibility
// These delegate to TradeExecutionContext methods
// ============================================================================

/// Helper to execute calendar spread: select + execute
pub async fn execute_calendar_spread(
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    selector: &dyn StrikeSelector,
    criteria: &ExpirationCriteria,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    option_type: OptionType,
    config: &ExecutionConfig,
) -> Option<CalendarSpreadResult> {
    let ctx = TradeExecutionContext::new(
        options_repo, equity_repo, selector, criteria,
        event, entry_time, exit_time, config,
    );
    ctx.execute_calendar_spread(option_type).await
}

/// Helper to execute straddle: select + execute
pub async fn execute_straddle(
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    selector: &dyn StrikeSelector,
    criteria: &ExpirationCriteria,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    config: &ExecutionConfig,
) -> Option<StraddleResult> {
    let ctx = TradeExecutionContext::new(
        options_repo, equity_repo, selector, criteria,
        event, entry_time, exit_time, config,
    );
    ctx.execute_straddle().await
}

/// Helper to execute iron butterfly: select + execute
pub async fn execute_iron_butterfly(
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    selector: &dyn StrikeSelector,
    criteria: &ExpirationCriteria,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    config: &ExecutionConfig,
) -> Option<IronButterflyResult> {
    let ctx = TradeExecutionContext::new(
        options_repo, equity_repo, selector, criteria,
        event, entry_time, exit_time, config,
    );
    // Default wing width of 5
    ctx.execute_iron_butterfly(Decimal::new(5, 0)).await
}

/// Helper to execute calendar straddle: select + execute
pub async fn execute_calendar_straddle(
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    selector: &dyn StrikeSelector,
    criteria: &ExpirationCriteria,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    config: &ExecutionConfig,
) -> Option<CalendarStraddleResult> {
    let ctx = TradeExecutionContext::new(
        options_repo, equity_repo, selector, criteria,
        event, entry_time, exit_time, config,
    );
    ctx.execute_calendar_straddle().await
}
