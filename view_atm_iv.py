#!/usr/bin/env python3
"""
Quick viewer for ATM IV parquet files
"""
import sys
import pyarrow.parquet as pq

def view_parquet(filepath):
    """Read and display parquet file contents"""
    table = pq.read_table(filepath)
    df = table.to_pandas()

    print("=" * 100)
    print(f"ATM IV Time Series: {filepath}")
    print("=" * 100)
    print()

    # Display all rows
    print("Data:")
    print("-" * 100)
    pd_opts = {
        'display.max_rows': None,
        'display.max_columns': None,
        'display.width': None,
        'display.max_colwidth': None,
        'display.float_format': lambda x: f'{x:.4f}' if pd.notna(x) else 'NaN'
    }

    import pandas as pd
    with pd.option_context(*[item for pair in pd_opts.items() for item in pair]):
        print(df.to_string(index=False))

    print()
    print("=" * 100)
    print("Summary Statistics:")
    print("=" * 100)
    print(df[['atm_iv_30d', 'atm_iv_60d', 'atm_iv_90d', 'term_spread_30_60', 'term_spread_30_90']].describe())
    print()

    # Detect potential earnings signals
    print("=" * 100)
    print("Potential Earnings Signals:")
    print("=" * 100)

    # Convert date function
    from datetime import datetime, timedelta
    epoch = datetime(1970, 1, 1)

    def date_num_to_str(date_num):
        """Convert days since epoch to YYYY-MM-DD string"""
        dt = epoch + timedelta(days=int(date_num))
        return dt.strftime('%Y-%m-%d')

    for i in range(1, len(df)):
        date = df.iloc[i]['date']
        prev_date = df.iloc[i-1]['date']

        iv_30_curr = df.iloc[i]['atm_iv_30d']
        iv_30_prev = df.iloc[i-1]['atm_iv_30d']
        term_spread = df.iloc[i]['term_spread_30_60']

        signals = []

        # IV Crush detection
        if pd.notna(iv_30_curr) and pd.notna(iv_30_prev) and iv_30_prev > 0:
            change_pct = (iv_30_curr - iv_30_prev) / iv_30_prev
            if change_pct < -0.15:  # 15% drop
                signals.append(f"IV CRUSH: {change_pct*100:.1f}% drop")
            elif change_pct > 0.20:  # 20% spike
                signals.append(f"IV SPIKE: {change_pct*100:.1f}% increase")

        # Backwardation detection
        if pd.notna(term_spread) and term_spread > 0.05:  # 5% backwardation
            signals.append(f"BACKWARDATION: {term_spread*100:.1f}%")

        if signals:
            date_str = date_num_to_str(date)
            print(f"{date_str}: {', '.join(signals)}")

    print()

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python3 view_atm_iv.py <parquet_file>")
        sys.exit(1)

    view_parquet(sys.argv[1])
