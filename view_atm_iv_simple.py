#!/usr/bin/env python3
"""Simple viewer for ATM IV parquet files using only pyarrow"""
import sys
import pyarrow.parquet as pq
import pyarrow.compute as pc

def view_parquet(filepath):
    """Read and display parquet file contents"""
    table = pq.read_table(filepath)

    print("=" * 100)
    print(f"ATM IV Time Series: {filepath}")
    print("=" * 100)
    print()
    print(f"Total rows: {table.num_rows}")
    print(f"Columns: {table.column_names}")
    print()

    # Display schema
    print("Schema:")
    print("-" * 100)
    print(table.schema)
    print()

    # Display all data
    print("Data:")
    print("-" * 100)

    # Get column data
    symbols = table['symbol'].to_pylist()
    dates = table['date'].to_pylist()
    spots = table['spot'].to_pylist()
    iv_30d = table['atm_iv_30d'].to_pylist()
    iv_60d = table['atm_iv_60d'].to_pylist()
    iv_90d = table['atm_iv_90d'].to_pylist()
    spread_30_60 = table['term_spread_30_60'].to_pylist()
    spread_30_90 = table['term_spread_30_90'].to_pylist()

    # Print header
    print(f"{'Date':<12} {'Spot':<10} {'IV_30d':<10} {'IV_60d':<10} {'IV_90d':<10} {'Spread_30_60':<12} {'Spread_30_90':<12}")
    print("-" * 100)

    # Print rows
    for i in range(len(dates)):
        date_str = str(dates[i])
        spot_str = f"{spots[i]:.2f}" if spots[i] is not None else "N/A"
        iv30_str = f"{iv_30d[i]:.4f}" if iv_30d[i] is not None else "N/A"
        iv60_str = f"{iv_60d[i]:.4f}" if iv_60d[i] is not None else "N/A"
        iv90_str = f"{iv_90d[i]:.4f}" if iv_90d[i] is not None else "N/A"
        sp30_60_str = f"{spread_30_60[i]:.4f}" if spread_30_60[i] is not None else "N/A"
        sp30_90_str = f"{spread_30_90[i]:.4f}" if spread_30_90[i] is not None else "N/A"

        print(f"{date_str:<12} {spot_str:<10} {iv30_str:<10} {iv60_str:<10} {iv90_str:<10} {sp30_60_str:<12} {sp30_90_str:<12}")

    print()

    # Calculate basic statistics
    print("=" * 100)
    print("Summary Statistics:")
    print("=" * 100)

    def calc_stats(values):
        """Calculate mean, min, max for non-None values"""
        valid = [v for v in values if v is not None]
        if not valid:
            return "N/A", "N/A", "N/A", "N/A"
        return len(valid), f"{min(valid):.4f}", f"{max(valid):.4f}", f"{sum(valid)/len(valid):.4f}"

    print(f"{'Metric':<15} {'Count':<10} {'Min':<10} {'Max':<10} {'Mean':<10}")
    print("-" * 100)

    for name, values in [
        ("IV_30d", iv_30d),
        ("IV_60d", iv_60d),
        ("IV_90d", iv_90d),
        ("Spread_30_60", spread_30_60),
        ("Spread_30_90", spread_30_90),
    ]:
        count, min_val, max_val, mean_val = calc_stats(values)
        print(f"{name:<15} {str(count):<10} {min_val:<10} {max_val:<10} {mean_val:<10}")

    print()

    # Detect potential earnings signals
    print("=" * 100)
    print("Potential Earnings Signals:")
    print("=" * 100)

    for i in range(1, len(dates)):
        iv_30_curr = iv_30d[i]
        iv_30_prev = iv_30d[i-1]
        term_spread = spread_30_60[i]

        signals = []

        # IV change detection
        if iv_30_curr is not None and iv_30_prev is not None and iv_30_prev > 0:
            change_pct = (iv_30_curr - iv_30_prev) / iv_30_prev
            if change_pct < -0.15:  # 15% drop
                signals.append(f"IV CRUSH: {change_pct*100:.1f}% drop")
            elif change_pct > 0.20:  # 20% spike
                signals.append(f"IV SPIKE: {change_pct*100:.1f}% increase")

        # Backwardation detection
        if term_spread is not None and term_spread > 0.05:  # 5% backwardation
            signals.append(f"BACKWARDATION: {term_spread*100:.1f}%")

        if signals:
            print(f"{dates[i]}: {', '.join(signals)}")

    if not any(signals for i in range(1, len(dates))):
        print("No significant signals detected in this period")

    print()

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python3 view_atm_iv_simple.py <parquet_file>")
        sys.exit(1)

    view_parquet(sys.argv[1])
