use cs_domain::infrastructure::EarningsReaderAdapter;
use cs_domain::repositories::EarningsRepository;
use chrono::NaiveDate;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let earnings_dir = PathBuf::from("/Users/mohamedali/trading_project/nasdaq_earnings/data");

    let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();

    println!("Searching for VWAV earnings between {} and {}\n", start, end);

    // Check TradingView source
    let tv_repo = EarningsReaderAdapter::with_source(
        earnings_dir.clone(),
        earnings_rs::DataSource::TradingView
    );
    let tv_events = tv_repo.load_earnings(start, end, Some(&["VWAV".to_string()])).await?;
    println!("TradingView: {} VWAV events", tv_events.len());
    for event in &tv_events {
        println!("  {} at {:?}", event.earnings_date, event.earnings_time);
    }

    // Check Nasdaq source
    let nasdaq_repo = EarningsReaderAdapter::with_source(
        earnings_dir.clone(),
        earnings_rs::DataSource::Nasdaq
    );
    let nasdaq_events = nasdaq_repo.load_earnings(start, end, Some(&["VWAV".to_string()])).await?;
    println!("\nNasdaq: {} VWAV events", nasdaq_events.len());
    for event in &nasdaq_events {
        println!("  {} at {:?}", event.earnings_date, event.earnings_time);
    }

    // Check Yahoo source
    let yahoo_repo = EarningsReaderAdapter::with_source(
        earnings_dir,
        earnings_rs::DataSource::Yahoo
    );
    let yahoo_events = yahoo_repo.load_earnings(start, end, Some(&["VWAV".to_string()])).await?;
    println!("\nYahoo: {} VWAV events", yahoo_events.len());
    for event in &yahoo_events {
        println!("  {} at {:?}", event.earnings_date, event.earnings_time);
    }

    Ok(())
}
