# Bug Fix Summary - Spot Price Retrieval for IV Calculation

**Date**: January 2, 2026
**Impact**: Critical - affects all EOD and minute-aligned IV calculations

---

## Bugs Fixed

### Bug #1: Timestamp Unit Mismatch (milliseconds vs nanoseconds)

**Location**: `cs-domain/src/infrastructure/finq_equity_repo.rs:50`

**Problem**:
- DataFrame timestamps stored as `Datetime[ms]` (milliseconds since epoch)
- Code compared them to `target_nanos` (nanoseconds)
- All millisecond values < any nanosecond value → filter matched ALL bars
- Result: Always retrieved **last bar of the day** (extended hours) instead of market close

**Fix**:
```rust
// Before (buggy)
.filter(col("timestamp").lt_eq(lit(target_nanos)))

// After (fixed)
let target_millis = target_nanos / 1_000_000;  // Convert to milliseconds
.filter(col("timestamp").lt_eq(lit(target_millis)))
```

**Impact**:
- Was retrieving extended hours price (e.g., $212.59 at 7:59pm ET)
- Should retrieve market close price (e.g., $208.01 at 4:00pm ET)

---

### Bug #2: Missing Timezone Conversion (ET → UTC)

**Location**: `cs-domain/src/datetime.rs:100-106`

**Problem**:
- `MarketTime::new(16, 0)` intended to mean 4:00 PM Eastern Time
- Code treated it as 4:00 PM UTC
- 4:00 PM ET = 8:00 PM UTC (during EDT, summer)
- Retrieved 12:00 PM ET bar instead of 4:00 PM ET bar

**Fix**:
```rust
// Before (buggy)
pub fn with_time(&self, time: &MarketTime) -> TradingTimestamp {
    let day_nanos = (self.0 as i64) * NANOS_PER_DAY;
    let time_nanos = (time.hour as i64 * 3600 + time.minute as i64 * 60) * NANOS_PER_SECOND;
    TradingTimestamp(day_nanos + time_nanos)  // No timezone conversion!
}

// After (fixed)
pub fn with_time(&self, time: &MarketTime) -> TradingTimestamp {
    // Convert MarketTime (Eastern) to UTC
    let naive_time = NaiveTime::from_hms_opt(time.hour, time.minute, 0)
        .expect("Valid market time");
    let utc_datetime = eastern_to_utc(self.to_naive_date(), naive_time);
    TradingTimestamp::from_datetime_utc(utc_datetime)
}
```

**Impact**:
- Was retrieving spot price from 4 hours earlier (12pm ET instead of 4pm ET)
- Combined with Bug #1, was getting extended hours price

---

## Impact on AAPL July 31, 2025 Earnings

**Before Fixes (Both Bugs)**:
- Spot retrieved: **$212.59** (extended hours at 7:59pm ET)
- 30d IV: **34.10%** (inflated)
- Next day crush: **-14.55%** (just below -15% threshold → MISSED)

**After Bug #1 Fix Only** (timestamp conversion):
- Spot retrieved: **$208.72** (12:00 PM ET - still wrong timezone)
- 30d IV: **33.30%**
- Next day crush: **-12.63%** (still missed)

**After Both Fixes**:
- Spot retrieved: **$208.01** (4:00 PM ET market close - CORRECT!)
- 30d IV: **33.30%**
- Next day crush: **-12.63%** (still missed, but accurate)

---

## Key Insight

The July 31 earnings is **correctly missed** after the bug fixes. The actual IV crush was only **-12.63%**, which is below the -15% detection threshold. The original detection was a **false signal** caused by:

1. Using extended hours price ($212.59) inflated the earnings day IV to 34.10%
2. This created an artificial -14.55% crush next day
3. True market close IV was 33.30%, with real crush only -12.63%

**Minute-aligned method shows -4.21% crush** because it uses time-aligned option prices (options traded earlier in day when spot ~$208, not post-earnings $212).

---

## VNOM Test Results

**Findings**:
- VNOM has **sparse option data** - missing IV on actual earnings dates (May 5, Aug 4, Nov 3)
- Detected signals (Oct 20, Oct 24, Nov 13) don't match known earnings
- Likely **too illiquid** for reliable IV-based earnings detection

**Conclusion**: IV-based earnings detection works best for **highly liquid stocks** like AAPL where options trade continuously throughout the day.

---

## Files Modified

1. `cs-domain/src/infrastructure/finq_equity_repo.rs` - Fixed timestamp unit conversion
2. `cs-domain/src/datetime.rs` - Fixed timezone conversion ET→UTC
3. `view_atm_iv.py` - Fixed date display (days since epoch → YYYY-MM-DD)

---

## Validation

**Test Case**: AAPL 2025 full year
- **Detection Rate**: 3/4 earnings (75%)
- **Detected**: Q1 (Jan 31), Q2 (May 2), Q4 (Oct 31)
- **Missed**: Q3 (Jul 31) - actual crush -12.63% < -15% threshold
- **False Positives**: April volatility event, June/July end-of-quarter

**Sources**:
- [Apple Q1 2025 Earnings](https://www.apple.com/newsroom/2025/01/apple-reports-first-quarter-results/)
- [Apple Q3 2025 Earnings](https://9to5mac.com/2025/07/03/apple-to-release-q3-2025-earnings-results-on-thursday-july-31/)
- [Viper Energy Earnings](https://www.marketbeat.com/stocks/NASDAQ/VNOM/earnings/)
