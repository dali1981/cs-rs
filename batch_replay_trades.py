#!/usr/bin/env python3
"""
Batch Trade Replay - Generate trade replays for multiple trades.

Automatically generates individual trade analysis PDFs for all or a subset
of trades from a backtest results file, with filtering options.

Usage:
    uv run python3 batch_replay_trades.py --result <results.parquet> [options]
    uv run python3 batch_replay_trades.py --result <results.json> [options]
"""

import argparse
import json
import sys
from pathlib import Path
from datetime import datetime
import polars as pl
import warnings
from concurrent.futures import ThreadPoolExecutor, as_completed

warnings.filterwarnings("ignore")

try:
    from replay_trade import TradeReplayAnalyzer, load_trade_from_file
except ImportError:
    print("ERROR: Could not import replay_trade module")
    print("Ensure replay_trade.py is in the same directory")
    sys.exit(1)


class BatchTradeReplayer:
    """Generate trade replays for multiple trades."""

    def __init__(self, result_file: str, data_dir: Path = None, output_dir: Path = None):
        """Initialize batch replayer."""
        self.result_file = Path(result_file)
        self.data_dir = data_dir or Path.home() / "finq_data"
        self.output_dir = output_dir or Path.cwd() / "trade_replays"
        self.output_dir.mkdir(parents=True, exist_ok=True)

        # Load results
        self.trades = self._load_trades()
        self.total_trades = len(self.trades)

    def _load_trades(self) -> list:
        """Load all trades from result file."""
        if self.result_file.suffix == ".json":
            with open(self.result_file) as f:
                data = json.load(f)
                return data if isinstance(data, list) else [data]
        elif self.result_file.suffix == ".parquet":
            df = pl.read_parquet(self.result_file)
            return [
                {col: row[col].item() if hasattr(row[col], 'item') else row[col]
                 for col in df.columns}
                for row in df.iter_rows(named=True)
            ]
        else:
            raise ValueError(f"Unsupported file format: {self.result_file.suffix}")

    def filter_trades(
        self,
        min_pnl: float = None,
        max_pnl: float = None,
        min_pnl_pct: float = None,
        max_pnl_pct: float = None,
        symbols: list = None,
        success_only: bool = False,
        failed_only: bool = False,
    ) -> list:
        """Filter trades based on criteria."""
        filtered = self.trades

        if success_only:
            filtered = [t for t in filtered if t.get("success", False)]

        if failed_only:
            filtered = [t for t in filtered if not t.get("success", False)]

        if min_pnl is not None:
            filtered = [t for t in filtered if (t.get("pnl") or 0) >= min_pnl]

        if max_pnl is not None:
            filtered = [t for t in filtered if (t.get("pnl") or 0) <= max_pnl]

        if min_pnl_pct is not None:
            filtered = [t for t in filtered if (t.get("pnl_pct") or 0) >= min_pnl_pct]

        if max_pnl_pct is not None:
            filtered = [t for t in filtered if (t.get("pnl_pct") or 0) <= max_pnl_pct]

        if symbols:
            filtered = [t for t in filtered if t.get("symbol") in symbols]

        return filtered

    def analyze_trade(self, trade: dict, index: int) -> tuple:
        """Analyze a single trade."""
        try:
            analyzer = TradeReplayAnalyzer(trade, self.data_dir)

            # Load market data
            if not analyzer.load_market_data():
                return index, False, "Could not load market data"

            # Generate output path
            symbol = trade.get("symbol", "UNKNOWN")
            entry_time = trade.get("entry_time", "")
            if isinstance(entry_time, str):
                entry_dt = datetime.fromisoformat(entry_time.replace("Z", "+00:00"))
            else:
                entry_dt = entry_time

            output_file = self.output_dir / f"trade_{symbol}_{entry_dt.strftime('%Y%m%d_%H%M')}_idx{index}.png"

            # Generate visualization
            analyzer.plot_trade_analysis(str(output_file))

            return index, True, str(output_file)

        except Exception as e:
            return index, False, f"Error: {str(e)}"

    def process_trades(
        self,
        trades: list,
        max_workers: int = 4,
        verbose: bool = True,
    ) -> dict:
        """Process multiple trades in parallel."""
        results = {}
        successful = 0
        failed = 0

        print(f"\nProcessing {len(trades)} trades (max {max_workers} parallel workers)...")
        print(f"Output directory: {self.output_dir}")
        print()

        with ThreadPoolExecutor(max_workers=max_workers) as executor:
            # Submit all tasks
            futures = {
                executor.submit(self.analyze_trade, trade, i): i
                for i, trade in enumerate(trades)
            }

            # Process results as they complete
            for i, future in enumerate(futures, 1):
                idx, success, message = future.result()
                trade = trades[idx]

                if success:
                    symbol = trade.get("symbol", "?")
                    pnl = trade.get("pnl", 0)
                    pnl_str = f"${pnl:+.2f}" if isinstance(pnl, (int, float)) else "N/A"
                    pnl_pct = trade.get("pnl_pct", 0)
                    pnl_pct_str = f"{pnl_pct:+.1f}%" if isinstance(pnl_pct, (int, float)) else "N/A"

                    print(f"[{i:3d}/{len(trades)}] ✓ {symbol:6s} P&L: {pnl_str:10s} ({pnl_pct_str:7s}) → {Path(message).name}")
                    successful += 1
                else:
                    symbol = trade.get("symbol", "?")
                    print(f"[{i:3d}/{len(trades)}] ✗ {symbol:6s} {message}")
                    failed += 1

                results[idx] = (success, message)

        print()
        print(f"{'='*60}")
        print(f"Summary: {successful} successful, {failed} failed")
        print(f"{'='*60}")

        return results


