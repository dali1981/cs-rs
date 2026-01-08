//! Session-based trade execution
//!
//! This module provides a higher-level interface for executing trades based on TradingSession.
//! It bridges the gap between campaign planning (TradingSession) and trade execution (TradeExecutor).

use chrono::NaiveDate;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

use cs_domain::{
    EquityDataRepository, OptionsDataRepository,
    TradeFactory, TradingSession, SessionContext,
    OptionStrategy, RollableTrade, TradeResult, EarningsEvent, EarningsTime,
    Straddle, CalendarSpread, IronButterfly,
};

use crate::execution::{ExecutableTrade, ExecutionConfig};
use crate::trade_executor::TradeExecutor;
use crate::timing_strategy::TimingStrategy;

/// Result of executing a single session
#[derive(Debug)]
pub struct SessionResult {
    /// The session that was executed
    pub session: TradingSession,

    /// Whether execution succeeded
    pub success: bool,

    /// Error message if execution failed
    pub error: Option<String>,

    /// P&L metrics (extracted from trade result for easy access)
    pub pnl: Option<SessionPnL>,

    /// The underlying trade result (if successful)
    /// Boxed to avoid generic parameters
    pub trade_result: Option<Box<dyn std::any::Any + Send>>,
}

/// P&L metrics extracted from trade results
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionPnL {
    pub entry_cost: rust_decimal::Decimal,
    pub exit_value: rust_decimal::Decimal,
    pub pnl: rust_decimal::Decimal,
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,
    pub iv_entry: Option<f64>,
    pub iv_exit: Option<f64>,

    // Hedge details (when hedging is enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hedge_pnl: Option<rust_decimal::Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pnl_with_hedge: Option<rust_decimal::Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hedge_count: Option<usize>,

    // Attribution summary (when hedging with attribution enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<SessionAttribution>,

    // Volatility metrics (when track_realized_vol is enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realized_vol_metrics: Option<cs_domain::RealizedVolatilityMetrics>,
}

/// P&L attribution summary for a session
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionAttribution {
    pub gross_delta_pnl: rust_decimal::Decimal,
    pub hedge_delta_pnl: rust_decimal::Decimal,
    pub net_delta_pnl: rust_decimal::Decimal,
    pub gamma_pnl: rust_decimal::Decimal,
    pub theta_pnl: rust_decimal::Decimal,
    pub vega_pnl: rust_decimal::Decimal,
    pub unexplained: rust_decimal::Decimal,
    pub hedge_efficiency: f64,
}

impl SessionResult {
    /// Create a successful result with P&L
    pub fn success_with_pnl(
        session: TradingSession,
        pnl: SessionPnL,
        trade_result: Box<dyn std::any::Any + Send>,
    ) -> Self {
        Self {
            session,
            success: true,
            error: None,
            pnl: Some(pnl),
            trade_result: Some(trade_result),
        }
    }

    /// Create a successful result (legacy, no P&L extraction)
    pub fn success(session: TradingSession, trade_result: Box<dyn std::any::Any + Send>) -> Self {
        Self {
            session,
            success: true,
            error: None,
            pnl: None,
            trade_result: Some(trade_result),
        }
    }

    /// Create a failed result
    pub fn failure(session: TradingSession, error: String) -> Self {
        Self {
            session,
            success: false,
            error: Some(error),
            pnl: None,
            trade_result: None,
        }
    }
}

/// Batch execution result for multiple sessions
#[derive(Debug)]
pub struct BatchResult {
    /// Results for each session
    pub results: Vec<SessionResult>,

    /// Summary statistics
    pub total_sessions: usize,
    pub successful: usize,
    pub failed: usize,
}

impl BatchResult {
    /// Create from session results
    pub fn from_results(results: Vec<SessionResult>) -> Self {
        let total = results.len();
        let successful = results.iter().filter(|r| r.success).count();
        let failed = total - successful;

        Self {
            results,
            total_sessions: total,
            successful,
            failed,
        }
    }

    /// Print summary
    pub fn print_summary(&self) {
        info!(
            "Batch execution complete: {} total, {} successful, {} failed",
            self.total_sessions, self.successful, self.failed
        );
    }

    /// Calculate total P&L across all successful sessions
    pub fn total_pnl(&self) -> rust_decimal::Decimal {
        self.results
            .iter()
            .filter_map(|r| r.pnl.as_ref())
            .map(|p| p.pnl)
            .sum()
    }

