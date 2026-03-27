#!/usr/bin/env rust
//! Standalone test to inspect IB option chain DataFrame schema
//!
//! Run with:
//! ```bash
//! cargo run --bin test_ib_chain_schema
//! ```

use chrono::{DateTime, Utc};
use cs_domain::OptionsDataRepository;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    // Test parameters (from your actual failing case)
    let symbol = "VWAV";
    let timestamp_str = "2026-01-09T14:35:00Z";
    let timestamp: DateTime<Utc> = timestamp_str.parse()?;
    let ib_data_dir = dirs::home_dir()
        .unwrap()
        .join("trading_project/ib-data-collector/ib_data");

    println!("Testing IB options repository for:");
    println!("  Symbol: {}", symbol);
    println!("  Timestamp: {}", timestamp);
    println!("  Data dir: {}", ib_data_dir.display());
    println!();

    // Create IB options repository
    let options_repo = match cs_domain::infrastructure::IbOptionsRepository::new(&ib_data_dir) {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("Failed to create IB options repository: {}", e);
            return Err(e.into());
        }
    };

    println!("✓ IB options repository created successfully");
    println!();

    // Get option bars at specific time (this is what the backtest uses)
    println!("Fetching option bars at time...");
    let chain_df = match options_repo.get_option_bars_at_time(symbol, timestamp).await {
        Ok(df) => df,
        Err(e) => {
            eprintln!("Failed to get option bars at time: {}", e);
            return Err(e.into());
        }
    };

    println!("✓ Option bars at time fetched successfully");
    println!();

    // Inspect DataFrame
    println!("DataFrame Info:");
    println!("  Rows: {}", chain_df.height());
    println!("  Columns: {}", chain_df.width());
    println!();

    // List all columns with types
    println!("Schema (column name → type):");
    for field in chain_df.schema().iter_fields() {
        println!("  {} → {:?}", field.name(), field.data_type());
    }
    println!();

    // Check specific columns we need
    println!("Checking required columns:");

    let required = vec!["strike", "expiration", "close", "option_type", "timestamp"];
    for col_name in &required {
        match chain_df.column(col_name) {
            Ok(col) => {
                println!("  ✓ '{}' exists - dtype: {:?}", col_name, col.dtype());

                // Try to extract as expected type
                match *col_name {
                    "strike" | "close" => {
                        match col.f64() {
                            Ok(_) => println!("    → Can cast to f64"),
                            Err(e) => println!("    ✗ Cannot cast to f64: {}", e),
                        }
                    }
                    "expiration" => {
                        match col.date() {
                            Ok(_) => println!("    → Can cast to Date"),
                            Err(e) => {
                                println!("    ✗ Cannot cast to Date: {}", e);

                                // Try alternative extractions
                                if let Ok(s) = col.str() {
                                    println!("    → Can extract as String");
                                    if chain_df.height() > 0 {
                                        println!("    → First value: {:?}", s.get(0));
                                    }
                                } else if let Ok(i) = col.i64() {
                                    println!("    → Can extract as i64");
                                    if chain_df.height() > 0 {
                                        println!("    → First value: {:?}", i.get(0));
                                    }
                                } else if let Ok(i) = col.i32() {
                                    println!("    → Can extract as i32");
                                    if chain_df.height() > 0 {
                                        println!("    → First value: {:?}", i.get(0));
                                    }
                                }
                            }
                        }
                    }
                    "option_type" => {
                        match col.str() {
                            Ok(_) => println!("    → Can cast to Str"),
                            Err(e) => println!("    ✗ Cannot cast to Str: {}", e),
                        }
                    }
                    "timestamp" => {
                        // Try datetime extraction
                        match col.datetime() {
                            Ok(_) => println!("    → Can extract as Datetime"),
                            Err(_) => {
                                // Try casting to i64
                                match col.cast(&polars::prelude::DataType::Int64) {
                                    Ok(casted) => {
                                        if let Ok(_i64_col) = casted.i64() {
                                            println!("    → Can cast to Int64 (milliseconds)");
                                        }
                                    }
                                    Err(e) => println!("    ✗ Cannot cast to Int64: {}", e),
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(_) => {
                println!("  ✗ '{}' does NOT exist", col_name);
            }
        }
    }
    println!();

    // Show first few rows
    println!("First 3 rows:");
    if chain_df.height() > 0 {
        println!("{}", chain_df.head(Some(3)));
    } else {
        println!("  (DataFrame is empty)");
    }

    Ok(())
}
