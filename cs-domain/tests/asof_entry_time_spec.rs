//! Regression spec for the as-of option filter.
//!
//! The bug this pins (fixed in 88525fe): the flatfile `timestamp` column is
//! `Datetime(Milliseconds, UTC)` while `TradingTimestamp::to_nanos()` is nanoseconds
//! since epoch. The as-of filters compared the two directly, so the bound was ~10^6
//! times larger than any value in the column, `timestamp <= target` was true for every
//! row of the session, and the `.first()` after a descending sort returned the LAST bar
//! of the day for every contract.
//!
//! The effect was silent look-ahead: a backtest configured to enter at 09:35 was priced
//! on the 15:5x trade of the same day, and nothing failed. The observable symptom is the
//! one asserted here — **the entry price did not respond to the entry time**. Any future
//! change that reintroduces a unit mismatch, or drops the as-of filter altogether, makes
//! these prices equal again and trips `asof_price_responds_to_entry_time`.
//!
//! Hermetic: builds its own one-symbol flatfile in a temp dir, so it runs on a CI runner
//! with no market data mounted.

use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use cs_domain::infrastructure::FinqOptionsRepository;
use cs_domain::repositories::{OptionsDataRepository, RepositoryError};
use polars::prelude::*;

const TEST_SYMBOL: &str = "TSTX";
/// 2025-02-25 was a Tuesday; the date only has to be a real calendar date.
const SESSION: (i32, u32, u32) = (2025, 2, 25);

/// ET is UTC-5 on this date, so 09:35 ET = 14:35Z, 12:00 ET = 17:00Z, 15:55 ET = 20:55Z.
const BAR_0935: (u32, u32) = (14, 35);
const BAR_1200: (u32, u32) = (17, 0);
const BAR_1555: (u32, u32) = (20, 55);

const CLOSE_0935: f64 = 1.25;
const CLOSE_1200: f64 = 2.50;
const CLOSE_1555: f64 = 3.75;

fn utc_at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(SESSION.0, SESSION.1, SESSION.2, hour, minute, 0)
        .single()
        .expect("valid timestamp")
}

/// Write a minute-bar flatfile for one contract with three bars at known prices.
///
/// Layout must match `finq_flatfiles::utils::option_bar_path`:
/// `<data_dir>/flatfiles/options/minute_aggs/<YYYY>/<YYYY-MM-DD>/<SYMBOL>.parquet`,
/// and the `timestamp` column must be `Datetime(ms, UTC)` — the storage unit whose
/// mismatch with nanoseconds caused the bug.
fn write_fixture(data_dir: &Path) -> PathBuf {
    let session = NaiveDate::from_ymd_opt(SESSION.0, SESSION.1, SESSION.2).expect("valid date");
    let dir = data_dir
        .join("flatfiles")
        .join("options")
        .join("minute_aggs")
        .join(session.format("%Y").to_string())
        .join(session.format("%Y-%m-%d").to_string());
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join(format!("{TEST_SYMBOL}.parquet"));

    let millis: Vec<i64> = [BAR_0935, BAR_1200, BAR_1555]
        .iter()
        .map(|(h, m)| utc_at(*h, *m).timestamp_millis())
        .collect();

    // One contract, three minute bars. Expiration is a Date column, as on disk.
    let expiration_days = NaiveDate::from_ymd_opt(2025, 3, 21)
        .expect("valid expiry")
        .signed_duration_since(NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch"))
        .num_days() as i32;

    let timestamp = Series::new("timestamp", millis)
        .cast(&DataType::Datetime(TimeUnit::Milliseconds, None))
        .expect("cast to datetime[ms]")
        .cast(&DataType::Datetime(
            TimeUnit::Milliseconds,
            Some("UTC".to_string()),
        ))
        .expect("tag UTC");

    let expiration = Series::new("expiration", vec![expiration_days; 3])
        .cast(&DataType::Date)
        .expect("cast to date");

    let mut df = DataFrame::new(vec![
        Series::new("ticker", vec!["O:TSTX250321C00100000"; 3]),
        Series::new("underlying", vec![TEST_SYMBOL; 3]),
        Series::new("strike", vec![100.0f64; 3]),
        expiration,
        Series::new("option_type", vec!["call"; 3]),
        Series::new("close", vec![CLOSE_0935, CLOSE_1200, CLOSE_1555]),
        Series::new("volume", vec![10i64, 10, 10]),
        timestamp,
    ])
    .expect("build fixture frame");

    let file = std::fs::File::create(&path).expect("create parquet");
    ParquetWriter::new(file)
        .finish(&mut df)
        .expect("write parquet");

    path
}

async fn close_at(repo: &FinqOptionsRepository, at: DateTime<Utc>) -> (f64, DateTime<Utc>) {
    let bars = repo
        .get_option_bars_at_time(TEST_SYMBOL, at)
        .await
        .unwrap_or_else(|e| panic!("as-of query at {at} failed: {e}"));
    assert_eq!(bars.len(), 1, "fixture has exactly one contract");
    let bar = &bars[0];
    (
        bar.close.expect("fixture bars all have a close"),
        bar.timestamp.expect("as-of query returns the bar's timestamp"),
    )
}

/// The regression guard. Before the fix both entry times priced off the 15:55 bar, so
/// these two prices were equal; that equality IS the look-ahead.
#[tokio::test]
async fn asof_price_responds_to_entry_time() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_fixture(tmp.path());
    let repo = FinqOptionsRepository::new(tmp.path().to_path_buf());

    let (morning_close, morning_ts) = close_at(&repo, utc_at(BAR_0935.0, BAR_0935.1)).await;
    let (close_close, close_ts) = close_at(&repo, utc_at(BAR_1555.0, BAR_1555.1)).await;

    assert_ne!(
        morning_close, close_close,
        "entry price did not change with entry time — the as-of filter is a no-op again \
         and every entry is being priced from later in the session (silent look-ahead)"
    );
    assert_eq!(
        morning_close, CLOSE_0935,
        "09:35 entry must price on the 09:35 bar, not a later one"
    );
    assert_eq!(
        close_close, CLOSE_1555,
        "15:55 entry must price on the 15:55 bar"
    );
    assert_eq!(morning_ts, utc_at(BAR_0935.0, BAR_0935.1));
    assert_eq!(close_ts, utc_at(BAR_1555.0, BAR_1555.1));
}

