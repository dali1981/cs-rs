Great idea — this is exactly the kind of thing that should live in a spec.

Here’s a clean, implementation-ready Markdown specification you can drop into your project (e.g. docs/hedged_option_returns.md).

⸻


# Hedged Options Return & Capital Normalization Specification

## Purpose

Define a mathematically correct and implementation-safe method for:
- Computing PnL of delta-hedged option trades
- Accounting for hedge costs and financing
- Normalizing returns across different underlyings and option structures
- Producing comparable performance metrics (returns, Sharpe, drawdown)

This framework avoids common pitfalls such as:
- Ignoring hedge capital usage
- Overstating returns by dividing only by option premium
- Mixing incompatible return units across trades

---

## 1. Trade Components

For each trade *i*, track the following quantities over its lifetime:

### Option Leg
- `C_opt`: option premium paid (or max loss for spreads)
- `Option_PnL`: realized PnL of option at exit

### Hedge Leg (Delta Hedge)
At each hedge rebalance time *t*:
- `Delta_t`: option delta at time *t*
- `S_t`: underlying price at time *t*
- `Hedge_Notional_t = |Delta_t| × S_t × ContractMultiplier`
- `Hedge_PnL_t`: PnL from hedge trades
- `HedgeCost_t`: transaction cost of hedge rebalance

### Financing / Margin
- `h`: financing haircut (e.g. 0.25)
- `Hedge_Capital_t = h × Hedge_Notional_t`

---

## 2. Total PnL Computation

For each trade:

Total_PnL =
Option_PnL
•	Σ Hedge_PnL_t

	•	Σ HedgeCost_t

This value represents the full economic result of the strategy.

---

## 3. Capital at Risk Definition

The correct economic capital deployed is:

Capital_t = C_opt + Hedge_Capital_t

The trade’s capital requirement is defined as the maximum capital required at any time:

Capital_i = max_t (Capital_t)

This captures:
- option premium
- hedge margin usage
- financing constraints

---

## 4. Normalized Return Per Trade

Each trade’s normalized return is:

r_i = Total_PnL_i / Capital_i

This return is dimensionless and comparable across:
- different underlyings
- different option prices
- different hedge sizes
- different volatility regimes

---

## 5. Time Normalization (for Sharpe)

Let `T_i` be the duration of trade *i* in days.

Compute the daily-equivalent return:

r_i_daily = (1 + r_i)^(1 / T_i) - 1

---

## 6. Strategy Sharpe Ratio (Pre-Portfolio)

Using the series `{ r_i_daily }`:

Sharpe = mean(r_i_daily) / std(r_i_daily) × sqrt(252)

This Sharpe evaluates **strategy quality independent of position sizing**.

---

## 7. Optional Portfolio Construction (Later Stage)

For portfolio simulation with overlapping trades:

- Allocate a fixed fraction `f` of NAV to each trade’s `Capital_i`
- Daily NAV evolves as:

NAV_{t+1} = NAV_t + Σ PnL_{i,t}

From daily NAV, compute:
- daily returns
- portfolio Sharpe
- max drawdown
- CAGR

---

## 8. Diagnostic Metrics (Required)

For each trade:

HedgeCostRatio = (Σ HedgeCost_t) / C_opt

High values (>30–40%) indicate hedge friction destroying edge.

---

## Summary

This specification ensures:
- realistic capital usage
- honest performance metrics
- correct treatment of hedging and financing
- full comparability across assets and strategies


⸻

If you’d like, next I can help you design the data structures and unit tests for this so it becomes bulletproof in your backtester.