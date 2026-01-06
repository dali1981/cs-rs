//! Session-based trade execution
//!
//! This module provides a higher-level interface for executing trades based on TradingSession.
//! It bridges the gap between campaign planning (TradingSession) and trade execution (TradeExecutor).

use chrono::{DateTime, NaiveDate, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use cs_domain::{
    EquityDataRepository, OptionsDataRepository,
    TradeFactory, TradingSession, SessionAction, SessionContext,
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
#[derive(Debug, Clone)]
pub struct SessionPnL {
    pub entry_cost: rust_decimal::Decimal,
    pub exit_value: rust_decimal::Decimal,
    pub pnl: rust_decimal::Decimal,
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,
    pub iv_entry: Option<f64>,
    pub iv_exit: Option<f64>,
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

        // Create executor
        let pricer = SpreadPricer::new();
        let executor = TradeExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.config.clone(),
        );

        // Execute
        let result = executor.execute(
            &trade,
            earnings_event,
            session.entry_datetime,
            session.exit_datetime,
        ).await;

        // Wrap result
        if result.success() {
            SessionResult::success(session.clone(), Box::new(result))
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

        // Execute
        let result = executor.execute(
            &trade,
            earnings_event,
            session.entry_datetime,
            session.exit_datetime,
        ).await;

        // Wrap result with P&L extraction
        if result.success() {
            let pnl = SessionPnL {
                entry_cost: result.entry_cost(),
                exit_value: result.exit_value(),
                pnl: result.pnl(),
                spot_at_entry: result.spot_at_entry(),
                spot_at_exit: result.spot_at_exit(),
                iv_entry: result.entry_iv().map(|iv| iv.primary),
                iv_exit: result.exit_iv().map(|iv| iv.primary),
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

        // Create trade
        let trade = match IronButterfly::create(
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
        };

        // Create executor
        let pricer = IronButterflyPricer::new(SpreadPricer::new());
        let executor = TradeExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.config.clone(),
        );

        // Execute
        let result = executor.execute(
            &trade,
            earnings_event,
            session.entry_datetime,
            session.exit_datetime,
        ).await;

        // Wrap result
        if result.success() {
            SessionResult::success(session.clone(), Box::new(result))
        } else {
            SessionResult::failure(
                session.clone(),
                "Iron butterfly execution failed".to_string(),
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