def generate_index_html(output_dir: Path, trades: list, results: dict):
    """Generate an HTML index of all trade replays."""
    index_file = output_dir / "index.html"

    html_parts = [
        "<!DOCTYPE html>",
        "<html>",
        "<head>",
        "<title>Trade Replay Index</title>",
        "<style>",
        "  body { font-family: Arial, sans-serif; margin: 20px; }",
        "  table { border-collapse: collapse; width: 100%; }",
        "  th, td { border: 1px solid #ddd; padding: 8px; text-align: left; }",
        "  th { background-color: #4CAF50; color: white; }",
        "  tr:nth-child(even) { background-color: #f2f2f2; }",
        "  .success { color: green; font-weight: bold; }",
        "  .failed { color: red; font-weight: bold; }",
        "  a { color: #0066cc; }",
        "  .pnl-positive { color: green; }",
        "  .pnl-negative { color: red; }",
        "</style>",
        "</head>",
        "<body>",
        "<h1>Trade Replay Analysis</h1>",
        f"<p>Generated: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}</p>",
        "<table>",
        "<tr>",
        "<th>Index</th>",
        "<th>Symbol</th>",
        "<th>Entry Date</th>",
        "<th>P&L</th>",
        "<th>P&L %</th>",
        "<th>Status</th>",
        "<th>Analysis</th>",
        "</tr>",
    ]

    for idx, trade in enumerate(trades):
        success, message = results.get(idx, (False, "Not processed"))

        symbol = trade.get("symbol", "?")
        entry_time = trade.get("entry_time", "")
        if isinstance(entry_time, str):
            try:
                entry_dt = datetime.fromisoformat(entry_time.replace("Z", "+00:00"))
                entry_str = entry_dt.strftime("%Y-%m-%d %H:%M")
            except:
                entry_str = entry_time
        else:
            entry_str = str(entry_time)

        pnl = trade.get("pnl", 0)
        pnl_str = f"${pnl:+.2f}" if isinstance(pnl, (int, float)) else "N/A"
        pnl_class = "pnl-positive" if pnl and pnl > 0 else "pnl-negative"

        pnl_pct = trade.get("pnl_pct", 0)
        pnl_pct_str = f"{pnl_pct:+.1f}%" if isinstance(pnl_pct, (int, float)) else "N/A"

        status = "✓ Success" if success else "✗ Failed"
        status_class = "success" if success else "failed"

        analysis_link = f'<a href="{Path(message).name}">View</a>' if success else "N/A"

        html_parts.extend([
            "<tr>",
            f"<td>{idx}</td>",
            f"<td>{symbol}</td>",
            f"<td>{entry_str}</td>",
            f"<td class='{pnl_class}'>{pnl_str}</td>",
            f"<td class='{pnl_class}'>{pnl_pct_str}</td>",
            f"<td class='{status_class}'>{status}</td>",
            f"<td>{analysis_link}</td>",
            "</tr>",
        ])

    html_parts.extend([
        "</table>",
        "</body>",
        "</html>",
    ])

    with open(index_file, "w") as f:
        f.write("\n".join(html_parts))

    return index_file


