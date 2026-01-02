// Plot ATM IV time series from parquet file

use chrono::{Duration, NaiveDate};
use plotters::prelude::*;
use polars::prelude::*;
use std::env;
use std::process;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <parquet_file> [output_png]", args[0]);
        process::exit(1);
    }

    let input_path = &args[1];
    let output_path = if args.len() > 2 {
        args[2].clone()
    } else {
        input_path.replace(".parquet", "_plot.png")
    };

    // Read parquet file
    let df = LazyFrame::scan_parquet(input_path, Default::default())?.collect()?;

    // Extract data
    let dates_col = df.column("date")?.i32()?;
    let symbols_col = df.column("symbol")?.str()?;
    let iv_30d = df.column("atm_iv_30d")?.f64()?;
    let iv_60d = df.column("atm_iv_60d")?.f64()?;
    let iv_90d = df.column("atm_iv_90d")?.f64()?;

    // Convert dates from days since epoch to NaiveDate
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
    let dates: Vec<NaiveDate> = dates_col
        .into_iter()
        .map(|d| d.map(|days| epoch + Duration::days(days as i64)).unwrap())
        .collect();

    // Get symbol name
    let symbol = symbols_col.get(0).unwrap_or("UNKNOWN");

    // Prepare data series
    let mut data_30d: Vec<(NaiveDate, f64)> = Vec::new();
    let mut data_60d: Vec<(NaiveDate, f64)> = Vec::new();
    let mut data_90d: Vec<(NaiveDate, f64)> = Vec::new();

    for i in 0..dates.len() {
        if let Some(iv) = iv_30d.get(i) {
            data_30d.push((dates[i], iv));
        }
        if let Some(iv) = iv_60d.get(i) {
            data_60d.push((dates[i], iv));
        }
        if let Some(iv) = iv_90d.get(i) {
            data_90d.push((dates[i], iv));
        }
    }

    // Detect signals
    let mut crush_points = Vec::new();
    let mut spike_points = Vec::new();

    for i in 1..dates.len() {
        if let (Some(curr), Some(prev)) = (iv_30d.get(i), iv_30d.get(i - 1)) {
            if prev > 0.0 {
                let change_pct = (curr - prev) / prev;
                if change_pct < -0.15 {
                    // IV crush
                    crush_points.push((dates[i], curr));
                } else if change_pct > 0.20 {
                    // IV spike
                    spike_points.push((dates[i], curr));
                }
            }
        }
    }

    // Create plot
    let root = BitMapBackend::new(&output_path, (1200, 600)).into_drawing_area();
    root.fill(&WHITE)?;

    // Find min/max dates and IV values
    let min_date = dates.iter().min().unwrap();
    let max_date = dates.iter().max().unwrap();

    let all_ivs: Vec<f64> = data_30d
        .iter()
        .chain(data_60d.iter())
        .chain(data_90d.iter())
        .map(|(_, iv)| *iv)
        .collect();

    let min_iv = all_ivs
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min)
        * 0.95;
    let max_iv = all_ivs
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max)
        * 1.05;

    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!("{} - ATM Implied Volatility", symbol),
            ("sans-serif", 40).into_font(),
        )
        .margin(15)
        .x_label_area_size(50)
        .y_label_area_size(60)
        .build_cartesian_2d(*min_date..*max_date, min_iv..max_iv)?;

    chart
        .configure_mesh()
        .x_desc("Date")
        .y_desc("ATM IV")
        .x_label_formatter(&|d| d.format("%Y-%m-%d").to_string())
        .y_label_formatter(&|y| format!("{:.1}%", y * 100.0))
        .draw()?;

    // Plot 30d IV
    if !data_30d.is_empty() {
        chart
            .draw_series(LineSeries::new(data_30d.clone(), BLUE.stroke_width(2)))?
            .label("30d IV")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], BLUE.stroke_width(2)));
    }

    // Plot 60d IV
    if !data_60d.is_empty() {
        chart
            .draw_series(LineSeries::new(data_60d.clone(), GREEN.stroke_width(2)))?
            .label("60d IV")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], GREEN.stroke_width(2)));
    }

    // Plot 90d IV
    if !data_90d.is_empty() {
        chart
            .draw_series(LineSeries::new(data_90d.clone(), MAGENTA.mix(0.7).stroke_width(2)))?
            .label("90d IV")
            .legend(|(x, y)| {
                PathElement::new(vec![(x, y), (x + 20, y)], MAGENTA.mix(0.7).stroke_width(2))
            });
    }

    // Mark IV crushes
    if !crush_points.is_empty() {
        chart
            .draw_series(crush_points.iter().map(|(date, iv)| {
                Circle::new((*date, *iv), 6, RED.filled())
            }))?
            .label("IV Crush")
            .legend(|(x, y)| Circle::new((x + 10, y), 5, RED.filled()));
    }

    // Mark IV spikes
    if !spike_points.is_empty() {
        chart
            .draw_series(spike_points.iter().map(|(date, iv)| {
                Circle::new((*date, *iv), 6, YELLOW.mix(0.7).filled())
            }))?
            .label("IV Spike")
            .legend(|(x, y)| Circle::new((x + 10, y), 5, YELLOW.mix(0.7).filled()));
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    root.present()?;

    println!("Plot saved to: {}", output_path);

    Ok(())
}
