//! Campaign output handling (display and save results)

use anyhow::Result;
use std::path::PathBuf;
use std::collections::HashMap;
use console::style;
use tabled::{Table, Tabled};

use cs_backtest::BatchResult;

/// Handler for campaign output (display and persistence)
pub struct CampaignOutputHandler;

impl CampaignOutputHandler {
    /// Display campaign results with summary and per-symbol breakdown
    pub fn display(result: &BatchResult) {
        Self::display_summary(result);
        Self::display_sample_sessions(result);

        if result.has_hedge_data() {
            Self::display_hedge_summary(result);
        }

        if result.has_attribution_data() {
            Self::display_attribution_summary(result);
        }
    }

    /// Display overall summary statistics
    fn display_summary(result: &BatchResult) {
        println!("{}", style("Campaign Results:").bold().green());

        let win_rate = result.win_rate().unwrap_or(0.0);
        let total_pnl = result.total_pnl();
        let avg_pnl = result.avg_pnl().unwrap_or(rust_decimal::Decimal::ZERO);

        #[derive(Tabled)]
        struct SummaryRow {
            metric: String,
            value: String,
        }

        let mut rows = vec![
            SummaryRow { metric: "Total Sessions".into(), value: result.total_sessions.to_string() },
            SummaryRow { metric: "Successful Sessions".into(), value: result.successful.to_string() },
            SummaryRow { metric: "Failed Sessions".into(), value: result.failed.to_string() },
            SummaryRow { metric: "".into(), value: "".into() },
            SummaryRow { metric: "Win Rate".into(), value: format!("{:.2}%", win_rate) },
        ];

        // Add P&L rows - show both option-only and hedged if hedging is enabled
        if result.has_hedge_data() {
            let hedge_pnl = result.total_hedge_pnl().unwrap_or(rust_decimal::Decimal::ZERO);
            let total_with_hedge = result.total_pnl_with_hedge();
            rows.extend(vec![
                SummaryRow { metric: "Option P&L".into(), value: format!("${:.2}", total_pnl) },
                SummaryRow { metric: "Hedge P&L".into(), value: format!("${:.2}", hedge_pnl) },
                SummaryRow { metric: "Total P&L (with hedge)".into(), value: format!("${:.2}", total_with_hedge) },
                SummaryRow { metric: "Avg P&L per Session".into(), value: format!("${:.2}", avg_pnl) },
            ]);
        } else {
            rows.extend(vec![
                SummaryRow { metric: "Total P&L".into(), value: format!("${:.2}", total_pnl) },
                SummaryRow { metric: "Avg P&L per Session".into(), value: format!("${:.2}", avg_pnl) },
            ]);
        }

        let table = Table::new(rows);
        println!("{}", table);
        println!();
    }

    /// Display sample sessions
    fn display_sample_sessions(result: &BatchResult) {
        let sessions_with_pnl = result.successful_with_pnl();

        if !sessions_with_pnl.is_empty() {
            println!("{}", style("Sample Sessions:").bold());
            for (i, session_result) in sessions_with_pnl.iter().take(5).enumerate() {
                if let Some(ref pnl) = session_result.pnl {
                    println!(
                        "  {}. {} ({}) | P&L: ${:.2}",
                        i + 1,
                        session_result.session.symbol,
                        session_result.session.entry_date(),
                        pnl.pnl
                    );
                }
            }
            if sessions_with_pnl.len() > 5 {
                println!("  ... and {} more", sessions_with_pnl.len() - 5);
            }
            println!();
        }
    }

    /// Display hedge summary
    fn display_hedge_summary(result: &BatchResult) {
        println!("{}", style("Hedge Summary:").bold());

        let total_hedge_count = result.total_hedge_count();
        let total_hedge_pnl = result.total_hedge_pnl().unwrap_or(rust_decimal::Decimal::ZERO);

        println!("  Total Hedge Trades: {}", total_hedge_count);
        println!("  Hedge P&L: ${:.2}", total_hedge_pnl);
        println!();
    }

    /// Display attribution summary (placeholder - implement when attribution data structure is available)
    fn display_attribution_summary(_result: &BatchResult) {
        println!("{}", style("Attribution Summary:").bold());
        println!("  Attribution details available in session results");
        println!();
    }

    /// Save summary results to JSON file
    pub fn save(result: &BatchResult, output: &PathBuf) -> Result<()> {
        use serde_json::json;
        use std::fs;

        // Collect summary statistics
        let summary = json!({
            "total_sessions": result.total_sessions,
            "successful": result.successful,
            "failed": result.failed,
            "win_rate": result.win_rate(),
            "total_pnl": result.total_pnl().to_string(),
            "avg_pnl": result.avg_pnl().map(|p| p.to_string()),
            "total_hedge_pnl": result.total_hedge_pnl().map(|p| p.to_string()),
            "total_pnl_with_hedge": result.total_pnl_with_hedge().to_string(),
            "total_hedge_count": result.total_hedge_count(),
        });

        // Save to file
        let json_str = serde_json::to_string_pretty(&summary)?;
        fs::write(output, json_str)?;

        println!("Results saved to: {}", style(output.display()).cyan());
        Ok(())
    }

    /// Save detailed per-symbol results to separate JSON files
    pub fn save_detailed(result: &BatchResult, output_dir: &PathBuf) -> Result<()> {
        use serde_json;
        use std::fs;

        fs::create_dir_all(output_dir)?;

        // Group results by symbol
        let mut by_symbol: HashMap<String, Vec<_>> = HashMap::new();
        for session_result in &result.results {
            by_symbol
                .entry(session_result.session.symbol.clone())
                .or_insert_with(Vec::new)
                .push(&session_result.pnl);
        }

        // Save per-symbol files
        for (symbol, pnls) in by_symbol {
            let output_file = output_dir.join(format!("{}_results.json", symbol));
            let json_str = serde_json::to_string_pretty(&pnls)?;
            fs::write(&output_file, json_str)?;
        }

        println!("Detailed results saved to: {}", style(output_dir.display()).cyan());
        Ok(())
    }
}
