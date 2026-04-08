#!/usr/bin/env rust
//! Standalone test to inspect IB option chain data
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
    let chain = match options_repo.get_option_bars_at_time(symbol, timestamp).await {
        Ok(bars) => bars,
        Err(e) => {
            eprintln!("Failed to get option bars at time: {}", e);
            return Err(e.into());
        }
    };

    println!("✓ Option bars at time fetched successfully");
    println!();

    // Inspect results
    println!("Option Chain Info:");
    println!("  Bars: {}", chain.len());
    println!();

    // Check field presence
    println!("Checking required fields:");
    let has_strike = chain.iter().all(|b| b.strike > 0.0);
    let has_expiration = !chain.is_empty();
    let has_close = chain.iter().any(|b| b.close.is_some());
    let has_timestamp = chain.iter().any(|b| b.timestamp.is_some());

    println!("  ✓ strike: all valid = {}", has_strike);
    println!("  ✓ expiration: present = {}", has_expiration);
    println!("  ✓ close: any non-null = {}", has_close);
    println!("  ✓ timestamp: any non-null = {}", has_timestamp);
    println!();

    // Show first 3 bars
    println!("First 3 bars:");
    for (i, bar) in chain.iter().take(3).enumerate() {
        println!(
            "  [{}] strike={:.2} exp={} type={:?} close={:?} ts={:?}",
            i, bar.strike, bar.expiration, bar.option_type, bar.close, bar.timestamp
        );
    }

    Ok(())
}
