use std::sync::Arc;
use chrono::NaiveDate;
use tracing::{info, debug};

use cs_domain::*;
use cs_domain::services::EarningsTradeTiming;
use cs_domain::DeltaStrategy;
use crate::config::{BacktestConfig, StrategyType};
use crate::trade_executor::TradeExecutor;
use crate::iv_surface_builder::build_iv_surface;

/// Backtest execution result
#[derive(Debug)]
pub struct BacktestResult {
    pub results: Vec<CalendarSpreadResult>,
    pub sessions_processed: usize,
    pub total_entries: usize,
    pub total_opportunities: usize,
    pub dropped_events: Vec<TradeGenerationError>,
}

impl BacktestResult {
    pub fn win_rate(&self) -> f64 {
        let winners = self.results.iter().filter(|r| r.is_winner()).count();
        let successful_trades = self.results.iter().filter(|r| r.success).count();
        if successful_trades == 0 {
            0.0
        } else {
            winners as f64 / successful_trades as f64
        }
    }

    pub fn total_pnl(&self) -> rust_decimal::Decimal {
        // Only sum PnL from successful trades
        self.results.iter()
            .filter(|r| r.success)
            .map(|r| r.pnl)
            .sum()
    }

    pub fn successful_trades(&self) -> usize {
        self.results.iter().filter(|r| r.success).count()
    }

    /// Get percentage returns for statistical analysis
    fn pnl_pcts(&self) -> Vec<f64> {
        self.results.iter()
            .filter(|r| r.success)
            .map(|r| {
                let pnl_pct: f64 = r.pnl_pct.try_into().unwrap_or(0.0);
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
            let sum: rust_decimal::Decimal = winners.iter().map(|r| r.pnl).sum();
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
                    let pct: f64 = r.pnl_pct.try_into().unwrap_or(0.0);
                    pct / 100.0
                })
                .sum();
            sum / winners.len() as f64
        }
    }

    /// Average losing trade (in dollars)
    pub fn avg_loser(&self) -> rust_decimal::Decimal {
        let losers: Vec<_> = self.results.iter()
            .filter(|r| r.success && r.pnl < rust_decimal::Decimal::ZERO)
            .collect();
        if losers.is_empty() {
            rust_decimal::Decimal::ZERO
        } else {
            let sum: rust_decimal::Decimal = losers.iter().map(|r| r.pnl).sum();
            sum / rust_decimal::Decimal::from(losers.len())
        }
    }

    /// Average losing trade (in percent)
    pub fn avg_loser_pct(&self) -> f64 {
        let losers: Vec<_> = self.results.iter()
            .filter(|r| r.success && r.pnl < rust_decimal::Decimal::ZERO)
            .collect();
        if losers.is_empty() {
            0.0
        } else {
            let sum: f64 = losers.iter()
                .map(|r| {
                    let pct: f64 = r.pnl_pct.try_into().unwrap_or(0.0);
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
        let mut all_results = Vec::new();
        let mut dropped_events = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        info!(
            start_date = %start_date,
            end_date = %end_date,
            option_type = ?option_type,
            "Starting backtest"
        );

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
                    .map(|event| self.process_event(event, session_date, &*strategy, option_type))
                    .collect();

                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    results.push(
                        self.process_event(event, session_date, &*strategy, option_type).await
                    );
                }
                results
            };

            // Collect results
            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                match result {
                    Ok(trade_result) => {
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

    fn create_strategy(&self) -> Box<dyn TradingStrategy> {
        match self.config.strategy {
            StrategyType::ATM => Box::new(ATMStrategy {
                criteria: self.config.selection.clone(),
            }),
            StrategyType::Delta => Box::new(DeltaStrategy::fixed(
                self.config.target_delta,
                self.config.selection.clone(),
            )),
            StrategyType::DeltaScan => Box::new(DeltaStrategy::scanning(
                self.config.delta_range,
                self.config.delta_scan_steps,
                self.config.selection.clone(),
            )),
        }
    }

    async fn process_event(
        &self,
        event: &EarningsEvent,
        session_date: NaiveDate,
        strategy: &dyn TradingStrategy,
        option_type: finq_core::OptionType,
    ) -> Result<CalendarSpreadResult, TradeGenerationError> {
        // Use event-based timing for entry/exit, not session_date
        // This ensures trades hold overnight through earnings
        let entry_time = self.earnings_timing.entry_datetime(event);
        let spot_result = self.equity_repo.get_spot_price(&event.symbol, entry_time).await;

        let spot = match spot_result {
            Ok(s) => s,
            Err(_) => {
                return Err(TradeGenerationError {
                    symbol: event.symbol.clone(),
                    earnings_date: event.earnings_date,
                    earnings_time: event.earnings_time,
                    reason: "NO_SPOT_PRICE".into(),
                    details: Some(format!("No spot price at {}", entry_time)),
                    phase: "spot_price".into(),
                });
            }
        };

        // Get option chain data (validate it exists)
        let chain_df = match self.options_repo.get_option_bars(&event.symbol, session_date).await {
            Ok(df) => df,
            Err(_) => {
                return Err(TradeGenerationError {
                    symbol: event.symbol.clone(),
                    earnings_date: event.earnings_date,
                    earnings_time: event.earnings_time,
                    reason: "NO_OPTIONS_DATA".into(),
                    details: Some(format!("No option data on {}", session_date)),
                    phase: "option_data".into(),
                });
            }
        };

        // Build IV surface once for strategy selection
        let iv_surface = build_iv_surface(
            &chain_df,
            spot.to_f64(),
            entry_time,
            &event.symbol,
        );

        // Get available expirations and strikes
        let expirations = self.options_repo
            .get_available_expirations(&event.symbol, session_date)
            .await
            .unwrap_or_default();

        let strikes = if !expirations.is_empty() {
            self.options_repo
                .get_available_strikes(&event.symbol, expirations[0], session_date)
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

        // Build chain data for strategy with pre-computed IV surface
        let chain_data = OptionChainData {
            expirations,
            strikes,
            deltas: None,
            volumes: None,
            iv_ratios: None,
            iv_surface,
        };

        // Select spread using strategy
        let spread = match strategy.select(event, &spot, &chain_data, option_type) {
            Ok(s) => s,
            Err(e) => {
                return Err(TradeGenerationError {
                    symbol: event.symbol.clone(),
                    earnings_date: event.earnings_date,
                    earnings_time: event.earnings_time,
                    reason: "STRATEGY_SELECTION_FAILED".into(),
                    details: Some(e.to_string()),
                    phase: "strategy".into(),
                });
            }
        };

        // Execute trade
        // Use event-based exit timing - this will be on a DIFFERENT date for earnings trades
        let exit_time = self.earnings_timing.exit_datetime(event);
        let executor = TradeExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
        )
        .with_iv_model(self.config.iv_model);

        let result = executor.execute_trade(&spread, event, entry_time, exit_time).await;

        Ok(result)
    }

    fn passes_iv_filter(&self, result: &CalendarSpreadResult) -> bool {
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
