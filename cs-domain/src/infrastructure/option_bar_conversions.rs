/// Private helpers for converting polars DataFrames to domain OptionBar / EquityBar types.
///
/// These functions live in the infrastructure layer and are intentionally not public.
/// They handle the impedance mismatch between storage formats (DataFrames, provider DTOs)
/// and the domain types exposed by repository traits.

use polars::prelude::*;

use crate::datetime::{TradingDate, TradingTimestamp};
use crate::repositories::RepositoryError;
use crate::value_objects::{CallPut, EquityBar, OptionBar};

/// Convert a polars DataFrame with option chain columns to `Vec<OptionBar>`.
///
/// Expected columns:
/// - `strike` (Float64)
/// - `expiration` (Date)
/// - `option_type` (String: "call" or "put")
/// - `close` (Float64, nullable)
/// - `timestamp` (Int64 nanos, or Datetime[ms,UTC] — both handled; optional column)
pub(crate) fn dataframe_to_option_bars(df: &DataFrame) -> Result<Vec<OptionBar>, RepositoryError> {
    let strikes = df
        .column("strike")
        .map_err(|e| RepositoryError::Parse(format!("Missing 'strike' column: {}", e)))?
        .f64()
        .map_err(|e| RepositoryError::Parse(format!("Invalid 'strike' type: {}", e)))?
        .clone();

    let expirations = df
        .column("expiration")
        .map_err(|e| RepositoryError::Parse(format!("Missing 'expiration' column: {}", e)))?
        .date()
        .map_err(|e| RepositoryError::Parse(format!("Invalid 'expiration' type: {}", e)))?
        .clone();

    let option_types = df
        .column("option_type")
        .map_err(|e| RepositoryError::Parse(format!("Missing 'option_type' column: {}", e)))?
        .str()
        .map_err(|e| RepositoryError::Parse(format!("Invalid 'option_type' type: {}", e)))?
        .clone();

    let closes = df
        .column("close")
        .map_err(|e| RepositoryError::Parse(format!("Missing 'close' column: {}", e)))?
        .f64()
        .map_err(|e| RepositoryError::Parse(format!("Invalid 'close' type: {}", e)))?
        .clone();

    // Timestamp column is optional. Cast to Int64 to unify both i64-nanos and Datetime[ms] formats.
    let timestamps_raw: Option<ChunkedArray<Int64Type>> = df
        .column("timestamp")
        .ok()
        .and_then(|c| c.cast(&DataType::Int64).ok())
        .and_then(|c| c.i64().ok().map(|ca| ca.clone()));

    // Detect unit: nanos are > 1e15; millis are ~1e12 for current epoch
    let ts_is_nanos = timestamps_raw
        .as_ref()
        .and_then(|ts| ts.iter().find_map(|v| v))
        .map(|v| v > 1_000_000_000_000_000i64)
        .unwrap_or(false);

    let to_datetime = |ts_raw: i64| -> chrono::DateTime<chrono::Utc> {
        let nanos = if ts_is_nanos { ts_raw } else { ts_raw * 1_000_000 };
        TradingTimestamp::from_nanos(nanos).to_datetime_utc()
    };

    let mut bars = Vec::with_capacity(df.height());

    for i in 0..df.height() {
        let strike = match strikes.get(i) {
            Some(s) if s > 0.0 => s,
            _ => continue,
        };

        let expiration = match expirations.get(i) {
            Some(days) => TradingDate::from_polars_date(days).to_naive_date(),
            None => continue,
        };

        let option_type = match option_types.get(i) {
            Some("call") => CallPut::Call,
            Some("put") => CallPut::Put,
            _ => continue,
        };

        let close = closes.get(i).filter(|&c| c > 0.0);

        let timestamp = timestamps_raw
            .as_ref()
            .and_then(|ts| ts.get(i))
            .map(to_datetime);

        bars.push(OptionBar {
            strike,
            expiration,
            option_type,
            close,
            timestamp,
        });
    }

    Ok(bars)
}

/// Convert a polars DataFrame with equity bar columns to `Vec<EquityBar>`.
///
/// Expected columns:
/// - `close` (Float64)
/// - `timestamp` (Int64 nanos, or Datetime[ms,UTC])
pub(crate) fn dataframe_to_equity_bars(df: &DataFrame) -> Result<Vec<EquityBar>, RepositoryError> {
    let closes = df
        .column("close")
        .map_err(|e| RepositoryError::Parse(format!("Missing 'close' column: {}", e)))?
        .f64()
        .map_err(|e| RepositoryError::Parse(format!("Invalid 'close' type: {}", e)))?
        .clone();

    // Cast timestamp to Int64 (handles both i64 nanos and Datetime[ms,UTC])
    let timestamps_raw = df
        .column("timestamp")
        .map_err(|e| RepositoryError::Parse(format!("Missing 'timestamp' column: {}", e)))?
        .cast(&DataType::Int64)
        .map_err(|e| RepositoryError::Parse(format!("Failed to cast 'timestamp': {}", e)))?;
    let timestamps_i64 = timestamps_raw
        .i64()
        .map_err(|e| RepositoryError::Parse(format!("Invalid 'timestamp' type: {}", e)))?
        .clone();

    // Detect unit: nanos > 1e15, millis ~1e12
    let ts_is_nanos = timestamps_i64
        .iter()
        .find_map(|v| v)
        .map(|v| v > 1_000_000_000_000_000i64)
        .unwrap_or(false);

    let to_datetime = |ts_raw: i64| -> chrono::DateTime<chrono::Utc> {
        let nanos = if ts_is_nanos { ts_raw } else { ts_raw * 1_000_000 };
        TradingTimestamp::from_nanos(nanos).to_datetime_utc()
    };

    let mut bars = Vec::with_capacity(df.height());

    for i in 0..df.height() {
        let close = match closes.get(i) {
            Some(c) => c,
            None => continue,
        };
        let ts_raw = match timestamps_i64.get(i) {
            Some(t) => t,
            None => continue,
        };
        bars.push(EquityBar {
            close,
            timestamp: to_datetime(ts_raw),
        });
    }

    Ok(bars)
}