    /// Calculate average P&L per successful trade
    pub fn avg_pnl(&self) -> Option<rust_decimal::Decimal> {
        let pnls: Vec<_> = self.results
            .iter()
            .filter_map(|r| r.pnl.as_ref())
            .map(|p| p.pnl)
            .collect();

        if pnls.is_empty() {
            None
        } else {
            Some(pnls.iter().sum::<rust_decimal::Decimal>() / rust_decimal::Decimal::from(pnls.len()))
        }
    }

    /// Calculate win rate (percentage of profitable trades)
    pub fn win_rate(&self) -> Option<f64> {
        let pnls: Vec<_> = self.results
            .iter()
            .filter_map(|r| r.pnl.as_ref())
            .collect();

        if pnls.is_empty() {
            None
        } else {
            let wins = pnls.iter().filter(|p| p.pnl > rust_decimal::Decimal::ZERO).count();
            Some(100.0 * wins as f64 / pnls.len() as f64)
        }
    }

    /// Get all session results with P&L data
    pub fn successful_with_pnl(&self) -> Vec<&SessionResult> {
        self.results
            .iter()
            .filter(|r| r.success && r.pnl.is_some())
            .collect()
    }

    /// Check if any sessions have hedge data
    pub fn has_hedge_data(&self) -> bool {
        self.results
            .iter()
            .filter_map(|r| r.pnl.as_ref())
            .any(|p| p.hedge_pnl.is_some())
    }

    /// Calculate total hedge P&L across all sessions
    pub fn total_hedge_pnl(&self) -> Option<rust_decimal::Decimal> {
        let hedge_pnls: Vec<_> = self.results
            .iter()
            .filter_map(|r| r.pnl.as_ref())
            .filter_map(|p| p.hedge_pnl)
            .collect();

        if hedge_pnls.is_empty() {
            None
        } else {
            Some(hedge_pnls.iter().sum())
        }
    }

    /// Calculate total P&L including hedges
    pub fn total_pnl_with_hedge(&self) -> rust_decimal::Decimal {
        self.results
            .iter()
            .filter_map(|r| r.pnl.as_ref())
            .map(|p| p.total_pnl_with_hedge.unwrap_or(p.pnl))
            .sum()
    }

    /// Calculate total hedge trade count
    pub fn total_hedge_count(&self) -> usize {
        self.results
            .iter()
            .filter_map(|r| r.pnl.as_ref())
            .filter_map(|p| p.hedge_count)
            .sum()
    }

    /// Check if any sessions have attribution data
    pub fn has_attribution_data(&self) -> bool {
        self.results
            .iter()
            .filter_map(|r| r.pnl.as_ref())
            .any(|p| p.attribution.is_some())
    }

    /// Calculate aggregated attribution summary
    pub fn attribution_summary(&self) -> Option<SessionAttribution> {
        let attrs: Vec<_> = self.results
            .iter()
            .filter_map(|r| r.pnl.as_ref())
            .filter_map(|p| p.attribution.as_ref())
            .collect();

        if attrs.is_empty() {
            return None;
        }

        let gross_delta_pnl: rust_decimal::Decimal = attrs.iter().map(|a| a.gross_delta_pnl).sum();
        let hedge_delta_pnl: rust_decimal::Decimal = attrs.iter().map(|a| a.hedge_delta_pnl).sum();
        let net_delta_pnl: rust_decimal::Decimal = attrs.iter().map(|a| a.net_delta_pnl).sum();
        let gamma_pnl: rust_decimal::Decimal = attrs.iter().map(|a| a.gamma_pnl).sum();
        let theta_pnl: rust_decimal::Decimal = attrs.iter().map(|a| a.theta_pnl).sum();
        let vega_pnl: rust_decimal::Decimal = attrs.iter().map(|a| a.vega_pnl).sum();
        let unexplained: rust_decimal::Decimal = attrs.iter().map(|a| a.unexplained).sum();
        let avg_hedge_efficiency = attrs.iter().map(|a| a.hedge_efficiency).sum::<f64>() / attrs.len() as f64;

        Some(SessionAttribution {
            gross_delta_pnl,
            hedge_delta_pnl,
            net_delta_pnl,
            gamma_pnl,
            theta_pnl,
            vega_pnl,
            unexplained,
            hedge_efficiency: avg_hedge_efficiency,
        })
    }
}

