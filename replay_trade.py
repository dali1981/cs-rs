#!/usr/bin/env python3
"""
Trade Replay Analyzer - Reconstruct market context for individual trades.

Loads a trade result and replays it through market context data, creating
comprehensive visualizations of:
- Stock price movement with entry/exit/earnings markers
- IV surface evolution (ATM and strikes)
- Realized vs Historical volatility
- Greeks evolution
- Trade P&L attribution

Usage:
    uv run python3 replay_trade.py --result <result.json> [options]
    uv run python3 replay_trade.py --result <result.parquet> --index 0 [options]
"""

import argparse
import json
import sys
from pathlib import Path
from datetime import datetime, timedelta
import numpy as np
import polars as pl
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from matplotlib.patches import Rectangle
import warnings

warnings.filterwarnings("ignore")

# Try to import finq for market data - provide helpful error if not available
try:
    import finq
except ImportError:
    print("ERROR: finq library not found. Install with: uv add finq")
    sys.exit(1)


class TradeReplayAnalyzer:
    """Analyzes and replays a single trade through market context."""

    def __init__(self, trade_data: dict, data_dir: Path = None):
        """Initialize analyzer with trade data."""
        self.trade = trade_data
        self.symbol = trade_data.get("symbol")
        self.entry_time = self._parse_datetime(trade_data.get("entry_time"))
        self.exit_time = self._parse_datetime(trade_data.get("exit_time"))
        self.earnings_date = self._parse_date(trade_data.get("earnings_date"))
        self.data_dir = data_dir or Path.home() / "finq_data"

        # Market data (loaded on demand)
        self.spot_history = None
        self.iv_history = None
        self.hv_history = None
        self.option_chains = None

    @staticmethod
    def _parse_datetime(dt_str):
        """Parse ISO datetime string."""
        if not dt_str:
            return None
        try:
            return datetime.fromisoformat(dt_str.replace("Z", "+00:00"))
        except:
            return None

    @staticmethod
    def _parse_date(date_str):
        """Parse ISO date string."""
        if not date_str:
            return None
        try:
            return datetime.fromisoformat(date_str).date()
        except:
            return None

    def load_market_data(self):
        """Load market data from finq."""
        if not self.symbol:
            print("ERROR: No symbol found in trade data")
            return False

        # Expand date range: 30 days before entry to 1 day after exit
        start = (self.entry_time - timedelta(days=30)).date()
        end = (self.exit_time + timedelta(days=1)).date()

        print(f"Loading market data for {self.symbol} from {start} to {end}...")

        try:
            # Load spot price history
            self.spot_history = finq.fetch_eod_quotes(
                self.symbol,
                start_date=start,
                end_date=end,
                data_dir=self.data_dir
            )

            if self.spot_history is None or len(self.spot_history) == 0:
                print(f"  WARNING: No spot data found for {self.symbol}")
                return False

            print(f"  ✓ Loaded {len(self.spot_history)} spot quotes")

            # Load option chains (at entry and exit times)
            chains = {}
            for dt in [self.entry_time.date(), self.exit_time.date()]:
                try:
                    chain = finq.fetch_option_chain(
                        self.symbol,
                        date=dt,
                        data_dir=self.data_dir
                    )
                    if chain is not None:
                        chains[dt] = chain
                        print(f"  ✓ Loaded option chain for {dt}")
                except:
                    pass

            self.option_chains = chains if chains else None

            return True

        except Exception as e:
            print(f"ERROR loading market data: {e}")
            return False

    def calculate_historical_volatility(self, window=30):
        """Calculate historical volatility from spot data."""
        if self.spot_history is None:
            return None

        try:
            # Extract closing prices
            if hasattr(self.spot_history, 'to_pandas'):
                prices = self.spot_history['close'].to_pandas() if 'close' in self.spot_history.columns else None
            else:
                prices = self.spot_history.get('close')

            if prices is None or len(prices) < window:
                return None

            # Calculate log returns
            log_returns = np.log(prices / prices.shift(1)).dropna()

            # Rolling HV
            hv = log_returns.rolling(window=window).std() * np.sqrt(252)

            return hv
        except:
            return None

    def get_trade_info(self) -> dict:
        """Extract key trade information."""
        return {
            "symbol": self.symbol,
            "entry_time": self.entry_time,
            "exit_time": self.exit_time,
            "earnings_date": self.earnings_date,
            "entry_price": self.trade.get("entry_cost") or self.trade.get("entry_debit") or self.trade.get("entry_credit"),
            "exit_price": self.trade.get("exit_value") or self.trade.get("exit_credit") or self.trade.get("exit_cost"),
            "pnl": self.trade.get("pnl"),
            "pnl_pct": self.trade.get("pnl_pct"),
            "success": self.trade.get("success", False),
            "failure_reason": self.trade.get("failure_reason"),
            "iv_entry": self.trade.get("iv_entry"),
            "iv_exit": self.trade.get("iv_exit"),
            "spot_entry": self.trade.get("spot_at_entry"),
            "spot_exit": self.trade.get("spot_at_exit"),
        }

    def plot_trade_analysis(self, output_path: str = None) -> str:
        """Generate multi-panel trade analysis visualization."""

        trade_info = self.get_trade_info()

        # Create figure with multiple subplots
        fig = plt.figure(figsize=(18, 12))
        gs = fig.add_gridspec(3, 2, hspace=0.3, wspace=0.3)

        # Panel 1: Stock price with trade markers
        ax1 = fig.add_subplot(gs[0, :])
        self._plot_stock_price(ax1, trade_info)

        # Panel 2: IV Evolution (if available)
        ax2 = fig.add_subplot(gs[1, 0])
        self._plot_iv_evolution(ax2, trade_info)

        # Panel 3: Historical vs Realized Volatility
        ax3 = fig.add_subplot(gs[1, 1])
        self._plot_volatility_comparison(ax3)

        # Panel 4: Greeks Evolution (if available)
        ax4 = fig.add_subplot(gs[2, 0])
        self._plot_greeks_evolution(ax4, trade_info)

        # Panel 5: P&L Attribution (if available)
        ax5 = fig.add_subplot(gs[2, 1])
        self._plot_pnl_summary(ax5, trade_info)

        # Main title
        title = f"{trade_info['symbol']} Trade Analysis\n"
        title += f"Entry: {trade_info['entry_time'].strftime('%Y-%m-%d %H:%M')} "
        title += f"Exit: {trade_info['exit_time'].strftime('%Y-%m-%d %H:%M')}"
        if trade_info['earnings_date']:
            title += f" | Earnings: {trade_info['earnings_date']}"

        fig.suptitle(title, fontsize=14, fontweight="bold", y=0.995)

        # Save
        if output_path is None:
            output_path = f"trade_replay_{self.symbol}_{self.entry_time.strftime('%Y%m%d_%H%M')}.png"

        plt.savefig(output_path, dpi=150, bbox_inches="tight")
        print(f"\n✓ Saved analysis to: {output_path}")

        return output_path

    def _plot_stock_price(self, ax, trade_info):
        """Plot stock price with entry/exit and earnings markers."""

        if self.spot_history is None:
            ax.text(0.5, 0.5, "No spot data available",
                   ha="center", va="center", transform=ax.transAxes, fontsize=12)
            return

        # Extract dates and closes
        try:
            if hasattr(self.spot_history, 'to_pandas'):
                df_spot = self.spot_history.to_pandas()
                dates = df_spot.index if hasattr(df_spot.index, 'date') else pd.to_datetime(df_spot.index).date
                closes = df_spot['close'].values if 'close' in df_spot.columns else df_spot.iloc[:, 0].values
            else:
                dates = self.spot_history['date']
                closes = self.spot_history['close']
        except:
            ax.text(0.5, 0.5, "Could not parse spot data",
                   ha="center", va="center", transform=ax.transAxes, fontsize=12)
            return

        # Plot spot price
        ax.plot(dates, closes, linewidth=2, color="steelblue", label="Spot Price", zorder=2)

        # Add entry marker
        if trade_info['spot_entry']:
            ax.axvline(trade_info['entry_time'], color="green", linestyle="--",
                      linewidth=2, alpha=0.7, label="Entry", zorder=1)
            ax.scatter([trade_info['entry_time']], [trade_info['spot_entry']],
                      color="green", s=100, marker="o", zorder=3)

        # Add exit marker
        if trade_info['spot_exit']:
            ax.axvline(trade_info['exit_time'], color="red", linestyle="--",
                      linewidth=2, alpha=0.7, label="Exit", zorder=1)
            ax.scatter([trade_info['exit_time']], [trade_info['spot_exit']],
                      color="red", s=100, marker="s", zorder=3)

        # Add earnings marker
        if trade_info['earnings_date']:
            earnings_dt = datetime.combine(trade_info['earnings_date'],
                                          datetime.min.time()).replace(hour=9, minute=30)
            ax.axvline(earnings_dt, color="orange", linestyle="-",
                      linewidth=2, alpha=0.5, label="Earnings", zorder=0)

        ax.set_xlabel("Date")
        ax.set_ylabel("Spot Price ($)")
        ax.set_title("Stock Price with Trade Markers")
        ax.legend(loc="best")
        ax.grid(True, alpha=0.3)
        ax.xaxis.set_major_formatter(mdates.DateFormatter("%Y-%m-%d"))
        plt.setp(ax.xaxis.get_majorticklabels(), rotation=45, ha="right")

    def _plot_iv_evolution(self, ax, trade_info):
        """Plot IV evolution at entry and exit."""

        if not self.option_chains:
            ax.text(0.5, 0.5, "No option chain data available",
                   ha="center", va="center", transform=ax.transAxes, fontsize=11)
            return

        try:
            # Collect IVs from chains
            entry_date = self.entry_time.date()
            exit_date = self.exit_time.date()

            dates = []
            atm_ivs = []

            for dt in sorted(self.option_chains.keys()):
                chain = self.option_chains[dt]

                # Get ATM IV (from nearest calls/puts)
                try:
                    if hasattr(chain, 'to_pandas'):
                        df_chain = chain.to_pandas()
                    else:
                        df_chain = chain

                    # Simple ATM: use median of near-the-money options
                    calls = df_chain[df_chain['option_type'] == 'C'] if 'option_type' in df_chain.columns else df_chain[df_chain['type'] == 'C']
                    if len(calls) > 0:
                        atm_iv = calls['iv'].median() if 'iv' in calls.columns else 0.0
                        if atm_iv > 0:
                            dates.append(dt)
                            atm_ivs.append(atm_iv * 100)  # Convert to percentage
                except:
                    pass

            if not dates or not atm_ivs:
                ax.text(0.5, 0.5, "Could not extract IV from option chains",
                       ha="center", va="center", transform=ax.transAxes, fontsize=11)
                return

            # Plot IV evolution
            ax.plot(dates, atm_ivs, marker="o", linewidth=2, markersize=8, color="purple")

            # Mark entry and exit
            ax.axvline(entry_date, color="green", linestyle="--", alpha=0.5, label="Entry")
            ax.axvline(exit_date, color="red", linestyle="--", alpha=0.5, label="Exit")

            ax.set_xlabel("Date")
            ax.set_ylabel("ATM IV (%)")
            ax.set_title("IV Evolution")
            ax.legend()
            ax.grid(True, alpha=0.3)

        except Exception as e:
            ax.text(0.5, 0.5, f"Error plotting IV:\n{str(e)[:50]}",
                   ha="center", va="center", transform=ax.transAxes, fontsize=10)

    def _plot_volatility_comparison(self, ax):
        """Plot historical vs realized volatility."""

        hv = self.calculate_historical_volatility()

        if hv is None:
            ax.text(0.5, 0.5, "Could not calculate HV",
                   ha="center", va="center", transform=ax.transAxes, fontsize=11)
            return

        try:
            # Plot HV
            ax.plot(hv.index, hv.values * 100, linewidth=2, label="30d HV", color="steelblue")

            # Mark entry/exit
            ax.axvline(self.entry_time, color="green", linestyle="--", alpha=0.5, label="Entry")
            ax.axvline(self.exit_time, color="red", linestyle="--", alpha=0.5, label="Exit")

            # Add entry IV if available
            if self.trade.get("iv_entry"):
                ax.axhline(self.trade["iv_entry"] * 100, color="orange",
                          linestyle=":", alpha=0.7, label=f"Entry IV: {self.trade['iv_entry']*100:.1f}%")

            ax.set_xlabel("Date")
            ax.set_ylabel("Volatility (%)")
            ax.set_title("Historical Volatility")
            ax.legend()
            ax.grid(True, alpha=0.3)
            ax.xaxis.set_major_formatter(mdates.DateFormatter("%Y-%m-%d"))
            plt.setp(ax.xaxis.get_majorticklabels(), rotation=45, ha="right")

        except Exception as e:
            ax.text(0.5, 0.5, f"Error plotting volatility:\n{str(e)[:50]}",
                   ha="center", va="center", transform=ax.transAxes, fontsize=10)

    def _plot_greeks_evolution(self, ax, trade_info):
        """Plot Greeks evolution if available in trade data."""

        greeks = {
            'delta': self.trade.get('net_delta'),
            'gamma': self.trade.get('net_gamma'),
            'theta': self.trade.get('net_theta'),
            'vega': self.trade.get('net_vega'),
        }

        available_greeks = {k: v for k, v in greeks.items() if v is not None}

        if not available_greeks:
            ax.text(0.5, 0.5, "No Greeks data in trade result",
                   ha="center", va="center", transform=ax.transAxes, fontsize=11)
            return

        # Display Greeks as text table
        ax.axis("off")

        title_text = "Greeks at Entry"
        ax.text(0.5, 0.95, title_text, ha="center", va="top",
               fontsize=12, fontweight="bold", transform=ax.transAxes)

        y_pos = 0.75
        for greek, value in available_greeks.items():
            if value is not None:
                ax.text(0.1, y_pos, f"{greek.upper():6s}:",
                       ha="left", va="top", fontsize=11, family="monospace",
                       transform=ax.transAxes)
                ax.text(0.4, y_pos, f"{value:10.4f}",
                       ha="left", va="top", fontsize=11, family="monospace",
                       transform=ax.transAxes, color="steelblue", fontweight="bold")
                y_pos -= 0.15

    def _plot_pnl_summary(self, ax, trade_info):
        """Plot P&L summary."""

        ax.axis("off")

        # Title
        pnl = trade_info['pnl']
        pnl_color = "green" if pnl and pnl > 0 else "red"

        ax.text(0.5, 0.95, "P&L Summary", ha="center", va="top",
               fontsize=12, fontweight="bold", transform=ax.transAxes)

        # P&L
        if pnl is not None:
            ax.text(0.1, 0.75, "P&L:", ha="left", va="top",
                   fontsize=11, family="monospace", transform=ax.transAxes)
            ax.text(0.4, 0.75, f"${pnl:10.2f}", ha="left", va="top",
                   fontsize=11, family="monospace", color=pnl_color,
                   fontweight="bold", transform=ax.transAxes)

        # P&L %
        if trade_info['pnl_pct'] is not None:
            ax.text(0.1, 0.60, "P&L %:", ha="left", va="top",
                   fontsize=11, family="monospace", transform=ax.transAxes)
            ax.text(0.4, 0.60, f"{trade_info['pnl_pct']:10.2f}%", ha="left", va="top",
                   fontsize=11, family="monospace", color=pnl_color,
                   fontweight="bold", transform=ax.transAxes)

        # IV metrics
        if trade_info['iv_entry'] and trade_info['iv_exit']:
            iv_change = (trade_info['iv_exit'] - trade_info['iv_entry']) * 100
            ax.text(0.1, 0.45, "IV Entry:", ha="left", va="top",
                   fontsize=10, family="monospace", transform=ax.transAxes)
            ax.text(0.4, 0.45, f"{trade_info['iv_entry']*100:6.1f}%", ha="left", va="top",
                   fontsize=10, family="monospace", transform=ax.transAxes)

            ax.text(0.1, 0.32, "IV Exit:", ha="left", va="top",
                   fontsize=10, family="monospace", transform=ax.transAxes)
            ax.text(0.4, 0.32, f"{trade_info['iv_exit']*100:6.1f}%", ha="left", va="top",
                   fontsize=10, family="monospace", transform=ax.transAxes)

            ax.text(0.1, 0.19, "IV Change:", ha="left", va="top",
                   fontsize=10, family="monospace", transform=ax.transAxes)
            iv_color = "red" if iv_change < 0 else "green"
            ax.text(0.4, 0.19, f"{iv_change:+6.1f}pp", ha="left", va="top",
                   fontsize=10, family="monospace", color=iv_color, transform=ax.transAxes)

        # Status
        status = "✓ Success" if trade_info['success'] else "✗ Failed"
        status_color = "green" if trade_info['success'] else "red"
        ax.text(0.5, 0.05, status, ha="center", va="bottom",
               fontsize=11, fontweight="bold", color=status_color,
               transform=ax.transAxes)


