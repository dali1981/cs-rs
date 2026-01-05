use std::sync::Arc;
use chrono::NaiveDate;
use tracing::{info, debug, warn};
use rust_decimal::Decimal;

use cs_domain::*;
use cs_domain::timing::{EarningsTradeTiming, StraddleTradeTiming, PostEarningsStraddleTiming};
use cs_domain::strike_selection::{StrikeSelector, ATMStrategy, DeltaStrategy, ExpirationCriteria, IronButterflyStrategy, StraddleStrategy};
use crate::config::{BacktestConfig, SpreadType, SelectionType};
use crate::execution::ExecutionConfig;
use crate::backtest_use_case_helpers;

/// Backtest execution result
#[derive(Debug)]
pub struct BacktestResult<R> {
    pub results: Vec<R>,
    pub sessions_processed: usize,
    pub total_entries: usize,
    pub total_opportunities: usize,
    pub dropped_events: Vec<TradeGenerationError>,
}

// Trait to unify result types for BacktestResult
pub trait TradeResultMethods {
    fn is_winner(&self) -> bool;
    fn pnl(&self) -> Decimal;
    fn pnl_pct(&self) -> Decimal;
    fn has_hedge_data(&self) -> bool {
        false
    }
    fn hedge_pnl(&self) -> Option<Decimal> {
        None
    }
    fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        None
    }
}

impl TradeResultMethods for CalendarSpreadResult {
    fn is_winner(&self) -> bool {
        self.is_winner()
    }
    fn pnl(&self) -> Decimal {
        self.pnl
    }
    fn pnl_pct(&self) -> Decimal {
        self.pnl_pct
    }
    fn has_hedge_data(&self) -> bool {
        self.hedge_pnl.is_some()
    }
    fn hedge_pnl(&self) -> Option<Decimal> {
        self.hedge_pnl
    }
    fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        self.total_pnl_with_hedge
    }
}

impl TradeResultMethods for StraddleResult {
    fn is_winner(&self) -> bool {
        self.is_winner()
    }
    fn pnl(&self) -> Decimal {
        self.pnl
    }
    fn pnl_pct(&self) -> Decimal {
        self.pnl_pct
    }
    fn has_hedge_data(&self) -> bool {
        self.hedge_pnl.is_some()
    }
    fn hedge_pnl(&self) -> Option<Decimal> {
        self.hedge_pnl
    }
    fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        self.total_pnl_with_hedge
    }
}

impl TradeResultMethods for IronButterflyResult {
    fn is_winner(&self) -> bool {
        self.is_winner()
    }
    fn pnl(&self) -> Decimal {
        self.pnl
    }
    fn pnl_pct(&self) -> Decimal {
        self.pnl_pct
    }
    fn has_hedge_data(&self) -> bool {
        self.hedge_pnl.is_some()
    }
    fn hedge_pnl(&self) -> Option<Decimal> {
        self.hedge_pnl
    }
    fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        self.total_pnl_with_hedge
    }
}

impl TradeResultMethods for CalendarStraddleResult {
    fn is_winner(&self) -> bool {
        self.is_winner()
    }
    fn pnl(&self) -> Decimal {
        self.pnl
    }
    fn pnl_pct(&self) -> Decimal {
        self.pnl_pct
    }
    fn has_hedge_data(&self) -> bool {
        self.hedge_pnl.is_some()
    }
    fn hedge_pnl(&self) -> Option<Decimal> {
        self.hedge_pnl
    }
    fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        self.total_pnl_with_hedge
    }
}

impl<R: TradeResultMethods> BacktestResult<R> {
    pub fn win_rate(&self) -> f64 {
        let winners = self.results.iter().filter(|r| r.is_winner()).count();
        let successful_trades = self.results.len();
        if successful_trades == 0 {
            0.0
        } else {
            winners as f64 / successful_trades as f64
        }
    }

    pub fn total_pnl(&self) -> rust_decimal::Decimal {
        self.results.iter()
            .map(|r| r.pnl())
            .sum()
    }

    /// Check if any trades have hedging data
    pub fn has_hedging(&self) -> bool {
        self.results.iter().any(|r| r.has_hedge_data())
    }

