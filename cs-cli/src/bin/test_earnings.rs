use chrono::NaiveDate;
use std::path::PathBuf;
use cs_domain::infrastructure::EarningsReaderAdapter;
use cs_domain::EarningsRepository;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = dirs::home_dir()
        .unwrap()
        .join("trading_project/nasdaq_earnings/data");

    println!("Loading earnings from: {:?}", data_dir);

    let repo = EarningsReaderAdapter::new(data_dir);

    let start = NaiveDate::from_ymd_opt(2025, 11, 3).unwrap();
    let end = NaiveDate::from_ymd_opt(2025, 11, 4).unwrap();

    let events = repo.load_earnings(start, end, None).await?;

    println!("Found {} earnings events for {}-{}", events.len(), start, end);

    for (i, event) in events.iter().take(10).enumerate() {
        println!(
            "  {}. {} on {} ({:?}) - {}",
            i + 1,
            event.symbol,
            event.earnings_date,
            event.earnings_time,
            event.company_name.as_ref().unwrap_or(&"Unknown".to_string())
        );
    }

    Ok(())
}
