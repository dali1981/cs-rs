//! Backtest output handling (display and save results)

use anyhow::Result;
use std::path::PathBuf;
use console::style;
use tabled::{Table, Tabled};

use cs_backtest::{BacktestResult, TradeResultMethods, UnifiedBacktestResult};
use cs_domain::{TradeResult as TradeResultTrait, HasAccounting, HasTradingCost};

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

    /// Display backtest results with capital-weighted metrics and trading costs
    pub fn display_with_accounting<R>(result: &BacktestResult<R>)
    where
        R: TradeResultTrait + TradeResultMethods + HasAccounting + HasTradingCost,
    {
        Self::display_summary(result);
        Self::display_capital_weighted(result);
        Self::display_trading_costs(result);
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
            ResultRow { metric: "Mean Return (simple)".into(), value: format!("{:.2}%", mean_return) },
            ResultRow { metric: "Std Dev".into(), value: format!("{:.2}%", std_return) },
            ResultRow { metric: "Sharpe Ratio (simple)".into(), value: format!("{:.2}", sharpe) },
            ResultRow { metric: "".into(), value: "".into() },
            ResultRow { metric: "Avg Winner".into(), value: format!("${:.2} ({:.2}%)", avg_winner, avg_winner_pct) },
            ResultRow { metric: "Avg Loser".into(), value: format!("${:.2} ({:.2}%)", avg_loser, avg_loser_pct) },
        ]);

        let table = Table::new(rows);
        println!("{}", table);
        println!();
    }

    /// Display capital-weighted metrics (more accurate for varying position sizes)
    fn display_capital_weighted<R>(result: &BacktestResult<R>)
    where
        R: TradeResultTrait + TradeResultMethods + HasAccounting,
    {
        use rust_decimal::prelude::ToPrimitive;

        println!("{}", style("Capital-Weighted Metrics:").bold().cyan());

        let cw_return = result.capital_weighted_return() * 100.0;
        let cw_sharpe = result.capital_weighted_sharpe();
        let profit_factor = result.profit_factor();
        let total_capital = result.total_capital_deployed();
        let roc = result.return_on_capital() * 100.0;

        let rows = vec![
            ResultRow {
                metric: "Return on Capital".into(),
                value: format!("{:.2}%", roc),
            },
            ResultRow {
                metric: "Capital-Weighted Return".into(),
                value: format!("{:.2}%", cw_return),
            },
            ResultRow {
                metric: "Capital-Weighted Sharpe".into(),
                value: format!("{:.2}", cw_sharpe),
            },
            ResultRow {
                metric: "Profit Factor".into(),
                value: if profit_factor.is_infinite() {
                    "∞ (no losses)".into()
                } else {
                    format!("{:.2}", profit_factor)
                },
            },
            ResultRow {
                metric: "Total Capital Deployed".into(),
                value: format!("${:.2}", total_capital),
            },
        ];

        let table = Table::new(rows);
        println!("{}", table);
        println!();
    }

    /// Display trading costs breakdown (only if costs were applied)
    fn display_trading_costs<R>(result: &BacktestResult<R>)
    where
        R: TradeResultTrait + TradeResultMethods + HasTradingCost,
    {
        // Only display if costs were actually applied
        if !result.has_trading_costs() {
            return;
        }

        println!("{}", style("Trading Costs:").bold().magenta());

        let total_costs = result.total_trading_costs();
        let gross_pnl = result.total_gross_pnl();
        let net_pnl = result.total_pnl();
        let slippage = result.total_slippage();
        let commissions = result.total_commissions();
        let cost_impact = result.cost_impact_pct();
        let avg_cost = result.avg_cost_per_trade();
        let trades_with_costs = result.trades_with_costs();

        let rows = vec![
            ResultRow {
                metric: "Gross P&L".into(),
                value: format!("${:.2}", gross_pnl),
            },
            ResultRow {
                metric: "Total Costs".into(),
                value: format!("${:.2}", total_costs),
            },
            ResultRow {
                metric: "  - Slippage".into(),
                value: format!("${:.2}", slippage),
            },
            ResultRow {
                metric: "  - Commissions".into(),
                value: format!("${:.2}", commissions),
            },
            ResultRow {
                metric: "Net P&L".into(),
                value: format!("${:.2}", net_pnl),
            },
            ResultRow {
                metric: "".into(),
                value: "".into(),
            },
            ResultRow {
                metric: "Cost Impact".into(),
                value: format!("{:.2}% of gross P&L", cost_impact),
            },
            ResultRow {
                metric: "Avg Cost per Trade".into(),
                value: format!("${:.2}", avg_cost),
            },
            ResultRow {
                metric: "Trades with Costs".into(),
                value: format!("{} of {}", trades_with_costs, result.results.len()),
            },
        ];

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

    /// Save results to file
    pub fn save<R>(result: &BacktestResult<R>, output: &PathBuf) -> Result<()>
    where
        R: TradeResultTrait + TradeResultMethods + serde::Serialize,
    {
        use anyhow::Context;

        // Create parent directory if needed
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create output directory")?;
        }

        // Detect output format based on extension
        let is_json = output.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("json"))
            .unwrap_or(false);

        if is_json {
            // Save results as JSON
            let json_content = serde_json::to_string_pretty(&result.results)
                .context("Failed to serialize results to JSON")?;
            std::fs::write(output, json_content)
                .context("Failed to write JSON file")?;
            println!("{}", style(format!("Results saved to {:?}", output)).green());
        } else {
            // Default to JSON if no extension
            let json_content = serde_json::to_string_pretty(&result.results)
                .context("Failed to serialize results to JSON")?;
            std::fs::write(output, json_content)
                .context("Failed to write JSON file")?;
            println!("{}", style(format!("Results saved to {:?} (JSON format)", output)).green());
        }

        Ok(())
    }

    /// Display unified backtest results (dispatches to appropriate display method)
    /// Uses capital-weighted metrics for all supported trade types
    pub fn display_unified(result: &UnifiedBacktestResult) {
        match result {
            UnifiedBacktestResult::CalendarSpread(r) => Self::display_with_accounting(r),
            UnifiedBacktestResult::IronButterfly(r) => Self::display_with_accounting(r),
            UnifiedBacktestResult::Straddle(r) => Self::display_with_accounting(r),
            UnifiedBacktestResult::CalendarStraddle(r) => Self::display_with_accounting(r),
            UnifiedBacktestResult::PostEarningsStraddle(r) => Self::display_with_accounting(r),
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
