use chrono::NaiveDate;
use std::path::PathBuf;
use cs_domain::infrastructure::{FinqOptionsRepository};
use cs_domain::OptionsDataRepository;
use polars::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = dirs::home_dir()
        .unwrap()
        .join("polygon/data");

    let options_repo = FinqOptionsRepository::new(data_dir);

    let symbol = "IDXX";
    let date = NaiveDate::from_ymd_opt(2025, 11, 3).unwrap();

    println!("Loading options for {} on {}\n", symbol, date);

    let df = options_repo.get_option_bars(symbol, date).await?;

    println!("Loaded {} rows", df.height());
    println!("Columns: {:?}\n", df.get_column_names());

    // Check expiration column
    if let Ok(exp_col) = df.column("expiration") {
        println!("Expiration column dtype: {:?}", exp_col.dtype());

        // Get unique values
        if let Ok(unique) = exp_col.unique() {
            println!("Unique expiration values (first 10):");
            for i in 0..10.min(unique.len()) {
                println!("  {}: {:?}", i, unique.get(i)?);
            }
        }

        // Try to get as Date
        if let Ok(dates) = exp_col.date() {
            println!("\nAs Date column:");
            let unique_dates = dates.unique()?;
            println!("Unique dates: {:?}", unique_dates);

            // Convert to NaiveDates
            let naive_dates: Vec<Option<NaiveDate>> = unique_dates
                .into_iter()
                .map(|opt| {
                    opt.and_then(|days| {
                        let nd = NaiveDate::from_num_days_from_ce_opt(days);
                        println!("  days={} -> {:?}", days, nd);
                        nd
                    })
                })
                .collect();

            println!("\nConverted NaiveDates: {:?}", naive_dates);

            // Filter future dates
            let future: Vec<NaiveDate> = naive_dates
                .into_iter()
                .flatten()
                .filter(|&d| d > date)
                .collect();
            println!("Future dates (> {}): {:?}", date, future);
        }
    }

    Ok(())
}
