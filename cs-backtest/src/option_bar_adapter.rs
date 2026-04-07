//! Adapter: convert `&[OptionBar]` to a polars `DataFrame` for use inside pricers.
//!
//! This adapter lives at the boundary between the repository layer (which returns
//! domain `OptionBar` slices) and the pricing internals (which filter with polars).
//! Only call this at the top of `price_with_surface` implementations; do not spread
//! DataFrame construction throughout the pricer logic.

use cs_domain::{OptionBar, TradingDate};
use finq_core::OptionType;
use polars::prelude::*;

/// Convert a slice of `OptionBar` to a polars `DataFrame`.
///
/// Produces columns: `strike` (f64), `expiration` (Date), `option_type` (str),
/// `close` (f64, nullable). This matches the schema expected by all pricer internals.
pub fn to_dataframe(chain: &[OptionBar]) -> DataFrame {
    let strikes: Vec<f64> = chain.iter().map(|b| b.strike).collect();
    let expirations: Vec<i32> = chain
        .iter()
        .map(|b| TradingDate::from_naive_date(b.expiration).to_polars_date())
        .collect();
    let option_types: Vec<&str> = chain
        .iter()
        .map(|b| match b.option_type {
            OptionType::Call => "call",
            OptionType::Put => "put",
        })
        .collect();
    let closes: Vec<Option<f64>> = chain.iter().map(|b| b.close).collect();

    let expiration_series = Series::new("expiration".into(), expirations)
        .cast(&DataType::Date)
        .unwrap_or_else(|_| Series::new("expiration".into(), Vec::<i32>::new()));

    DataFrame::new(vec![
        Series::new("strike".into(), strikes),
        expiration_series,
        Series::new("option_type".into(), option_types),
        Series::new("close".into(), closes),
    ])
    .unwrap_or_else(|_| DataFrame::default())
}
