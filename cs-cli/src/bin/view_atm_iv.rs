// Simple utility to view ATM IV parquet files

use polars::prelude::*;
use std::env;
use std::process;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <parquet_file>", args[0]);
        process::exit(1);
    }

    let file_path = &args[1];

    // Read parquet file
    let df = LazyFrame::scan_parquet(file_path, Default::default())?
        .collect()?;

    println!("{}", "=".repeat(100));
    println!("ATM IV Time Series: {}", file_path);
    println!("{}", "=".repeat(100));
    println!();

    println!("Shape: {} rows × {} columns", df.height(), df.width());
    println!();

    // Convert date column from days since epoch to readable format
    let df_display = df.clone().lazy()
        .with_column(
            col("date")
                .cast(DataType::Date)
                .alias("date")
        )
        .collect()?;

    // Display data
    println!("{}", df_display);
    println!();

    // Calculate statistics
    println!("{}", "=".repeat(100));
    println!("Summary Statistics:");
    println!("{}", "=".repeat(100));

    let stats_cols = vec!["atm_iv_30d", "atm_iv_60d", "atm_iv_90d", "term_spread_30_60", "term_spread_30_90"];

    for col_name in stats_cols {
        if let Ok(col) = df.column(col_name) {
            if let Ok(stats) = col.f64() {
                let valid: Vec<f64> = stats.into_iter().filter_map(|x| x).collect();
                if !valid.is_empty() {
                    let min = valid.iter().cloned().fold(f64::INFINITY, f64::min);
                    let max = valid.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    let mean = valid.iter().sum::<f64>() / valid.len() as f64;
                    println!("{:20} count={:3}, min={:.4}, max={:.4}, mean={:.4}",
                        col_name, valid.len(), min, max, mean);
                }
            }
        }
    }
    println!();

    // Detect earnings signals
    println!("{}", "=".repeat(100));
    println!("Potential Earnings Signals:");
    println!("{}", "=".repeat(100));

    if let (Ok(dates), Ok(iv_30d), Ok(spreads)) = (
        df.column("date"),
        df.column("atm_iv_30d"),
        df.column("term_spread_30_60"),
    ) {
        let dates_i32 = dates.i32()?;
        let iv_30d = iv_30d.f64()?;
        let spreads = spreads.f64()?;

        let mut found_signals = false;

        for i in 1..df.height() {
            let date_days = dates_i32.get(i);
            let iv_curr = iv_30d.get(i);
            let iv_prev = iv_30d.get(i - 1);
            let spread = spreads.get(i);

            let mut signals = Vec::new();

            // IV change detection
            if let (Some(curr), Some(prev)) = (iv_curr, iv_prev) {
                if prev > 0.0 {
                    let change_pct = (curr - prev) / prev;
                    if change_pct < -0.15 {
                        signals.push(format!("IV CRUSH: {:.1}% drop", change_pct * 100.0));
                    } else if change_pct > 0.20 {
                        signals.push(format!("IV SPIKE: {:.1}% increase", change_pct * 100.0));
                    }
                }
            }

            // Backwardation detection
            if let Some(s) = spread {
                if s > 0.05 {
                    signals.push(format!("BACKWARDATION: {:.1}%", s * 100.0));
                }
            }

            if !signals.is_empty() {
                found_signals = true;
                // Convert days since epoch to date
                if let Some(days) = date_days {
                    use chrono::{NaiveDate, Duration};
                    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
                    let date = epoch + Duration::days(days as i64);
                    println!("{}: {}", date, signals.join(", "));
                }
            }
        }

        if !found_signals {
            println!("No significant signals detected in this period");
        }
    }

    println!();

    Ok(())
}