/// The as-of rule is "last bar at or before the target", not "nearest bar" and not
/// "first bar of the day": a midday query lands on the midday bar even though a later
/// one exists in the same file.
#[tokio::test]
async fn asof_takes_the_last_bar_at_or_before_the_target() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_fixture(tmp.path());
    let repo = FinqOptionsRepository::new(tmp.path().to_path_buf());

    // Exactly on a bar.
    let (close, ts) = close_at(&repo, utc_at(BAR_1200.0, BAR_1200.1)).await;
    assert_eq!(close, CLOSE_1200);
    assert_eq!(ts, utc_at(BAR_1200.0, BAR_1200.1));

    // Between two bars: takes the earlier one, never the later.
    let (close, ts) = close_at(&repo, utc_at(BAR_1200.0, BAR_1200.1 + 30)).await;
    assert_eq!(
        close, CLOSE_1200,
        "a query between bars must not reach forward to {CLOSE_1555}"
    );
    assert_eq!(ts, utc_at(BAR_1200.0, BAR_1200.1));
}

/// An entry earlier than the contract's first trade has no price. Before the fix this
/// returned the last bar of the day instead — the worst case of the look-ahead, because
/// it invented a trade that could not have been entered at all.
#[tokio::test]
async fn asof_before_the_first_bar_is_not_found() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_fixture(tmp.path());
    let repo = FinqOptionsRepository::new(tmp.path().to_path_buf());

    let before_open = utc_at(13, 0); // 08:00 ET, before the first bar at 09:35
    let result = repo.get_option_bars_at_time(TEST_SYMBOL, before_open).await;
    assert!(
        result.is_err(),
        "a query before the first bar must fail, not silently price off the close"
    );
}

/// The forward-fill path, which the as-of fix made reachable for the first time: with no
/// bar at or before the target, take the nearest bar after it and report the timestamp
/// actually used. The reported time must be the real bar time — a unit slip here would
/// report a 1970 date and silently mis-stamp every forward-filled entry.
#[tokio::test]
async fn forward_fill_reports_the_bar_it_actually_used() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_fixture(tmp.path());
    let repo = FinqOptionsRepository::new(tmp.path().to_path_buf());

    let before_open = utc_at(13, 0); // 08:00 ET, 95 minutes before the first bar
    let (bars, actual) = repo
        .get_option_bars_at_or_after_time(TEST_SYMBOL, before_open, 120)
        .await
        .expect("forward fill within 120 minutes should find the 09:35 bar");

    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].close, Some(CLOSE_0935));
    assert_eq!(
        actual,
        utc_at(BAR_0935.0, BAR_0935.1),
        "forward fill must report the timestamp of the bar it used"
    );
}

/// Forward fill is bounded: a bar beyond the window is not used.
#[tokio::test]
async fn forward_fill_respects_its_window() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_fixture(tmp.path());
    let repo = FinqOptionsRepository::new(tmp.path().to_path_buf());

    let before_open = utc_at(13, 0); // 95 minutes before the first bar
    let err = repo
        .get_option_bars_at_or_after_time(TEST_SYMBOL, before_open, 30)
        .await
        .expect_err("a 30-minute window must not reach a bar 95 minutes away");

    // Must be "nothing in the window", not a dtype or parse error. Asserting only
    // `is_err()` here would have passed while the forward path was broken outright.
    assert!(
        matches!(err, RepositoryError::NotFound(_)),
        "expected NotFound, got {err:?} — the forward path is failing for the wrong reason"
    );
}
