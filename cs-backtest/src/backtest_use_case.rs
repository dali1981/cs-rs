use std::sync::Arc;
use chrono::NaiveDate;
use tracing::{info, debug};

use cs_domain::*;
use cs_domain::timing::{EarningsTradeTiming, StraddleTradeTiming, PostEarningsStraddleTiming};
use cs_domain::strike_selection::{DeltaStrategy, IronButterflyStrategy, StraddleStrategy};
use crate::config::{BacktestConfig, SpreadType, SelectionType};
use crate::trade_executor::TradeExecutor;
use crate::iron_butterfly_executor::IronButterflyExecutor;
use crate::straddle_executor::StraddleExecutor;
use crate::calendar_straddle_executor::CalendarStraddleExecutor;
use crate::iv_surface_builder::build_iv_surface_minute_aligned;

/// Unified trade result (calendar spread, iron butterfly, straddle, or calendar straddle)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TradeResult {
    CalendarSpread(CalendarSpreadResult),
    IronButterfly(IronButterflyResult),
    Straddle(StraddleResult),
    CalendarStraddle(CalendarStraddleResult),
}

impl TradeResult {
    pub fn is_winner(&self) -> bool {
        match self {
            TradeResult::CalendarSpread(r) => r.is_winner(),
            TradeResult::IronButterfly(r) => r.is_winner(),
            TradeResult::Straddle(r) => r.is_winner(),
            TradeResult::CalendarStraddle(r) => r.is_winner(),
        }
    }

    pub fn success(&self) -> bool {
        match self {
            TradeResult::CalendarSpread(r) => r.success,
            TradeResult::IronButterfly(r) => r.success,
            TradeResult::Straddle(r) => r.success,
            TradeResult::CalendarStraddle(r) => r.success,
        }
    }

    pub fn pnl(&self) -> rust_decimal::Decimal {
        match self {
            TradeResult::CalendarSpread(r) => r.pnl,
            TradeResult::IronButterfly(r) => r.pnl,
            TradeResult::Straddle(r) => r.pnl,
            TradeResult::CalendarStraddle(r) => r.pnl,
        }
    }

    pub fn pnl_pct(&self) -> rust_decimal::Decimal {
        match self {
            TradeResult::CalendarSpread(r) => r.pnl_pct,
            TradeResult::IronButterfly(r) => r.pnl_pct,
            TradeResult::Straddle(r) => r.pnl_pct,
            TradeResult::CalendarStraddle(r) => r.pnl_pct,
        }
    }

    pub fn symbol(&self) -> &str {
        match self {
            TradeResult::CalendarSpread(r) => &r.symbol,
            TradeResult::IronButterfly(r) => &r.symbol,
            TradeResult::Straddle(r) => &r.symbol,
            TradeResult::CalendarStraddle(r) => &r.symbol,
        }
    }

    pub fn option_type(&self) -> Option<finq_core::OptionType> {
        match self {
            TradeResult::CalendarSpread(r) => Some(r.option_type),
            TradeResult::IronButterfly(_) => None, // Has both call and put
            TradeResult::Straddle(_) => None, // Has both call and put
            TradeResult::CalendarStraddle(_) => None, // Has both call and put
        }
    }

    pub fn strike(&self) -> Strike {
        match self {
            TradeResult::CalendarSpread(r) => r.strike,
            TradeResult::IronButterfly(r) => r.center_strike,
            TradeResult::Straddle(r) => r.strike,
            TradeResult::CalendarStraddle(r) => r.short_strike, // Use short strike as reference
        }
    }
}

/// Backtest execution result
#[derive(Debug)]
pub struct BacktestResult {
    pub results: Vec<TradeResult>,
    pub sessions_processed: usize,
    pub total_entries: usize,
    pub total_opportunities: usize,
    pub dropped_events: Vec<TradeGenerationError>,
}

impl BacktestResult {
    pub fn win_rate(&self) -> f64 {
        let winners = self.results.iter().filter(|r| r.is_winner()).count();
        let successful_trades = self.results.iter().filter(|r| r.success()).count();
        if successful_trades == 0 {
            0.0
        } else {
            winners as f64 / successful_trades as f64
        }
    }