/// Executor for TradingSession
///
/// This is a higher-level executor that:
/// - Takes TradingSession as input
/// - Creates trades using TradeFactory
/// - Executes using the appropriate TradeExecutor<T>
/// - Handles batch execution for multiple sessions
pub struct SessionExecutor {
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    trade_factory: Arc<dyn TradeFactory>,
    config: ExecutionConfig,
    // Hedging support
    hedge_config: Option<cs_domain::HedgeConfig>,
    timing_strategy: Option<crate::timing_strategy::TimingStrategy>,
    // Attribution support
    attribution_config: Option<cs_domain::AttributionConfig>,
}

impl SessionExecutor {
    /// Create a new session executor
    pub fn new(
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        trade_factory: Arc<dyn TradeFactory>,
        config: ExecutionConfig,
    ) -> Self {
        Self {
            options_repo,
            equity_repo,
            trade_factory,
            config,
            hedge_config: None,
            timing_strategy: None,
            attribution_config: None,
        }
    }

    /// Enable hedging (builder pattern)
    pub fn with_hedging(
        mut self,
        hedge_config: cs_domain::HedgeConfig,
        timing_strategy: crate::timing_strategy::TimingStrategy,
    ) -> Self {
        self.hedge_config = Some(hedge_config);
        self.timing_strategy = Some(timing_strategy);
        self
    }

    /// Enable attribution (builder pattern)
    ///
    /// Attribution requires hedging to be enabled. If hedging is not enabled,
    /// attribution will have no effect.
    pub fn with_attribution(mut self, config: cs_domain::AttributionConfig) -> Self {
        self.attribution_config = Some(config);
        self
    }

    /// Execute a single session
    ///
    /// This is the main entry point for session-based execution.
    pub async fn execute_session(&self, session: &TradingSession) -> SessionResult {
        info!(
            "Executing session: {} {:?} ({})",
            session.symbol,
            session.strategy,
            session.entry_date()
        );

        // Create earnings event from session context
        let earnings_event = self.create_earnings_event(session);

        // Dispatch to strategy-specific executor
        match session.strategy {
            OptionStrategy::CalendarSpread => {
                self.execute_calendar_spread(session, &earnings_event).await
            }
            OptionStrategy::Straddle => {
                self.execute_straddle(session, &earnings_event).await
            }
            OptionStrategy::CalendarStraddle => {
                // TODO: CalendarStraddle doesn't implement RollableTrade yet
                // Need to implement a different creation pattern
                SessionResult::failure(
                    session.clone(),
                    "CalendarStraddle not yet supported in SessionExecutor".to_string(),
                )
            }
            OptionStrategy::IronButterfly => {
                self.execute_iron_butterfly(session, &earnings_event).await
            }
            OptionStrategy::Strangle => {
                self.execute_strangle(session, &earnings_event).await
            }
            OptionStrategy::Butterfly => {
                self.execute_butterfly(session, &earnings_event).await
            }
            OptionStrategy::Condor => {
                self.execute_condor(session, &earnings_event).await
            }
            OptionStrategy::IronCondor => {
                self.execute_iron_condor(session, &earnings_event).await
            }
        }
    }

    /// Execute multiple sessions in batch
    ///
    /// Sessions are executed sequentially. For parallel execution,
    /// use tokio::spawn or similar.
    pub async fn execute_batch(&self, sessions: &[TradingSession]) -> BatchResult {
        info!("Starting batch execution for {} sessions", sessions.len());

        let mut results = Vec::with_capacity(sessions.len());

        for session in sessions {
            let result = self.execute_session(session).await;
            results.push(result);
        }

        let batch_result = BatchResult::from_results(results);
        batch_result.print_summary();

        batch_result
    }

    /// Execute sessions grouped by date
    ///
    /// This is useful for multi-stock backtests where you want to
    /// execute all sessions on the same date together.
    pub async fn execute_by_date(
        &self,
        sessions: &[TradingSession],
    ) -> HashMap<NaiveDate, BatchResult> {
        // Group sessions by entry date
        let mut by_date: HashMap<NaiveDate, Vec<TradingSession>> = HashMap::new();

        for session in sessions {
            by_date
                .entry(session.entry_date())
                .or_default()
                .push(session.clone());
        }

        info!("Executing {} sessions across {} dates", sessions.len(), by_date.len());

        // Execute each date's sessions
        let mut results = HashMap::new();

        for (date, date_sessions) in by_date {
            info!("Executing {} sessions for {}", date_sessions.len(), date);
            let batch_result = self.execute_batch(&date_sessions).await;
            results.insert(date, batch_result);
        }

        results
    }

    // =========================================================================
    // Strategy-specific execution methods
    // =========================================================================

