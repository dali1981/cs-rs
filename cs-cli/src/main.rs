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

use cs_backtest::{BacktestConfig, BacktestUseCase, StrategyType, IVModel};
use cs_domain::{
    infrastructure::{
        EarningsReaderAdapter, FinqEquityRepository, FinqOptionsRepository,
        ParquetResultsRepository,
    },
    ResultsRepository,
    TimingConfig, TradeSelectionCriteria,
};

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
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start: String,
        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end: String,
        /// Option type (call/put)
        #[arg(long, default_value = "call")]
        option_type: String,
        /// Strategy type (atm)
        #[arg(long, default_value = "atm")]
        strategy: String,
        /// Filter to specific symbols
        #[arg(long)]
        symbols: Option<Vec<String>>,
        /// Output file path
        #[arg(long)]
        output: Option<PathBuf>,
        /// Entry hour (0-23)
        #[arg(long, default_value = "9")]
        entry_hour: u32,
        /// Entry minute (0-59)
        #[arg(long, default_value = "35")]
        entry_minute: u32,
        /// Exit hour (0-23)
        #[arg(long, default_value = "15")]
        exit_hour: u32,
        /// Exit minute (0-59)
        #[arg(long, default_value = "55")]
        exit_minute: u32,
        /// Minimum market cap filter
        #[arg(long)]
        min_market_cap: Option<u64>,
        /// Minimum short DTE
        #[arg(long, default_value = "3")]
        min_short_dte: i32,
        /// Maximum short DTE
        #[arg(long, default_value = "45")]
        max_short_dte: i32,
        /// Minimum long DTE
        #[arg(long, default_value = "14")]
        min_long_dte: i32,
        /// Maximum long DTE
        #[arg(long, default_value = "90")]
        max_long_dte: i32,
        /// Target delta
        #[arg(long)]
        target_delta: Option<f64>,
        /// Minimum IV ratio (long/short)
        #[arg(long)]
        min_iv_ratio: Option<f64>,
        /// Disable parallel processing
        #[arg(long)]
        no_parallel: bool,
        /// IV interpolation model (sticky-strike, sticky-moneyness, sticky-delta)
        #[arg(long, default_value = "sticky-strike")]
        iv_model: String,
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
            start,
            end,
            option_type,
            strategy,
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
            iv_model,
        } => {
            run_backtest(
                cli.data_dir,
                &start,
                &end,
                &option_type,
                &strategy,
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
                &iv_model,
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

async fn run_backtest(
    data_dir: Option<PathBuf>,
    start_str: &str,
    end_str: &str,
    option_type_str: &str,
    strategy_str: &str,
    symbols: Option<Vec<String>>,
    output: Option<PathBuf>,
    entry_hour: u32,
    entry_minute: u32,
    exit_hour: u32,
    exit_minute: u32,
    min_market_cap: Option<u64>,
    min_short_dte: i32,
    max_short_dte: i32,
    min_long_dte: i32,
    max_long_dte: i32,
    target_delta: Option<f64>,
    min_iv_ratio: Option<f64>,
    parallel: bool,
    iv_model_str: &str,
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

    // Parse strategy
    let strategy = match strategy_str.to_lowercase().as_str() {
        "atm" => StrategyType::ATM,
        _ => anyhow::bail!("Invalid strategy: {}. Must be 'atm'", strategy_str),
    };

    // Parse IV model
    let iv_model = IVModel::from_string(iv_model_str);

    // Get data directory and expand tilde
    let data_dir = data_dir
        .or_else(|| std::env::var("FINQ_DATA_DIR").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("data"));

    // Expand tilde in path
    let data_dir = if data_dir.starts_with("~") {
        if let Some(home) = dirs::home_dir() {
            let path_str = data_dir.to_string_lossy();
            let without_tilde = path_str.strip_prefix("~").unwrap_or(&path_str);
            let without_tilde = without_tilde.strip_prefix("/").unwrap_or(without_tilde);
            home.join(without_tilde)
        } else {
            data_dir
        }
    } else {
        data_dir
    };

    println!("{}", style("Configuration:").bold());
    println!("  Data dir:      {:?}", data_dir);
    println!("  Date range:    {} to {}", start_date, end_date);
    println!("  Option type:   {:?}", option_type);
    println!("  Strategy:      {:?}", strategy);
    println!("  Entry time:    {:02}:{:02}", entry_hour, entry_minute);
    println!("  Exit time:     {:02}:{:02}", exit_hour, exit_minute);
    println!("  Short DTE:     {}-{}", min_short_dte, max_short_dte);
    println!("  Long DTE:      {}-{}", min_long_dte, max_long_dte);
    if let Some(delta) = target_delta {
        println!("  Target delta:  {:.3}", delta);
    }
    if let Some(iv) = min_iv_ratio {
        println!("  Min IV ratio:  {:.3}", iv);
    }
    if let Some(cap) = min_market_cap {
        println!("  Min mkt cap:   ${}", cap);
    }
    if let Some(ref syms) = symbols {
        println!("  Symbols:       {:?}", syms);
    }
    println!("  Parallel:      {}", parallel);
    println!("  IV model:      {}", iv_model);
    println!();

    // Create repositories
    info!("Initializing repositories...");

    // Earnings data from nasdaq_earnings (earnings-rs)
    let earnings_data_dir = std::env::var("EARNINGS_DATA_DIR")
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("trading_project/nasdaq_earnings/data")
                .to_string_lossy()
                .to_string()
        });
    let earnings_repo = EarningsReaderAdapter::new(PathBuf::from(earnings_data_dir));

    // Options and equity data from FINQ_DATA_DIR
    let options_repo = FinqOptionsRepository::new(data_dir.clone());
    let equity_repo = FinqEquityRepository::new(data_dir.clone());

    // Create backtest config
    let config = BacktestConfig {
        data_dir: data_dir.clone(),
        timing: TimingConfig {
            entry_hour,
            entry_minute,
            exit_hour,
            exit_minute,
        },
        selection: TradeSelectionCriteria {
            min_short_dte,
            max_short_dte,
            min_long_dte,
            max_long_dte,
            target_delta,
            min_iv_ratio,
            max_bid_ask_spread_pct: None,
        },
        strategy,
        symbols,
        min_market_cap,
        parallel,
        iv_model,
    };

    // Create backtest use case
    let backtest = BacktestUseCase::new(
        earnings_repo,
        options_repo,
        equity_repo,
        config,
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
            println!("  {}. {} {} @ {} | P&L: ${:.2} ({:.2}%)",
                i + 1,
                trade.symbol,
                if trade.option_type == finq_core::OptionType::Call { "Call" } else { "Put" },
                trade.strike.value(),
                trade.pnl,
                trade.pnl_pct,
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
        let results_repo = ParquetResultsRepository::new(output_path.parent().unwrap().to_path_buf());
        let run_id = output_path.file_stem().unwrap().to_str().unwrap();
        results_repo.save_results(&result.results, run_id).await?;
        println!("{}", style(format!("Results saved to {:?}", output_path)).green());
    }

    Ok(())
}
