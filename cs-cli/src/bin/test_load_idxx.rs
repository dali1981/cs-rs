use chrono::{NaiveDate, TimeZone, Utc};
use std::path::PathBuf;
use cs_domain::infrastructure::{FinqEquityRepository, FinqOptionsRepository};
use cs_domain::{EquityDataRepository, OptionsDataRepository};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = dirs::home_dir()
        .unwrap()
        .join("polygon/data");

    println!("Using data dir: {:?}\n", data_dir);

    let equity_repo = FinqEquityRepository::new(data_dir.clone());
    let options_repo = FinqOptionsRepository::new(data_dir.clone());

    let symbol = "IDXX";
    let date = NaiveDate::from_ymd_opt(2025, 11, 3).unwrap();
    let entry_time = Utc.with_ymd_and_hms(2025, 11, 3, 9, 35, 0).unwrap();

    println!("Testing {} on {}", symbol, date);
    println!("Entry time: {}\n", entry_time);

    // Test equity data
    println!("=== Equity Data ===");
    match equity_repo.get_spot_price(symbol, entry_time).await {
        Ok(spot) => {
            println!("✓ Spot price: ${:.2} at {}", spot.to_f64(), spot.timestamp);
        }
        Err(e) => {
            println!("✗ Failed to get spot price: {}", e);
        }
    }

    match equity_repo.get_bars(symbol, date).await {
        Ok(df) => {
            println!("✓ Loaded {} equity bars", df.height());
        }
        Err(e) => {
            println!("✗ Failed to get bars: {}", e);
        }
    }

    // Test options data
    println!("\n=== Options Data ===");
    match options_repo.get_option_bars(symbol, date).await {
        Ok(df) => {
            println!("✓ Loaded {} option bars", df.height());
            if df.height() > 0 {
                println!("  Columns: {:?}", df.get_column_names());

                // Check expiration column
                if let Ok(exp_col) = df.column("expiration") {
                    println!("\n  Expiration column dtype: {:?}", exp_col.dtype());
                    println!("  First 5 expiration values:");
                    for i in 0..5.min(df.height()) {
                        println!("    {}: {:?}", i, exp_col.get(i));
                    }

                    // Try to parse as dates
                    if let Ok(date_col) = exp_col.date() {
                        println!("\n  Parsing expiration dates:");
                        for i in 0..5.min(df.height()) {
                            if let Some(days) = date_col.get(i) {
                                let exp_date = NaiveDate::from_num_days_from_ce_opt(days);
                                println!("    days={} -> date={:?}, is > {}? {}",
                                    days, exp_date, date,
                                    exp_date.map(|d| d > date).unwrap_or(false));
                            }
                        }
                    }
                } else {
                    println!("  ✗ No expiration column found!");
                }
            }
        }
        Err(e) => {
            println!("✗ Failed to get option bars: {}", e);
        }
    }

    match options_repo.get_available_expirations(symbol, date).await {
        Ok(exps) => {
            println!("\n✓ Found {} expirations", exps.len());
            for (i, exp) in exps.iter().take(5).enumerate() {
                println!("  {}. {}", i + 1, exp);
            }
        }
        Err(e) => {
            println!("\n✗ Failed to get expirations: {}", e);
        }
    }

    Ok(())
}
