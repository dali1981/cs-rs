//! Earnings analysis output handlers

use anyhow::Result;
use std::path::PathBuf;

/// Save earnings analysis to Parquet format
pub fn save_earnings_parquet(
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

/// Save earnings analysis to CSV format
pub fn save_earnings_csv(
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

/// Save earnings analysis to JSON format
pub fn save_earnings_json(
    result: &cs_backtest::EarningsAnalysisResult,
    path: &PathBuf,
) -> Result<()> {
    let json = serde_json::to_string_pretty(result)?;
    std::fs::write(path, json)?;
    Ok(())
}