    /// Total hedge P&L from all trades
    pub fn total_hedge_pnl(&self) -> rust_decimal::Decimal {
        self.results.iter()
            .filter_map(|r| r.hedge_pnl())
            .sum()
    }

    /// Total P&L including hedges
    pub fn total_pnl_with_hedge(&self) -> rust_decimal::Decimal {
        self.results.iter()
            .map(|r| r.total_pnl_with_hedge().unwrap_or(r.pnl()))
            .sum()
    }

    pub fn successful_trades(&self) -> usize {
        self.results.len()
    }

    /// Get percentage returns for statistical analysis
    fn pnl_pcts(&self) -> Vec<f64> {
        self.results.iter()
            .map(|r| {
                let pnl_pct: f64 = r.pnl_pct().try_into().unwrap_or(0.0);
                pnl_pct / 100.0  // Convert from percentage to decimal (50% -> 0.5)
            })
            .collect()
    }

    /// Mean return (as decimal, e.g., 0.05 = 5%)
    pub fn mean_return(&self) -> f64 {
        let returns = self.pnl_pcts();
        if returns.is_empty() {
            0.0
        } else {
            returns.iter().sum::<f64>() / returns.len() as f64
        }
    }

    /// Standard deviation of returns
    pub fn std_return(&self) -> f64 {
        let returns = self.pnl_pcts();
        if returns.len() < 2 {
            return 0.0;
        }
        let mean = self.mean_return();
        let variance = returns.iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>() / (returns.len() - 1) as f64;
        variance.sqrt()
    }

    /// Sharpe ratio (annualized, assuming ~252 trading days)
    pub fn sharpe_ratio(&self) -> f64 {
        let std = self.std_return();
        if std == 0.0 {
            0.0
        } else {
            let mean = self.mean_return();
            (mean / std) * 16.0  // sqrt(252) ≈ 16
        }
    }

    /// Average winning trade (in dollars)
    pub fn avg_winner(&self) -> rust_decimal::Decimal {
        let winners: Vec<_> = self.results.iter()
            .filter(|r| r.is_winner())
            .collect();
        if winners.is_empty() {
            rust_decimal::Decimal::ZERO
        } else {
            let sum: rust_decimal::Decimal = winners.iter().map(|r| r.pnl()).sum();
            sum / rust_decimal::Decimal::from(winners.len())
        }
    }

    /// Average winning trade (in percent)
    pub fn avg_winner_pct(&self) -> f64 {
        let winners: Vec<_> = self.results.iter()
            .filter(|r| r.is_winner())
            .collect();
        if winners.is_empty() {
            0.0
        } else {
            let sum: f64 = winners.iter()
                .map(|r| {
                    let pct: f64 = r.pnl_pct().try_into().unwrap_or(0.0);
                    pct / 100.0
                })
                .sum();
            sum / winners.len() as f64
        }
    }

    /// Average losing trade (in dollars)
    pub fn avg_loser(&self) -> rust_decimal::Decimal {
        let losers: Vec<_> = self.results.iter()
            .filter(|r| r.pnl() < rust_decimal::Decimal::ZERO)
            .collect();
        if losers.is_empty() {
            rust_decimal::Decimal::ZERO
        } else {
            let sum: rust_decimal::Decimal = losers.iter().map(|r| r.pnl()).sum();
            sum / rust_decimal::Decimal::from(losers.len())
        }
    }

    /// Average losing trade (in percent)
    pub fn avg_loser_pct(&self) -> f64 {
        let losers: Vec<_> = self.results.iter()
            .filter(|r| r.pnl() < rust_decimal::Decimal::ZERO)
            .collect();
        if losers.is_empty() {
            0.0
        } else {
            let sum: f64 = losers.iter()
                .map(|r| {
                    let pct: f64 = r.pnl_pct().try_into().unwrap_or(0.0);
                    pct / 100.0
                })
                .sum();
            sum / losers.len() as f64
        }
    }
}

/// Session progress callback
#[derive(Debug, Clone)]
pub struct SessionProgress {
    pub session_date: NaiveDate,
    pub entries_count: usize,
    pub events_found: usize,
}

