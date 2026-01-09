use std::sync::Arc;
use chrono::NaiveDate;
use tracing::{info, debug};
use rust_decimal::Decimal;

use cs_domain::*;
use cs_domain::strike_selection::{StrikeSelector, ATMStrategy, DeltaStrategy, ExpirationCriteria};
use cs_domain::pnl::{TradePnlRecord, PnlStatistics, ToPnlRecord};
use crate::config::{BacktestConfig, SelectionType};
use crate::execution::ExecutionConfig;
use crate::trade_strategy::{
    TradeStrategy, StrategyDispatch,
    CalendarSpreadStrategy, IronButterflyStrategy, StraddleStrategy,
    PostEarningsStraddleStrategy, CalendarStraddleStrategy,
};

/// Backtest execution result
#[derive(Debug)]
pub struct BacktestResult<R> {
    pub results: Vec<R>,
    pub sessions_processed: usize,
    pub total_entries: usize,
    pub total_opportunities: usize,
    pub dropped_events: Vec<TradeGenerationError>,
}

/// Unified backtest result that can hold any strategy result type
#[derive(Debug)]
pub enum UnifiedBacktestResult {
    CalendarSpread(BacktestResult<CalendarSpreadResult>),
    IronButterfly(BacktestResult<IronButterflyResult>),
    Straddle(BacktestResult<StraddleResult>),
    CalendarStraddle(BacktestResult<CalendarStraddleResult>),
    PostEarningsStraddle(BacktestResult<StraddleResult>),
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

// Additional methods for types that implement HasAccounting
impl<R: TradeResultMethods + cs_domain::HasAccounting> BacktestResult<R> {
    /// Capital-weighted return (THE CORRECT METRIC)
    ///
    /// This weights each trade's return by the capital deployed, ensuring that
    /// trades with larger positions contribute proportionally more to the
    /// overall return calculation.
    ///
    /// Formula: sum(capital_i * return_i) / sum(capital_i)
    ///
    /// This fixes the issue where simple mean return can show positive returns
    /// while total P&L is negative (when larger positions have losses).
    pub fn capital_weighted_return(&self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;

        let weighted_sum: f64 = self.results.iter()
            .map(|r| {
                let capital = r.capital_required().to_f64().unwrap_or(0.0);
                let return_pct = r.return_on_capital();
                capital * return_pct
            })
            .sum();

        let total_capital: f64 = self.results.iter()
            .map(|r| r.capital_required().to_f64().unwrap_or(0.0))
            .sum();

        if total_capital > 0.0 {
            weighted_sum / total_capital
        } else {
            0.0
        }
    }

    /// Total capital deployed across all trades
    pub fn total_capital_deployed(&self) -> Decimal {
        self.results.iter()
            .map(|r| r.capital_required())
            .sum()
    }

    /// Return on total capital (total P&L / total capital)
    pub fn return_on_capital(&self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        let total_capital = self.total_capital_deployed();
        if total_capital.is_zero() {
            return 0.0;
        }
        let total_pnl = self.total_pnl_with_hedge();
        (total_pnl / total_capital).to_f64().unwrap_or(0.0)
    }

    /// Profit factor (gross profit / gross loss)
    pub fn profit_factor(&self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        let gross_profit: Decimal = self.results.iter()
            .filter(|r| r.is_winner())
            .map(|r| r.pnl())
            .sum();
        let gross_loss: Decimal = self.results.iter()
            .filter(|r| r.pnl() < Decimal::ZERO)
            .map(|r| r.pnl().abs())
            .sum();

        if gross_loss.is_zero() {
            if gross_profit > Decimal::ZERO {
                f64::INFINITY
            } else {
                0.0
            }
        } else {
            (gross_profit / gross_loss).to_f64().unwrap_or(0.0)
        }
    }

    /// Sharpe ratio using capital-weighted returns (more accurate)
    pub fn capital_weighted_sharpe(&self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;

        let returns: Vec<f64> = self.results.iter()
            .map(|r| r.return_on_capital())
            .collect();

        if returns.len() < 2 {
            return 0.0;
        }

        let mean = self.capital_weighted_return();
        let variance = returns.iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>() / (returns.len() - 1) as f64;
        let std = variance.sqrt();

        if std > 0.0 {
            mean / std * 16.0 // sqrt(252)
        } else {
            0.0
        }
    }