    pub fn total_pnl(&self) -> rust_decimal::Decimal {
        // Only sum PnL from successful trades
        self.results.iter()
            .filter(|r| r.success())
            .map(|r| r.pnl())
            .sum()
    }

    pub fn successful_trades(&self) -> usize {
        self.results.iter().filter(|r| r.success()).count()
    }

    /// Get percentage returns for statistical analysis
    fn pnl_pcts(&self) -> Vec<f64> {
        self.results.iter()
            .filter(|r| r.success())
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
            .filter(|r| r.success() && r.pnl() < rust_decimal::Decimal::ZERO)
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
            .filter(|r| r.success() && r.pnl() < rust_decimal::Decimal::ZERO)
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
pub struct BacktestUseCase<Earn, Opt, Eq>
where
    Earn: EarningsRepository,
    Opt: OptionsDataRepository,
    Eq: EquityDataRepository,
{
    earnings_repo: Arc<Earn>,
    options_repo: Arc<Opt>,
    equity_repo: Arc<Eq>,
    config: BacktestConfig,
    earnings_timing: EarningsTradeTiming,
}

impl<Earn, Opt, Eq> BacktestUseCase<Earn, Opt, Eq>
where
    Earn: EarningsRepository + 'static,
    Opt: OptionsDataRepository + 'static,
    Eq: EquityDataRepository + 'static,
{
    pub fn new(
        earnings_repo: Earn,
        options_repo: Opt,
        equity_repo: Eq,
        config: BacktestConfig,
    ) -> Self {
        let earnings_timing = EarningsTradeTiming::new(config.timing);
        Self {
            earnings_repo: Arc::new(earnings_repo),
            options_repo: Arc::new(options_repo),
            equity_repo: Arc::new(equity_repo),
            config,
            earnings_timing,
        }
    }

    pub async fn execute(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        option_type: finq_core::OptionType,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult, BacktestError> {
        let mut all_results: Vec<TradeResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        info!(
            start_date = %start_date,
            end_date = %end_date,
            option_type = ?option_type,
            spread = ?self.config.spread,
            selection = ?self.config.selection_strategy,
            "Starting backtest"
        );

        // Branch based on spread type
        match self.config.spread {
            SpreadType::IronButterfly => {
                self.execute_iron_butterfly(start_date, end_date, on_progress).await
            }
            SpreadType::Calendar => {
                self.execute_calendar_spread(start_date, end_date, option_type, on_progress).await
            }
            SpreadType::Straddle => {
                self.execute_straddle(start_date, end_date, on_progress).await
            }
            SpreadType::CalendarStraddle => {
                self.execute_calendar_straddle(start_date, end_date, on_progress).await
            }
            SpreadType::PostEarningsStraddle => {
                self.execute_post_earnings_straddle(start_date, end_date, on_progress).await
            }
        }
    }

    async fn execute_calendar_spread(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        option_type: finq_core::OptionType,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult, BacktestError> {
        let mut all_results: Vec<TradeResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        let strategy = self.create_strategy();

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings for this session
            let events = self.load_earnings_window(session_date).await?;
            let to_enter = self.filter_for_entry(&events, session_date);

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
                "Processing session"
            );

            // Process events (parallel or sequential)
            let session_results: Vec<_> = if self.config.parallel {
                // Use futures::future::join_all for concurrent async processing
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| self.process_earnings_event(event, &*strategy, OptionStrategy::CalendarSpread, Some(option_type)))
                    .collect();

                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    results.push(
                        self.process_earnings_event(event, &*strategy, OptionStrategy::CalendarSpread, Some(option_type)).await
                    );
                }
                results
            };

            // Collect results
            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                match result {
                    Ok(TradeResult::CalendarSpread(trade_result)) => {
                        if self.passes_iv_filter(&trade_result) {
                            all_results.push(TradeResult::CalendarSpread(trade_result));
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
                    Ok(TradeResult::IronButterfly(_)) => {
                        // This should never happen when processing calendar spreads
                        unreachable!("Got iron butterfly result when processing calendar spread");
                    }
                    Ok(TradeResult::Straddle(_)) => {
                        // This should never happen when processing calendar spreads
                        unreachable!("Got straddle result when processing calendar spread");
                    }
                    Ok(TradeResult::CalendarStraddle(_)) => {
                        // This should never happen when processing calendar spreads
                        unreachable!("Got calendar straddle result when processing calendar spread");
                    }
                    Err(e) => dropped_events.push(e),
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

    async fn execute_iron_butterfly(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult, BacktestError> {
        let mut all_results: Vec<TradeResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        // Create iron butterfly strategy
        let strategy = self.create_iron_butterfly_strategy();

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings for this session
            let events = self.load_earnings_window(session_date).await?;
            let to_enter = self.filter_for_entry(&events, session_date);

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

            // Process events (parallel or sequential)
            let session_results: Vec<_> = if self.config.parallel {
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| self.process_earnings_event(event, &strategy, OptionStrategy::IronButterfly, None))
                    .collect();

                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    results.push(
                        self.process_earnings_event(event, &strategy, OptionStrategy::IronButterfly, None).await
                    );
                }
                results
            };

            // Collect results
            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                match result {
                    Ok(TradeResult::IronButterfly(trade_result)) => {
                        all_results.push(TradeResult::IronButterfly(trade_result));
                        session_entries += 1;
                    }
                    Ok(TradeResult::CalendarSpread(_)) => {
                        // This should never happen when processing iron butterflies
                        unreachable!("Got calendar spread result when processing iron butterfly");
                    }
                    Ok(TradeResult::Straddle(_)) => {
                        // This should never happen when processing iron butterflies
                        unreachable!("Got straddle result when processing iron butterfly");
                    }
                    Ok(TradeResult::CalendarStraddle(_)) => {
                        // This should never happen when processing iron butterflies
                        unreachable!("Got calendar straddle result when processing iron butterfly");
                    }
                    Err(e) => dropped_events.push(e),
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

    async fn execute_straddle(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult, BacktestError> {
        let mut all_results: Vec<TradeResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        // Create straddle timing
        let timing = StraddleTradeTiming::new(self.config.timing)
            .with_entry_days(self.config.straddle_entry_days)
            .with_exit_days(self.config.straddle_exit_days);

        info!(
            entry_days = self.config.straddle_entry_days,
            exit_days = self.config.straddle_exit_days,
            "Starting straddle backtest"
        );

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings for wider window (need events where entry falls on session_date)
            // Entry is N days before earnings, so look for earnings N days ahead
            let lookahead = self.config.straddle_entry_days as i64 + 5;  // Buffer for weekends
            let events_end = session_date + chrono::Duration::days(lookahead);
            let events = self.earnings_repo
                .load_earnings(session_date, events_end, self.config.symbols.as_deref())
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
                "Processing straddle session"
            );

            // Process events
            let session_results: Vec<_> = if self.config.parallel {
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| self.process_straddle_event(event, &timing))
                    .collect();
                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    results.push(
                        self.process_straddle_event(event, &timing).await
                    );
                }
                results
            };

            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                match result {
                    Ok(straddle_result) => {
                        all_results.push(TradeResult::Straddle(straddle_result));
                        session_entries += 1;
                    }
                    Err(e) => dropped_events.push(e),
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
    async fn execute_post_earnings_straddle(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult, BacktestError> {
        let mut all_results: Vec<TradeResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        // Create post-earnings timing
        let timing = PostEarningsStraddleTiming::new(self.config.timing)
            .with_holding_days(self.config.post_earnings_holding_days);

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

            // Process events
            let session_results: Vec<_> = if self.config.parallel {
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| self.process_post_earnings_straddle_event(event, &timing))
                    .collect();
                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    results.push(
                        self.process_post_earnings_straddle_event(event, &timing).await
                    );
                }
                results
            };

            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                match result {
                    Ok(straddle_result) => {
                        all_results.push(TradeResult::Straddle(straddle_result));
                        session_entries += 1;
                    }
                    Err(e) => dropped_events.push(e),
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
    async fn execute_calendar_straddle(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult, BacktestError> {
        let mut all_results: Vec<TradeResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        let strategy = self.create_strategy();

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings for this session
            let events = self.load_earnings_window(session_date).await?;
            let to_enter = self.filter_for_entry(&events, session_date);

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

            // Process events (parallel or sequential)
            let session_results: Vec<_> = if self.config.parallel {
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| self.process_earnings_event(event, &*strategy, OptionStrategy::CalendarStraddle, None))
                    .collect();

                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    results.push(
                        self.process_earnings_event(event, &*strategy, OptionStrategy::CalendarStraddle, None).await
                    );
                }
                results
            };

            // Collect results
            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                match result {
                    Ok(TradeResult::CalendarStraddle(trade_result)) => {
                        // Apply IV ratio filter if configured
                        if self.passes_calendar_straddle_iv_filter(&trade_result) {
                            all_results.push(TradeResult::CalendarStraddle(trade_result));
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
                    Ok(_) => {
                        unreachable!("Got non-CalendarStraddle result when processing calendar straddle");
                    }
                    Err(e) => dropped_events.push(e),
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

    async fn process_straddle_event(
        &self,
        event: &EarningsEvent,
        timing: &StraddleTradeTiming,
    ) -> Result<StraddleResult, TradeGenerationError> {
        let entry_time = timing.entry_datetime(event);
        let exit_time = timing.exit_datetime(event);
        let entry_date = entry_time.date_naive();

        // Create strategy with entry date and min DTE from config
        let strategy = StraddleStrategy::with_min_dte(
            self.config.min_straddle_dte,
            entry_date
        );

        // Get spot price at entry
        let spot = self.equity_repo
            .get_spot_price(&event.symbol, entry_time)
            .await
            .map_err(|_| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_SPOT_PRICE".into(),
                details: Some(format!("No spot price at {}", entry_time)),
                phase: "spot_price".into(),
            })?;

        // Get option chain data with timestamps for minute-aligned IV computation
        let chain_df = self.options_repo
            .get_option_bars_at_time(&event.symbol, entry_time)
            .await
            .map_err(|_| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_OPTIONS_DATA".into(),
                details: Some(format!("No option data at {}", entry_time)),
                phase: "option_data".into(),
            })?;

        // Check minimum daily notional filter
        if !self.passes_notional_filter(&chain_df, spot.value, event)? {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "INSUFFICIENT_NOTIONAL".into(),
                details: Some("Daily option notional below minimum threshold".to_string()),
                phase: "notional_filter".into(),
            });
        }

        // Get available expirations and strikes at entry
        let expirations = self.options_repo
            .get_available_expirations(&event.symbol, entry_date)
            .await
            .unwrap_or_default();

        if expirations.is_empty() {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_EXPIRATIONS".into(),
                details: None,
                phase: "chain_data".into(),
            });
        }

        // Filter expirations to those after earnings
        let valid_expirations: Vec<_> = expirations
            .iter()
            .filter(|&&exp| exp > event.earnings_date)
            .copied()
            .collect();

        if valid_expirations.is_empty() {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_POST_EARNINGS_EXPIRATION".into(),
                details: Some("Need expiration after earnings date".into()),
                phase: "chain_data".into(),
            });
        }

        // Get strikes available across ALL valid expirations
        let mut all_strikes = std::collections::HashSet::new();
        for &expiration in &valid_expirations {
            let exp_strikes = self.options_repo
                .get_available_strikes(&event.symbol, expiration, entry_date)
                .await
                .unwrap_or_default();
            all_strikes.extend(exp_strikes);
        }
        let mut strikes: Vec<_> = all_strikes.into_iter().collect();
        strikes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        if strikes.is_empty() {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_STRIKES".into(),
                details: None,
                phase: "chain_data".into(),
            });
        }

        // Build IV surface with per-option spot prices (minute-aligned)
        let iv_surface = build_iv_surface_minute_aligned(
            &chain_df,
            self.equity_repo.as_ref(),
            &event.symbol,
        ).await;

        let chain_data = OptionChainData {
            expirations: valid_expirations,
            strikes,
            deltas: None,
            volumes: None,
            iv_ratios: None,
            iv_surface,
        };

        // Select straddle
        let straddle = strategy.select_straddle(event, &spot, &chain_data)
            .map_err(|e| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "STRATEGY_SELECTION_FAILED".into(),
                details: Some(e.to_string()),
                phase: "strategy".into(),
            })?;

