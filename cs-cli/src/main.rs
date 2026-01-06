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

use cs_backtest::{BacktestUseCase, EarningsAnalysisUseCase, GenerateIvTimeSeriesUseCase, MinuteAlignedIvUseCase};
use cs_domain::{
    infrastructure::{
        EarningsReaderAdapter, FinqEquityRepository, FinqOptionsRepository,
        ParquetEarningsRepository, ParquetResultsRepository,
    },
    value_objects::{AtmIvConfig, TimingConfig},
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
        /// Earnings data directory (for earnings-rs adapter)
        #[arg(long, env = "EARNINGS_DATA_DIR", conflicts_with = "earnings_file")]
        earnings_dir: Option<PathBuf>,
        /// Custom earnings file (parquet or JSON) - alternative to --earnings-dir
        #[arg(long, conflicts_with = "earnings_dir")]
        earnings_file: Option<PathBuf>,
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start: String,
        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end: String,
        /// Trade structure (calendar, iron-butterfly, straddle, calendar-straddle)
        #[arg(long)]
        spread: Option<String>,
        /// Strike selection method (atm, delta, delta-scan)
        #[arg(long)]
        selection: Option<String>,
        /// Option type (call/put) - required for calendar spreads only
        #[arg(long)]
        option_type: Option<String>,
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
        /// Entry time in HH:MM format (default: 09:35)
        #[arg(long)]
        entry_time: Option<String>,
        /// Exit time in HH:MM format (default: 15:55)
        #[arg(long)]
        exit_time: Option<String>,
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
        /// Straddle: Entry N trading days before earnings (default: 5)
        #[arg(long, default_value = "5")]
        straddle_entry_days: usize,
        /// Straddle: Exit N trading days before earnings (default: 1)
        #[arg(long, default_value = "1")]
        straddle_exit_days: usize,
        /// Straddle: Minimum days from entry to expiration (default: 7)
        #[arg(long, default_value = "7")]
        min_straddle_dte: i32,
        /// Straddle: Minimum entry price - total debit paid (e.g., 2.50)
        #[arg(long)]
        min_entry_price: Option<f64>,
        /// Straddle: Maximum entry price - caps max loss (e.g., 10.00)
        #[arg(long)]
        max_entry_price: Option<f64>,
        /// Post-earnings straddle: holding period in trading days (default: 5)
        #[arg(long, default_value = "5")]
        post_earnings_holding_days: usize,
        /// Minimum daily option notional: sum(all option volumes) × 100 × stock_price (e.g., 100000 for $100k)
        #[arg(long)]
        min_notional: Option<f64>,
        /// Enable delta hedging
        #[arg(long)]
        hedge: bool,
        /// Hedging strategy: time, delta, gamma (default: delta)
        #[arg(long, default_value = "delta")]
        hedge_strategy: String,
        /// For time-based: rehedge interval in hours (default: 24)
        #[arg(long, default_value = "24")]
        hedge_interval_hours: u64,
        /// For delta-based: threshold to trigger rehedge (default: 0.10)
        #[arg(long, default_value = "0.10")]
        delta_threshold: f64,
        /// Maximum number of rehedges per trade
        #[arg(long)]
        max_rehedges: Option<usize>,
        /// Transaction cost per share (default: 0.01)
        #[arg(long, default_value = "0.01")]
        hedge_cost_per_share: f64,
        /// Rolling strategy (weekly, monthly, or days:N) - only for straddle spreads
        #[arg(long)]
        roll_strategy: Option<String>,
        /// Day of week for weekly rolls (monday, tuesday, ..., friday)
        #[arg(long)]
        roll_day: Option<String>,
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

    /// Generate ATM IV time series for earnings detection
    AtmIv {
        /// Symbol(s) to analyze (comma-separated)
        #[arg(long, value_delimiter = ',')]
        symbols: Vec<String>,
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start: String,
        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end: String,
        /// Target maturities in days (default: 7,14,21,30,60,90)
        #[arg(long, value_delimiter = ',')]
        maturities: Option<Vec<u32>>,
        /// Maturity tolerance in days (default: 7)
        #[arg(long)]
        tolerance: Option<u32>,
        /// Output directory for parquet files and plots
        #[arg(long)]
        output: PathBuf,
        /// Generate plots
        #[arg(long)]
        plot: bool,
        /// Use EOD pricing instead of minute-aligned (default: minute-aligned)
        #[arg(long)]
        eod_pricing: bool,
        /// Use constant-maturity IV interpolation (variance-space interpolation to exact DTEs)
        #[arg(long, alias = "cm")]
        constant_maturity: bool,
        /// Minimum DTE for expiration inclusion (default: 3)
        #[arg(long, default_value = "3")]
        min_dte: i64,
        /// Include historical volatility computation
        #[arg(long)]
        with_hv: bool,
        /// HV windows in days (default: 10,20,30,60)
        #[arg(long, value_delimiter = ',')]
        hv_windows: Option<Vec<usize>>,
    },

    /// Analyze expected vs actual moves on earnings events
    EarningsAnalysis {
        /// Symbol(s) to analyze (comma-separated)
        #[arg(long, value_delimiter = ',', required = true)]
        symbols: Vec<String>,
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start: String,
        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end: String,
        /// Earnings data directory
        #[arg(long, env = "EARNINGS_DATA_DIR")]
        earnings_dir: Option<PathBuf>,
        /// Output format (parquet, csv, json)
        #[arg(long, default_value = "parquet")]
        format: String,
        /// Output file path (optional, defaults to ./earnings_analysis_<symbol>.parquet)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Run campaign-based backtest (declarative scheduling)
    Campaign {
        /// Symbols to trade
        #[arg(long)]
        symbols: Vec<String>,
        /// Strategy (calendar-spread, straddle, iron-butterfly)
        #[arg(long)]
        strategy: String,
        /// Period policy (earnings-only, inter-earnings, continuous, fixed)
        #[arg(long)]
        period_policy: String,
        /// Roll policy (none, weekly, monthly)
        #[arg(long, default_value = "none")]
        roll_policy: String,
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start: String,
        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end: String,
        /// Custom earnings file (parquet or JSON)
        #[arg(long)]
        earnings_file: Option<PathBuf>,
        /// Entry time in HH:MM format (default: 09:35)
        #[arg(long, default_value = "09:35")]
        entry_time: String,
        /// Exit time in HH:MM format (default: 15:55)
        #[arg(long, default_value = "15:55")]
        exit_time: String,
        /// Days before earnings to enter (for pre-earnings policy)
        #[arg(long)]
        entry_days_before: Option<u16>,
        /// Days after earnings to exit (for cross-earnings policy)
        #[arg(long)]
        exit_days_after: Option<u16>,
        /// Days after earnings to start inter-period trading
        #[arg(long, default_value = "2")]
        inter_entry_days_after: u16,
        /// Days before next earnings to stop inter-period trading
        #[arg(long, default_value = "3")]
        inter_exit_days_before: u16,
        /// Roll day for weekly policy (Mon, Tue, Wed, Thu, Fri)
        #[arg(long, default_value = "Fri")]
        roll_day: String,
        /// Output file path
        #[arg(long)]
        output: Option<PathBuf>,
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
            earnings_file,
            start,
            end,
            spread,
            selection,
            option_type,
            delta_range,
            delta_scan_steps,
            symbols,
            output,
            entry_time,
            exit_time,
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
            straddle_entry_days,
            straddle_exit_days,
            min_straddle_dte,
            min_entry_price,
            max_entry_price,
            post_earnings_holding_days,
            min_notional,
            hedge,
            hedge_strategy,
            hedge_interval_hours,
            delta_threshold,
            max_rehedges,
            hedge_cost_per_share,
            roll_strategy,
            roll_day,
        } => {
            run_backtest(
                conf,
                cli.data_dir,
                earnings_dir,
                earnings_file,
                &start,
                &end,
                spread.as_deref(),
                selection.as_deref(),
                option_type.as_deref(),
                delta_range,
                delta_scan_steps,
                symbols,
                output,
                entry_time,
                exit_time,
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
                straddle_entry_days,
                straddle_exit_days,
                min_straddle_dte,
                min_entry_price,
                max_entry_price,
                post_earnings_holding_days,
                min_notional,
                hedge,
                hedge_strategy,
                hedge_interval_hours,
                delta_threshold,
                max_rehedges,
                hedge_cost_per_share,
                roll_strategy,
                roll_day,
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
        Commands::AtmIv {
            symbols,
            start,
            end,
            maturities,
            tolerance,
            output,
            plot,
            eod_pricing,
            constant_maturity,
            min_dte,
            with_hv,
            hv_windows,
        } => {
            run_atm_iv_command(
                cli.data_dir.as_ref(),
                symbols,
                &start,
                &end,
                maturities,
                tolerance,
                output,
                plot,
                eod_pricing,
                constant_maturity,
                min_dte,
                with_hv,
                hv_windows,
            )
            .await?;
        }
        Commands::EarningsAnalysis {
            symbols,
            start,
            end,
            earnings_dir,
            format,
            output,
        } => {
            run_earnings_analysis_command(
                cli.data_dir.as_ref(),
                earnings_dir.as_ref(),
                symbols,
                &start,
                &end,
                &format,
                output,
            )
            .await?;
        }

        Commands::Campaign {
            symbols,
            strategy,
            period_policy,
            roll_policy,
            start,
            end,
            earnings_file,
            entry_time,
            exit_time,
            entry_days_before,
            exit_days_after,
            inter_entry_days_after,
            inter_exit_days_before,
            roll_day,
            output,
        } => {
            run_campaign_command(
                cli.data_dir.as_ref(),
                symbols,
                &strategy,
                &period_policy,
                &roll_policy,
                &start,
                &end,
                earnings_file.as_ref(),
                &entry_time,
                &exit_time,
                entry_days_before,
                exit_days_after,
                inter_entry_days_after,
                inter_exit_days_before,
                &roll_day,
                output,
            )
            .await?;
        }
    }

    Ok(())
}

/// Run rolling straddle strategy
async fn run_rolling_straddle(
    start_date: NaiveDate,
    end_date: NaiveDate,
    roll_policy: cs_domain::RollPolicy,
    backtest_config: cs_backtest::BacktestConfig,
    options_repo: FinqOptionsRepository,
    equity_repo: FinqEquityRepository,
    output: Option<PathBuf>,
) -> Result<()> {
    use cs_backtest::{TradeExecutor, DefaultTradeFactory, StraddlePricer, SpreadPricer, ExecutionConfig};
    use cs_domain::{MarketTime, Straddle, TradeFactory, OptionsDataRepository, EquityDataRepository};
    use std::sync::Arc;

    println!("{}", style("Rolling Straddle Strategy").bold().green());
    println!("  Roll policy:   {}", roll_policy.description());
    if backtest_config.hedge_config.is_enabled() {
        println!("  Hedging:       Enabled ({:?})", backtest_config.hedge_config.strategy);
    }
    println!();

    // Get symbol to run (for now, require single symbol)
    let symbol = match &backtest_config.symbols {
        Some(syms) if syms.len() == 1 => &syms[0],
        Some(syms) => anyhow::bail!("Rolling strategy currently supports single symbol only, got {} symbols", syms.len()),
        None => anyhow::bail!("--symbols is required for rolling strategy"),
    };

    // Create shared repos as trait objects
    let options_repo: Arc<dyn OptionsDataRepository> = Arc::new(options_repo);
    let equity_repo: Arc<dyn EquityDataRepository> = Arc::new(equity_repo);

    // Create trade factory for constructing straddles with real expirations
    let trade_factory = Arc::new(DefaultTradeFactory::new(
        Arc::clone(&options_repo),
        Arc::clone(&equity_repo),
    )) as Arc<dyn TradeFactory>;

    // Create straddle pricer with pricing model
    let spread_pricer = SpreadPricer::new().with_pricing_model(backtest_config.pricing_model.clone());
    let pricer = StraddlePricer::new(spread_pricer);

    // Execution config for straddles
    let config = ExecutionConfig::for_straddle(None);

    // Create trade executor for Straddle with rolling support
    let mut rolling_executor = TradeExecutor::<Straddle>::new(
        Arc::clone(&options_repo),
        Arc::clone(&equity_repo),
        pricer,
        trade_factory,
        config,
    )
    .with_roll_policy(roll_policy);

    // Apply hedging if enabled
    if backtest_config.hedge_config.is_enabled() {
        use cs_backtest::TimingStrategy;
        use cs_domain::StraddleTradeTiming;

        // Create a timing strategy for hedging (rehedge_times only uses HedgeStrategy, not earnings timing)
        let timing_strategy = TimingStrategy::Straddle(
            StraddleTradeTiming::new(backtest_config.timing.clone())
                .with_entry_days(backtest_config.straddle_entry_days)
                .with_exit_days(backtest_config.straddle_exit_days)
        );

        rolling_executor = rolling_executor.with_hedging(
            backtest_config.hedge_config.clone(),
            timing_strategy,
        );
    }

    // Define entry and exit times
    let entry_time = MarketTime {
        hour: backtest_config.timing.entry_hour,
        minute: backtest_config.timing.entry_minute,
    };
    let exit_time = MarketTime {
        hour: backtest_config.timing.exit_hour,
        minute: backtest_config.timing.exit_minute,
    };

    println!("{}", style("Executing rolling straddle...").bold());

    // Execute the rolling strategy
    let result = rolling_executor.execute_rolling(
        symbol,
        start_date,
        end_date,
        entry_time,
        exit_time,
    ).await;

    println!("{}", style("Results:").bold().green());
    println!();

    // Display results
    display_rolling_results(&result);

    // Save results if output specified
    if let Some(output_path) = output {
        info!("Saving results to {:?}...", output_path);
        let json_content = serde_json::to_string_pretty(&result)
            .context("Failed to serialize results to JSON")?;
        std::fs::write(&output_path, json_content)
            .context("Failed to write JSON file")?;
        println!("{}", style(format!("Results saved to {:?}", output_path)).green());
    }

    Ok(())
}

/// Display rolling straddle results
fn display_rolling_results(result: &cs_domain::RollingResult) {
    println!("  Symbol:               {}", result.symbol);
    println!("  Period:               {} to {}", result.start_date, result.end_date);
    println!("  Number of rolls:      {}", result.num_rolls);
    println!();
    println!("  Total Option P&L:     ${:.2}", result.total_option_pnl);
    if result.total_hedge_pnl != rust_decimal::Decimal::ZERO {
        println!("  Total Hedge P&L:      ${:.2}", result.total_hedge_pnl);
        println!("  Transaction Cost:     ${:.2}", result.total_transaction_cost);
    }
    println!("  Total P&L:            ${:.2}", result.total_pnl);
    println!();
    println!("  Win Rate:             {:.1}%", result.win_rate * 100.0);
    println!("  Avg P&L per Roll:     ${:.2}", result.avg_roll_pnl);
    println!("  Max Drawdown:         ${:.2}", result.max_drawdown);
    println!();

    // Show individual rolls
    if !result.rolls.is_empty() {
        println!("{}", style("Individual Rolls:").bold());
        println!();

        use tabled::{Table, Tabled};

        #[derive(Tabled)]
        struct RollRow {
            #[tabled(rename = "#")]
            num: usize,
            #[tabled(rename = "Entry")]
            entry: String,
            #[tabled(rename = "Exit")]
            exit: String,
            #[tabled(rename = "Expiry")]
            expiration: String,
            #[tabled(rename = "Strike")]
            strike: String,
            #[tabled(rename = "Opt P&L")]
            pnl: String,
            #[tabled(rename = "Hedge P&L")]
            hedge_pnl: String,
            #[tabled(rename = "Hedges")]
            hedge_count: String,
            #[tabled(rename = "Cost")]
            tx_cost: String,
            #[tabled(rename = "Net P&L")]
            net_pnl: String,
            #[tabled(rename = "Spot Δ%")]
            spot_move: String,
            #[tabled(rename = "IV Δ%")]
            iv_change: String,
            #[tabled(rename = "Reason")]
            reason: String,
        }

        let rows: Vec<RollRow> = result.rolls.iter().enumerate().map(|(i, roll)| {
            let hedge_pnl_val = roll.hedge_pnl.unwrap_or(rust_decimal::Decimal::ZERO);
            let net_pnl = roll.pnl + hedge_pnl_val - roll.transaction_cost;

            RollRow {
                num: i + 1,
                entry: roll.entry_date.format("%Y-%m-%d").to_string(),
                exit: roll.exit_date.format("%Y-%m-%d").to_string(),
                expiration: roll.expiration.format("%Y-%m-%d").to_string(),
                strike: format!("${:.2}", roll.strike),
                pnl: format!("${:.2}", roll.pnl),
                hedge_pnl: if roll.hedge_count > 0 {
                    format!("${:.2}", hedge_pnl_val)
                } else {
                    "-".to_string()
                },
                hedge_count: if roll.hedge_count > 0 {
                    format!("{}", roll.hedge_count)
                } else {
                    "-".to_string()
                },
                tx_cost: if roll.transaction_cost > rust_decimal::Decimal::ZERO {
                    format!("${:.2}", roll.transaction_cost)
                } else {
                    "-".to_string()
                },
                net_pnl: format!("${:.2}", net_pnl),
                spot_move: format!("{:.1}%", roll.spot_move_pct),
                iv_change: roll.iv_change.map(|c| format!("{:+.1}%", c * 100.0)).unwrap_or_else(|| "N/A".to_string()),
                reason: roll.roll_reason.to_string(),
            }
        }).collect();

        let table = Table::new(rows);
        println!("{}", table);

        // Show P&L attribution - prefer integrated position attribution if available
        let has_position_attribution = result.rolls.iter().any(|r| r.position_attribution.is_some());
        let has_legacy_attribution = result.rolls.iter().any(|r| r.delta_pnl.is_some());

        if has_position_attribution {
            // Display integrated position attribution (delta-hedged)
            println!();
            println!("{}", style("Integrated Position Attribution (Daily Recomputed):").bold());
            println!();

            #[derive(Tabled)]
            struct IntegratedAttributionRow {
                #[tabled(rename = "#")]
                num: usize,
                #[tabled(rename = "Days")]
                days: String,
                #[tabled(rename = "Gross Δ P&L")]
                gross_delta: String,
                #[tabled(rename = "Hedge Δ P&L")]
                hedge_delta: String,
                #[tabled(rename = "Net Δ P&L")]
                net_delta: String,
                #[tabled(rename = "Γ P&L")]
                gamma: String,
                #[tabled(rename = "Θ P&L")]
                theta: String,
                #[tabled(rename = "ν P&L")]
                vega: String,
                #[tabled(rename = "Unexplained")]
                unexplained: String,
                #[tabled(rename = "Hedge Eff.")]
                hedge_eff: String,
            }

            let attr_rows: Vec<IntegratedAttributionRow> = result.rolls.iter().enumerate().map(|(i, roll)| {
                if let Some(ref attr) = roll.position_attribution {
                    IntegratedAttributionRow {
                        num: i + 1,
                        days: attr.num_days().to_string(),
                        gross_delta: format!("${:.2}", attr.total_gross_delta_pnl),
                        hedge_delta: format!("${:.2}", attr.total_hedge_delta_pnl),
                        net_delta: format!("${:.2}", attr.total_net_delta_pnl),
                        gamma: format!("${:.2}", attr.total_gamma_pnl),
                        theta: format!("${:.2}", attr.total_theta_pnl),
                        vega: format!("${:.2}", attr.total_vega_pnl),
                        unexplained: format!("${:.2}", attr.total_unexplained),
                        hedge_eff: format!("{:.1}%", attr.hedge_efficiency),
                    }
                } else {
                    IntegratedAttributionRow {
                        num: i + 1,
                        days: "-".to_string(),
                        gross_delta: "-".to_string(),
                        hedge_delta: "-".to_string(),
                        net_delta: "-".to_string(),
                        gamma: "-".to_string(),
                        theta: "-".to_string(),
                        vega: "-".to_string(),
                        unexplained: "-".to_string(),
                        hedge_eff: "-".to_string(),
                    }
                }
            }).collect();

            let attr_table = Table::new(attr_rows);
            println!("{}", attr_table);

            println!();
            println!("{}", style("Legend:").dim());
            println!("  {}", style("Gross Δ P&L: Options delta P&L (unhedged)").dim());
            println!("  {}", style("Hedge Δ P&L: Stock hedge P&L").dim());
            println!("  {}", style("Net Δ P&L: Combined delta P&L (Gross + Hedge)").dim());
            println!("  {}", style("Hedge Eff.: |Hedge Δ| / |Gross Δ| × 100%").dim());
        } else if has_legacy_attribution {
            // Display legacy attribution (non-hedged)
            println!();
            println!("{}", style("P&L Attribution (Greeks):").bold());
            println!();

            #[derive(Tabled)]
            struct AttributionRow {
                #[tabled(rename = "#")]
                num: usize,
                #[tabled(rename = "Delta P&L")]
                delta: String,
                #[tabled(rename = "Gamma P&L")]
                gamma: String,
                #[tabled(rename = "Theta P&L")]
                theta: String,
                #[tabled(rename = "Vega P&L")]
                vega: String,
                #[tabled(rename = "Unexplained")]
                unexplained: String,
            }

            let attr_rows: Vec<AttributionRow> = result.rolls.iter().enumerate().map(|(i, roll)| {
                AttributionRow {
                    num: i + 1,
                    delta: roll.delta_pnl.map(|v| format!("${:.2}", v)).unwrap_or_else(|| "-".to_string()),
                    gamma: roll.gamma_pnl.map(|v| format!("${:.2}", v)).unwrap_or_else(|| "-".to_string()),
                    theta: roll.theta_pnl.map(|v| format!("${:.2}", v)).unwrap_or_else(|| "-".to_string()),
                    vega: roll.vega_pnl.map(|v| format!("${:.2}", v)).unwrap_or_else(|| "-".to_string()),
                    unexplained: roll.unexplained_pnl.map(|v| format!("${:.2}", v)).unwrap_or_else(|| "-".to_string()),
                }
            }).collect();

            let attr_table = Table::new(attr_rows);
            println!("{}", attr_table);
        }
    }
}

/// Parse roll strategy from CLI arguments
fn parse_roll_policy(strategy_str: &str, roll_day_str: Option<&str>) -> Result<cs_domain::RollPolicy> {
    use chrono::Weekday;

    match strategy_str.to_lowercase().as_str() {
        "weekly" => {
            // Require --roll-day for weekly
            let day_str = roll_day_str.context("--roll-day is required for weekly roll strategy")?;
            let weekday = match day_str.to_lowercase().as_str() {
                "monday" => Weekday::Mon,
                "tuesday" => Weekday::Tue,
                "wednesday" => Weekday::Wed,
                "thursday" => Weekday::Thu,
                "friday" => Weekday::Fri,
                _ => anyhow::bail!("Invalid --roll-day: {}. Must be monday-friday", day_str),
            };
            Ok(cs_domain::RollPolicy::Weekly { roll_day: weekday })
        }
        s if s.starts_with("days:") => {
            // Parse days:N format
            let interval_str = &s[5..];
            let interval: u16 = interval_str.parse()
                .with_context(|| format!("Invalid interval in '{}'. Expected days:N (e.g., days:5)", strategy_str))?;
            Ok(cs_domain::RollPolicy::TradingDays { interval })
        }
        _ => anyhow::bail!("Invalid --roll-strategy: {}. Must be 'weekly' or 'days:N'", strategy_str),
    }
}

/// Build CLI overrides from command-line arguments
fn build_cli_overrides(
    data_dir: Option<PathBuf>,
    earnings_dir: Option<PathBuf>,
    spread: Option<&str>,
    selection: Option<&str>,
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
    straddle_entry_days: Option<usize>,
    straddle_exit_days: Option<usize>,
    min_straddle_dte: Option<i32>,
    min_entry_price: Option<f64>,
    max_entry_price: Option<f64>,
    post_earnings_holding_days: Option<usize>,
    min_notional: Option<f64>,
    hedge: bool,
    hedge_strategy: Option<String>,
    hedge_interval_hours: Option<u64>,
    delta_threshold: Option<f64>,
    max_rehedges: Option<usize>,
    hedge_cost_per_share: Option<f64>,
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
        strategy: if spread.is_some() || selection.is_some() || delta_range.is_some() || delta_scan_steps.is_some() || wing_width.is_some() || straddle_entry_days.is_some() || straddle_exit_days.is_some() || min_straddle_dte.is_some() || min_entry_price.is_some() || max_entry_price.is_some() || post_earnings_holding_days.is_some() {
            Some(CliStrategy {
                spread_type: spread.map(|s| s.to_string()),
                selection_type: selection.map(|s| s.to_string()),
                target_delta: None,
                delta_range,
                delta_scan_steps,
                wing_width,
                straddle_entry_days,
                straddle_exit_days,
                min_straddle_dte,
                min_entry_price,
                max_entry_price,
                post_earnings_holding_days,
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
        hedging: if hedge || hedge_strategy.is_some() || hedge_interval_hours.is_some() || delta_threshold.is_some() || max_rehedges.is_some() || hedge_cost_per_share.is_some() {
            Some(CliHedging {
                enabled: Some(hedge),
                strategy: hedge_strategy,
                interval_hours: hedge_interval_hours,
                delta_threshold,
                max_rehedges,
                cost_per_share: hedge_cost_per_share,
            })
        } else {
            None
        },
        strike_match_mode: strike_match_mode_str,
        symbols,
        min_market_cap,
        parallel: if no_parallel { Some(false) } else { None },
        max_entry_iv,
        min_notional,
    })
}

async fn run_backtest(
    conf: Vec<PathBuf>,
    data_dir: Option<PathBuf>,
    earnings_dir: Option<PathBuf>,
    earnings_file: Option<PathBuf>,
    start_str: &str,
    end_str: &str,
    spread_str: Option<&str>,
    selection_str: Option<&str>,
    option_type_str: Option<&str>,
    delta_range_str: Option<String>,
    delta_scan_steps: Option<usize>,
    symbols: Option<Vec<String>>,
    output: Option<PathBuf>,
    entry_time: Option<String>,
    exit_time: Option<String>,
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
    straddle_entry_days: usize,
    straddle_exit_days: usize,
    min_straddle_dte: i32,
    min_entry_price: Option<f64>,
    max_entry_price: Option<f64>,
    post_earnings_holding_days: usize,
    min_notional: Option<f64>,
    hedge: bool,
    hedge_strategy: String,
    hedge_interval_hours: u64,
    delta_threshold: f64,
    max_rehedges: Option<usize>,
    hedge_cost_per_share: f64,
    roll_strategy_str: Option<String>,
    roll_day_str: Option<String>,
) -> Result<()> {
    // Parse dates
    let start_date = NaiveDate::parse_from_str(start_str, "%Y-%m-%d")
        .with_context(|| format!("Invalid start date: {}", start_str))?;
    let end_date = NaiveDate::parse_from_str(end_str, "%Y-%m-%d")
        .with_context(|| format!("Invalid end date: {}", end_str))?;

    // Parse and validate spread/selection if provided
    let (spread_opt, selection_opt, option_type) = if let Some(spread_str) = spread_str {
        // Parse spread type
        let spread = match spread_str.to_lowercase().replace('-', "_").as_str() {
            "calendar" => "calendar",
            "iron_butterfly" | "ironbutterfly" | "butterfly" => "iron_butterfly",
            "straddle" | "long_straddle" => "straddle",
            "calendar_straddle" | "calendarstraddle" => "calendar_straddle",
            "post_earnings_straddle" | "postearningstraddle" | "post_straddle" => "post_earnings_straddle",
            _ => anyhow::bail!("Invalid spread type: {}. Must be 'calendar', 'iron-butterfly', 'straddle', 'calendar-straddle', or 'post-earnings-straddle'", spread_str),
        };

        // Validate arguments based on spread type
        let option_type = match spread {
            "calendar" => {
                // Calendar spreads REQUIRE option-type
                match option_type_str {
                    Some(ot) => match ot.to_lowercase().as_str() {
                        "call" => finq_core::OptionType::Call,
                        "put" => finq_core::OptionType::Put,
                        _ => anyhow::bail!("Invalid option type: {}. Must be 'call' or 'put'", ot),
                    },
                    None => anyhow::bail!("--option-type is required for calendar spreads"),
                }
            }
            "iron_butterfly" => {
                // Iron butterfly FORBIDS option-type
                if option_type_str.is_some() {
                    anyhow::bail!("--option-type is invalid for iron-butterfly (uses both calls and puts)");
                }
                // Default to Call for iron butterfly (will be ignored in execution)
                finq_core::OptionType::Call
            }
            "straddle" => {
                // Straddle FORBIDS option-type
                if option_type_str.is_some() {
                    anyhow::bail!("--option-type is invalid for straddle (uses both calls and puts)");
                }
                // Default to Call for straddle (will be ignored in execution)
                finq_core::OptionType::Call
            }
            "calendar_straddle" => {
                // Calendar straddle FORBIDS option-type
                if option_type_str.is_some() {
                    anyhow::bail!("--option-type is invalid for calendar-straddle (uses both calls and puts)");
                }
                // Default to Call for calendar straddle (will be ignored in execution)
                finq_core::OptionType::Call
            }
            "post_earnings_straddle" => {
                // Post-earnings straddle FORBIDS option-type
                if option_type_str.is_some() {
                    anyhow::bail!("--option-type is invalid for post-earnings-straddle (uses both calls and puts)");
                }
                // Default to Call for post-earnings straddle (will be ignored in execution)
                finq_core::OptionType::Call
            }
            _ => unreachable!(),
        };

        // Additional validation for iron butterfly
        if spread == "iron_butterfly" && strike_match_mode_str.is_some() {
            anyhow::bail!("--strike-match-mode is only valid for calendar spreads");
        }

        // Additional validation for calendar
        if spread == "calendar" && wing_width.is_some() {
            anyhow::bail!("--wing-width is only valid for iron-butterfly spreads");
        }

        // Additional validation for straddle
        if spread == "straddle" && strike_match_mode_str.is_some() {
            anyhow::bail!("--strike-match-mode is not applicable to straddle strategy");
        }
        if spread == "straddle" && wing_width.is_some() {
            anyhow::bail!("--wing-width is not applicable to straddle strategy");
        }

        // Additional validation for calendar straddle
        if spread == "calendar_straddle" && strike_match_mode_str.is_some() {
            anyhow::bail!("--strike-match-mode is not applicable to calendar-straddle (always uses same strike for straddles)");
        }
        if spread == "calendar_straddle" && wing_width.is_some() {
            anyhow::bail!("--wing-width is not applicable to calendar-straddle strategy");
        }

        // Parse selection type if provided
        let selection = if let Some(sel_str) = selection_str {
            Some(match sel_str.to_lowercase().replace('-', "_").as_str() {
                "atm" => "atm",
                "delta" => "delta",
                "delta_scan" | "deltascan" => "delta_scan",
                _ => anyhow::bail!("Invalid selection type: {}. Must be 'atm', 'delta', or 'delta-scan'", sel_str),
            })
        } else {
            None
        };

        (Some(spread), selection, option_type)
    } else {
        // No spread specified - will use config file
        // Parse selection if provided
        let selection = if let Some(sel_str) = selection_str {
            Some(match sel_str.to_lowercase().replace('-', "_").as_str() {
                "atm" => "atm",
                "delta" => "delta",
                "delta_scan" | "deltascan" => "delta_scan",
                _ => anyhow::bail!("Invalid selection type: {}. Must be 'atm', 'delta', or 'delta-scan'", sel_str),
            })
        } else {
            None
        };

        // Parse option-type if provided (validation will happen after config load)
        let option_type = if let Some(ot) = option_type_str {
            match ot.to_lowercase().as_str() {
                "call" => finq_core::OptionType::Call,
                "put" => finq_core::OptionType::Put,
                _ => anyhow::bail!("Invalid option type: {}. Must be 'call' or 'put'", ot),
            }
        } else {
            // Default to Call if not specified (may be overridden by validation after config load)
            finq_core::OptionType::Call
        };

        (None, selection, option_type)
    };

    // Parse time strings (HH:MM format)
    let parse_time = |time_str: Option<String>| -> Result<(Option<u32>, Option<u32>)> {
        match time_str {
            None => Ok((None, None)),
            Some(s) => {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() != 2 {
                    anyhow::bail!("Invalid time format '{}'. Expected HH:MM (e.g., 09:35)", s);
                }
                let hour: u32 = parts[0].parse()
                    .map_err(|_| anyhow::anyhow!("Invalid hour in time '{}'", s))?;
                let minute: u32 = parts[1].parse()
                    .map_err(|_| anyhow::anyhow!("Invalid minute in time '{}'", s))?;

                if hour > 23 {
                    anyhow::bail!("Hour must be 0-23, got {}", hour);
                }
                if minute > 59 {
                    anyhow::bail!("Minute must be 0-59, got {}", minute);
                }

                Ok((Some(hour), Some(minute)))
            }
        }
    };

    let (entry_hour, entry_minute) = parse_time(entry_time)?;
    let (exit_hour, exit_minute) = parse_time(exit_time)?;

    // Parse roll strategy if provided
    let roll_policy_opt = if let Some(strategy_str) = roll_strategy_str {
        // Validate that spread is straddle
        if spread_opt != Some("straddle") {
            anyhow::bail!("--roll-strategy is only valid for straddle spreads");
        }

        Some(parse_roll_policy(&strategy_str, roll_day_str.as_deref())?)
    } else {
        None
    };

    // Build CLI overrides
    let cli_overrides = build_cli_overrides(
        data_dir,
        earnings_dir,
        spread_opt,
        selection_opt,
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
        Some(straddle_entry_days),
        Some(straddle_exit_days),
        Some(min_straddle_dte),
        min_entry_price,
        max_entry_price,
        Some(post_earnings_holding_days),
        min_notional,
        hedge,
        Some(hedge_strategy),
        Some(hedge_interval_hours),
        Some(delta_threshold),
        max_rehedges,
        Some(hedge_cost_per_share),
    )?;

    // Load configuration with layering
    let app_config = config::load_config(&conf, cli_overrides)?;

    // Convert to backtest config
    let backtest_config = app_config.to_backtest_config();

    let data_dir = backtest_config.data_dir.clone();
    let earnings_data_dir = backtest_config.earnings_dir.clone();
    let spread = backtest_config.spread;
    let selection = backtest_config.selection_strategy;

    println!("{}", style("Configuration:").bold());
    println!("  Data dir:      {:?}", data_dir);
    println!("  Earnings dir:  {:?}", earnings_data_dir);
    println!("  Date range:    {} to {}", start_date, end_date);

    // Display spread and selection
    match spread {
        cs_backtest::SpreadType::Calendar => {
            println!("  Spread:        Calendar");
            println!("  Option type:   {:?}", option_type);
        }
        cs_backtest::SpreadType::IronButterfly => {
            println!("  Spread:        Iron Butterfly");
            println!("  Wing width:    ${:.2}", backtest_config.wing_width);
        }
        cs_backtest::SpreadType::Straddle => {
            println!("  Spread:        Straddle (Long Volatility)");
            println!("  Entry:         {} trading days before earnings", backtest_config.straddle_entry_days);
            println!("  Exit:          {} trading day(s) before earnings", backtest_config.straddle_exit_days);
            println!("  Expiration:    First expiry after earnings");
        }
        cs_backtest::SpreadType::CalendarStraddle => {
            println!("  Spread:        Calendar Straddle");
            println!("  Structure:     Short near-term straddle + Long far-term straddle");
            println!("  Delta:         Neutral (call + put at same strike)");
        }
        cs_backtest::SpreadType::PostEarningsStraddle => {
            println!("  Spread:        Post-Earnings Straddle");
            println!("  Entry:         Day after earnings (BMO: same day, AMC: next day)");
            println!("  Exit:          {} trading days after entry", backtest_config.post_earnings_holding_days);
            println!("  Expiration:    First expiry after exit date");
        }
    }

    // Display selection strategy
    match selection {
        cs_backtest::SelectionType::ATM => {
            println!("  Selection:     ATM");
        }
        cs_backtest::SelectionType::Delta => {
            println!("  Selection:     Delta (target: {:.2})", backtest_config.target_delta);
        }
        cs_backtest::SelectionType::DeltaScan => {
            println!("  Selection:     Delta Scan");
            println!("  Delta range:   {:.2}-{:.2}", backtest_config.delta_range.0, backtest_config.delta_range.1);
            println!("  Scan steps:    {}", backtest_config.delta_scan_steps);
        }
    }

    println!("  Entry time:    {:02}:{:02}", backtest_config.timing.entry_hour, backtest_config.timing.entry_minute);
    println!("  Exit time:     {:02}:{:02}", backtest_config.timing.exit_hour, backtest_config.timing.exit_minute);
    println!("  Short DTE:     {}-{}", backtest_config.selection.min_short_dte, backtest_config.selection.max_short_dte);
    println!("  Long DTE:      {}-{}", backtest_config.selection.min_long_dte, backtest_config.selection.max_long_dte);

    // Only show strike match mode for calendar spreads
    if matches!(spread, cs_backtest::SpreadType::Calendar) {
        let strike_mode = match backtest_config.strike_match_mode {
            cs_domain::StrikeMatchMode::SameStrike => "same-strike (calendar)",
            cs_domain::StrikeMatchMode::SameDelta => "same-delta (diagonal)",
        };
        println!("  Strike match:  {}", strike_mode);
    }
    // Calendar straddle always uses same strike
    if matches!(spread, cs_backtest::SpreadType::CalendarStraddle) {
        println!("  Strike match:  same-strike (straddle pairs)");
    }
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

    // Earnings data repository - choose between custom file or earnings-rs adapter
    use cs_domain::infrastructure::{CustomFileEarningsReader, EarningsReaderAdapter};
    let earnings_repo: Box<dyn cs_domain::repositories::EarningsRepository> = match (earnings_file, earnings_data_dir.clone()) {
        (Some(file), _) => {
            info!("Using custom earnings file: {:?}", file);
            Box::new(CustomFileEarningsReader::from_file(file)?)
        },
        (None, dir) => {
            info!("Using earnings-rs adapter with directory: {:?}", dir);
            Box::new(EarningsReaderAdapter::new(dir))
        },
    };

    // Options and equity data from FINQ_DATA_DIR
    let options_repo = FinqOptionsRepository::new(data_dir.clone());
    let equity_repo = FinqEquityRepository::new(data_dir.clone());

    // Check if we're doing rolling strategy
    if let Some(roll_policy) = roll_policy_opt {
        // Rolling straddle execution - different path
        return run_rolling_straddle(
            start_date,
            end_date,
            roll_policy,
            backtest_config,
            options_repo,
            equity_repo,
            output,
        ).await;
    }

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

    // Create progress callback
    let progress_callback = Box::new(move |progress: cs_backtest::SessionProgress| {
        let mut data = progress_data_clone.lock().unwrap();
        data.0 += progress.entries_count;
        data.1 += 1;
        pb_clone.set_message(format!(
            "Session {} | {} entries | {} total",
            progress.session_date, progress.entries_count, data.0
        ));
    }) as Box<dyn Fn(cs_backtest::SessionProgress) + Send + Sync>;

    // Call specific backtest method based on spread type and display results
    match spread {
        cs_backtest::SpreadType::Calendar => {
            let result = backtest.execute_calendar_spread(start_date, end_date, option_type, Some(progress_callback)).await?;
            pb.finish_with_message("Backtest complete");
            println!();
            display_backtest_results(&result);
            save_backtest_results(&result, spread, output).await?;
        }
        cs_backtest::SpreadType::Straddle => {
            let result = backtest.execute_straddle(start_date, end_date, Some(progress_callback)).await?;
            pb.finish_with_message("Backtest complete");
            println!();
            display_backtest_results(&result);
            save_backtest_results(&result, spread, output).await?;
        }
        cs_backtest::SpreadType::IronButterfly => {
            let result = backtest.execute_iron_butterfly(start_date, end_date, Some(progress_callback)).await?;
            pb.finish_with_message("Backtest complete");
            println!();
            display_backtest_results(&result);
            save_backtest_results(&result, spread, output).await?;
        }
        cs_backtest::SpreadType::CalendarStraddle => {
            let result = backtest.execute_calendar_straddle(start_date, end_date, Some(progress_callback)).await?;
            pb.finish_with_message("Backtest complete");
            println!();
            display_backtest_results(&result);
            save_backtest_results(&result, spread, output).await?;
        }
        cs_backtest::SpreadType::PostEarningsStraddle => {
            let result = backtest.execute_post_earnings_straddle(start_date, end_date, Some(progress_callback)).await?;
            pb.finish_with_message("Backtest complete");
            println!();
            display_backtest_results(&result);
            save_backtest_results(&result, spread, output).await?;
        }
    }

    Ok(())
}

/// Display backtest results (generic over trade result type)
fn display_backtest_results<R>(result: &cs_backtest::BacktestResult<R>)
where
    R: cs_domain::TradeResult + cs_backtest::TradeResultMethods,
{
    use cs_domain::TradeResult as TradeResultTrait;

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

    // Show some sample trades - using TradeResultMethods trait
    if !result.results.is_empty() {
        use cs_backtest::TradeResultMethods;
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

    // Show dropped events summary
    if !result.dropped_events.is_empty() {
        println!("{}", style("Dropped Events:").bold().yellow());

        // Group by reason for summary
        let mut reason_groups: std::collections::HashMap<String, Vec<_>> = std::collections::HashMap::new();
        for event in &result.dropped_events {
            reason_groups.entry(event.reason.clone()).or_insert_with(Vec::new).push(event);
        }

        // Show each reason group with examples
        for (reason, events) in reason_groups.iter() {
            println!("  {}: {} events", reason, events.len());

            // Show first 3 examples with symbol and date
            for (i, event) in events.iter().take(3).enumerate() {
                let details_str = event.details.as_ref()
                    .map(|d| format!(" - {}", d))
                    .unwrap_or_default();
                println!("    {} {} ({}){}",
                    if i == 0 { "↳" } else { " " },
                    event.symbol,
                    event.earnings_date,
                    details_str
                );
            }

            // Show "and N more" if there are more than 3
            if events.len() > 3 {
                println!("      ... and {} more", events.len() - 3);
            }
        }
        println!();
    }
}

/// Save backtest results to file (generic over trade result type)
async fn save_backtest_results<R>(
    result: &cs_backtest::BacktestResult<R>,
    spread: cs_backtest::SpreadType,
    output: Option<PathBuf>,
) -> Result<()>
where
    R: cs_domain::TradeResult + serde::Serialize,
{
    if let Some(output_path) = output {
        info!("Saving results to {:?}...", output_path);

        // Detect output format based on extension
        let is_json = output_path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("json"))
            .unwrap_or(false);

        if is_json {
            // Save all results as JSON (supports all spread types)
            let json_content = serde_json::to_string_pretty(&result.results)
                .context("Failed to serialize results to JSON")?;
            std::fs::write(&output_path, json_content)
                .context("Failed to write JSON file")?;
            println!("{}", style(format!("Results saved to {:?}", output_path)).green());
        } else {
            // Parquet export only supported for calendar spreads
            match spread {
                cs_backtest::SpreadType::Calendar => {
                    // Only call save_results when we know the concrete type is CalendarSpreadResult
                    // This branch is only reached for Calendar spreads, so we can safely cast
                    let results_repo = ParquetResultsRepository::new(output_path.parent().unwrap().to_path_buf());
                    let run_id = output_path.file_stem().unwrap().to_str().unwrap();
                    // This will fail at runtime if R is not CalendarSpreadResult, but that's guaranteed by the match
                    // We need to use unsafe transmute or dynamic dispatch here - for now, just warn
                    println!("{}", style("Warning: Parquet export only supported via JSON for now. Saving as JSON instead.").yellow());
                    let json_content = serde_json::to_string_pretty(&result.results)
                        .context("Failed to serialize results to JSON")?;
                    std::fs::write(&output_path, json_content)
                        .context("Failed to write JSON file")?;
                    println!("{}", style(format!("Results saved to {:?}", output_path)).green());
                }
                _ => {
                    println!("{}", style("Warning: Parquet export only supported for calendar spreads. Use .json extension for other strategies.").yellow());
                }
            }
        }
    }

    Ok(())
}

/// Run ATM IV time series generation command
async fn run_atm_iv_command(
    data_dir: Option<&PathBuf>,
    symbols: Vec<String>,
    start: &str,
    end: &str,
    maturities: Option<Vec<u32>>,
    tolerance: Option<u32>,
    output: PathBuf,
    plot: bool,
    eod_pricing: bool,
    constant_maturity: bool,
    min_dte: i64,
    with_hv: bool,
    hv_windows: Option<Vec<usize>>,
) -> Result<()> {
    use cs_domain::value_objects::{IvInterpolationMethod, HvConfig};

    // Parse dates
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .context("Invalid start date format. Use YYYY-MM-DD")?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .context("Invalid end date format. Use YYYY-MM-DD")?;

    // Build config
    let mut config = AtmIvConfig::default();
    if let Some(mats) = maturities {
        config.maturity_targets = mats;
    }
    if let Some(tol) = tolerance {
        config.maturity_tolerance = tol;
    }
    config.interpolation_method = if constant_maturity {
        IvInterpolationMethod::ConstantMaturity
    } else {
        IvInterpolationMethod::Rolling
    };
    config.min_dte = min_dte;

    // Build HV config if requested
    let hv_config = if with_hv {
        let mut hv_cfg = HvConfig::default();
        if let Some(windows) = hv_windows {
            hv_cfg.windows = windows;
        }
        Some(hv_cfg)
    } else {
        None
    };

    // Determine data directory
    let data_dir = data_dir
        .cloned()
        .or_else(|| std::env::var("FINQ_DATA_DIR").ok().map(PathBuf::from))
        .context("Data directory not specified. Use --data-dir or set FINQ_DATA_DIR")?;

    println!("{}", style("ATM IV Time Series Generation").bold().cyan());
    println!("Mode: {}", if eod_pricing { "EOD" } else { "Minute-Aligned (default)" });
    println!("Interpolation: {}", match config.interpolation_method {
        IvInterpolationMethod::Rolling => "Rolling TTE",
        IvInterpolationMethod::ConstantMaturity => "Constant-Maturity (variance interpolation)",
    });
    println!("Symbols: {}", symbols.join(", "));
    println!("Date range: {} to {}", start_date, end_date);
    println!("Maturities: {:?}", config.maturity_targets);
    println!("Tolerance: {} days", config.maturity_tolerance);
    println!("Min DTE: {}", config.min_dte);
    println!("Output: {:?}", output);
    println!();

    // Create output directory
    std::fs::create_dir_all(&output)?;

    // Create repositories
    let equity_repo = FinqEquityRepository::new(data_dir.clone());
    let options_repo = FinqOptionsRepository::new(data_dir);

    // Process each symbol based on mode (minute-aligned is default)
    if !eod_pricing {
        // Use minute-aligned IV computation (default)
        let use_case = MinuteAlignedIvUseCase::new(equity_repo, options_repo);

        for symbol in &symbols {
            println!("{}", style(format!("Processing {}...", symbol)).bold());

            let result = use_case.execute(symbol, start_date, end_date, &config, hv_config.as_ref()).await?;

            println!(
                "  {} trading days processed, {} successful observations",
                result.total_days, result.successful_days
            );

            if result.observations.is_empty() {
                println!("{}", style("  Warning: No observations generated").yellow());
                continue;
            }

            // Save to parquet
            let output_path = output.join(format!("atm_iv_{}.parquet", symbol));
            MinuteAlignedIvUseCase::<FinqEquityRepository, FinqOptionsRepository>::save_to_parquet(
                &result,
                &output_path,
            )?;

            println!(
                "  {}",
                style(format!("Saved {} observations to {:?}", result.observations.len(), output_path))
                    .green()
            );

            // Generate plots if requested
            if plot {
                println!("  {}", style("Plot generation not yet implemented").yellow());
                // TODO: Add plotting implementation
            }
        }
    } else {
        // Use EOD IV computation (--eod-pricing flag specified)
        let use_case = GenerateIvTimeSeriesUseCase::new(equity_repo, options_repo);

        for symbol in &symbols {
            println!("{}", style(format!("Processing {}...", symbol)).bold());

            let result = use_case.execute(symbol, start_date, end_date, &config).await?;

            println!(
                "  {} trading days processed, {} successful observations",
                result.total_days, result.successful_days
            );

            if result.observations.is_empty() {
                println!("{}", style("  Warning: No observations generated").yellow());
                continue;
            }

            // Save to parquet
            let output_path = output.join(format!("atm_iv_{}.parquet", symbol));
            GenerateIvTimeSeriesUseCase::<FinqEquityRepository, FinqOptionsRepository>::save_to_parquet(
                &result,
                &output_path,
            )?;

            println!(
                "  {}",
                style(format!("Saved {} observations to {:?}", result.observations.len(), output_path))
                    .green()
            );

            // Generate plots if requested
            if plot {
                println!("  {}", style("Plot generation not yet implemented").yellow());
                // TODO: Add plotting implementation
            }
        }
    }

    println!();
    println!("{}", style("Done!").bold().green());

    Ok(())
}

/// Run earnings analysis command
async fn run_earnings_analysis_command(
    data_dir: Option<&PathBuf>,
    earnings_dir: Option<&PathBuf>,
    symbols: Vec<String>,
    start_str: &str,
    end_str: &str,
    format: &str,
    output: Option<PathBuf>,
) -> Result<()> {
    let data_dir = data_dir
        .map(|p| p.clone())
        .or_else(|| std::env::var("FINQ_DATA_DIR").ok().map(PathBuf::from))
        .context("Data directory not specified. Use --data-dir or set FINQ_DATA_DIR")?;

    let earnings_dir = earnings_dir
        .map(|p| p.clone())
        .or_else(|| std::env::var("EARNINGS_DATA_DIR").ok().map(PathBuf::from))
        .context("Earnings directory not specified. Use --earnings-dir or set EARNINGS_DATA_DIR")?;

    let start_date = NaiveDate::parse_from_str(start_str, "%Y-%m-%d")
        .context(format!("Invalid start date: {}", start_str))?;
    let end_date = NaiveDate::parse_from_str(end_str, "%Y-%m-%d")
        .context(format!("Invalid end date: {}", end_str))?;

    println!("{}", style("Earnings Analysis").bold().cyan());
    println!("{}", style("=".repeat(60)).cyan());
    println!();
    println!("  Symbols:     {}", symbols.join(", "));
    println!("  Date Range:  {} to {}", start_date, end_date);
    println!("  Data Dir:    {:?}", data_dir);
    println!("  Earnings:    {:?}", earnings_dir);
    println!();

    // Create repositories
    let equity_repo = Arc::new(FinqEquityRepository::new(data_dir.clone()));
    let options_repo = Arc::new(FinqOptionsRepository::new(data_dir));
    let earnings_repo = Arc::new(EarningsReaderAdapter::new(earnings_dir));

    // Create use case
    let use_case = EarningsAnalysisUseCase::with_default_timing(
        equity_repo,
        options_repo,
        earnings_repo,
    );

    // Default config
    let config = AtmIvConfig::default();

    // Accumulate all outcomes across symbols
    let mut all_outcomes = Vec::new();

    // Process each symbol
    for symbol in symbols {
        println!("{}", style(format!("Analyzing {}...", symbol)).bold());
        println!();

        match use_case.analyze_earnings(&symbol, start_date, end_date, &config).await {
            Ok(result) => {
                println!();
                println!("{}", style("Summary Statistics:").bold());
                println!("  Total Events: {}", result.summary.total_events);
                println!("  Gamma Wins:   {} ({:.1}%)",
                         result.summary.gamma_dominated_count,
                         100.0 * result.summary.gamma_dominated_count as f64 / result.summary.total_events as f64);
                println!("  Vega Wins:    {} ({:.1}%)",
                         result.summary.vega_dominated_count,
                         100.0 * result.summary.vega_dominated_count as f64 / result.summary.total_events as f64);
                println!("  Avg Expected: {:.2}%", result.summary.avg_expected_move_pct);
                println!("  Avg Actual:   {:.2}%", result.summary.avg_actual_move_pct);
                println!("  Avg Ratio:    {:.2}x", result.summary.avg_move_ratio);

                if result.summary.avg_iv_crush_pct > 0.0 {
                    println!("  Avg IV Crush: {:.1}%", result.summary.avg_iv_crush_pct * 100.0);
                }

                // Collect outcomes from this symbol
                all_outcomes.extend(result.outcomes);
            }
            Err(e) => {
                println!("  {}", style(format!("Error: {}", e)).red());
            }
        }

        println!();
    }

    // Save combined results
    if !all_outcomes.is_empty() {
        let output_path = output.unwrap_or_else(|| {
            PathBuf::from(format!("./earnings_analysis.{}", format))
        });

        // Create combined result
        use cs_backtest::EarningsAnalysisResult;
        use cs_domain::value_objects::EarningsSummaryStats;

        let summary = EarningsSummaryStats::from_outcomes(&all_outcomes);
        let combined_result = EarningsAnalysisResult {
            symbol: "MULTI".to_string(),
            outcomes: all_outcomes,
            summary,
        };

        match format {
            "parquet" => save_earnings_parquet(&combined_result, &output_path)?,
            "csv" => save_earnings_csv(&combined_result, &output_path)?,
            "json" => save_earnings_json(&combined_result, &output_path)?,
            _ => return Err(anyhow::anyhow!("Unsupported format: {}", format)),
        }

        println!();
        println!("{}", style(format!("Combined results saved to {:?}", output_path)).green().bold());
        println!();
        println!("{}", style("Overall Summary:").bold());
        println!("  Total Events: {}", combined_result.summary.total_events);
        println!("  Gamma Wins:   {} ({:.1}%)",
                 combined_result.summary.gamma_dominated_count,
                 100.0 * combined_result.summary.gamma_dominated_count as f64 / combined_result.summary.total_events as f64);
        println!("  Vega Wins:    {} ({:.1}%)",
                 combined_result.summary.vega_dominated_count,
                 100.0 * combined_result.summary.vega_dominated_count as f64 / combined_result.summary.total_events as f64);
        println!("  Avg Expected: {:.2}%", combined_result.summary.avg_expected_move_pct);
        println!("  Avg Actual:   {:.2}%", combined_result.summary.avg_actual_move_pct);
        println!("  Avg Ratio:    {:.2}x", combined_result.summary.avg_move_ratio);
    }

    println!("{}", style("Done!").bold().green());

    Ok(())
}

/// Save earnings analysis to Parquet
fn save_earnings_parquet(
    result: &cs_backtest::EarningsAnalysisResult,
    path: &PathBuf,
) -> Result<()> {
    use polars::prelude::*;
    use cs_domain::datetime::TradingDate;

    let outcomes = &result.outcomes;

    // Build DataFrame
    let symbols: Vec<String> = outcomes.iter().map(|o| o.symbol.clone()).collect();
    let dates: Vec<i32> = outcomes
        .iter()
        .map(|o| TradingDate::from_naive_date(o.earnings_date).to_polars_date())
        .collect();
    let earnings_time: Vec<String> = outcomes
        .iter()
        .map(|o| match o.earnings_time {
            cs_domain::value_objects::EarningsTime::BeforeMarketOpen => "BMO".to_string(),
            cs_domain::value_objects::EarningsTime::AfterMarketClose => "AMC".to_string(),
            cs_domain::value_objects::EarningsTime::Unknown => "Unknown".to_string(),
        })
        .collect();
    let pre_spot: Vec<f64> = outcomes.iter().map(|o| o.pre_spot.to_string().parse::<f64>().unwrap_or(0.0)).collect();
    let pre_straddle: Vec<f64> = outcomes.iter().map(|o| o.pre_straddle.to_string().parse::<f64>().unwrap_or(0.0)).collect();
    let expected_move_pct: Vec<f64> = outcomes.iter().map(|o| o.expected_move_pct).collect();
    let post_spot: Vec<f64> = outcomes.iter().map(|o| o.post_spot.to_string().parse::<f64>().unwrap_or(0.0)).collect();
    let actual_move_pct: Vec<f64> = outcomes.iter().map(|o| o.actual_move_pct).collect();
    let move_ratio: Vec<f64> = outcomes.iter().map(|o| o.move_ratio).collect();
    let gamma_dominated: Vec<bool> = outcomes.iter().map(|o| o.gamma_dominated).collect();

    let df = DataFrame::new(vec![
        Series::new("symbol", symbols),
        Series::new("earnings_date", dates),
        Series::new("earnings_time", earnings_time),
        Series::new("pre_spot", pre_spot),
        Series::new("pre_straddle", pre_straddle),
        Series::new("expected_move_pct", expected_move_pct),
        Series::new("post_spot", post_spot),
        Series::new("actual_move_pct", actual_move_pct),
        Series::new("move_ratio", move_ratio),
        Series::new("gamma_dominated", gamma_dominated),
    ])?;

    let mut file = std::fs::File::create(path)?;
    ParquetWriter::new(&mut file).finish(&mut df.clone())?;

    Ok(())
}

/// Save earnings analysis to CSV
fn save_earnings_csv(
    result: &cs_backtest::EarningsAnalysisResult,
    path: &PathBuf,
) -> Result<()> {
    use std::io::Write;

    let mut file = std::fs::File::create(path)?;

    // Header
    writeln!(file, "symbol,earnings_date,earnings_time,pre_spot,pre_straddle,expected_move_pct,post_spot,actual_move_pct,move_ratio,gamma_dominated")?;

    // Data rows
    for outcome in &result.outcomes {
        writeln!(
            file,
            "{},{},{},{},{},{},{},{},{},{}",
            outcome.symbol,
            outcome.earnings_date,
            match outcome.earnings_time {
                cs_domain::value_objects::EarningsTime::BeforeMarketOpen => "BMO",
                cs_domain::value_objects::EarningsTime::AfterMarketClose => "AMC",
                cs_domain::value_objects::EarningsTime::Unknown => "Unknown",
            },
            outcome.pre_spot,
            outcome.pre_straddle,
            outcome.expected_move_pct,
            outcome.post_spot,
            outcome.actual_move_pct,
            outcome.move_ratio,
            outcome.gamma_dominated,
        )?;
    }

    Ok(())
}

/// Save earnings analysis to JSON
fn save_earnings_json(
    result: &cs_backtest::EarningsAnalysisResult,
    path: &PathBuf,
) -> Result<()> {
    let json = serde_json::to_string_pretty(result)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Run campaign-based backtest
async fn run_campaign_command(
    data_dir: Option<&PathBuf>,
    symbols: Vec<String>,
    strategy: &str,
    period_policy: &str,
    roll_policy: &str,
    start: &str,
    end: &str,
    earnings_file: Option<&PathBuf>,
    entry_time: &str,
    exit_time: &str,
    entry_days_before: Option<u16>,
    exit_days_after: Option<u16>,
    inter_entry_days_after: u16,
    inter_exit_days_before: u16,
    roll_day: &str,
    output: Option<PathBuf>,
) -> Result<()> {
    use cs_domain::{
        TradingCampaign, SessionSchedule, OptionStrategy, PeriodPolicy, RollPolicy,
        ExpirationPolicy, EarningsEvent, TradingPeriodSpec,
        infrastructure::{FinqOptionsRepository, FinqEquityRepository},
    };
    use cs_backtest::{SessionExecutor, DefaultTradeFactory, ExecutionConfig};
    use chrono::NaiveDate;
    use std::sync::Arc;

    println!("{}", style("Campaign-Based Backtest").bold().green());
    println!("  Symbols:  {}", symbols.join(", "));
    println!("  Strategy: {}", strategy);
    println!("  Period:   {} to {}", start, end);
    println!();

    // Parse dates
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .with_context(|| format!("Invalid start date: {}", start))?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .with_context(|| format!("Invalid end date: {}", end))?;

    // Parse strategy
    let strategy = match strategy.to_lowercase().as_str() {
        "calendar" | "calendar-spread" => OptionStrategy::CalendarSpread,
        "straddle" => OptionStrategy::Straddle,
        "calendar-straddle" => OptionStrategy::CalendarStraddle,
        "iron-butterfly" | "ironfly" => OptionStrategy::IronButterfly,
        _ => anyhow::bail!("Unknown strategy: {}. Use: calendar, straddle, calendar-straddle, iron-butterfly", strategy),
    };

    // Default times
    use chrono::NaiveTime;
    let entry_time = NaiveTime::from_hms_opt(9, 35, 0).unwrap();
    let exit_time = NaiveTime::from_hms_opt(15, 55, 0).unwrap();

    // Parse period policy
    let period_policy = match period_policy.to_lowercase().as_str() {
        "earnings-only" | "earnings" => {
            // Use entry_days_before and exit_days_after
            let entry_days = entry_days_before.unwrap_or(1);
            let exit_days = exit_days_after.unwrap_or(1);

            PeriodPolicy::EarningsOnly {
                timing: TradingPeriodSpec::CrossEarnings {
                    entry_days_before: entry_days,
                    exit_days_after: exit_days,
                    entry_time,
                    exit_time,
                },
            }
        }
        "inter-earnings" | "between" => {
            let roll_policy = parse_campaign_roll_policy(roll_policy, roll_day)?;
            PeriodPolicy::InterEarnings {
                entry_days_after_earnings: inter_entry_days_after,
                exit_days_before_earnings: inter_exit_days_before,
                roll_policy,
            }
        }
        "pre-earnings" => {
            let entry_days = entry_days_before.unwrap_or(14);
            PeriodPolicy::pre_earnings(entry_days)
        }
        "post-earnings" => {
            let holding_days = exit_days_after.unwrap_or(14);
            PeriodPolicy::EarningsOnly {
                timing: TradingPeriodSpec::PostEarnings {
                    entry_offset: 0,
                    holding_days,
                    entry_time,
                    exit_time,
                },
            }
        }
        "continuous" => {
            let roll_policy = parse_campaign_roll_policy(roll_policy, roll_day)?;
            PeriodPolicy::Continuous {
                earnings_timing: TradingPeriodSpec::CrossEarnings {
                    entry_days_before: entry_days_before.unwrap_or(1),
                    exit_days_after: exit_days_after.unwrap_or(1),
                    entry_time,
                    exit_time,
                },
                inter_period_roll: roll_policy,
            }
        }
        _ => anyhow::bail!("Unknown period policy: {}. Use: earnings-only, inter-earnings, pre-earnings, post-earnings, continuous", period_policy),
    };

    // Load earnings calendar
    let earnings_calendar = if let Some(path) = earnings_file {
        // Load from file
        load_earnings_from_file(path)?
    } else {
        // Load from default location based on symbols
        load_earnings_for_symbols(&symbols, data_dir)?
    };

    println!("Loaded {} earnings events", earnings_calendar.len());

    // Create campaigns for each symbol
    let campaigns: Vec<TradingCampaign> = symbols
        .iter()
        .map(|symbol| TradingCampaign {
            symbol: symbol.clone(),
            strategy,
            start_date,
            end_date,
            period_policy: period_policy.clone(),
            expiration_policy: ExpirationPolicy::FirstAfter {
                min_date: start_date,
            },
        })
        .collect();

    // Generate sessions
    let schedule = SessionSchedule::from_campaigns(&campaigns, &earnings_calendar);

    println!("Generated {} trading sessions", schedule.session_count());
    println!("  Symbols: {}", schedule.symbols().iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
    println!();

    // Create repositories
    let data_dir = data_dir
        .cloned()
        .or_else(|| std::env::var("FINQ_DATA_DIR").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("data"));

    let options_repo = FinqOptionsRepository::new(data_dir.clone());
    let equity_repo = FinqEquityRepository::new(data_dir);

    // Create trade factory
    let options_repo_arc: Arc<dyn cs_domain::OptionsDataRepository> = Arc::new(options_repo);
    let equity_repo_arc: Arc<dyn cs_domain::EquityDataRepository> = Arc::new(equity_repo);

    let trade_factory = Arc::new(DefaultTradeFactory::new(
        Arc::clone(&options_repo_arc),
        Arc::clone(&equity_repo_arc),
    ));

    // Create session executor
    // Use calendar spread config as a reasonable default for all strategies
    let config = ExecutionConfig::for_calendar_spread(None);
    let executor = SessionExecutor::new(
        options_repo_arc,
        equity_repo_arc,
        trade_factory,
        config,
    );

    // Collect all sessions from schedule
    let all_sessions: Vec<_> = schedule.iter_entry_dates()
        .flat_map(|(_, sessions)| sessions.iter().cloned())
        .collect();

    // Execute sessions
    println!("{}", style("Executing sessions...").bold());
    let results = executor.execute_batch(&all_sessions).await;

    println!();
    println!("{}", style("Results:").bold().green());
    println!("  Total sessions:    {}", results.total_sessions);
    println!("  Successful:        {}", results.successful);
    println!("  Failed:            {}", results.failed);
    println!("  Success rate:      {:.1}%",
        100.0 * results.successful as f64 / results.total_sessions as f64);

    // Save results if requested
    if let Some(output_path) = output {
        println!();
        println!("Saving results to: {}", output_path.display());
        save_campaign_results(&results, &output_path)?;
    }

    println!();
    println!("{}", style("Done!").bold().green());

    Ok(())
}

/// Parse roll policy from string for campaign commands
fn parse_campaign_roll_policy(roll_policy: &str, roll_day: &str) -> Result<cs_domain::RollPolicy> {
    use cs_domain::RollPolicy;
    use chrono::Weekday;

    match roll_policy.to_lowercase().as_str() {
        "weekly" => {
            // Parse roll_day as weekday
            let weekday = match roll_day.to_lowercase().as_str() {
                "monday" => Weekday::Mon,
                "tuesday" => Weekday::Tue,
                "wednesday" => Weekday::Wed,
                "thursday" => Weekday::Thu,
                "friday" => Weekday::Fri,
                _ => anyhow::bail!("Invalid --roll-day: {}. Must be monday-friday", roll_day),
            };
            Ok(RollPolicy::Weekly { roll_day: weekday })
        }
        "monthly" => {
            // Parse roll_day as offset
            let offset = roll_day.parse::<i8>()
                .with_context(|| format!("Invalid roll day offset: {}. Use integer weeks offset (e.g., 0 for 3rd Friday)", roll_day))?;

            Ok(RollPolicy::Monthly { roll_week_offset: offset })
        }
        s if s.starts_with("days:") => {
            // Parse days:N format
            let interval_str = &s[5..];
            let interval: u16 = interval_str.parse()
                .with_context(|| format!("Invalid interval in '{}'. Expected days:N (e.g., days:5)", roll_policy))?;
            Ok(RollPolicy::TradingDays { interval })
        }
        _ => anyhow::bail!("Unknown roll policy: {}. Use: weekly, monthly, or days:N", roll_policy),
    }
}

/// Load earnings events from file
fn load_earnings_from_file(path: &PathBuf) -> Result<Vec<cs_domain::EarningsEvent>> {
    use cs_domain::{EarningsEvent, value_objects::EarningsTime};
    use chrono::NaiveDate;

    let content = std::fs::read_to_string(path)?;
    let mut earnings = Vec::new();

    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 3 {
            continue;
        }

        let symbol = parts[0].to_string();
        let date = NaiveDate::parse_from_str(parts[1], "%Y-%m-%d")?;
        let time = match parts[2].to_lowercase().as_str() {
            "bmo" | "before" => EarningsTime::BeforeMarketOpen,
            "amc" | "after" => EarningsTime::AfterMarketClose,
            _ => EarningsTime::Unknown,
        };

        earnings.push(EarningsEvent::new(symbol, date, time));
    }

    Ok(earnings)
}

/// Load earnings for specific symbols from data directory
fn load_earnings_for_symbols(symbols: &[String], data_dir: Option<&PathBuf>) -> Result<Vec<cs_domain::EarningsEvent>> {
    use cs_domain::{EarningsEvent, value_objects::EarningsTime};
    use chrono::NaiveDate;

    let data_dir = data_dir
        .cloned()
        .or_else(|| std::env::var("FINQ_DATA_DIR").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("data"));

    let mut all_earnings = Vec::new();

    for symbol in symbols {
        let earnings_path = data_dir.join(format!("earnings/{}.csv", symbol));

        if !earnings_path.exists() {
            eprintln!("Warning: No earnings file found for {}", symbol);
            continue;
        }

        let content = std::fs::read_to_string(&earnings_path)?;

        for line in content.lines().skip(1) {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() < 2 {
                continue;
            }

            let date = NaiveDate::parse_from_str(parts[0], "%Y-%m-%d")?;
            let time = if parts.len() >= 3 {
                match parts[2].to_lowercase().as_str() {
                    "bmo" | "before" => EarningsTime::BeforeMarketOpen,
                    "amc" | "after" => EarningsTime::AfterMarketClose,
                    _ => EarningsTime::Unknown,
                }
            } else {
                EarningsTime::Unknown
            };

            all_earnings.push(EarningsEvent::new(symbol.clone(), date, time));
        }
    }

    Ok(all_earnings)
}

/// Save campaign results to file
fn save_campaign_results(results: &cs_backtest::BatchResult, path: &PathBuf) -> Result<()> {
    use std::io::Write;

    let mut file = std::fs::File::create(path)?;

    writeln!(file, "symbol,strategy,entry_date,exit_date,action,success,error")?;

    for result in &results.results {
        writeln!(
            file,
            "{},{:?},{},{},{:?},{},{}",
            result.session.symbol,
            result.session.strategy,
            result.session.entry_date(),
            result.session.exit_date(),
            result.session.action,
            result.success,
            result.error.as_deref().unwrap_or(""),
        )?;
    }

    Ok(())
}
