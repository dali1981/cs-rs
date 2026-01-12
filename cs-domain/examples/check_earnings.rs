use cs_domain::infrastructure::FinqEarningsRepository;
use cs_domain::repositories::EarningsRepository;
use chrono::NaiveDate;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let earnings_dir = PathBuf::from("/Users/mohamedali/trading_project/nasdaq_earnings/data");
    let repo = FinqEarningsRepository::new(&earnings_dir);

    let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();

    println!("Searching for VWAV earnings between {} and {}\n", start, end);

    let events = repo.get_earnings_events(start, end).await?;

    let vwav_events: Vec<_> = events.iter()
        .filter(|e| e.symbol == "VWAV")
        .collect();

    println!("Found {} VWAV earnings events in range", vwav_events.len());
    for event in vwav_events {
        println!("  {} at {:?} (market_cap: {:?})",
            event.earnings_date, event.earnings_time, event.market_cap);
    }

    println!("\nAll VWAV earnings events in database:");
    let all_events = repo.get_earnings_events(
        NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2030, 12, 31).unwrap()
    ).await?;

    let all_vwav: Vec<_> = all_events.iter()
        .filter(|e| e.symbol == "VWAV")
        .collect();

    println!("  Total VWAV events: {}", all_vwav.len());
    for event in all_vwav.iter().take(10) {
        println!("  {} at {:?} (market_cap: {:?})",
            event.earnings_date, event.earnings_time, event.market_cap);
    }

    Ok(())
}
