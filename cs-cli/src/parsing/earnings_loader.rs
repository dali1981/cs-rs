//! Earnings data loading utilities

#![allow(dead_code)]

use anyhow::Result;
use std::path::PathBuf;
use cs_domain::{EarningsEvent, value_objects::EarningsTime};
use chrono::NaiveDate;

/// Load earnings events from file (Parquet or JSON)
pub async fn load_earnings_from_file(path: &PathBuf) -> Result<Vec<EarningsEvent>> {
    use cs_domain::infrastructure::CustomFileEarningsReader;
    use cs_domain::EarningsRepository;

    let reader = CustomFileEarningsReader::from_file(path.clone())
        .map_err(|e| anyhow::anyhow!("Failed to open earnings file: {}", e))?;

    // Load all earnings (use wide date range)
    let start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2030, 12, 31).unwrap();

    let earnings = reader.load_earnings(start, end, None).await
        .map_err(|e| anyhow::anyhow!("Failed to load earnings: {}", e))?;

    Ok(earnings)
}

/// Load earnings for specific symbols from data directory
pub fn load_earnings_for_symbols(symbols: &[String], data_dir: Option<&PathBuf>) -> Result<Vec<EarningsEvent>> {
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
