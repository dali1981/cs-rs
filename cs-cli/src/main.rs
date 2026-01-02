// cs-cli: Command-line interface for calendar spread backtesting

use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tabled::{Table, Tabled};
use tracing::{info, Level};
use tracing_subscriber;

use cs_backtest::{BacktestUseCase, StrategyType};
use cs_domain::{
    infrastructure::{
        EarningsReaderAdapter, FinqEquityRepository, FinqOptionsRepository,
        ParquetResultsRepository,
    },
    ResultsRepository,
};

mod config;
mod cli_args;

use cli_args::*;

#[derive(Parser)]
#[command(name = "cs")]
#[command(about = "Calendar Spread Backtest CLI")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Data directory
    #[arg(long, env = "FINQ_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(long, short)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run backtest
    Backtest {
        /// Configuration file(s) - can specify multiple, each merges on top of previous
        #[arg(long, short = 'c')]
        conf: Vec<PathBuf>,
        /// Earnings data directory
        #[arg(long, env = "EARNINGS_DATA_DIR")]
        earnings_dir: Option<PathBuf>,
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start: String,
        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end: String,
        /// Option type (call/put)
        #[arg(long, default_value = "call")]
        option_type: String,
        /// Strategy type (atm, delta, delta-scan)
        #[arg(long)]
        strategy: Option<String>,
        /// Delta range for delta-scan strategy (format: "0.25,0.75")
        #[arg(long)]
        delta_range: Option<String>,
        /// Number of delta steps for delta-scan strategy
        #[arg(long)]
        delta_scan_steps: Option<usize>,
        /// Filter to specific symbols
        #[arg(long)]
        symbols: Option<Vec<String>>,
        /// Output file path
        #[arg(long)]
        output: Option<PathBuf>,
        /// Entry hour (0-23)
        #[arg(long)]
        entry_hour: Option<u32>,
        /// Entry minute (0-59)
        #[arg(long)]
        entry_minute: Option<u32>,
        /// Exit hour (0-23)
        #[arg(long)]
        exit_hour: Option<u32>,
        /// Exit minute (0-59)
        #[arg(long)]
        exit_minute: Option<u32>,
        /// Minimum market cap filter
        #[arg(long)]
        min_market_cap: Option<u64>,
        /// Minimum short DTE
        #[arg(long)]
        min_short_dte: Option<i32>,
        /// Maximum short DTE
        #[arg(long)]
        max_short_dte: Option<i32>,
        /// Minimum long DTE
        #[arg(long)]
        min_long_dte: Option<i32>,
        /// Maximum long DTE
        #[arg(long)]
        max_long_dte: Option<i32>,
        /// Target delta
        #[arg(long)]
        target_delta: Option<f64>,
        /// Minimum IV ratio (long/short)
        #[arg(long)]
        min_iv_ratio: Option<f64>,
        /// Disable parallel processing
        #[arg(long)]
        no_parallel: bool,
        /// Pricing IV interpolation model (sticky-strike, sticky-moneyness, sticky-delta)
        #[arg(long)]
        pricing_model: Option<String>,
        /// Volatility interpolation mode (linear, svi)
        #[arg(long)]
        vol_model: Option<String>,
        /// Strike matching mode (same-strike, same-delta)
        #[arg(long)]
        strike_match_mode: Option<String>,
        /// Maximum allowed IV at entry (filters trades with unreliable pricing, e.g., 1.5 for 150%)
        #[arg(long)]
        max_entry_iv: Option<f64>,
        /// Wing width for iron butterfly strategy (distance from ATM to wings)
        #[arg(long)]
        wing_width: Option<f64>,
    },

    /// Analyze results from a run
    Analyze {
        #[arg(long)]
        run_dir: PathBuf,
    },

    /// Price a single spread (for debugging)
    Price {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        strike: f64,
        #[arg(long)]
        short_expiry: String,
        #[arg(long)]
        long_expiry: String,
        #[arg(long)]
        date: String,
    },
}