        // Execute trade
        let executor = StraddleExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
        )
        .with_pricing_model(self.config.pricing_model)
        .with_max_entry_iv(self.config.max_entry_iv);

        let result = executor.execute_trade(&straddle, event, entry_time, exit_time).await;

        if !result.success {
            return Err(TradeGenerationError {
                symbol: result.symbol,
                earnings_date: result.earnings_date,
                earnings_time: result.earnings_time,
                reason: result.failure_reason.map(|r| format!("{:?}", r)).unwrap_or("UNKNOWN".into()),
                details: None,
                phase: "execution".into(),
            });
        }

        // Filter by entry price if configured
        let entry_price: f64 = result.entry_debit.to_string().parse().unwrap_or(0.0);

        if let Some(min_price) = self.config.min_entry_price {
            if entry_price < min_price {
                return Err(TradeGenerationError {
                    symbol: result.symbol,
                    earnings_date: result.earnings_date,
                    earnings_time: result.earnings_time,
                    reason: "ENTRY_PRICE_TOO_LOW".into(),
                    details: Some(format!("Entry price ${:.2} < min ${:.2}", entry_price, min_price)),
                    phase: "entry_price_filter".into(),
                });
            }
        }

        if let Some(max_price) = self.config.max_entry_price {
            if entry_price > max_price {
                return Err(TradeGenerationError {
                    symbol: result.symbol,
                    earnings_date: result.earnings_date,
                    earnings_time: result.earnings_time,
                    reason: "ENTRY_PRICE_TOO_HIGH".into(),
                    details: Some(format!("Entry price ${:.2} > max ${:.2}", entry_price, max_price)),
                    phase: "entry_price_filter".into(),
                });
            }
        }

        Ok(result)
    }

    /// Process a single post-earnings straddle event
    async fn process_post_earnings_straddle_event(
        &self,
        event: &EarningsEvent,
        timing: &PostEarningsStraddleTiming,
    ) -> Result<StraddleResult, TradeGenerationError> {
        let entry_time = timing.entry_datetime(event);
        let exit_time = timing.exit_datetime(event);
        let entry_date = entry_time.date_naive();

        // Create strategy with entry date and min DTE from config
        // For post-earnings straddle, we need expiration beyond the exit date
        let strategy = StraddleStrategy::with_min_dte(
            self.config.min_straddle_dte,
            entry_date
        );

        // Get spot price at entry
        let spot = self.equity_repo
            .get_spot_price(&event.symbol, entry_time)
            .await
            .map_err(|_| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_SPOT_PRICE".into(),
                details: Some(format!("No spot price at {}", entry_time)),
                phase: "spot_price".into(),
            })?;

        // Get option chain data with timestamps for minute-aligned IV computation
        let chain_df = self.options_repo
            .get_option_bars_at_time(&event.symbol, entry_time)
            .await
            .map_err(|_| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_OPTIONS_DATA".into(),
                details: Some(format!("No option data at {}", entry_time)),
                phase: "option_data".into(),
            })?;

        // Check minimum daily notional filter
        if !self.passes_notional_filter(&chain_df, spot.value, event)? {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "INSUFFICIENT_NOTIONAL".into(),
                details: Some("Daily option notional below minimum threshold".to_string()),
                phase: "notional_filter".into(),
            });
        }

        // Get available expirations and strikes at entry
        let expirations = self.options_repo
            .get_available_expirations(&event.symbol, entry_date)
            .await
            .unwrap_or_default();

        if expirations.is_empty() {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_EXPIRATIONS".into(),
                details: None,
                phase: "chain_data".into(),
            });
        }

        // For post-earnings straddle, we want expirations AFTER the exit date
        // to ensure we can hold the position for the full holding period
        let exit_date = timing.exit_date(event);
        let valid_expirations: Vec<_> = expirations
            .iter()
            .filter(|&&exp| exp > exit_date)
            .copied()
            .collect();

        if valid_expirations.is_empty() {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_POST_EXIT_EXPIRATION".into(),
                details: Some(format!("Need expiration after exit date {}", exit_date)),
                phase: "chain_data".into(),
            });
        }

        // Get strikes available across ALL valid expirations
        let mut all_strikes = std::collections::HashSet::new();
        for &expiration in &valid_expirations {
            let exp_strikes = self.options_repo
                .get_available_strikes(&event.symbol, expiration, entry_date)
                .await
                .unwrap_or_default();
            all_strikes.extend(exp_strikes);
        }
        let mut strikes: Vec<_> = all_strikes.into_iter().collect();
        strikes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        if strikes.is_empty() {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_STRIKES".into(),
                details: None,
                phase: "chain_data".into(),
            });
        }

        // Build IV surface with per-option spot prices (minute-aligned)
        let iv_surface = build_iv_surface_minute_aligned(
            &chain_df,
            self.equity_repo.as_ref(),
            &event.symbol,
        ).await;

        let chain_data = OptionChainData {
            expirations: valid_expirations,
            strikes,
            deltas: None,
            volumes: None,
            iv_ratios: None,
            iv_surface,
        };

        // Select straddle
        let straddle = strategy.select_straddle(event, &spot, &chain_data)
            .map_err(|e| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "STRATEGY_SELECTION_FAILED".into(),
                details: Some(e.to_string()),
                phase: "strategy".into(),
            })?;

        // Execute trade
        let executor = StraddleExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
        )
        .with_pricing_model(self.config.pricing_model)
        .with_max_entry_iv(self.config.max_entry_iv);

        let result = executor.execute_trade(&straddle, event, entry_time, exit_time).await;

        if !result.success {
            return Err(TradeGenerationError {
                symbol: result.symbol,
                earnings_date: result.earnings_date,
                earnings_time: result.earnings_time,
                reason: result.failure_reason.map(|r| format!("{:?}", r)).unwrap_or("UNKNOWN".into()),
                details: None,
                phase: "execution".into(),
            });
        }

        // Filter by entry price if configured
        let entry_price: f64 = result.entry_debit.to_string().parse().unwrap_or(0.0);

        if let Some(min_price) = self.config.min_entry_price {
            if entry_price < min_price {
                return Err(TradeGenerationError {
                    symbol: result.symbol,
                    earnings_date: result.earnings_date,
                    earnings_time: result.earnings_time,
                    reason: "ENTRY_PRICE_TOO_LOW".into(),
                    details: Some(format!("Entry price ${:.2} < min ${:.2}", entry_price, min_price)),
                    phase: "entry_price_filter".into(),
                });
            }
        }

        if let Some(max_price) = self.config.max_entry_price {
            if entry_price > max_price {
                return Err(TradeGenerationError {
                    symbol: result.symbol,
                    earnings_date: result.earnings_date,
                    earnings_time: result.earnings_time,
                    reason: "ENTRY_PRICE_TOO_HIGH".into(),
                    details: Some(format!("Entry price ${:.2} > max ${:.2}", entry_price, max_price)),
                    phase: "entry_price_filter".into(),
                });
            }
        }

        Ok(result)
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

    /// Unified earnings event processor - handles both calendar spreads and iron butterflies
    async fn process_earnings_event(
        &self,
        event: &EarningsEvent,
        strategy: &dyn SelectionStrategy,
        option_strategy: OptionStrategy,
        option_type: Option<finq_core::OptionType>,
    ) -> Result<TradeResult, TradeGenerationError> {
        // Use event-based timing for entry/exit
        let entry_time = self.earnings_timing.entry_datetime(event);
        let entry_date = entry_time.date_naive();

        // Get spot price
        let spot = self.equity_repo
            .get_spot_price(&event.symbol, entry_time)
            .await
            .map_err(|_| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_SPOT_PRICE".into(),
                details: Some(format!("No spot price at {}", entry_time)),
                phase: "spot_price".into(),
            })?;

        // Get option chain data with timestamps for minute-aligned IV computation
        let chain_df = self.options_repo
            .get_option_bars_at_time(&event.symbol, entry_time)
            .await
            .map_err(|_| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_OPTIONS_DATA".into(),
                details: Some(format!("No option data at {}", entry_time)),
                phase: "option_data".into(),
            })?;

        // Check minimum daily notional filter
        if !self.passes_notional_filter(&chain_df, spot.value, event)? {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "INSUFFICIENT_NOTIONAL".into(),
                details: Some("Daily option notional below minimum threshold".to_string()),
                phase: "notional_filter".into(),
            });
        }

        // Build IV surface with per-option spot prices (minute-aligned)
        let iv_surface = build_iv_surface_minute_aligned(
            &chain_df,
            self.equity_repo.as_ref(),
            &event.symbol,
        ).await;

        // Get available expirations and strikes
        let expirations = self.options_repo
            .get_available_expirations(&event.symbol, entry_date)
            .await
            .unwrap_or_default();

        let strikes = if !expirations.is_empty() {
            self.options_repo
                .get_available_strikes(&event.symbol, expirations[0], entry_date)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        if expirations.is_empty() || strikes.is_empty() {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "INSUFFICIENT_CHAIN_DATA".into(),
                details: Some(format!("expirations: {}, strikes: {}", expirations.len(), strikes.len())),
                phase: "chain_data".into(),
            });
        }

        // Build chain data for strategy
        let chain_data = OptionChainData {
            expirations,
            strikes,
            deltas: None,
            volumes: None,
            iv_ratios: None,
            iv_surface,
        };

        let exit_time = self.earnings_timing.exit_datetime(event);

        // Execute based on option strategy type
        match option_strategy {
            OptionStrategy::CalendarSpread => {
                let option_type = option_type.expect("CalendarSpread requires option_type");

                // Select spread
                let spread = strategy.select_calendar_spread(event, &spot, &chain_data, option_type)
                    .map_err(|e| TradeGenerationError {
                        symbol: event.symbol.clone(),
                        earnings_date: event.earnings_date,
                        earnings_time: event.earnings_time,
                        reason: "STRATEGY_SELECTION_FAILED".into(),
                        details: Some(e.to_string()),
                        phase: "strategy".into(),
                    })?;

                // Execute trade
                let executor = TradeExecutor::new(
                    self.options_repo.clone(),
                    self.equity_repo.clone(),
                )
                .with_pricing_model(self.config.pricing_model)
                .with_max_entry_iv(self.config.max_entry_iv);

                let result = executor.execute_trade(&spread, event, entry_time, exit_time).await;

                // Apply IV filter
                if !self.passes_iv_filter(&result) {
                    return Err(TradeGenerationError {
                        symbol: result.symbol.clone(),
                        earnings_date: result.earnings_date,
                        earnings_time: result.earnings_time,
                        reason: "IV_RATIO_FILTER".into(),
                        details: result.iv_ratio().map(|r| format!("IV ratio: {:.2}", r)),
                        phase: "filter".into(),
                    });
                }

                Ok(TradeResult::CalendarSpread(result))
            }
            OptionStrategy::IronButterfly => {
                // Select iron butterfly
                let butterfly = strategy.select_iron_butterfly(event, &spot, &chain_data)
                    .map_err(|e| TradeGenerationError {
                        symbol: event.symbol.clone(),
                        earnings_date: event.earnings_date,
                        earnings_time: event.earnings_time,
                        reason: "STRATEGY_ERROR".into(),
                        details: Some(e.to_string()),
                        phase: "strategy".into(),
                    })?;

                // Execute trade
                let executor = IronButterflyExecutor::new(
                    self.options_repo.clone(),
                    self.equity_repo.clone(),
                )
                .with_pricing_model(self.config.pricing_model)
                .with_max_entry_iv(self.config.max_entry_iv);

                let result = executor.execute_trade(&butterfly, event, entry_time, exit_time).await;

                if !result.success {
                    return Err(TradeGenerationError {
                        symbol: result.symbol,
                        earnings_date: result.earnings_date,
                        earnings_time: result.earnings_time,
                        reason: result.failure_reason.map(|r| format!("{:?}", r)).unwrap_or_else(|| "UNKNOWN".into()),
                        details: None,
                        phase: "execution".into(),
                    });
                }

                Ok(TradeResult::IronButterfly(result))
            }
            OptionStrategy::Straddle => {
                // Select straddle
                let straddle = strategy.select_straddle(event, &spot, &chain_data)
                    .map_err(|e| TradeGenerationError {
                        symbol: event.symbol.clone(),
                        earnings_date: event.earnings_date,
                        earnings_time: event.earnings_time,
                        reason: "STRATEGY_ERROR".into(),
                        details: Some(e.to_string()),
                        phase: "strategy".into(),
                    })?;

                // Execute trade
                let executor = StraddleExecutor::new(
                    self.options_repo.clone(),
                    self.equity_repo.clone(),
                )
                .with_pricing_model(self.config.pricing_model)
                .with_max_entry_iv(self.config.max_entry_iv);

                let result = executor.execute_trade(&straddle, event, entry_time, exit_time).await;

                if !result.success {
                    return Err(TradeGenerationError {
                        symbol: result.symbol,
                        earnings_date: result.earnings_date,
                        earnings_time: result.earnings_time,
                        reason: result.failure_reason.map(|r| format!("{:?}", r)).unwrap_or_else(|| "UNKNOWN".into()),
                        details: None,
                        phase: "execution".into(),
                    });
                }

                Ok(TradeResult::Straddle(result))
            }
            OptionStrategy::CalendarStraddle => {
                // Select calendar straddle
                let calendar_straddle = strategy.select_calendar_straddle(event, &spot, &chain_data)
                    .map_err(|e| TradeGenerationError {
                        symbol: event.symbol.clone(),
                        earnings_date: event.earnings_date,
                        earnings_time: event.earnings_time,
                        reason: "STRATEGY_ERROR".into(),
                        details: Some(e.to_string()),
                        phase: "strategy".into(),
                    })?;

                // Execute trade
                let executor = CalendarStraddleExecutor::new(
                    self.options_repo.clone(),
                    self.equity_repo.clone(),
                )
                .with_pricing_model(self.config.pricing_model)
                .with_max_entry_iv(self.config.max_entry_iv);

                let result = executor.execute_trade(&calendar_straddle, event, entry_time, exit_time).await;

                if !result.success {
                    return Err(TradeGenerationError {
                        symbol: result.symbol,
                        earnings_date: result.earnings_date,
                        earnings_time: result.earnings_time,
                        reason: result.failure_reason.map(|r| format!("{:?}", r)).unwrap_or_else(|| "UNKNOWN".into()),
                        details: None,
                        phase: "execution".into(),
                    });
                }

                Ok(TradeResult::CalendarStraddle(result))
            }
        }
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

    fn filter_for_entry(&self, events: &[EarningsEvent], session_date: NaiveDate) -> Vec<EarningsEvent> {
        events
            .iter()
            .filter(|e| self.should_enter_today(e, session_date))
            .filter(|e| self.passes_market_cap_filter(e))
            .cloned()
            .collect()
    }

    fn should_enter_today(&self, event: &EarningsEvent, session_date: NaiveDate) -> bool {
        // Use earnings_timing service to determine entry date
        self.earnings_timing.entry_date(event) == session_date
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
