//! Backtest output handling (display and save results)

use anyhow::Result;
use std::path::PathBuf;
use console::style;
use tabled::{Table, Tabled};

use cs_backtest::{BacktestResult, TradeResultMethods, UnifiedBacktestResult};
use cs_backtest::bpr::{build_portfolio_bpr_timeline, HasBprTimeline};
use cs_domain::{TradeResult as TradeResultTrait, HasAccounting, HasTradingCost, ToPnlRecord, ReturnBasis};
use rust_decimal::Decimal;

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
        R: TradeResultTrait
            + TradeResultMethods
            + HasAccounting
            + HasTradingCost
            + ToPnlRecord
            + HasBprTimeline,
    {
        Self::display_summary(result);
        Self::display_capital_weighted(result);
        Self::display_bpr_metrics(result);
        Self::display_pnl_statistics(result);
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
        R: TradeResultTrait + TradeResultMethods + HasAccounting + HasBprTimeline,
    {
        use rust_decimal::prelude::ToPrimitive;

        println!("{}", style("Basis Metrics:").bold().cyan());

        let profit_factor = result.profit_factor();
        let configured_basis = result.return_basis.label();

        let rows = vec![
            ResultRow {
                metric: "Configured Return Basis".into(),
                value: configured_basis.to_string(),
            },
            ResultRow {
                metric: "Profit Factor".into(),
                value: if profit_factor.is_infinite() {
                    "∞ (no losses)".into()
                } else {
                    format!("{:.2}", profit_factor)
                },
            },
        ];

        let table = Table::new(rows);
        println!("{}", table);

        #[derive(Tabled)]
        struct BasisRow {
            basis: String,
            return_on_basis: String,
            weighted_return: String,
            mean_return: String,
            std_return: String,
            sharpe: String,
            total_basis: String,
            coverage: String,
        }

        let basis_rows = [
            ReturnBasis::Premium,
            ReturnBasis::CapitalRequired,
            ReturnBasis::MaxLoss,
            ReturnBasis::BprPeak,
            ReturnBasis::BprAvg,
        ]
        .iter()
        .map(|basis| {
            let metrics = compute_basis_metrics(result, *basis);
            BasisRow {
                basis: basis.label().to_string(),
                return_on_basis: format!("{:.2}%", metrics.return_on_basis * 100.0),
                weighted_return: format!("{:.2}%", metrics.weighted_return * 100.0),
                mean_return: format!("{:.2}%", metrics.mean_return * 100.0),
                std_return: format!("{:.2}%", metrics.std_return * 100.0),
                sharpe: format!("{:.2}", metrics.sharpe),
                total_basis: format!("${:.2}", metrics.total_basis),
                coverage: format!("{}/{}", metrics.coverage_supported, metrics.coverage_total),
            }
        })
        .collect::<Vec<_>>();

        let table = Table::new(basis_rows);
        println!("{}", table);
        println!();
    }

    /// Display normalized PnL statistics (time-adjusted metrics from spec)
    fn display_pnl_statistics<R>(result: &BacktestResult<R>)
    where
        R: TradeResultTrait + TradeResultMethods + ToPnlRecord,
    {
        let Some(stats) = result.pnl_statistics() else {
            return;
        };

        println!("{}", style("Normalized PnL Metrics:").bold().blue());

        let mut rows = vec![
            ResultRow {
                metric: "Daily-Normalized Sharpe".into(),
                value: format!("{:.2}", stats.sharpe_ratio),
            },
            ResultRow {
                metric: "Mean Daily Return".into(),
                value: format!("{:.4}%", stats.mean_daily_return * 100.0),
            },
            ResultRow {
                metric: "Std Daily Return".into(),
                value: format!("{:.4}%", stats.std_daily_return * 100.0),
            },
            ResultRow {
                metric: "Avg Trade Duration".into(),
                value: format!("{:.1} days", stats.avg_duration_days),
            },
        ];

        // Add hedge cost metrics if hedging was used
        if stats.mean_hedge_cost_ratio > 0.0 {
            rows.push(ResultRow {
                metric: "".into(),
                value: "".into(),
            });
            rows.push(ResultRow {
                metric: "Mean Hedge Cost Ratio".into(),
                value: format!("{:.1}%", stats.mean_hedge_cost_ratio * 100.0),
            });

            if stats.trades_with_excessive_hedge_costs > 0 {
                let warning = if stats.has_hedge_cost_problem() {
                    format!("{} ⚠️", stats.trades_with_excessive_hedge_costs)
                } else {
                    stats.trades_with_excessive_hedge_costs.to_string()
                };
                rows.push(ResultRow {
                    metric: "Trades with High Hedge Costs".into(),
                    value: warning,
                });
            }
        }

        let table = Table::new(rows);
        println!("{}", table);

        // Warning for hedge cost problems
        if stats.has_hedge_cost_problem() {
            println!("{}", style("⚠️  Warning: High hedge costs may be destroying edge (>30% of premium)").yellow());
        }

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

    fn display_bpr_metrics<R>(result: &BacktestResult<R>)
    where
        R: TradeResultTrait + TradeResultMethods + HasBprTimeline,
    {
        use rust_decimal::prelude::ToPrimitive;

        let Some(portfolio) = build_portfolio_bpr_timeline(&result.results) else {
            return;
        };

        let use_maintenance = result.margin_config.use_maintenance;
        let basis_label = if use_maintenance { "maint" } else { "initial" };
        let (portfolio_max, portfolio_avg) = if use_maintenance {
            (portfolio.summary.max_total_maint, portfolio.summary.avg_total_maint)
        } else {
            (portfolio.summary.max_total_initial, portfolio.summary.avg_total_initial)
        };
        let (max_option_bpr, max_hedge_bpr) = if use_maintenance {
            (portfolio.summary.max_option_maint, portfolio.summary.max_hedge_maint)
        } else {
            let mut max_option = Decimal::ZERO;
            let mut max_hedge = Decimal::ZERO;
            for snap in &portfolio.snapshots {
                if snap.option_initial > max_option {
                    max_option = snap.option_initial;
                }
                if snap.hedge_initial > max_hedge {
                    max_hedge = snap.hedge_initial;
                }
            }
            (max_option, max_hedge)
        };

        let mut trade_rocs = Vec::new();
        let mut covered = 0usize;
        for trade in &result.results {
            let Some(timeline) = trade.bpr_timeline() else { continue };
            covered += 1;
            let denom = if use_maintenance {
                timeline.summary.max_total_maint
            } else {
                timeline.summary.max_total_initial
            };
            if denom.is_zero() {
                continue;
            }
            let pnl = TradeResultMethods::total_pnl_with_hedge(trade)
                .unwrap_or_else(|| TradeResultMethods::pnl(trade));
            let roc = (pnl / denom).to_f64().unwrap_or(0.0);
            trade_rocs.push(roc);
        }

        if trade_rocs.is_empty() {
            return;
        }

        let mean_roc = trade_rocs.iter().sum::<f64>() / trade_rocs.len() as f64;
        let variance = trade_rocs
            .iter()
            .map(|r| {
                let diff = r - mean_roc;
                diff * diff
            })
            .sum::<f64>()
            / trade_rocs.len() as f64;
        let std_roc = variance.sqrt();
        let sharpe = if std_roc > 0.0 { mean_roc / std_roc } else { 0.0 };

        let total_pnl = if result.has_hedging() {
            result.total_pnl_with_hedge()
        } else {
            result.total_pnl()
        };
        let portfolio_roc = if portfolio_max.is_zero() {
            0.0
        } else {
            (total_pnl / portfolio_max).to_f64().unwrap_or(0.0)
        };

        println!("{}", style("Margin & Buying Power (IBKR-like):").bold().magenta());

        let rows = vec![
            ResultRow {
                metric: "Trades with BPR".into(),
                value: format!("{}/{}", covered, result.results.len()),
            },
            ResultRow {
                metric: "Return Denominator".into(),
                value: basis_label.to_string(),
            },
            ResultRow {
                metric: "Portfolio Max BPR".into(),
                value: format!("${:.2}", portfolio_max),
            },
            ResultRow {
                metric: "Portfolio Avg BPR".into(),
                value: format!("${:.2}", portfolio_avg),
            },
            ResultRow {
                metric: "Portfolio ROC on BPR".into(),
                value: format!("{:.2}%", portfolio_roc * 100.0),
            },
            ResultRow {
                metric: "Mean Trade ROC".into(),
                value: format!("{:.2}%", mean_roc * 100.0),
            },
            ResultRow {
                metric: "Std Trade ROC".into(),
                value: format!("{:.2}%", std_roc * 100.0),
            },
            ResultRow {
                metric: "Trade ROC Sharpe".into(),
                value: format!("{:.2}", sharpe),
            },
            ResultRow {
                metric: "Max Option BPR".into(),
                value: format!("${:.2}", max_option_bpr),
            },
            ResultRow {
                metric: "Max Hedge BPR".into(),
                value: format!("${:.2}", max_hedge_bpr),
            },
        ];

        let table = Table::new(rows);
        println!("{}", table);
        println!();
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
        R: TradeResultTrait + TradeResultMethods + HasBprTimeline + serde::Serialize,
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

        let bpr_portfolio = build_portfolio_bpr_timeline(&result.results).map(|timeline| {
            let use_maintenance = result.margin_config.use_maintenance;
            let (max_basis, avg_basis) = if use_maintenance {
                (timeline.summary.max_total_maint, timeline.summary.avg_total_maint)
            } else {
                (timeline.summary.max_total_initial, timeline.summary.avg_total_initial)
            };
            BprPortfolioOutput {
                summary: timeline.summary,
                use_maintenance,
                max_basis,
                avg_basis,
            }
        });

        let payload = BacktestJsonOutput {
            results: &result.results,
            bpr_portfolio,
        };

        if is_json {
            // Save results as JSON
            let json_content = serde_json::to_string_pretty(&payload)
                .context("Failed to serialize results to JSON")?;
            std::fs::write(output, json_content)
                .context("Failed to write JSON file")?;
            println!("{}", style(format!("Results saved to {:?}", output)).green());
        } else {
            // Default to JSON if no extension
            let json_content = serde_json::to_string_pretty(&payload)
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

struct BasisMetrics {
    return_on_basis: f64,
    weighted_return: f64,
    mean_return: f64,
    std_return: f64,
    sharpe: f64,
    total_basis: Decimal,
    coverage_supported: usize,
    coverage_total: usize,
}

#[derive(serde::Serialize)]
struct BacktestJsonOutput<'a, R> {
    results: &'a [R],
    #[serde(skip_serializing_if = "Option::is_none")]
    bpr_portfolio: Option<BprPortfolioOutput>,
}

#[derive(serde::Serialize)]
struct BprPortfolioOutput {
    summary: cs_domain::BprSummary,
    use_maintenance: bool,
    max_basis: Decimal,
    avg_basis: Decimal,
}

fn compute_basis_metrics<R>(result: &BacktestResult<R>, basis: ReturnBasis) -> BasisMetrics
where
    R: TradeResultTrait + TradeResultMethods + HasAccounting + HasBprTimeline,
{
    use rust_decimal::prelude::ToPrimitive;
    let use_maintenance = result.margin_config.use_maintenance;
    let mut total_basis = Decimal::ZERO;
    let mut total_pnl = Decimal::ZERO;
    let mut returns = Vec::new();
    let mut supported = 0usize;

    let basis_value_for_trade = |trade: &R| -> Option<Decimal> {
        match basis {
            ReturnBasis::Premium | ReturnBasis::CapitalRequired | ReturnBasis::MaxLoss => {
                trade.return_basis_value(basis)
            }
            ReturnBasis::BprPeak => trade.bpr_timeline().map(|timeline| {
                if use_maintenance {
                    timeline.summary.max_total_maint
                } else {
                    timeline.summary.max_total_initial
                }
            }),
            ReturnBasis::BprAvg => trade.bpr_timeline().map(|timeline| {
                if use_maintenance {
                    timeline.summary.avg_total_maint
                } else {
                    timeline.summary.avg_total_initial
                }
            }),
        }
    };

    for trade in &result.results {
        let Some(basis_value) = basis_value_for_trade(trade) else {
            continue;
        };
        if basis_value.is_zero() {
            continue;
        }
        supported += 1;
        total_basis += basis_value;
        let pnl = trade.realized_pnl();
        total_pnl += pnl;
        let ret = (pnl / basis_value).to_f64().unwrap_or(0.0);
        returns.push(ret);
    }

    let return_on_basis = if total_basis.is_zero() {
        0.0
    } else {
        (total_pnl / total_basis).to_f64().unwrap_or(0.0)
    };

    let weighted_return = {
        let weighted_sum: f64 = result.results.iter()
            .filter_map(|trade| {
                let basis_value = basis_value_for_trade(trade)?;
                if basis_value.is_zero() {
                    return None;
                }
                let pnl = trade.realized_pnl();
                let ret = (pnl / basis_value).to_f64();
                let ret = ret.unwrap_or(0.0);
                let basis_f: f64 = basis_value.try_into().unwrap_or(0.0);
                if basis_f > 0.0 {
                    Some(basis_f * ret)
                } else {
                    None
                }
            })
            .sum();
        let total_basis_f: f64 = total_basis.try_into().unwrap_or(0.0);
        if total_basis_f > 0.0 {
            weighted_sum / total_basis_f
        } else {
            0.0
        }
    };

    let mean_return = if returns.is_empty() {
        0.0
    } else {
        returns.iter().sum::<f64>() / returns.len() as f64
    };

    let std_return = if returns.len() < 2 {
        0.0
    } else {
        let variance = returns.iter()
            .map(|r| (r - mean_return).powi(2))
            .sum::<f64>() / (returns.len() - 1) as f64;
        variance.sqrt()
    };

    let sharpe = if std_return > 0.0 {
        mean_return / std_return * 16.0
    } else {
        0.0
    };

    BasisMetrics {
        return_on_basis,
        weighted_return,
        mean_return,
        std_return,
        sharpe,
        total_basis,
        coverage_supported: supported,
        coverage_total: result.results.len(),
    }
}