#[derive(Tabled)]
struct ResultRow {
    #[tabled(rename = "Metric")]
    metric: String,
    #[tabled(rename = "Value")]
    value: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let log_level = if cli.verbose { Level::DEBUG } else { Level::INFO };
    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .init();

    println!("{}", style("Calendar Spread Backtest - Rust Edition").bold().cyan());
    println!();

    match cli.command {
        Commands::Backtest {
            conf,
            earnings_dir,
            start,
            end,
            option_type,
            strategy,
            delta_range,
            delta_scan_steps,
            symbols,
            output,
            entry_hour,
            entry_minute,
            exit_hour,
            exit_minute,
            min_market_cap,
            min_short_dte,
            max_short_dte,
            min_long_dte,
            max_long_dte,
            target_delta,
            min_iv_ratio,
            no_parallel,
            pricing_model,
            vol_model,
            strike_match_mode,
            max_entry_iv,
            wing_width,
        } => {
            run_backtest(
                conf,
                cli.data_dir,
                earnings_dir,
                &start,
                &end,
                &option_type,
                strategy,
                delta_range,
                delta_scan_steps,
                symbols,
                output,
                entry_hour,
                entry_minute,
                exit_hour,
                exit_minute,
                min_market_cap,
                min_short_dte,
                max_short_dte,
                min_long_dte,
                max_long_dte,
                target_delta,
                min_iv_ratio,
                !no_parallel,
                pricing_model,
                vol_model,
                strike_match_mode,
                max_entry_iv,
                wing_width,
            )
            .await?;
        }
        Commands::Analyze { run_dir } => {
            println!("Analyze command not yet implemented");
            println!("Run dir: {:?}", run_dir);
        }
        Commands::Price {
            symbol,
            strike,
            short_expiry,
            long_expiry,
            date,
        } => {
            println!("Price command not yet implemented");
            println!("Symbol: {}, Strike: {}, Short: {}, Long: {}, Date: {}",
                symbol, strike, short_expiry, long_expiry, date);
        }
    }

    Ok(())
}

/// Build CLI overrides from command-line arguments
fn build_cli_overrides(
    data_dir: Option<PathBuf>,
    earnings_dir: Option<PathBuf>,
    strategy_str: Option<String>,
    delta_range_str: Option<String>,
    delta_scan_steps: Option<usize>,
    symbols: Option<Vec<String>>,
    entry_hour: Option<u32>,
    entry_minute: Option<u32>,
    exit_hour: Option<u32>,
    exit_minute: Option<u32>,
    min_market_cap: Option<u64>,
    min_short_dte: Option<i32>,
    max_short_dte: Option<i32>,
    min_long_dte: Option<i32>,
    max_long_dte: Option<i32>,
    target_delta: Option<f64>,
    min_iv_ratio: Option<f64>,
    no_parallel: bool,
    pricing_model_str: Option<String>,
    vol_model_str: Option<String>,
    strike_match_mode_str: Option<String>,
    max_entry_iv: Option<f64>,
    wing_width: Option<f64>,
) -> Result<CliOverrides> {
    // Parse delta range if provided
    let delta_range = if let Some(ref range_str) = delta_range_str {
        let parts: Vec<&str> = range_str.split(',').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid delta range format. Use: --delta-range '0.25,0.75'");
        }
        let min: f64 = parts[0].trim().parse()
            .with_context(|| format!("Invalid delta range min: {}", parts[0]))?;
        let max: f64 = parts[1].trim().parse()
            .with_context(|| format!("Invalid delta range max: {}", parts[1]))?;
        Some((min, max))
    } else {
        None
    };

    Ok(CliOverrides {
        paths: if data_dir.is_some() || earnings_dir.is_some() {
            Some(CliPaths { data_dir, earnings_dir })
        } else {
            None
        },
        timing: if entry_hour.is_some() || entry_minute.is_some() || exit_hour.is_some() || exit_minute.is_some() {
            Some(CliTiming {
                entry_hour,
                entry_minute,
                exit_hour,
                exit_minute,
            })
        } else {
            None
        },
        selection: if min_short_dte.is_some() || max_short_dte.is_some() || min_long_dte.is_some() || max_long_dte.is_some() || target_delta.is_some() || min_iv_ratio.is_some() {
            Some(CliSelection {
                min_short_dte,
                max_short_dte,
                min_long_dte,
                max_long_dte,
                target_delta,
                min_iv_ratio,
            })
        } else {
            None
        },
        strategy: if strategy_str.is_some() || delta_range.is_some() || delta_scan_steps.is_some() || wing_width.is_some() {
            Some(CliStrategy {
                strategy_type: strategy_str,
                target_delta: None,
                delta_range,
                delta_scan_steps,
                wing_width,
            })
        } else {
            None
        },
        pricing: if pricing_model_str.is_some() || vol_model_str.is_some() {
            Some(CliPricing {
                model: pricing_model_str,
                vol_model: vol_model_str,
            })
        } else {
            None
        },
        strike_match_mode: strike_match_mode_str,
        symbols,
        min_market_cap,
        parallel: if no_parallel { Some(false) } else { None },
        max_entry_iv,
    })
}

