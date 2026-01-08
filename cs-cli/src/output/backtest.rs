//! Backtest output handling (display and save results)

use anyhow::Result;
use std::path::PathBuf;
use console::style;
use tabled::{Table, Tabled};

use cs_backtest::{BacktestResult, TradeResultMethods, UnifiedBacktestResult};
use cs_domain::TradeResult as TradeResultTrait;

use crate::display::ResultRow;

/// Handler for backtest output (display and persistence)
pub struct BacktestOutputHandler;

impl BacktestOutputHandler {
    /// Display backtest results for any trade type
    pub fn display<R>(result: &BacktestResult<R>)
    where
        R: TradeResultTrait + TradeResultMethods,
    {
        Self::display_summary(result);
        Self::display_sample_trades(result);
        Self::display_dropped_events(result);
    }

    /// Display summary statistics
    fn display_summary<R>(result: &BacktestResult<R>)
    where
        R: TradeResultTrait + TradeResultMethods,
    {
        println!("{}", style("Results:").bold().green());

        let win_rate = result.win_rate() * 100.0;
        let total_pnl = result.total_pnl();
        let mean_return = result.mean_return() * 100.0;
        let std_return = result.std_return() * 100.0;
        let sharpe = result.sharpe_ratio();
        let avg_winner = result.avg_winner();
        let avg_winner_pct = result.avg_winner_pct() * 100.0;
        let avg_loser = result.avg_loser();
        let avg_loser_pct = result.avg_loser_pct() * 100.0;

        let has_hedging = result.has_hedging();
        let mut rows = vec![
            ResultRow { metric: "Sessions Processed".into(), value: result.sessions_processed.to_string() },
            ResultRow { metric: "Total Opportunities".into(), value: result.total_opportunities.to_string() },
            ResultRow { metric: "Trades Entered".into(), value: result.total_entries.to_string() },
            ResultRow { metric: "Trades Dropped".into(), value: result.dropped_events.len().to_string() },
            ResultRow { metric: "".into(), value: "".into() },
            ResultRow { metric: "Win Rate".into(), value: format!("{:.2}%", win_rate) },
        ];

        // Add P&L rows - show both option-only and hedged if hedging is enabled
        if has_hedging {
            let hedge_pnl = result.total_hedge_pnl();
            let total_with_hedge = result.total_pnl_with_hedge();
            rows.extend(vec![
                ResultRow { metric: "Option P&L".into(), value: format!("${:.2}", total_pnl) },
                ResultRow { metric: "Hedge P&L".into(), value: format!("${:.2}", hedge_pnl) },
                ResultRow { metric: "Total P&L (with hedge)".into(), value: format!("${:.2}", total_with_hedge) },
                ResultRow {
                    metric: "Avg P&L per Trade".into(),
                    value: if result.successful_trades() > 0 {
                        format!("${:.2}", total_with_hedge / rust_decimal::Decimal::from(result.successful_trades()))
                    } else {
                        "$0.00".into()
                    }
                },
            ]);
        } else {
            rows.extend(vec![
                ResultRow { metric: "Total P&L".into(), value: format!("${:.2}", total_pnl) },
                ResultRow {
                    metric: "Avg P&L per Trade".into(),
                    value: if result.successful_trades() > 0 {
                        format!("${:.2}", total_pnl / rust_decimal::Decimal::from(result.successful_trades()))
                    } else {
                        "$0.00".into()
                    }
                },
            ]);
        }

        rows.extend(vec![
            ResultRow { metric: "".into(), value: "".into() },
            ResultRow { metric: "Mean Return".into(), value: format!("{:.2}%", mean_return) },
            ResultRow { metric: "Std Dev".into(), value: format!("{:.2}%", std_return) },
            ResultRow { metric: "Sharpe Ratio".into(), value: format!("{:.2}", sharpe) },
            ResultRow { metric: "".into(), value: "".into() },
            ResultRow { metric: "Avg Winner".into(), value: format!("${:.2} ({:.2}%)", avg_winner, avg_winner_pct) },
            ResultRow { metric: "Avg Loser".into(), value: format!("${:.2} ({:.2}%)", avg_loser, avg_loser_pct) },
        ]);

        let table = Table::new(rows);
        println!("{}", table);
        println!();
    }

    /// Display sample trades
    fn display_sample_trades<R>(result: &BacktestResult<R>)
    where
        R: TradeResultTrait + TradeResultMethods,
    {
        if !result.results.is_empty() {
            println!("{}", style("Sample Trades:").bold());
            for (i, trade) in result.results.iter().take(5).enumerate() {
                println!("  {}. {} | P&L: ${:.2} ({:.2}%)",
                    i + 1,
                    TradeResultTrait::symbol(trade),
                    TradeResultMethods::pnl(trade),
                    TradeResultMethods::pnl_pct(trade),
                );
            }
            if result.results.len() > 5 {
                println!("  ... and {} more", result.results.len() - 5);
            }
            println!();
        }
    }

    /// Display dropped events summary
    fn display_dropped_events<R>(result: &BacktestResult<R>)
    where
        R: TradeResultTrait + TradeResultMethods,
    {
        if !result.dropped_events.is_empty() {
            println!("{}", style("Dropped Events:").bold().yellow());

            // Group by reason for summary
            let mut reason_groups: std::collections::HashMap<String, Vec<_>> = std::collections::HashMap::new();
            for event in &result.dropped_events {
                reason_groups.entry(event.reason.clone()).or_insert_with(Vec::new).push(event);
            }

            for (reason, events) in reason_groups.iter() {
                println!("  {} ({})", reason, events.len());
            }
            println!();
        }
    }

    /// Save results to file (placeholder - implement as needed)
    pub fn save<R>(_result: &BacktestResult<R>, output: &PathBuf) -> Result<()>
    where
        R: TradeResultTrait + TradeResultMethods,
    {
        println!("Saving results to {:?}", output);
        // TODO: Implement actual saving logic (CSV, JSON, Parquet)
        Ok(())
    }

    /// Display unified backtest results (dispatches to appropriate display method)
    pub fn display_unified(result: &UnifiedBacktestResult) {
        match result {
            UnifiedBacktestResult::CalendarSpread(r) => Self::display(r),
            UnifiedBacktestResult::IronButterfly(r) => Self::display(r),
            UnifiedBacktestResult::Straddle(r) => Self::display(r),
            UnifiedBacktestResult::CalendarStraddle(r) => Self::display(r),
            UnifiedBacktestResult::PostEarningsStraddle(r) => Self::display(r),
        }
    }

    /// Save unified backtest results (dispatches to appropriate save method)
    pub fn save_unified(result: &UnifiedBacktestResult, output: &PathBuf) -> Result<()> {
        match result {
            UnifiedBacktestResult::CalendarSpread(r) => Self::save(r, output),
            UnifiedBacktestResult::IronButterfly(r) => Self::save(r, output),
            UnifiedBacktestResult::Straddle(r) => Self::save(r, output),
            UnifiedBacktestResult::CalendarStraddle(r) => Self::save(r, output),
            UnifiedBacktestResult::PostEarningsStraddle(r) => Self::save(r, output),
        }
    }
}