def main():
    parser = argparse.ArgumentParser(
        description="Generate trade replays for multiple trades",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Replay all trades
  uv run python3 batch_replay_trades.py --result results.parquet

  # Replay only winners
  uv run python3 batch_replay_trades.py --result results.json --success-only

  # Replay specific symbols
  uv run python3 batch_replay_trades.py --result results.parquet --symbols AAPL MSFT GOOGL

  # Replay trades with P&L between $100-500
  uv run python3 batch_replay_trades.py --result results.parquet --min-pnl 100 --max-pnl 500

  # Replay losers (P&L < -$50)
  uv run python3 batch_replay_trades.py --result results.parquet --max-pnl -50

  # Replay top 10 winners (results must be sorted)
  uv run python3 batch_replay_trades.py --result results.parquet --max-trades 10
        """
    )

    parser.add_argument("--result", type=str, required=True,
                       help="Path to trade result file (JSON or parquet)")
    parser.add_argument("--symbols", nargs="+", default=None,
                       help="Filter to specific symbols")
    parser.add_argument("--success-only", action="store_true",
                       help="Only replay successful trades")
    parser.add_argument("--failed-only", action="store_true",
                       help="Only replay failed trades")
    parser.add_argument("--min-pnl", type=float, default=None,
                       help="Minimum P&L (in dollars)")
    parser.add_argument("--max-pnl", type=float, default=None,
                       help="Maximum P&L (in dollars)")
    parser.add_argument("--min-pnl-pct", type=float, default=None,
                       help="Minimum P&L percentage")
    parser.add_argument("--max-pnl-pct", type=float, default=None,
                       help="Maximum P&L percentage")
    parser.add_argument("--max-trades", type=int, default=None,
                       help="Maximum number of trades to process")
    parser.add_argument("--output-dir", type=Path, default=None,
                       help="Output directory (default: ./trade_replays)")
    parser.add_argument("--data-dir", type=Path, default=None,
                       help="finq data directory (default: ~/finq_data)")
    parser.add_argument("--workers", type=int, default=4,
                       help="Number of parallel workers (default: 4)")
    parser.add_argument("--no-index", action="store_true",
                       help="Skip HTML index generation")

    args = parser.parse_args()

    # Create batch replayer
    try:
        replayer = BatchTradeReplayer(args.result, args.data_dir, args.output_dir)
    except Exception as e:
        print(f"ERROR: {e}")
        sys.exit(1)

    print(f"{'='*60}")
    print(f"Batch Trade Replayer")
    print(f"{'='*60}")
    print(f"Total trades loaded: {replayer.total_trades}")

    # Filter trades
    trades = replayer.filter_trades(
        min_pnl=args.min_pnl,
        max_pnl=args.max_pnl,
        min_pnl_pct=args.min_pnl_pct,
        max_pnl_pct=args.max_pnl_pct,
        symbols=args.symbols,
        success_only=args.success_only,
        failed_only=args.failed_only,
    )

    print(f"Trades after filtering: {len(trades)}")

    # Limit if requested
    if args.max_trades and len(trades) > args.max_trades:
        trades = trades[:args.max_trades]
        print(f"Limited to first {args.max_trades} trades")

    if not trades:
        print("No trades matched filter criteria")
        sys.exit(0)

    # Process trades
    results = replayer.process_trades(trades, args.workers)

    # Generate index
    if not args.no_index:
        index_file = generate_index_html(replayer.output_dir, trades, results)
        print(f"\n✓ Generated HTML index: {index_file}")

    print(f"\n✓ Batch replay complete!")
    print(f"  Output directory: {replayer.output_dir}")


if __name__ == "__main__":
    main()