async fn run_backtest(
    conf: Vec<PathBuf>,
    data_dir: Option<PathBuf>,
    earnings_dir: Option<PathBuf>,
    start_str: &str,
    end_str: &str,
    option_type_str: &str,
    strategy_str: Option<String>,
    delta_range_str: Option<String>,
    delta_scan_steps: Option<usize>,
    symbols: Option<Vec<String>>,
    output: Option<PathBuf>,
    entry_hour: Option<u32>,
    entry_minute: Option<u32>,
    exit_hour: Option<u32>,
    exit_minute: Option<u32>,
    min_market_cap: Option<u64>,
    min_short_dte: Option<i32>,
    max_short_dte: Option<i32>,
    min_long_dte: Option<i32>,
    max_long_dte: Option<i32>,
    target_delta: Option<f64>,
    min_iv_ratio: Option<f64>,
    no_parallel: bool,
    pricing_model_str: Option<String>,
    vol_model_str: Option<String>,
    strike_match_mode_str: Option<String>,
    max_entry_iv: Option<f64>,
    wing_width: Option<f64>,
) -> Result<()> {
    // Parse dates
    let start_date = NaiveDate::parse_from_str(start_str, "%Y-%m-%d")
        .with_context(|| format!("Invalid start date: {}", start_str))?;
    let end_date = NaiveDate::parse_from_str(end_str, "%Y-%m-%d")
        .with_context(|| format!("Invalid end date: {}", end_str))?;

    // Parse option type
    let option_type = match option_type_str.to_lowercase().as_str() {
        "call" => finq_core::OptionType::Call,
        "put" => finq_core::OptionType::Put,
        _ => anyhow::bail!("Invalid option type: {}. Must be 'call' or 'put'", option_type_str),
    };

    // Build CLI overrides
    let cli_overrides = build_cli_overrides(
        data_dir,
        earnings_dir,
        strategy_str,
        delta_range_str,
        delta_scan_steps,
        symbols,
        entry_hour,
        entry_minute,
        exit_hour,
        exit_minute,
        min_market_cap,
        min_short_dte,
        max_short_dte,
        min_long_dte,
        max_long_dte,
        target_delta,
        min_iv_ratio,
        no_parallel,
        pricing_model_str,
        vol_model_str,
        strike_match_mode_str,
        max_entry_iv,
        wing_width,
    )?;

    // Load configuration with layering
    let app_config = config::load_config(&conf, cli_overrides)?;

    // Convert to backtest config
    let backtest_config = app_config.to_backtest_config();

    let data_dir = backtest_config.data_dir.clone();
    let earnings_data_dir = backtest_config.earnings_dir.clone();
    let strategy = backtest_config.strategy;

    println!("{}", style("Configuration:").bold());
    println!("  Data dir:      {:?}", data_dir);
    println!("  Earnings dir:  {:?}", earnings_data_dir);
    println!("  Date range:    {} to {}", start_date, end_date);
    println!("  Option type:   {:?}", option_type);
    println!("  Strategy:      {:?}", strategy);
    println!("  Entry time:    {:02}:{:02}", backtest_config.timing.entry_hour, backtest_config.timing.entry_minute);
    println!("  Exit time:     {:02}:{:02}", backtest_config.timing.exit_hour, backtest_config.timing.exit_minute);
    println!("  Short DTE:     {}-{}", backtest_config.selection.min_short_dte, backtest_config.selection.max_short_dte);
    println!("  Long DTE:      {}-{}", backtest_config.selection.min_long_dte, backtest_config.selection.max_long_dte);
    if let Some(delta) = backtest_config.selection.target_delta {
        println!("  Target delta:  {:.3}", delta);
    }
    match strategy {
        StrategyType::DeltaScan => {
            println!("  Delta range:   {:.2}-{:.2}", backtest_config.delta_range.0, backtest_config.delta_range.1);
            println!("  Scan steps:    {}", backtest_config.delta_scan_steps);
        }
        StrategyType::IronButterfly => {
            println!("  Wing width:    ${:.2}", backtest_config.wing_width);
        }
        _ => {}
    }
    let strike_mode = match backtest_config.strike_match_mode {
        cs_domain::StrikeMatchMode::SameStrike => "same-strike (calendar)",
        cs_domain::StrikeMatchMode::SameDelta => "same-delta (diagonal)",
    };
    println!("  Strike match:  {}", strike_mode);
    if let Some(iv) = backtest_config.selection.min_iv_ratio {
        println!("  Min IV ratio:  {:.3}", iv);
    }
    if let Some(cap) = backtest_config.min_market_cap {
        println!("  Min mkt cap:   ${}", cap);
    }
    if let Some(ref syms) = backtest_config.symbols {
        println!("  Symbols:       {:?}", syms);
    }
    println!("  Parallel:      {}", backtest_config.parallel);
    println!("  Pricing model: {}", backtest_config.pricing_model);
    println!("  Vol model:     {:?}", backtest_config.vol_model);
    println!();

    // Create repositories
    info!("Initializing repositories...");

    // Earnings data repository
    let earnings_repo = EarningsReaderAdapter::new(earnings_data_dir.clone());

    // Options and equity data from FINQ_DATA_DIR
    let options_repo = FinqOptionsRepository::new(data_dir.clone());
    let equity_repo = FinqEquityRepository::new(data_dir.clone());

    // Create backtest use case
    let backtest = BacktestUseCase::new(
        earnings_repo,
        options_repo,
        equity_repo,
        backtest_config,
    );

    // Progress bar
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} [{elapsed_precise}] {msg}")
            .unwrap(),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let progress_data = Arc::new(Mutex::new((0usize, 0usize)));
    let progress_data_clone = Arc::clone(&progress_data);
    let pb_clone = pb.clone();

    // Run backtest
    info!("Starting backtest...");
    pb.set_message("Running backtest...");

    let result = backtest
        .execute(
            start_date,
            end_date,
            option_type,
            Some(Box::new(move |progress| {
                let mut data = progress_data_clone.lock().unwrap();
                data.0 += progress.entries_count;
                data.1 += 1;
                pb_clone.set_message(format!(
                    "Session {} | {} entries | {} total",
                    progress.session_date, progress.entries_count, data.0
                ));
            })),
        )
        .await?;

    pb.finish_with_message("Backtest complete");
    println!();

    // Display results
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

    let rows = vec![
        ResultRow { metric: "Sessions Processed".into(), value: result.sessions_processed.to_string() },
        ResultRow { metric: "Total Opportunities".into(), value: result.total_opportunities.to_string() },
        ResultRow { metric: "Trades Entered".into(), value: result.total_entries.to_string() },
        ResultRow { metric: "Trades Dropped".into(), value: result.dropped_events.len().to_string() },
        ResultRow { metric: "".into(), value: "".into() },
        ResultRow { metric: "Win Rate".into(), value: format!("{:.2}%", win_rate) },
        ResultRow { metric: "Total P&L".into(), value: format!("${:.2}", total_pnl) },
        ResultRow {
            metric: "Avg P&L per Trade".into(),
            value: if result.successful_trades() > 0 {
                format!("${:.2}", total_pnl / rust_decimal::Decimal::from(result.successful_trades()))
            } else {
                "$0.00".into()
            }
        },
        ResultRow { metric: "".into(), value: "".into() },
        ResultRow { metric: "Mean Return".into(), value: format!("{:.2}%", mean_return) },
        ResultRow { metric: "Std Dev".into(), value: format!("{:.2}%", std_return) },
        ResultRow { metric: "Sharpe Ratio".into(), value: format!("{:.2}", sharpe) },
        ResultRow { metric: "".into(), value: "".into() },
        ResultRow { metric: "Avg Winner".into(), value: format!("${:.2} ({:.2}%)", avg_winner, avg_winner_pct) },
        ResultRow { metric: "Avg Loser".into(), value: format!("${:.2} ({:.2}%)", avg_loser, avg_loser_pct) },
    ];

    let table = Table::new(rows);
    println!("{}", table);
    println!();

    // Show some sample trades
    if !result.results.is_empty() {
        println!("{}", style("Sample Trades:").bold());
        for (i, trade) in result.results.iter().take(5).enumerate() {
            let option_type_str = match trade.option_type() {
                Some(finq_core::OptionType::Call) => "Call",
                Some(finq_core::OptionType::Put) => "Put",
                None => "Straddle",
            };
            println!("  {}. {} {} @ {} | P&L: ${:.2} ({:.2}%)",
                i + 1,
                trade.symbol(),
                option_type_str,
                trade.strike().value(),
                trade.pnl(),
                trade.pnl_pct(),
            );
        }
        if result.results.len() > 5 {
            println!("  ... and {} more", result.results.len() - 5);
        }
        println!();
    }

    // Show dropped events summary
    if !result.dropped_events.is_empty() {
        println!("{}", style("Dropped Events:").bold().yellow());
        let mut reason_counts = std::collections::HashMap::new();
        for event in &result.dropped_events {
            *reason_counts.entry(&event.reason).or_insert(0) += 1;
        }
        for (reason, count) in reason_counts.iter() {
            println!("  {}: {} events", reason, count);
        }
        println!();
    }

    // Save results if output specified
    if let Some(output_path) = output {
        info!("Saving results to {:?}...", output_path);

        // Detect output format based on extension
        let is_json = output_path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("json"))
            .unwrap_or(false);

        if is_json {
            // Save all results as JSON (supports both calendar spreads and iron butterflies)
            let json_content = serde_json::to_string_pretty(&result.results)
                .context("Failed to serialize results to JSON")?;
            std::fs::write(&output_path, json_content)
                .context("Failed to write JSON file")?;
            println!("{}", style(format!("Results saved to {:?}", output_path)).green());
        } else {
            // Save as parquet (calendar spreads only for now)
            let results_repo = ParquetResultsRepository::new(output_path.parent().unwrap().to_path_buf());
            let run_id = output_path.file_stem().unwrap().to_str().unwrap();

            let calendar_results: Vec<_> = result.results.iter()
                .filter_map(|r| match r {
                    cs_backtest::TradeResult::CalendarSpread(cs) => Some(cs.clone()),
                    _ => None,
                })
                .collect();

            if calendar_results.is_empty() {
                println!("{}", style("Warning: No calendar spread results to save to parquet. Use .json extension for iron butterfly results.").yellow());
            } else {
                results_repo.save_results(&calendar_results, run_id).await?;
                println!("{}", style(format!("Results saved to {:?}", output_path)).green());
            }
        }
    }

    Ok(())
}