def load_trade_from_file(file_path: str, index: int = None) -> dict:
    """Load a single trade from JSON or parquet file."""

    path = Path(file_path)

    if path.suffix == ".json":
        with open(path) as f:
            data = json.load(f)

        if isinstance(data, list):
            if index is None:
                index = 0
            return data[index]
        return data

    elif path.suffix == ".parquet":
        df = pl.read_parquet(path)

        if index is None:
            index = 0

        if index >= len(df):
            raise ValueError(f"Index {index} out of range for {len(df)} rows")

        # Convert to dict
        row = df[index]
        return {col: row[col].item() if hasattr(row[col], 'item') else row[col]
               for col in df.columns}

    else:
        raise ValueError(f"Unsupported file format: {path.suffix}")


def main():
    parser = argparse.ArgumentParser(
        description="Replay a trade through market context data",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Analyze a single trade from JSON
  uv run python3 replay_trade.py --result backtest_results.json

  # Analyze first trade from parquet
  uv run python3 replay_trade.py --result results.parquet --index 0

  # Analyze 5th trade from parquet with custom output
  uv run python3 replay_trade.py --result results.parquet --index 5 --output my_analysis.png

  # Specify data directory
  uv run python3 replay_trade.py --result results.json --data-dir ~/finq_data
        """
    )

    parser.add_argument("--result", type=str, required=True,
                       help="Path to trade result file (JSON or parquet)")
    parser.add_argument("--index", type=int, default=None,
                       help="Row index for parquet files (default: 0)")
    parser.add_argument("--output", type=str, default=None,
                       help="Output PNG file path (default: auto-generated)")
    parser.add_argument("--data-dir", type=Path, default=None,
                       help="finq data directory (default: ~/finq_data)")

    args = parser.parse_args()

    # Load trade
    try:
        trade = load_trade_from_file(args.result, args.index)
    except Exception as e:
        print(f"ERROR loading trade: {e}")
        sys.exit(1)

    # Create analyzer
    analyzer = TradeReplayAnalyzer(trade, args.data_dir)

    print(f"\n{'='*60}")
    print(f"Trade Replay Analyzer")
    print(f"{'='*60}")
    print(f"Symbol: {analyzer.symbol}")
    print(f"Entry:  {analyzer.entry_time}")
    print(f"Exit:   {analyzer.exit_time}")
    if analyzer.earnings_date:
        print(f"Earnings: {analyzer.earnings_date}")
    print()

    # Load market data
    if not analyzer.load_market_data():
        print("Could not load all required market data - visualization may be incomplete")

    # Generate visualization
    try:
        output = analyzer.plot_trade_analysis(args.output)
        print(f"\n✓ Trade replay analysis complete!")
    except Exception as e:
        print(f"ERROR generating visualization: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    main()