    /// Execute a calendar spread session
    async fn execute_calendar_spread(
        &self,
        session: &TradingSession,
        earnings_event: &EarningsEvent,
    ) -> SessionResult {
        use crate::spread_pricer::SpreadPricer;

        // Create trade
        let trade = match CalendarSpread::create(
            self.trade_factory.as_ref(),
            &session.symbol,
            session.entry_datetime,
            session.exit_date(),
        ).await {
            Ok(t) => t,
            Err(e) => {
                return SessionResult::failure(
                    session.clone(),
                    format!("Failed to create calendar spread: {}", e),
                );
            }
        };

        // Create executor with optional hedging
        let pricer = SpreadPricer::new();
        let mut executor = TradeExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.config.clone(),
        );

        // Apply hedging if configured
        if let (Some(ref hedge_config), Some(ref timing)) = (&self.hedge_config, &self.timing_strategy) {
            executor = executor.with_hedging(hedge_config.clone(), timing.clone());
        }

        // Apply attribution if configured
        if let Some(ref attr_config) = self.attribution_config {
            executor = executor.with_attribution(attr_config.clone());
        }

        // Execute
        let result = executor.execute(
            &trade,
            earnings_event,
            session.entry_datetime,
            session.exit_datetime,
        ).await;

        // Wrap result with P&L extraction including hedge details
        if result.success() {
            // Extract attribution if available
            let attribution = result.position_attribution.as_ref().map(|attr| SessionAttribution {
                gross_delta_pnl: attr.total_gross_delta_pnl,
                hedge_delta_pnl: attr.total_hedge_delta_pnl,
                net_delta_pnl: attr.total_net_delta_pnl,
                gamma_pnl: attr.total_gamma_pnl,
                theta_pnl: attr.total_theta_pnl,
                vega_pnl: attr.total_vega_pnl,
                unexplained: attr.total_unexplained,
                hedge_efficiency: attr.hedge_efficiency,
            });

            let hedge_count: Option<usize> = None;

            let pnl = SessionPnL {
                entry_cost: result.entry_cost(),
                exit_value: result.exit_value(),
                pnl: result.pnl(),
                spot_at_entry: result.spot_at_entry(),
                spot_at_exit: result.spot_at_exit(),
                iv_entry: result.entry_iv().map(|iv| iv.primary),
                iv_exit: result.exit_iv().map(|iv| iv.primary),
                hedge_pnl: result.hedge_pnl,
                total_pnl_with_hedge: result.total_pnl_with_hedge,
                hedge_count,
                attribution,
                realized_vol_metrics: None,
            };
            SessionResult::success_with_pnl(session.clone(), pnl, Box::new(result))
        } else {
            SessionResult::failure(
                session.clone(),
                "Calendar spread execution failed".to_string(),
            )
        }
    }

    /// Execute a straddle session
    async fn execute_straddle(
        &self,
        session: &TradingSession,
        earnings_event: &EarningsEvent,
    ) -> SessionResult {
        use crate::straddle_pricer::StraddlePricer;
        use crate::spread_pricer::SpreadPricer;

        // Create trade
        let trade = match Straddle::create(
            self.trade_factory.as_ref(),
            &session.symbol,
            session.entry_datetime,
            session.exit_date(),
        ).await {
            Ok(t) => t,
            Err(e) => {
                return SessionResult::failure(
                    session.clone(),
                    format!("Failed to create straddle: {}", e),
                );
            }
        };

        // Create executor with optional hedging
        let pricer = StraddlePricer::new(SpreadPricer::new());
        let mut executor = TradeExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.config.clone(),
        );

        // Apply hedging if configured
        if let (Some(ref hedge_config), Some(ref timing)) = (&self.hedge_config, &self.timing_strategy) {
            executor = executor.with_hedging(hedge_config.clone(), timing.clone());
        }

        // Apply attribution if configured
        if let Some(ref attr_config) = self.attribution_config {
            executor = executor.with_attribution(attr_config.clone());
        }

        // Execute
        let result = executor.execute(
            &trade,
            earnings_event,
            session.entry_datetime,
            session.exit_datetime,
        ).await;

        // Wrap result with P&L extraction including hedge details
        if result.success() {
            // Extract attribution from the straddle result if available
            let attribution = result.position_attribution.as_ref().map(|attr| SessionAttribution {
                gross_delta_pnl: attr.total_gross_delta_pnl,
                hedge_delta_pnl: attr.total_hedge_delta_pnl,
                net_delta_pnl: attr.total_net_delta_pnl,
                gamma_pnl: attr.total_gamma_pnl,
                theta_pnl: attr.total_theta_pnl,
                vega_pnl: attr.total_vega_pnl,
                unexplained: attr.total_unexplained,
                hedge_efficiency: attr.hedge_efficiency,
            });

            // Extract hedge count from position if available
            let hedge_count: Option<usize> = None;

            let pnl = SessionPnL {
                entry_cost: result.entry_cost(),
                exit_value: result.exit_value(),
                pnl: result.pnl(),
                spot_at_entry: result.spot_at_entry(),
                spot_at_exit: result.spot_at_exit(),
                iv_entry: result.entry_iv().map(|iv| iv.primary),
                iv_exit: result.exit_iv().map(|iv| iv.primary),
                hedge_pnl: result.hedge_pnl,
                total_pnl_with_hedge: result.total_pnl_with_hedge,
                hedge_count,
                attribution,
                realized_vol_metrics: None, // TODO: Extract from result if available
            };
            SessionResult::success_with_pnl(session.clone(), pnl, Box::new(result))
        } else {
            SessionResult::failure(
                session.clone(),
                "Straddle execution failed".to_string(),
            )
        }
    }

    /// Execute an iron butterfly session
    async fn execute_iron_butterfly(
        &self,
        session: &TradingSession,
        earnings_event: &EarningsEvent,
    ) -> SessionResult {
        use crate::iron_butterfly_pricer::IronButterflyPricer;
        use crate::spread_pricer::SpreadPricer;
        use cs_domain::value_objects::IronButterflyConfig;

        // Create trade - use advanced method if config available, otherwise default
        let trade = if let Some(ref config) = session.iron_butterfly_config {
            // Use advanced factory method with config and direction
            match self.trade_factory.create_iron_butterfly_advanced(
                &session.symbol,
                session.entry_datetime,
                session.exit_date(),
                config,
                session.trade_direction,
            ).await {
                Ok(t) => t,
                Err(e) => {
                    return SessionResult::failure(
                        session.clone(),
                        format!("Failed to create iron butterfly with advanced config: {}", e),
                    );
                }
            }
        } else {
            // Fall back to default creation
            match IronButterfly::create(
                self.trade_factory.as_ref(),
                &session.symbol,
                session.entry_datetime,
                session.exit_date(),
            ).await {
                Ok(t) => t,
                Err(e) => {
                    return SessionResult::failure(
                        session.clone(),
                        format!("Failed to create iron butterfly: {}", e),
                    );
                }
            }
        };

        // Create executor with optional hedging
        let pricer = IronButterflyPricer::new(SpreadPricer::new());
        let mut executor = TradeExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.config.clone(),
        );

        // Apply hedging if configured
        if let (Some(ref hedge_config), Some(ref timing)) = (&self.hedge_config, &self.timing_strategy) {
            executor = executor.with_hedging(hedge_config.clone(), timing.clone());
        }

        // Apply attribution if configured
        if let Some(ref attr_config) = self.attribution_config {
            executor = executor.with_attribution(attr_config.clone());
        }

        // Execute
        let result = executor.execute(
            &trade,
            earnings_event,
            session.entry_datetime,
            session.exit_datetime,
        ).await;

        // Wrap result with P&L extraction including hedge details
        if result.success() {
            // Extract attribution if available
            let attribution = result.position_attribution.as_ref().map(|attr| SessionAttribution {
                gross_delta_pnl: attr.total_gross_delta_pnl,
                hedge_delta_pnl: attr.total_hedge_delta_pnl,
                net_delta_pnl: attr.total_net_delta_pnl,
                gamma_pnl: attr.total_gamma_pnl,
                theta_pnl: attr.total_theta_pnl,
                vega_pnl: attr.total_vega_pnl,
                unexplained: attr.total_unexplained,
                hedge_efficiency: attr.hedge_efficiency,
            });

            let hedge_count: Option<usize> = None;

            let pnl = SessionPnL {
                entry_cost: result.entry_cost(),
                exit_value: result.exit_value(),
                pnl: result.pnl(),
                spot_at_entry: result.spot_at_entry(),
                spot_at_exit: result.spot_at_exit(),
                iv_entry: result.entry_iv().map(|iv| iv.primary),
                iv_exit: result.exit_iv().map(|iv| iv.primary),
                hedge_pnl: result.hedge_pnl,
                total_pnl_with_hedge: result.total_pnl_with_hedge,
                hedge_count,
                attribution,
                realized_vol_metrics: None,
            };
            SessionResult::success_with_pnl(session.clone(), pnl, Box::new(result))
        } else {
            SessionResult::failure(
                session.clone(),
                "Iron butterfly execution failed".to_string(),
            )
        }
    }

    /// Execute a strangle session
    async fn execute_strangle(
        &self,
        session: &TradingSession,
        earnings_event: &EarningsEvent,
    ) -> SessionResult {
        // Create trade using multi-leg config
        let config = match &session.multi_leg_strategy_config {
            Some(cfg) => cfg.clone(),
            None => {
                return SessionResult::failure(
                    session.clone(),
                    "Strangle strategy requires multi_leg_strategy_config".to_string(),
                );
            }
        };

        let trade = match self.trade_factory.create_strangle(
            &session.symbol,
            session.entry_datetime,
            session.exit_date(),
            &config,
        ).await {
            Ok(t) => t,
            Err(e) => {
                return SessionResult::failure(
                    session.clone(),
                    format!("Failed to create strangle: {}", e),
                );
            }
        };

        // Create pricer and executor
        use crate::multi_leg_pricer::StranglePricer;
        use crate::spread_pricer::SpreadPricer;
        use crate::trade_executor::TradeExecutor;

        let pricer = StranglePricer::new(SpreadPricer::new());
        let mut executor = TradeExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.config.clone(),
        );

        // Apply hedging if configured
        if let (Some(ref hedge_config), Some(ref timing)) = (&self.hedge_config, &self.timing_strategy) {
            executor = executor.with_hedging(hedge_config.clone(), timing.clone());
        }

        // Apply attribution if configured
        if let Some(ref attr_config) = self.attribution_config {
            executor = executor.with_attribution(attr_config.clone());
        }

        // Execute
        let result = executor.execute(
            &trade,
            earnings_event,
            session.entry_datetime,
            session.exit_datetime,
        ).await;

        // Wrap result with P&L extraction including hedge details
        if result.success() {
            // Extract attribution if available
            let attribution = result.position_attribution.as_ref().map(|attr| SessionAttribution {
                gross_delta_pnl: attr.total_gross_delta_pnl,
                hedge_delta_pnl: attr.total_hedge_delta_pnl,
                net_delta_pnl: attr.total_net_delta_pnl,
                gamma_pnl: attr.total_gamma_pnl,
                theta_pnl: attr.total_theta_pnl,
                vega_pnl: attr.total_vega_pnl,
                unexplained: attr.total_unexplained,
                hedge_efficiency: attr.hedge_efficiency,
            });

            let hedge_count: Option<usize> = None;

            let pnl = SessionPnL {
                entry_cost: result.entry_cost(),
                exit_value: result.exit_value(),
                pnl: result.pnl(),
                spot_at_entry: result.spot_at_entry(),
                spot_at_exit: result.spot_at_exit(),
                iv_entry: result.entry_iv().map(|iv| iv.primary),
                iv_exit: result.exit_iv().map(|iv| iv.primary),
                hedge_pnl: result.hedge_pnl,
                total_pnl_with_hedge: result.total_pnl_with_hedge,
                hedge_count,
                attribution,
                realized_vol_metrics: None,
            };
            SessionResult::success_with_pnl(session.clone(), pnl, Box::new(result))
        } else {
            SessionResult::failure(
                session.clone(),
                "Strangle execution failed".to_string(),
            )
        }
    }

    /// Execute a butterfly session
    async fn execute_butterfly(
        &self,
        session: &TradingSession,
        earnings_event: &EarningsEvent,
    ) -> SessionResult {
        // Create trade using multi-leg config
        let config = match &session.multi_leg_strategy_config {
            Some(cfg) => cfg.clone(),
            None => {
                return SessionResult::failure(
                    session.clone(),
                    "Butterfly strategy requires multi_leg_strategy_config".to_string(),
                );
            }
        };

        let trade = match self.trade_factory.create_butterfly(
            &session.symbol,
            session.entry_datetime,
            session.exit_date(),
            &config,
        ).await {
            Ok(t) => t,
            Err(e) => {
                return SessionResult::failure(
                    session.clone(),
                    format!("Failed to create butterfly: {}", e),
                );
            }
        };

        // Create pricer and executor
        use crate::multi_leg_pricer::ButterflyPricer;
        use crate::spread_pricer::SpreadPricer;
        use crate::trade_executor::TradeExecutor;

        let pricer = ButterflyPricer::new(SpreadPricer::new());
        let mut executor = TradeExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.config.clone(),
        );

        // Apply hedging if configured
        if let (Some(ref hedge_config), Some(ref timing)) = (&self.hedge_config, &self.timing_strategy) {
            executor = executor.with_hedging(hedge_config.clone(), timing.clone());
        }

        // Apply attribution if configured
        if let Some(ref attr_config) = self.attribution_config {
            executor = executor.with_attribution(attr_config.clone());
        }

        // Execute
        let result = executor.execute(
            &trade,
            earnings_event,
            session.entry_datetime,
            session.exit_datetime,
        ).await;

        // Wrap result with P&L extraction including hedge details
        if result.success() {
            // Extract attribution if available
            let attribution = result.position_attribution.as_ref().map(|attr| SessionAttribution {
                gross_delta_pnl: attr.total_gross_delta_pnl,
                hedge_delta_pnl: attr.total_hedge_delta_pnl,
                net_delta_pnl: attr.total_net_delta_pnl,
                gamma_pnl: attr.total_gamma_pnl,
                theta_pnl: attr.total_theta_pnl,
                vega_pnl: attr.total_vega_pnl,
                unexplained: attr.total_unexplained,
                hedge_efficiency: attr.hedge_efficiency,
            });

            let hedge_count: Option<usize> = None;

            let pnl = SessionPnL {
                entry_cost: result.entry_cost(),
                exit_value: result.exit_value(),
                pnl: result.pnl(),
                spot_at_entry: result.spot_at_entry(),
                spot_at_exit: result.spot_at_exit(),
                iv_entry: result.entry_iv().map(|iv| iv.primary),
                iv_exit: result.exit_iv().map(|iv| iv.primary),
                hedge_pnl: result.hedge_pnl,
                total_pnl_with_hedge: result.total_pnl_with_hedge,
                hedge_count,
                attribution,
                realized_vol_metrics: None,
            };
            SessionResult::success_with_pnl(session.clone(), pnl, Box::new(result))
        } else {
            SessionResult::failure(
                session.clone(),
                "Butterfly execution failed".to_string(),
            )
        }
    }

    /// Execute a condor session
    async fn execute_condor(
        &self,
        session: &TradingSession,
        earnings_event: &EarningsEvent,
    ) -> SessionResult {
        // Create trade using multi-leg config
        let config = match &session.multi_leg_strategy_config {
            Some(cfg) => cfg.clone(),
            None => {
                return SessionResult::failure(
                    session.clone(),
                    "Condor strategy requires multi_leg_strategy_config".to_string(),
                );
            }
        };

        let trade = match self.trade_factory.create_condor(
            &session.symbol,
            session.entry_datetime,
            session.exit_date(),
            &config,
        ).await {
            Ok(t) => t,
            Err(e) => {
                return SessionResult::failure(
                    session.clone(),
                    format!("Failed to create condor: {}", e),
                );
            }
        };

        // Create pricer and executor
        use crate::multi_leg_pricer::CondorPricer;
        use crate::spread_pricer::SpreadPricer;
        use crate::trade_executor::TradeExecutor;

        let pricer = CondorPricer::new(SpreadPricer::new());
        let mut executor = TradeExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.config.clone(),
        );

        // Apply hedging if configured
        if let (Some(ref hedge_config), Some(ref timing)) = (&self.hedge_config, &self.timing_strategy) {
            executor = executor.with_hedging(hedge_config.clone(), timing.clone());
        }

        // Apply attribution if configured
        if let Some(ref attr_config) = self.attribution_config {
            executor = executor.with_attribution(attr_config.clone());
        }

        // Execute
        let result = executor.execute(
            &trade,
            earnings_event,
            session.entry_datetime,
            session.exit_datetime,
        ).await;

        // Wrap result with P&L extraction including hedge details
        if result.success() {
            // Extract attribution if available
            let attribution = result.position_attribution.as_ref().map(|attr| SessionAttribution {
                gross_delta_pnl: attr.total_gross_delta_pnl,
                hedge_delta_pnl: attr.total_hedge_delta_pnl,
                net_delta_pnl: attr.total_net_delta_pnl,
                gamma_pnl: attr.total_gamma_pnl,
                theta_pnl: attr.total_theta_pnl,
                vega_pnl: attr.total_vega_pnl,
                unexplained: attr.total_unexplained,
                hedge_efficiency: attr.hedge_efficiency,
            });

            let hedge_count: Option<usize> = None;

            let pnl = SessionPnL {
                entry_cost: result.entry_cost(),
                exit_value: result.exit_value(),
                pnl: result.pnl(),
                spot_at_entry: result.spot_at_entry(),
                spot_at_exit: result.spot_at_exit(),
                iv_entry: result.entry_iv().map(|iv| iv.primary),
                iv_exit: result.exit_iv().map(|iv| iv.primary),
                hedge_pnl: result.hedge_pnl,
                total_pnl_with_hedge: result.total_pnl_with_hedge,
                hedge_count,
                attribution,
                realized_vol_metrics: None,
            };
            SessionResult::success_with_pnl(session.clone(), pnl, Box::new(result))
        } else {
            SessionResult::failure(
                session.clone(),
                "Condor execution failed".to_string(),
            )
        }
    }

    /// Execute an iron condor session
    async fn execute_iron_condor(
        &self,
        session: &TradingSession,
        earnings_event: &EarningsEvent,
    ) -> SessionResult {
        // Create trade using multi-leg config
        let config = match &session.multi_leg_strategy_config {
            Some(cfg) => cfg.clone(),
            None => {
                return SessionResult::failure(
                    session.clone(),
                    "Iron condor strategy requires multi_leg_strategy_config".to_string(),
                );
            }
        };

        let trade = match self.trade_factory.create_iron_condor(
            &session.symbol,
            session.entry_datetime,
            session.exit_date(),
            &config,
        ).await {
            Ok(t) => t,
            Err(e) => {
                return SessionResult::failure(
                    session.clone(),
                    format!("Failed to create iron condor: {}", e),
                );
            }
        };

        // Create pricer and executor
        use crate::multi_leg_pricer::IronCondorPricer;
        use crate::spread_pricer::SpreadPricer;
        use crate::trade_executor::TradeExecutor;

        let pricer = IronCondorPricer::new(SpreadPricer::new());
        let mut executor = TradeExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.config.clone(),
        );

        // Apply hedging if configured
        if let (Some(ref hedge_config), Some(ref timing)) = (&self.hedge_config, &self.timing_strategy) {
            executor = executor.with_hedging(hedge_config.clone(), timing.clone());
        }

        // Apply attribution if configured
        if let Some(ref attr_config) = self.attribution_config {
            executor = executor.with_attribution(attr_config.clone());
        }

        // Execute
        let result = executor.execute(
            &trade,
            earnings_event,
            session.entry_datetime,
            session.exit_datetime,
        ).await;

        // Wrap result with P&L extraction including hedge details
        if result.success() {
            // Extract attribution if available
            let attribution = result.position_attribution.as_ref().map(|attr| SessionAttribution {
                gross_delta_pnl: attr.total_gross_delta_pnl,
                hedge_delta_pnl: attr.total_hedge_delta_pnl,
                net_delta_pnl: attr.total_net_delta_pnl,
                gamma_pnl: attr.total_gamma_pnl,
                theta_pnl: attr.total_theta_pnl,
                vega_pnl: attr.total_vega_pnl,
                unexplained: attr.total_unexplained,
                hedge_efficiency: attr.hedge_efficiency,
            });

            let hedge_count: Option<usize> = None;

            let pnl = SessionPnL {
                entry_cost: result.entry_cost(),
                exit_value: result.exit_value(),
                pnl: result.pnl(),
                spot_at_entry: result.spot_at_entry(),
                spot_at_exit: result.spot_at_exit(),
                iv_entry: result.entry_iv().map(|iv| iv.primary),
                iv_exit: result.exit_iv().map(|iv| iv.primary),
                hedge_pnl: result.hedge_pnl,
                total_pnl_with_hedge: result.total_pnl_with_hedge,
                hedge_count,
                attribution,
                realized_vol_metrics: None,
            };
            SessionResult::success_with_pnl(session.clone(), pnl, Box::new(result))
        } else {
            SessionResult::failure(
                session.clone(),
                "Iron condor execution failed".to_string(),
            )
        }
    }

    // =========================================================================
    // Helper methods
    // =========================================================================

    /// Create earnings event from session context
    fn create_earnings_event(&self, session: &TradingSession) -> EarningsEvent {
        match &session.context {
            SessionContext::Earnings { event, .. } => event.clone(),
            SessionContext::InterEarnings { earnings_after, .. } => {
                // For inter-earnings, use the next earnings date
                EarningsEvent::new(
                    session.symbol.clone(),
                    *earnings_after,
                    EarningsTime::Unknown,
                )
            }
            SessionContext::Standalone { .. } => {
                // For standalone, create a dummy event at exit date
                EarningsEvent::new(
                    session.symbol.clone(),
                    session.exit_date(),
                    EarningsTime::Unknown,
                )
            }
        }
    }
}