    /// Get comprehensive statistics using the accounting module
    pub fn accounting_statistics(&self) -> cs_domain::TradeStatistics {
        use cs_domain::HasAccounting;

        let accountings: Vec<_> = self.results.iter()
            .map(|r| r.to_accounting())
            .collect();

        cs_domain::TradeStatistics::from_trades(&accountings)
    }
}

// PnL statistics methods for types that implement ToPnlRecord
impl<R: TradeResultMethods + ToPnlRecord> BacktestResult<R> {
    /// Convert all results to TradePnlRecords
    pub fn to_pnl_records(&self) -> Vec<TradePnlRecord> {
        self.results.iter().map(|r| r.to_pnl_record()).collect()
    }

    /// Get comprehensive PnL statistics using daily-normalized returns.
    ///
    /// This implements the spec's normalized return computation:
    /// - Daily returns: r_daily = (1 + r)^(1/T) - 1
    /// - Sharpe: mean(r_daily) / std(r_daily) × sqrt(252)
    /// - Hedge cost ratio: Σ HedgeCost / C_opt
    pub fn pnl_statistics(&self) -> Option<PnlStatistics> {
        let records = self.to_pnl_records();
        PnlStatistics::from_records(&records)
    }
}

// Cost aggregation methods for types that implement HasTradingCost
impl<R: TradeResultMethods + cs_domain::HasTradingCost> BacktestResult<R> {
    /// Check if any trades have trading costs applied
    pub fn has_trading_costs(&self) -> bool {
        self.results.iter().any(|r| r.has_costs())
    }

    /// Total trading costs across all trades
    pub fn total_trading_costs(&self) -> Decimal {
        self.results.iter()
            .filter_map(|r| r.total_costs())
            .sum()
    }

    /// Total gross P&L before costs
    ///
    /// For trades with costs, uses the stored gross P&L.
    /// For trades without costs, uses the current P&L (which is gross).
    pub fn total_gross_pnl(&self) -> Decimal {
        self.results.iter()
            .map(|r| r.gross_pnl().unwrap_or_else(|| r.pnl()))
            .sum()
    }

    /// Total slippage costs across all trades
    pub fn total_slippage(&self) -> Decimal {
        self.results.iter()
            .filter_map(|r| r.cost_summary())
            .map(|cs| cs.costs.breakdown.slippage)
            .sum()
    }

    /// Total commission costs across all trades
    pub fn total_commissions(&self) -> Decimal {
        self.results.iter()
            .filter_map(|r| r.cost_summary())
            .map(|cs| cs.costs.breakdown.commission)
            .sum()
    }

    /// Cost impact as percentage of gross P&L
    ///
    /// Shows how much of the gross P&L was consumed by trading costs.
    /// Returns 0.0 if gross P&L is zero.
    pub fn cost_impact_pct(&self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;

        let gross = self.total_gross_pnl();
        if gross.is_zero() {
            return 0.0;
        }

        let costs = self.total_trading_costs();
        (costs / gross.abs()).to_f64().unwrap_or(0.0) * 100.0
    }

    /// Number of trades that had costs applied
    pub fn trades_with_costs(&self) -> usize {
        self.results.iter().filter(|r| r.has_costs()).count()
    }

