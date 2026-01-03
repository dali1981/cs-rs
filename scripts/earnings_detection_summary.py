#!/usr/bin/env python3
"""
Compare earnings signals detected from minute-aligned IV with actual earnings dates

Date Format: Parquet files store dates as "days since Unix epoch (1970-01-01)"
Example: 20119 = January 31, 2025 (20119 days after Jan 1, 1970)
"""
from datetime import datetime, timedelta

# Known AAPL 2025 earnings dates (verified from Apple newsroom and investor relations)
# Source: https://www.apple.com/newsroom/2025/01/apple-reports-first-quarter-results/
# Source: https://9to5mac.com/2025/07/03/apple-to-release-q3-2025-earnings-results-on-thursday-july-31/
actual_earnings = {
    "2025-01-30": "Q1 FY2025 (Oct-Dec 2024)",
    "2025-05-01": "Q2 FY2025 (Jan-Mar 2025)",
    "2025-07-31": "Q3 FY2025 (Apr-Jun 2025)",
    "2025-10-30": "Q4 FY2025 (Jul-Sep 2025)",
}

# Convert date number (days since epoch) to datetime
def date_num_to_datetime(date_num):
    """Convert Parquet date format to datetime"""
    epoch = datetime(1970, 1, 1)
    return epoch + timedelta(days=date_num)

# Detected signals from minute-aligned IV
signals = {
    20119: "IV CRUSH: -15.6%",
    20157: "IV SPIKE: 28.6%",
    20180: "IV SPIKE: 41.4%",
    20182: "IV SPIKE: 41.9%",
    20187: "IV CRUSH: -33.8%",
    20188: "IV SPIKE: 27.7%",
    20210: "IV CRUSH: -20.9%",
    20269: "IV CRUSH: -15.4%",
    20271: "IV CRUSH: -16.6%",
    20392: "IV CRUSH: -17.0%",
    20446: "IV SPIKE: 27.5%",
    20448: "IV CRUSH: -20.7%",
}

print("=" * 100)
print("AAPL 2025 Earnings Detection - Minute-Aligned IV Method")
print("=" * 100)
print()
print("Date Format: Parquet files use 'days since Unix epoch (1970-01-01)'")
print("  Example: date 20119 = Jan 31, 2025 (20,119 days after Jan 1, 1970)")
print("  This is the standard Arrow/Parquet date representation")
print()

print("Actual Earnings Dates:")
for date, quarter in actual_earnings.items():
    print(f"  {date}: {quarter}")
print()

print("=" * 100)
print("Detected Signals (IV Crush/Spike > threshold):")
print("=" * 100)
for date_num, signal in signals.items():
    date = date_num_to_datetime(date_num)
    is_earnings = "✅ EARNINGS!" if date.strftime("%Y-%m-%d") in actual_earnings else ""
    # Check if it's the day after earnings
    prev_day = (date - timedelta(days=1)).strftime("%Y-%m-%d")
    if prev_day in actual_earnings:
        is_earnings = f"✅ EARNINGS! (day after {actual_earnings[prev_day]})"
    print(f"{date.strftime('%Y-%m-%d')} (day {date_num}): {signal} {is_earnings}")

print()
print("=" * 100)
print("Detection Results:")
print("=" * 100)

# Check each earnings date
for earnings_date in actual_earnings:
    earnings_dt = datetime.strptime(earnings_date, "%Y-%m-%d")
    next_day = earnings_dt + timedelta(days=1)

    # Find signals within +/- 2 days
    detected = []
    for date_num, signal in signals.items():
        signal_date = date_num_to_datetime(date_num)
        if abs((signal_date - earnings_dt).days) <= 2:
            detected.append((signal_date, signal))

    if detected:
        print(f"\n{earnings_date} ({actual_earnings[earnings_date]}):")
        for sig_date, sig in detected:
            print(f"  ✅ Detected on {sig_date.strftime('%Y-%m-%d')}: {sig}")
    else:
        print(f"\n{earnings_date} ({actual_earnings[earnings_date]}):")
        print(f"  ❌ No signal detected")

# Count detection rate
detected_count = sum(1 for earnings_date in actual_earnings
                     if any(abs((date_num_to_datetime(date_num) -
                                datetime.strptime(earnings_date, "%Y-%m-%d")).days) <= 2
                            for date_num in signals))

print()
print("=" * 100)
print(f"Detection Rate: {detected_count}/{len(actual_earnings)} ({detected_count/len(actual_earnings)*100:.0f}%)")
print("=" * 100)
print()

# Non-earnings signals (potential false positives)
print("Non-Earnings Signals (potential false positives or other events):")
print("-" * 100)
for date_num, signal in signals.items():
    signal_date = date_num_to_datetime(date_num)
    is_earnings_related = any(abs((signal_date -
                                   datetime.strptime(e_date, "%Y-%m-%d")).days) <= 2
                              for e_date in actual_earnings)
    if not is_earnings_related:
        print(f"{signal_date.strftime('%Y-%m-%d')}: {signal}")