/// Trade generation error
#[derive(Debug, Clone)]
pub struct TradeGenerationError {
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub reason: String,
    pub details: Option<String>,
    pub phase: String,
}

/// Main backtest use case
pub struct BacktestUseCase<Opt, Eq>
where
    Opt: OptionsDataRepository,
    Eq: EquityDataRepository,
{
    earnings_repo: Arc<dyn EarningsRepository>,
    options_repo: Arc<Opt>,
    equity_repo: Arc<Eq>,
    config: BacktestConfig,
}

impl<Opt, Eq> BacktestUseCase<Opt, Eq>
where
    Opt: OptionsDataRepository + 'static,
    Eq: EquityDataRepository + 'static,
{
    pub fn new(
        earnings_repo: Box<dyn EarningsRepository>,
        options_repo: Opt,
        equity_repo: Eq,
        config: BacktestConfig,
    ) -> Self {
        Self {
            earnings_repo: Arc::from(earnings_repo),
            options_repo: Arc::new(options_repo),
            equity_repo: Arc::new(equity_repo),
            config,
        }
    }

    // NOTE: execute() dispatcher removed - CLI should call specific execute_* methods based on strategy type
    // Each method returns its own typed BacktestResult<R>:
    // - execute_calendar_spread() -> BacktestResult<CalendarSpreadResult>
    // - execute_straddle() -> BacktestResult<StraddleResult>
    // - execute_iron_butterfly() -> BacktestResult<IronButterflyResult>
    // - execute_calendar_straddle() -> BacktestResult<CalendarStraddleResult>
    // - execute_post_earnings_straddle() -> BacktestResult<StraddleResult>

    pub async fn execute_calendar_spread(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        option_type: finq_core::OptionType,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<CalendarSpreadResult>, BacktestError> {
        let mut all_results: Vec<CalendarSpreadResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        // Create selector and criteria
        let selector = self.create_selector();
        let criteria = self.build_expiration_criteria();
        let exec_config = ExecutionConfig::for_calendar_spread(self.config.max_entry_iv);

        // Calendar spreads use earnings timing (enter on/before earnings day)
        use crate::timing_strategy::TimingStrategy;
        let timing = TimingStrategy::Earnings(EarningsTradeTiming::new(self.config.timing));

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings for this session
            let events = self.load_earnings_window(session_date).await?;
            let to_enter = self.filter_for_entry(&events, session_date, &timing);

            if to_enter.is_empty() {
                if let Some(ref callback) = on_progress {
                    callback(SessionProgress {
                        session_date,
                        entries_count: 0,
                        events_found: 0,
                    });
                }
                continue;
            }

            debug!(
                session_date = %session_date,
                events_count = to_enter.len(),
                "Processing calendar spread session"
            );

            // Process events using helper
            let session_results: Vec<_> = if self.config.parallel {
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| {
                        let entry_time = timing.entry_datetime(event);
                        let exit_time = timing.exit_datetime(event);
                        backtest_use_case_helpers::execute_calendar_spread(
                            self.options_repo.as_ref(),
                            self.equity_repo.as_ref(),
                            &*selector,
                            &criteria,
                            event,
                            entry_time,
                            exit_time,
                            option_type,
                            &exec_config,
                        )
                    })
                    .collect();

                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    let entry_time = timing.entry_datetime(event);
                    let exit_time = timing.exit_datetime(event);
                    let result = backtest_use_case_helpers::execute_calendar_spread(
                        self.options_repo.as_ref(),
                        self.equity_repo.as_ref(),
                        &*selector,
                        &criteria,
                        event,
                        entry_time,
                        exit_time,
                        option_type,
                        &exec_config,
                    ).await;
                    results.push(result);
                }
                results
            };

            // Collect results and apply IV filter
            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                if let Some(trade_result) = result {
                    if self.passes_iv_filter(&trade_result) {
                        all_results.push(trade_result);
                        session_entries += 1;
                    } else {
                        dropped_events.push(TradeGenerationError {
                            symbol: trade_result.symbol.clone(),
                            earnings_date: trade_result.earnings_date,
                            earnings_time: trade_result.earnings_time,
                            reason: "IV_RATIO_FILTER".into(),
                            details: None,
                            phase: "filter".into(),
                        });
                    }
                }
            }

            if let Some(ref callback) = on_progress {
                callback(SessionProgress {
                    session_date,
                    entries_count: session_entries,
                    events_found: to_enter.len(),
                });
            }
        }

        let total_entries = all_results.len();

        info!(
            sessions_processed,
            total_opportunities,
            results_count = total_entries,
            dropped_count = dropped_events.len(),
            "Backtest completed"
        );

        Ok(BacktestResult {
            results: all_results,
            sessions_processed,
            total_entries,
            total_opportunities,
            dropped_events,
        })
    }

    pub async fn execute_iron_butterfly(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<IronButterflyResult>, BacktestError> {
        let mut all_results: Vec<IronButterflyResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        // Create selector and criteria
        let selector = self.create_selector();
        let criteria = self.build_expiration_criteria();
        let exec_config = ExecutionConfig::for_iron_butterfly(self.config.max_entry_iv);

        // Iron butterflies use earnings timing (enter on/before earnings day)
        use crate::timing_strategy::TimingStrategy;
        let timing = TimingStrategy::Earnings(EarningsTradeTiming::new(self.config.timing));

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings for this session
            let events = self.load_earnings_window(session_date).await?;
            let to_enter = self.filter_for_entry(&events, session_date, &timing);

            if to_enter.is_empty() {
                if let Some(ref callback) = on_progress {
                    callback(SessionProgress {
                        session_date,
                        entries_count: 0,
                        events_found: 0,
                    });
                }
                continue;
            }

            debug!(
                session_date = %session_date,
                events_count = to_enter.len(),
                "Processing iron butterfly session"
            );

            // Process events using helper
            let session_results: Vec<_> = if self.config.parallel {
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| {
                        let entry_time = timing.entry_datetime(event);
                        let exit_time = timing.exit_datetime(event);
                        backtest_use_case_helpers::execute_iron_butterfly(
                            self.options_repo.as_ref(),
                            self.equity_repo.as_ref(),
                            &*selector,
                            &criteria,
                            event,
                            entry_time,
                            exit_time,
                            &exec_config,
                        )
                    })
                    .collect();
                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    let entry_time = timing.entry_datetime(event);
                    let exit_time = timing.exit_datetime(event);
                    let result = backtest_use_case_helpers::execute_iron_butterfly(
                        self.options_repo.as_ref(),
                        self.equity_repo.as_ref(),
                        &*selector,
                        &criteria,
                        event,
                        entry_time,
                        exit_time,
                        &exec_config,
                    ).await;
                    results.push(result);
                }
                results
            };

            // Collect results
            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                if let Some(trade_result) = result {
                    all_results.push(trade_result);
                    session_entries += 1;
                }
            }

            if let Some(ref callback) = on_progress {
                callback(SessionProgress {
                    session_date,
                    entries_count: session_entries,
                    events_found: to_enter.len(),
                });
            }
        }

        let total_entries = all_results.len();

        info!(
            sessions_processed,
            total_opportunities,
            results_count = total_entries,
            dropped_count = dropped_events.len(),
            "Iron butterfly backtest completed"
        );

        Ok(BacktestResult {
            results: all_results,
            sessions_processed,
            total_entries,
            total_opportunities,
            dropped_events,
        })
    }

    pub async fn execute_straddle(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<StraddleResult>, BacktestError> {
        let mut all_results: Vec<StraddleResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        // Create selector and criteria
        let selector = self.create_selector();
        let criteria = self.build_expiration_criteria();
        let exec_config = ExecutionConfig::for_straddle(self.config.max_entry_iv);

        // Create straddle timing
        let timing_impl = StraddleTradeTiming::new(self.config.timing)
            .with_entry_days(self.config.straddle_entry_days)
            .with_exit_days(self.config.straddle_exit_days);

        // Wrap in TimingStrategy enum
        use crate::timing_strategy::TimingStrategy;
        let timing = TimingStrategy::Straddle(timing_impl);

        info!(
            entry_days = self.config.straddle_entry_days,
            exit_days = self.config.straddle_exit_days,
            "Starting straddle backtest"
        );

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings for wider window (need events where entry falls on session_date)
            // Entry is N days before earnings, so look for earnings N days ahead
            // Use timing-aware lookahead calculation
            let lookahead = timing.lookahead_days();
            let events_end = session_date + chrono::Duration::days(lookahead);
            let events = self.earnings_repo
                .load_earnings(session_date, events_end, self.config.symbols.as_deref())
                .await
                .map_err(|e| BacktestError::Repository(e.to_string()))?;

            // DEBUG: Log loaded earnings and entry dates
            if !events.is_empty() {
                debug!(
                    session_date = %session_date,
                    events_count = events.len(),
                    "Loaded earnings for session"
                );
                for event in &events {
                    let entry_date = timing.entry_date(event);
                    let passes_mc = self.passes_market_cap_filter(event);
                    debug!(
                        symbol = %event.symbol,
                        earnings_date = %event.earnings_date,
                        entry_date = %entry_date,
                        matches_session = entry_date == session_date,
                        passes_market_cap = passes_mc,
                        "Event timing check"
                    );
                }
            }

            // Filter: Entry date == session_date
            let to_enter: Vec<_> = events
                .iter()
                .filter(|e| timing.entry_date(e) == session_date)
                .filter(|e| self.passes_market_cap_filter(e))
                .cloned()
                .collect();

            if to_enter.is_empty() {
                if let Some(ref callback) = on_progress {
                    callback(SessionProgress {
                        session_date,
                        entries_count: 0,
                        events_found: 0,
                    });
                }
                continue;
            }

            debug!(
                session_date = %session_date,
                events_count = to_enter.len(),
                "Processing straddle session"
            );

            // Process events using helper
            let session_results: Vec<_> = if self.config.parallel {
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| {
                        let entry_time = timing.entry_datetime(event);
                        let exit_time = timing.exit_datetime(event);
                        backtest_use_case_helpers::execute_straddle(
                            self.options_repo.as_ref(),
                            self.equity_repo.as_ref(),
                            &*selector,
                            &criteria,
                            event,
                            entry_time,
                            exit_time,
                            &exec_config,
                        )
                    })
                    .collect();
                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    let entry_time = timing.entry_datetime(event);
                    let exit_time = timing.exit_datetime(event);
                    let result = backtest_use_case_helpers::execute_straddle(
                        self.options_repo.as_ref(),
                        self.equity_repo.as_ref(),
                        &*selector,
                        &criteria,
                        event,
                        entry_time,
                        exit_time,
                        &exec_config,
                    ).await;
                    results.push(result);
                }
                results
            };

            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                if let Some(trade_result) = result {
                    all_results.push(trade_result);
                    session_entries += 1;
                }
            }

            if let Some(ref callback) = on_progress {
                callback(SessionProgress {
                    session_date,
                    entries_count: session_entries,
                    events_found: to_enter.len(),
                });
            }
        }

        let total_entries = all_results.len();

        info!(
            sessions_processed,
            total_opportunities,
            results_count = total_entries,
            dropped_count = dropped_events.len(),
            "Straddle backtest completed"
        );

        Ok(BacktestResult {
            results: all_results,
            sessions_processed,
            total_entries,
            total_opportunities,
            dropped_events,
        })
    }

    /// Execute post-earnings straddle backtest
    ///
    /// Post-earnings straddle enters AFTER earnings (when IV has crushed) and holds
    /// for ~1 week to capture continued stock movement. Unlike pre-earnings straddle,
    /// this benefits from lower entry IV.
    pub async fn execute_post_earnings_straddle(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<StraddleResult>, BacktestError> {
        let mut all_results: Vec<StraddleResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        // Create selector and criteria
        let selector = self.create_selector();
        let criteria = self.build_expiration_criteria();
        let exec_config = ExecutionConfig::for_straddle(self.config.max_entry_iv);

        // Create post-earnings timing
        let timing_impl = PostEarningsStraddleTiming::new(self.config.timing)
            .with_holding_days(self.config.post_earnings_holding_days);

        // Wrap in TimingStrategy enum
        use crate::timing_strategy::TimingStrategy;
        let timing = TimingStrategy::PostEarnings(timing_impl);

        info!(
            holding_days = self.config.post_earnings_holding_days,
            "Starting post-earnings straddle backtest"
        );

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings events (look backwards since we enter AFTER earnings)
            // Entry can be same day (BMO) or next day (AMC), so check 1-2 days back
            let lookback_days = 3;  // Buffer for weekends
            let events_start = session_date - chrono::Duration::days(lookback_days);
            let events = self.earnings_repo
                .load_earnings(events_start, session_date, self.config.symbols.as_deref())
                .await
                .map_err(|e| BacktestError::Repository(e.to_string()))?;

            // Filter: Entry date == session_date
            let to_enter: Vec<_> = events
                .iter()
                .filter(|e| timing.entry_date(e) == session_date)
                .filter(|e| self.passes_market_cap_filter(e))
                .cloned()
                .collect();

            if to_enter.is_empty() {
                if let Some(ref callback) = on_progress {
                    callback(SessionProgress {
                        session_date,
                        entries_count: 0,
                        events_found: 0,
                    });
                }
                continue;
            }

            debug!(
                session_date = %session_date,
                events_count = to_enter.len(),
                "Processing post-earnings straddle session"
            );

            // Process events using helper
            let session_results: Vec<_> = if self.config.parallel {
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| {
                        let entry_time = timing.entry_datetime(event);
                        let exit_time = timing.exit_datetime(event);
                        backtest_use_case_helpers::execute_straddle(
                            self.options_repo.as_ref(),
                            self.equity_repo.as_ref(),
                            &*selector,
                            &criteria,
                            event,
                            entry_time,
                            exit_time,
                            &exec_config,
                        )
                    })
                    .collect();
                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    let entry_time = timing.entry_datetime(event);
                    let exit_time = timing.exit_datetime(event);
                    let result = backtest_use_case_helpers::execute_straddle(
                        self.options_repo.as_ref(),
                        self.equity_repo.as_ref(),
                        &*selector,
                        &criteria,
                        event,
                        entry_time,
                        exit_time,
                        &exec_config,
                    ).await;
                    results.push(result);
                }
                results
            };

            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                if let Some(trade_result) = result {
                    all_results.push(trade_result);
                    session_entries += 1;
                }
            }

            if let Some(ref callback) = on_progress {
                callback(SessionProgress {
                    session_date,
                    entries_count: session_entries,
                    events_found: to_enter.len(),
                });
            }
        }

        let total_entries = all_results.len();

        info!(
            sessions_processed,
            total_opportunities,
            results_count = total_entries,
            dropped_count = dropped_events.len(),
            "Post-earnings straddle backtest completed"
        );

        Ok(BacktestResult {
            results: all_results,
            sessions_processed,
            total_entries,
            total_opportunities,
            dropped_events,
        })
    }

    /// Execute calendar straddle backtest
    ///
    /// Calendar straddle uses the same timing as calendar spreads (EarningsTradeTiming):
    /// - Entry: Day of/before earnings (AMC/BMO aware)
    /// - Exit: Day after earnings (post IV crush)
    pub async fn execute_calendar_straddle(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<CalendarStraddleResult>, BacktestError> {
        let mut all_results: Vec<CalendarStraddleResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        // Create selector and criteria
        let selector = self.create_selector();
        let criteria = self.build_expiration_criteria();
        let exec_config = ExecutionConfig::for_straddle(self.config.max_entry_iv); // Calendar straddle uses same config as straddle

        // Calendar straddles use earnings timing (enter on/before earnings day)
        use crate::timing_strategy::TimingStrategy;
        let timing = TimingStrategy::Earnings(EarningsTradeTiming::new(self.config.timing));

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings for this session
            let events = self.load_earnings_window(session_date).await?;
            let to_enter = self.filter_for_entry(&events, session_date, &timing);

            if to_enter.is_empty() {
                if let Some(ref callback) = on_progress {
                    callback(SessionProgress {
                        session_date,
                        entries_count: 0,
                        events_found: 0,
                    });
                }
                continue;
            }

            debug!(
                session_date = %session_date,
                events_count = to_enter.len(),
                "Processing calendar straddle session"
            );

            // Process events using helper
            let session_results: Vec<_> = if self.config.parallel {
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| {
                        let entry_time = timing.entry_datetime(event);
                        let exit_time = timing.exit_datetime(event);
                        backtest_use_case_helpers::execute_calendar_straddle(
                            self.options_repo.as_ref(),
                            self.equity_repo.as_ref(),
                            &*selector,
                            &criteria,
                            event,
                            entry_time,
                            exit_time,
                            &exec_config,
                        )
                    })
                    .collect();
                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    let entry_time = timing.entry_datetime(event);
                    let exit_time = timing.exit_datetime(event);
                    let result = backtest_use_case_helpers::execute_calendar_straddle(
                        self.options_repo.as_ref(),
                        self.equity_repo.as_ref(),
                        &*selector,
                        &criteria,
                        event,
                        entry_time,
                        exit_time,
                        &exec_config,
                    ).await;
                    results.push(result);
                }
                results
            };

            // Collect results and apply IV filter
            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                if let Some(trade_result) = result {
                    // Apply IV ratio filter if configured
                    if self.passes_calendar_straddle_iv_filter(&trade_result) {
                        all_results.push(trade_result);
                        session_entries += 1;
                    } else {
                        dropped_events.push(TradeGenerationError {
                            symbol: trade_result.symbol.clone(),
                            earnings_date: trade_result.earnings_date,
                            earnings_time: trade_result.earnings_time,
                            reason: "IV_RATIO_FILTER".into(),
                            details: trade_result.iv_ratio().map(|r| format!("IV ratio: {:.2}", r)),
                            phase: "filter".into(),
                        });
                    }
                }
            }

            if let Some(ref callback) = on_progress {
                callback(SessionProgress {
                    session_date,
                    entries_count: session_entries,
                    events_found: to_enter.len(),
                });
            }
        }

        let total_entries = all_results.len();

        info!(
            sessions_processed,
            total_opportunities,
            results_count = total_entries,
            dropped_count = dropped_events.len(),
            "Calendar straddle backtest completed"
        );

        Ok(BacktestResult {
            results: all_results,
            sessions_processed,
            total_entries,
            total_opportunities,
            dropped_events,
        })
    }
    fn create_strategy(&self) -> Box<dyn SelectionStrategy> {
        match self.config.selection_strategy {
            SelectionType::ATM => Box::new(
                ATMStrategy::new(self.config.selection.clone())
                    .with_strike_match_mode(self.config.strike_match_mode)
            ),
            SelectionType::Delta => Box::new(
                DeltaStrategy::fixed(
                    self.config.target_delta,
                    self.config.selection.clone(),
                )
                .with_strike_match_mode(self.config.strike_match_mode)
            ),
            SelectionType::DeltaScan => Box::new(
                DeltaStrategy::scanning(
                    self.config.delta_range,
                    self.config.delta_scan_steps,
                    self.config.selection.clone(),
                )
                .with_strike_match_mode(self.config.strike_match_mode)
            ),
        }
    }

    fn create_iron_butterfly_strategy(&self) -> IronButterflyStrategy {
        IronButterflyStrategy::new(
            rust_decimal::Decimal::try_from(self.config.wing_width).unwrap_or(rust_decimal::Decimal::new(10, 0)),
            self.config.selection.min_short_dte,
            self.config.selection.max_short_dte,
        )
    }

    /// Create strike selector based on config
    pub fn create_selector(&self) -> Box<dyn StrikeSelector> {
        match self.config.selection_strategy {
            SelectionType::ATM => Box::new(
                ATMStrategy::new(self.config.selection.clone())
                    .with_strike_match_mode(self.config.strike_match_mode)
            ),
            SelectionType::Delta => Box::new(
                DeltaStrategy::fixed(
                    self.config.target_delta,
                    self.config.selection.clone(),
                )
                .with_strike_match_mode(self.config.strike_match_mode)
            ),
            SelectionType::DeltaScan => Box::new(
                DeltaStrategy::scanning(
                    self.config.delta_range,
                    self.config.delta_scan_steps,
                    self.config.selection.clone(),
                )
                .with_strike_match_mode(self.config.strike_match_mode)
            ),
        }
    }

    /// Build expiration criteria from config
    fn build_expiration_criteria(&self) -> ExpirationCriteria {
        ExpirationCriteria::new(
            self.config.selection.min_short_dte,
            self.config.selection.max_short_dte,
            self.config.selection.min_long_dte,
            self.config.selection.max_long_dte,
        )
    }


    fn passes_iv_filter(&self, result: &CalendarSpreadResult) -> bool {
        match (self.config.selection.min_iv_ratio, result.iv_ratio()) {
            (Some(min), Some(ratio)) => ratio >= min,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    fn passes_calendar_straddle_iv_filter(&self, result: &CalendarStraddleResult) -> bool {
        match (self.config.selection.min_iv_ratio, result.iv_ratio()) {
            (Some(min), Some(ratio)) => ratio >= min,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    async fn load_earnings_window(&self, session_date: NaiveDate) -> Result<Vec<EarningsEvent>, BacktestError> {
        let start = TradingCalendar::previous_trading_day(session_date);
        let end = TradingCalendar::next_trading_day(session_date);
        self.earnings_repo
            .load_earnings(start, end, self.config.symbols.as_deref())
            .await
            .map_err(|e| BacktestError::Repository(e.to_string()))
    }

    fn filter_for_entry(
        &self,
        events: &[EarningsEvent],
        session_date: NaiveDate,
        timing: &crate::timing_strategy::TimingStrategy,
    ) -> Vec<EarningsEvent> {
        events
            .iter()
            .filter(|e| timing.entry_date(e) == session_date)
            .filter(|e| self.passes_market_cap_filter(e))
            .cloned()
            .collect()
    }

    fn passes_market_cap_filter(&self, event: &EarningsEvent) -> bool {
        match (self.config.min_market_cap, event.market_cap) {
            (Some(min), Some(cap)) => cap >= min,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    /// Check if option chain meets minimum daily notional threshold
    /// Calculates: sum(all option volumes) × 100 × stock_price
    fn passes_notional_filter(
        &self,
        chain_df: &polars::frame::DataFrame,
        spot_price: rust_decimal::Decimal,
        event: &EarningsEvent,
    ) -> Result<bool, TradeGenerationError> {
        use polars::prelude::*;

        // If no filter configured, pass
        let Some(min_notional) = self.config.min_notional else {
            return Ok(true);
        };

        // Sum all volumes in the option chain
        let volume_col = chain_df
            .column("volume")
            .map_err(|e| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NOTIONAL_FILTER_ERROR".into(),
                details: Some(format!("Failed to read volume column: {}", e)),
                phase: "notional_filter".into(),
            })?;

        let total_volume: i64 = volume_col
            .i64()
            .map_err(|e| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NOTIONAL_FILTER_ERROR".into(),
                details: Some(format!("Failed to cast volume to i64: {}", e)),
                phase: "notional_filter".into(),
            })?
            .sum()
            .unwrap_or(0);

        // Calculate total notional: volume × 100 shares × stock price
        // Convert Decimal to f64 using string conversion (safe for display values)
        let spot_f64: f64 = spot_price.to_string().parse().unwrap_or(0.0);
        let daily_notional = (total_volume as f64) * 100.0 * spot_f64;

        if daily_notional < min_notional {
            debug!(
                symbol = %event.symbol,
                spot = %spot_price,
                total_volume = total_volume,
                daily_notional = daily_notional,
                min_required = min_notional,
                "Rejected: daily option notional below minimum"
            );
            return Ok(false);
        }

        Ok(true)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BacktestError {
    #[error("Repository error: {0}")]
    Repository(String),
    #[error("Strategy error: {0}")]
    Strategy(String),
    #[error("Pricing error: {0}")]
    Pricing(String),
}
