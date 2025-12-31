#!/usr/bin/env python3
"""
Example usage of cs_rust Python bindings

This script demonstrates:
1. Black-Scholes pricing and Greeks calculation
2. Implied volatility calculation
3. Running a backtest
4. Analyzing results
"""

from cs_rust import (
    py_bs_price,
    py_bs_greeks,
    py_bs_implied_volatility,
    PyBacktestConfig,
    PyBacktestUseCase,
)


def example_black_scholes():
    """Demonstrate Black-Scholes functions"""
    print("=" * 60)
    print("Black-Scholes Pricing Example")
    print("=" * 60)

    # Parameters
    spot = 100.0
    strike = 100.0
    time_to_expiry = 0.25  # 3 months
    volatility = 0.20      # 20% IV

    # Price ATM call
    call_price = py_bs_price(
        spot=spot,
        strike=strike,
        time_to_expiry=time_to_expiry,
        volatility=volatility,
        is_call=True
    )

    # Price ATM put
    put_price = py_bs_price(
        spot=spot,
        strike=strike,
        time_to_expiry=time_to_expiry,
        volatility=volatility,
        is_call=False
    )

    print(f"\nSpot: ${spot:.2f}")
    print(f"Strike: ${strike:.2f}")
    print(f"Time to expiry: {time_to_expiry * 365:.0f} days")
    print(f"Volatility: {volatility:.1%}")
    print(f"\nCall price: ${call_price:.2f}")
    print(f"Put price: ${put_price:.2f}")

    # Calculate Greeks
    greeks = py_bs_greeks(
        spot=spot,
        strike=strike,
        time_to_expiry=time_to_expiry,
        volatility=volatility,
        is_call=True
    )

    print(f"\nGreeks (Call):")
    print(f"  Delta: {greeks.delta:.4f}")
    print(f"  Gamma: {greeks.gamma:.4f}")
    print(f"  Theta: {greeks.theta:.4f} (per day)")
    print(f"  Vega:  {greeks.vega:.4f} (per 1% IV)")
    print(f"  Rho:   {greeks.rho:.4f} (per 1% rate)")

    # Calculate implied volatility
    iv = py_bs_implied_volatility(
        option_price=call_price,
        spot=spot,
        strike=strike,
        time_to_expiry=time_to_expiry,
        is_call=True
    )

    print(f"\nImplied volatility from price ${call_price:.2f}: {iv:.2%}")


def example_backtest():
    """Demonstrate backtest execution"""
    print("\n" + "=" * 60)
    print("Backtest Example")
    print("=" * 60)

    # Configure backtest
    config = PyBacktestConfig(
        data_dir="data",  # Update this to your data directory
        entry_hour=9,
        entry_minute=35,
        exit_hour=15,
        exit_minute=55,
        min_short_dte=0,
        min_long_dte=7,
        min_iv_ratio=1.05,  # Long IV must be >= 5% higher than short
        parallel=True,
    )

    print(f"\nConfiguration: {config}")

    # Create backtest instance
    backtest = PyBacktestUseCase(config)

    # Run backtest (update dates to match your data)
    print("\nRunning backtest...")
    try:
        result = backtest.execute(
            start_date="2025-11-03",
            end_date="2025-11-04",
            option_type="call"
        )

        # Display summary
        print(f"\n{result}")
        print(f"\nSessions processed: {result.sessions_processed}")
        print(f"Total opportunities: {result.total_opportunities}")
        print(f"Trades entered: {result.total_entries}")
        print(f"Win rate: {result.win_rate():.2%}")
        print(f"Total P&L: ${result.total_pnl():.2f}")

        if result.total_entries > 0:
            print(f"Average P&L: ${result.avg_pnl():.2f}")

            # Show sample trades
            print(f"\nSample trades (first 5):")
            for i, trade in enumerate(result.results[:5], 1):
                winner = "✓" if trade.is_winner() else "✗"
                iv_ratio = trade.iv_ratio()
                iv_str = f"IV ratio: {iv_ratio:.2f}" if iv_ratio else "No IV data"
                print(f"  {i}. {winner} {trade.symbol} @ ${trade.strike:.2f} | "
                      f"P&L: ${trade.pnl:.2f} ({trade.pnl_pct:+.2f}%) | {iv_str}")
        else:
            print("\nNo trades were entered (no earnings events or filtered out)")

    except Exception as e:
        print(f"\nBacktest error: {e}")
        print("\nNote: Make sure your data_dir contains valid market data")
        print("      Update the date range to match available data")


def main():
    """Run all examples"""
    example_black_scholes()
    example_backtest()

    print("\n" + "=" * 60)
    print("For more examples, see the README.md")
    print("=" * 60)


if __name__ == "__main__":
    main()