    /// Average cost per trade (only for trades with costs)
    pub fn avg_cost_per_trade(&self) -> Decimal {
        let count = self.trades_with_costs();
        if count == 0 {
            Decimal::ZERO
        } else {
            self.total_trading_costs() / Decimal::from(count)
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

    /// Execute backtest based on config.spread type
    ///
    /// This is the main entry point that dispatches to the appropriate strategy.
    pub async fn execute(&self) -> Result<UnifiedBacktestResult, BacktestError> {
        let strategy = StrategyDispatch::from_config(self.config.spread, &self.config);

        match strategy {
            StrategyDispatch::CalendarSpread(s) => {
                let result = self.execute_with_strategy(&s, None).await?;
                Ok(UnifiedBacktestResult::CalendarSpread(result))
            }
            StrategyDispatch::IronButterfly(s) => {
                let result = self.execute_with_strategy(&s, None).await?;
                Ok(UnifiedBacktestResult::IronButterfly(result))
            }
            StrategyDispatch::Straddle(s) => {
                let result = self.execute_with_strategy(&s, None).await?;
                Ok(UnifiedBacktestResult::Straddle(result))
            }
            StrategyDispatch::PostEarningsStraddle(s) => {
                let result = self.execute_with_strategy(&s, None).await?;
                Ok(UnifiedBacktestResult::PostEarningsStraddle(result))
            }
            StrategyDispatch::CalendarStraddle(s) => {
                let result = self.execute_with_strategy(&s, None).await?;
                Ok(UnifiedBacktestResult::CalendarStraddle(result))
            }
        }
    }

    /// Generic backtest executor for any trade strategy
    ///
    /// This method handles the common backtest loop:
    /// 1. Iterate through trading days
    /// 2. Load earnings events
    /// 3. Filter events for entry
    /// 4. Execute trades (parallel or sequential)
    /// 5. Apply post-execution filters
    /// 6. Collect results
    pub async fn execute_with_strategy<S, R>(
        &self,
        strategy: &S,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<R>, BacktestError>
    where
        S: TradeStrategy<R> + Sync,
        R: TradeResultMethods + Send + Clone,
    {
        let mut all_results: Vec<R> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        let selector = self.create_selector();
        let criteria = self.build_expiration_criteria();
        let exec_config = self.create_execution_config();

        // ===== NEW TRADE-CENTRIC APPROACH =====

        // 1. Determine trading range and timing spec
        let trading_range = self.config.trading_range();
        let timing_spec = self.config.timing_spec();
        let filter_criteria = self.config.filter_criteria();

        info!(
            spread = ?self.config.spread,
            start = %trading_range.start,
            end = %trading_range.end,
            "Starting trade-centric backtest"
        );

        // 2. Calculate event search range based on timing
        let (search_start, search_end) = timing_spec.event_search_range(&trading_range);

        debug!(
            search_start = %search_start,
            search_end = %search_end,
            "Loading events in search range"
        );

        // 3. Load all potentially relevant events
        let all_events = self.earnings_repo
            .load_earnings(search_start, search_end, self.config.symbols.as_deref())
            .await
            .map_err(|e| BacktestError::Repository(e.to_string()))?;

        info!(events_loaded = all_events.len(), "Events loaded");

        // 4. Discover tradable events (entry date in trading range)
        let tradable_events = trading_range.discover_tradable_events(&all_events, &timing_spec);

        info!(tradable_events = tradable_events.len(), "Tradable events discovered");

        // 5. Filter by market cap and other criteria
        let filtered_events: Vec<_> = tradable_events
            .into_iter()
            .filter(|te| filter_criteria.symbol_matches(te.symbol()))
            .filter(|te| filter_criteria.market_cap_matches(te.event.market_cap))
            .collect();

        info!(filtered_events = filtered_events.len(), "Events after filtering");

        total_opportunities = filtered_events.len();

        // 6. Execute trades (trade-by-trade)
        let tradable_refs: Vec<&TradableEvent> = filtered_events.iter().collect();
        let batch_results = self.execute_tradable_batch(
            &tradable_refs,
            strategy,
            &*selector,
            &criteria,
            &exec_config,
        ).await;

        // 7. Collect and apply post-execution filters
        let min_iv_ratio = self.config.selection.min_iv_ratio;
        for (result_opt, tradable) in batch_results.into_iter().zip(filtered_events.iter()) {
            if let Some(result) = result_opt {
                if strategy.apply_filter(&result, min_iv_ratio) {
                    all_results.push(result);
                } else if let Some(error) = strategy.create_filter_error(&result, &tradable.event) {
                    dropped_events.push(error);
                }
            }
        }

        sessions_processed = 1; // Single pass in new model

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

    /// Execute a batch of tradable events (NEW: trade-centric)
    ///
    /// Takes resolved TradableEvent objects with pre-computed entry/exit times.
    async fn execute_tradable_batch<S, R>(
        &self,
        tradable_events: &[&TradableEvent],
        strategy: &S,
        selector: &dyn StrikeSelector,
        criteria: &ExpirationCriteria,
        exec_config: &ExecutionConfig,
    ) -> Vec<Option<R>>
    where
        S: TradeStrategy<R> + Sync,
        R: TradeResultMethods + Send,
    {
        let events: Vec<&EarningsEvent> = tradable_events.iter().map(|te| &te.event).collect();

        crate::execution::run_batch(&events, self.config.parallel, |event| {
            // Find the corresponding TradableEvent for this event
            let tradable = tradable_events.iter()
                .find(|te| te.symbol() == event.symbol && te.earnings_date() == event.earnings_date)
                .expect("TradableEvent must exist for every event");

            strategy.execute_trade(
                self.options_repo.as_ref(),
                self.equity_repo.as_ref(),
                selector,
                criteria,
                exec_config,
                event,
                tradable.entry_datetime(),
                tradable.exit_datetime(),
            )
        }).await
    }

    /// Execute a batch of trades (OLD: date-centric - deprecated)
    ///
    /// This is the old method kept for backwards compatibility.
    /// New code should use execute_tradable_batch instead.
    #[allow(dead_code)]
    async fn execute_batch<S, R>(
        &self,
        events: &[EarningsEvent],
        strategy: &S,
        selector: &dyn StrikeSelector,
        criteria: &ExpirationCriteria,
        exec_config: &ExecutionConfig,
    ) -> Vec<Option<R>>
    where
        S: TradeStrategy<R> + Sync,
        R: TradeResultMethods + Send,
    {
        crate::execution::run_batch(events, self.config.parallel, |event| {
            let entry_time = strategy.entry_datetime(event);
            let exit_time = strategy.exit_datetime(event);
            strategy.execute_trade(
                self.options_repo.as_ref(),
                self.equity_repo.as_ref(),
                selector,
                criteria,
                exec_config,
                event,
                entry_time,
                exit_time,
            )
        }).await
    }

    /// Load earnings events for a strategy (OLD: date-centric - deprecated)
    ///
    /// This method is deprecated in favor of loading all events at once
    /// and using TradingRange.discover_tradable_events() for filtering.
    ///
    /// Different strategies need different lookahead windows:
    /// - Earnings timing: small window around session date
    /// - Straddle timing: large lookahead (entry N days before earnings)
    /// - Post-earnings: lookback (entry after earnings)
    #[allow(dead_code)]
    async fn load_earnings_for_strategy<S, R>(
        &self,
        session_date: NaiveDate,
        strategy: &S,
    ) -> Result<Vec<EarningsEvent>, BacktestError>
    where
        S: TradeStrategy<R>,
        R: TradeResultMethods + Send,
    {
        let lookahead = strategy.lookahead_days();

        let (start, end) = if lookahead < 0 {
            // Lookback (post-earnings): look backwards from session_date
            let lookback = -lookahead;
            (session_date - chrono::Duration::days(lookback), session_date)
        } else if lookahead <= 3 {
            // Small window (earnings timing): use adjacent days
            let start = TradingCalendar::previous_trading_day(session_date);
            let end = TradingCalendar::next_trading_day(session_date);
            (start, end)
        } else {
            // Large lookahead (straddle): session_date to session_date + lookahead
            (session_date, session_date + chrono::Duration::days(lookahead))
        };

        self.earnings_repo
            .load_earnings(start, end, self.config.symbols.as_deref())
            .await
            .map_err(|e| BacktestError::Repository(e.to_string()))
    }

    /// Report progress to callback (OLD: date-centric - deprecated)
    ///
    /// This is no longer used in trade-centric execution but kept for compatibility.
    #[allow(dead_code)]
    fn report_progress(
        &self,
        on_progress: &Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
        session_date: NaiveDate,
        entries_count: usize,
        events_found: usize,
    ) {
        if let Some(ref callback) = on_progress {
            callback(SessionProgress {
                session_date,
                entries_count,
                events_found,
            });
        }
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

    /// Create execution config based on spread type
    fn create_execution_config(&self) -> ExecutionConfig {
        use crate::config::SpreadType;
        let base_config = match self.config.spread {
            SpreadType::Calendar => ExecutionConfig::for_calendar_spread(self.config.max_entry_iv),
            SpreadType::IronButterfly => ExecutionConfig::for_iron_butterfly(self.config.max_entry_iv),
            SpreadType::Straddle => ExecutionConfig::for_straddle(self.config.max_entry_iv),
            SpreadType::CalendarStraddle => ExecutionConfig::for_calendar_straddle(self.config.max_entry_iv),
            SpreadType::PostEarningsStraddle => ExecutionConfig::for_straddle(self.config.max_entry_iv),
        };

        // Add trading costs and hedging config from backtest config
        let config = base_config.with_trading_costs(self.config.trading_costs.clone());

        // Add hedging if enabled
        if self.config.hedge_config.is_enabled() {
            config.with_hedging(self.config.hedge_config.clone())
        } else {
            config
        }
    }

    fn passes_market_cap_filter(&self, event: &EarningsEvent) -> bool {
        match (self.config.min_market_cap, event.market_cap) {
            (Some(min), Some(cap)) => cap >= min,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    // ========================================================================
    // Legacy API - Kept for backwards compatibility
    // These methods delegate to execute_with_strategy with specific strategies
    // ========================================================================

    /// Execute calendar spread backtest (legacy API)
    pub async fn execute_calendar_spread(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        option_type: finq_core::OptionType,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<CalendarSpreadResult>, BacktestError> {
        let mut config = self.config.clone();
        config.start_date = start_date;
        config.end_date = end_date;

        let strategy = CalendarSpreadStrategy::new(&config)
            .with_option_type(option_type);

        // Create a temporary use case with updated config
        let temp_use_case = BacktestUseCase {
            earnings_repo: Arc::clone(&self.earnings_repo),
            options_repo: Arc::clone(&self.options_repo),
            equity_repo: Arc::clone(&self.equity_repo),
            config,
        };

        temp_use_case.execute_with_strategy(&strategy, on_progress).await
    }

    /// Execute iron butterfly backtest (legacy API)
    pub async fn execute_iron_butterfly(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<IronButterflyResult>, BacktestError> {
        let mut config = self.config.clone();
        config.start_date = start_date;
        config.end_date = end_date;

        let strategy = IronButterflyStrategy::new(&config);

        let temp_use_case = BacktestUseCase {
            earnings_repo: Arc::clone(&self.earnings_repo),
            options_repo: Arc::clone(&self.options_repo),
            equity_repo: Arc::clone(&self.equity_repo),
            config,
        };

        temp_use_case.execute_with_strategy(&strategy, on_progress).await
    }

    /// Execute straddle backtest (legacy API)
    pub async fn execute_straddle(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<StraddleResult>, BacktestError> {
        let mut config = self.config.clone();
        config.start_date = start_date;
        config.end_date = end_date;

        let strategy = StraddleStrategy::new(&config);

        let temp_use_case = BacktestUseCase {
            earnings_repo: Arc::clone(&self.earnings_repo),
            options_repo: Arc::clone(&self.options_repo),
            equity_repo: Arc::clone(&self.equity_repo),
            config,
        };

        temp_use_case.execute_with_strategy(&strategy, on_progress).await
    }

    /// Execute post-earnings straddle backtest (legacy API)
    pub async fn execute_post_earnings_straddle(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<StraddleResult>, BacktestError> {
        let mut config = self.config.clone();
        config.start_date = start_date;
        config.end_date = end_date;

        let strategy = PostEarningsStraddleStrategy::new(&config);

        let temp_use_case = BacktestUseCase {
            earnings_repo: Arc::clone(&self.earnings_repo),
            options_repo: Arc::clone(&self.options_repo),
            equity_repo: Arc::clone(&self.equity_repo),
            config,
        };

        temp_use_case.execute_with_strategy(&strategy, on_progress).await
    }

    /// Execute calendar straddle backtest (legacy API)
    pub async fn execute_calendar_straddle(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult<CalendarStraddleResult>, BacktestError> {
        let mut config = self.config.clone();
        config.start_date = start_date;
        config.end_date = end_date;

        let strategy = CalendarStraddleStrategy::new(&config);

        let temp_use_case = BacktestUseCase {
            earnings_repo: Arc::clone(&self.earnings_repo),
            options_repo: Arc::clone(&self.options_repo),
            equity_repo: Arc::clone(&self.equity_repo),
            config,
        };

        temp_use_case.execute_with_strategy(&strategy, on_progress).await
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
