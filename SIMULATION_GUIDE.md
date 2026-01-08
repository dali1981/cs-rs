## Trade Simulation & Scenario Analysis System

A comprehensive Python simulation system for testing option strategies across multiple market scenarios with full delta hedging support.

### Features

✅ **Stock Dynamics**
- Geometric Brownian Motion (GBM/BSM model)
- Heston Stochastic Volatility model
- Configurable drift, volatility, mean reversion

✅ **Option Pricing & Greeks**
- Black-Scholes European option pricing
- Full Greeks: delta, gamma, vega, theta, rho
- Implied volatility solver
- Daily Greeks calculation with position tracking

✅ **Strategy Support**
- Single leg strategies (long/short calls/puts)
- Multi-leg strategies (strangles, spreads, etc.)
- Fully generic - can use any existing OptionStrategies from Rust
- Customizable exit conditions (hold to expiry, take profit, stop loss, time target)

✅ **Scenario Analysis**
- **IV = RV**: Realized volatility matches implied volatility
- **IV > RV**: Implied vol higher than realized (vega profit scenario)
- **IV < RV**: Implied vol lower than realized (vega loss scenario)
- **IV Increases**: Constant increase in IV over the trade lifetime
- **IV Crush**: IV decreases over time (post-earnings scenario)

✅ **Delta Hedging**
- Automatic delta hedge position sizing
- Configurable rehedge frequency (e.g., daily, every N days)
- Delta drift threshold-based rehedging
- Hedge cost tracking and rehedge counting
- Full P&L separation: position vs. hedge P&L

✅ **P&L Attribution**
- Greeks-based P&L decomposition
- Theta P&L (time decay)
- Gamma P&L (convexity, directional moves)
- Vega P&L (volatility changes)
- Delta P&L (directional, usually hedged)

✅ **Visualization**
- 6-panel analysis plots per strategy:
  1. Stock price paths with key levels
  2. Final P&L by scenario (bar chart)
  3. P&L evolution over time
  4. Delta evolution
  5. Max loss/gain comparison
  6. P&L components breakdown (Theta, Gamma, Vega)

✅ **Validation Against Rust**
- Export hedge decisions to JSON for Rust comparison
- Greeks calculation validation
- P&L attribution accuracy checking
- Hedging effectiveness metrics

---

## Quick Start

### Installation

```bash
# Sync dependencies
uv sync
```

### Run Full Analysis

```bash
# Simulate all strategies across all scenarios
uv run python3 simulation/trade_simulator.py

# Custom parameters
uv run python3 simulation/trade_simulator.py \
    --spot 105 \
    --iv 0.30 \
    --dte 45 \
    --model gbm \
    --output ~/my_results
```

### Validate Against Rust

```bash
# Validate a single strategy
uv run python3 simulation/validate_rust_hedge.py --strategy long_call

# With delta hedging
uv run python3 simulation/validate_rust_hedge.py \
    --strategy long_strangle \
    --hedge \
    --spot 105 \
    --iv 0.30
```

---

## Architecture

### Module Structure

```
simulation/
├── __init__.py                 # Package exports
├── core_simulator.py           # GBM and Heston models
├── black_scholes.py            # Option pricing and Greeks
├── strategy_simulator.py        # Generic strategy simulation engine
├── trade_simulator.py          # Main runner with visualization
└── validate_rust_hedge.py      # Validation against Rust
```

### Key Classes

#### `StockSimulator`
Simulates stock prices using GBM or Heston models.

```python
# GBM simulation
gbm_config = GBMConfig(
    spot_price=100.0,
    drift_rate=0.05,           # mu
    volatility=0.25,           # sigma
)
path, all_paths = StockSimulator.simulate_gbm(
    gbm_config,
    time_to_expiry=30/365,
    num_steps=30,
    num_paths=100,
    random_seed=42,
)

# Heston simulation
heston_config = HestonConfig(
    spot_price=100.0,
    drift_rate=0.05,
    initial_variance=0.25**2,
    mean_variance=0.25**2,
    variance_of_variance=0.1,  # vol of vol
    mean_reversion=5.0,        # kappa
    rho=-0.5,                  # correlation
)
path, all_paths = StockSimulator.simulate_heston(heston_config, ...)
```

#### `BlackScholes`
European option pricing and Greeks.

```python
# Price a call option
price = BlackScholes.price(
    S=105,
    K=100,
    T=0.082,               # 30 days
    r=0.05,
    sigma=0.25,
    option_type=OptionType.CALL,
)

# Calculate Greeks
greeks = calculate_greeks(S=105, K=100, T=0.082, r=0.05, sigma=0.25, option_type=OptionType.CALL)
print(f"Delta: {greeks.delta:.4f}")
print(f"Gamma: {greeks.gamma:.6f}")
print(f"Vega: {greeks.vega:.4f}")
print(f"Theta: {greeks.theta:.4f}")
```

#### `StrategySimulator`
Simulates a strategy over a stock price path with optional delta hedging.

```python
# Define a strategy
config = StrategyConfig(
    name="Long Call (ATM)",
    legs=[OptionLeg(
        option_type=OptionType.CALL,
        strike=100.0,
        expiration=30/365,  # Time to expiry from entry
        position_size=1.0,  # 1.0 = long, -1.0 = short
        quantity=1,
    )],
    entry_price=3.50,  # Debit paid
    hedging_enabled=True,
    hedging_frequency=1,       # Rehedge daily
    hedging_threshold=0.05,    # If delta drift > 0.05
    exit_condition=ExitCondition.HOLD_TO_EXPIRY,
    max_loss=500,  # Max acceptable loss
)

# Run simulation
simulator = StrategySimulator(config)
result = simulator.simulate(path, SCENARIO_IV_EQUALS_RV)

# Access results
print(f"Final P&L: ${result.final_pnl:.2f}")
print(f"Max Loss: ${result.max_loss:.2f}")
print(f"Max Gain: ${result.max_gain:.2f}")
print(f"Num Rehedges: {result.num_rehedges}")

# Inspect daily states
for state in result.daily_states:
    print(f"Day {state.day}: Spot=${state.spot_price:.2f}, Delta={state.delta:.4f}, P&L=${state.position_pnl:.2f}")
```

#### `SimulationResult`
Complete result of a simulation run.

```python
# Properties
result.final_pnl           # Final unrealized P&L
result.final_pnl_pct       # P&L as percentage
result.max_loss            # Maximum drawdown
result.max_gain            # Maximum profit
result.realized_volatility  # Realized vol of path
result.num_rehedges        # Number of hedge adjustments
result.exit_day            # Day position was exited (if early exit)
result.exit_reason         # Reason for exit

# Access daily states
for state in result.daily_states:
    state.day
    state.spot_price
    state.implied_volatility
    state.time_to_expiry
    state.delta / state.gamma / state.vega / state.theta
    state.position_price
    state.position_pnl
    state.position_pnl_pct
    state.pnl_theta / state.pnl_gamma / state.pnl_vega / state.pnl_delta
    state.hedge_shares
    state.hedge_cost
    state.hedge_pnl
```

---

## Usage Examples

### Example 1: Simple Long Call

```python
from simulation import *
from simulation.black_scholes import OptionType

# Setup
spot = 100
iv = 0.25

# Create strategy
call_price = BlackScholes.price(spot, spot, 30/365, 0.05, iv, OptionType.CALL)

config = StrategyConfig(
    name="Long Call",
    legs=[OptionLeg(OptionType.CALL, spot, 30/365, 1.0)],
    entry_price=call_price,
    hedging_enabled=False,  # No hedging
)

# Simulate
gbm_config = GBMConfig(spot, iv, iv)
path, _ = StockSimulator.simulate_gbm(gbm_config, 30/365, 30, 100, random_seed=42)

simulator = StrategySimulator(config)

# Run across scenarios
scenarios = [
    SCENARIO_IV_EQUALS_RV,
    SCENARIO_IV_GREATER_RV,
    SCENARIO_IV_LESS_RV,
]

for scenario in scenarios:
    result = simulator.simulate(path, scenario)
    print(f"{scenario.name}: P&L = ${result.final_pnl:.2f}")
```

### Example 2: Delta Hedged Long Call

```python
config = StrategyConfig(
    name="Long Call (Delta Hedged)",
    legs=[OptionLeg(OptionType.CALL, spot, 30/365, 1.0)],
    entry_price=call_price,
    hedging_enabled=True,
    hedging_frequency=1,        # Daily rehedge
    hedging_threshold=0.05,     # Rehedge if delta drift > 5%
    exit_condition=ExitCondition.HOLD_TO_EXPIRY,
    max_loss=100,  # Stop loss at $100
)

simulator = StrategySimulator(config)
result = simulator.simulate(path, SCENARIO_IV_GREATER_RV)

# Hedging reduces delta P&L, exposes gamma/vega
print(f"P&L Theta: ${sum(s.pnl_theta for s in result.daily_states):.2f}")
print(f"P&L Gamma: ${sum(s.pnl_gamma for s in result.daily_states):.2f}")
print(f"P&L Vega: ${sum(s.pnl_vega for s in result.daily_states):.2f}")
print(f"Rehedges: {result.num_rehedges}")
print(f"Hedge Cost: ${result.daily_states[-1].hedge_cost:.2f}")
```

### Example 3: Short Strangle (Theta/Vega Strategy)

```python
atm_strike = spot
otm_call = spot * 1.05
otm_put = spot * 0.95

call_price = BlackScholes.price(spot, otm_call, 30/365, 0.05, iv, OptionType.CALL)
put_price = BlackScholes.price(spot, otm_put, 30/365, 0.05, iv, OptionType.PUT)

config = StrategyConfig(
    name="Short Strangle",
    legs=[
        OptionLeg(OptionType.CALL, otm_call, 30/365, -1.0),
        OptionLeg(OptionType.PUT, otm_put, 30/365, -1.0),
    ],
    entry_price=-(call_price + put_price),  # Credit received
    hedging_enabled=False,
)

# Test in IV crush scenario
result = simulator.simulate(path, SCENARIO_IV_CRUSH)
# Short strangle profits from IV crush and time decay (theta)
```

### Example 4: Custom Exit Conditions

```python
# Take profit at $200
config = StrategyConfig(
    name="Long Call - Take Profit $200",
    legs=[OptionLeg(OptionType.CALL, spot, 30/365, 1.0)],
    entry_price=call_price,
    exit_condition=ExitCondition.PROFIT_TARGET,
    exit_param=200,  # Exit when P&L reaches $200
)

# Stop loss at $100
config = StrategyConfig(
    name="Long Call - Stop Loss $100",
    legs=[OptionLeg(OptionType.CALL, spot, 30/365, 1.0)],
    entry_price=call_price,
    exit_condition=ExitCondition.STOP_LOSS,
    exit_param=100,  # Exit when P&L < -$100
)

# Exit after 10 days
config = StrategyConfig(
    name="Long Call - 10 Day Exit",
    legs=[OptionLeg(OptionType.CALL, spot, 30/365, 1.0)],
    entry_price=call_price,
    exit_condition=ExitCondition.TIME_TARGET,
    exit_param=10,  # Exit after 10 days
)
```

---

## Volatility Scenarios

### SCENARIO_IV_EQUALS_RV
Implied volatility matches realized volatility throughout the trade.
- **Use case**: Baseline scenario, fair value pricing
- **P&L drivers**: Gamma (directional) + Theta (time decay)

```python
result = simulator.simulate(path, SCENARIO_IV_EQUALS_RV)
# Profit from: favorable gamma moves + theta decay
# Loss from: unfavorable gamma moves - hedge costs
```

### SCENARIO_IV_GREATER_RV
Implied volatility is higher than realized volatility (20% higher).
- **Use case**: Long volatility positions win
- **P&L drivers**: Vega profit (IV crush relative to expectation) + Theta

```python
result = simulator.simulate(path, SCENARIO_IV_GREATER_RV)
# Long vega positions (long call/put) profit from IV being too high at entry
# Short vega positions (short call/put) lose
```

### SCENARIO_IV_LESS_RV
Implied volatility is lower than realized volatility (20% lower).
- **Use case**: Short volatility positions win
- **P&L drivers**: Vega loss (IV expansion) + Theta

```python
result = simulator.simulate(path, SCENARIO_IV_LESS_RV)
# Short vega positions (short strangle) profit from higher realized vol
# Long vega positions lose from vol expansion
```

### SCENARIO_IV_INCREASES
Implied volatility increases linearly over time (50% over trade life).
- **Use case**: Vol term structure, earnings, macro uncertainty
- **P&L drivers**: Vega profit for long vol, Vega loss for short vol

```python
result = simulator.simulate(path, SCENARIO_IV_INCREASES)
# Long volatility positions profit
# IV crush positions lose from rising IV
```

### SCENARIO_IV_CRUSH
Implied volatility decreases linearly over time (50% over trade life).
- **Use case**: Post-earnings scenarios, vol term structure inversion
- **P&L drivers**: Vega loss for long vol, Vega profit for short vol

```python
result = simulator.simulate(path, SCENARIO_IV_CRUSH)
# Short volatility positions (short strangles) profit from IV crush
# Long volatility positions lose
```

---

## Command Line Usage

### trade_simulator.py - Full Scenario Analysis

Runs simulations across all strategies and scenarios with visualizations.

```bash
# Default parameters
uv run python3 simulation/trade_simulator.py

# Custom spot and IV
uv run python3 simulation/trade_simulator.py --spot 105 --iv 0.30

# 45 days to expiry
uv run python3 simulation/trade_simulator.py --dte 45

# Heston model instead of GBM
uv run python3 simulation/trade_simulator.py --model heston

# More accurate simulation (more paths/steps)
uv run python3 simulation/trade_simulator.py --paths 500 --steps 60

# Save to specific directory
uv run python3 simulation/trade_simulator.py --output ~/my_analysis
```

**Output Files:**
- `{strategy_name}_results.json` - Numerical results for each scenario
- `{strategy_name}_analysis.png` - 6-panel visualization

### validate_rust_hedge.py - Validate Against Rust Implementation

Compares Python Greeks calculations and hedge decisions with Rust.

```bash
# Basic validation
uv run python3 simulation/validate_rust_hedge.py --strategy long_call

# With delta hedging
uv run python3 simulation/validate_rust_hedge.py --strategy long_strangle --hedge

# Custom parameters
uv run python3 simulation/validate_rust_hedge.py \
    --strategy bull_call_spread \
    --spot 105 \
    --iv 0.30 \
    --dte 45 \
    --output ~/validation_results
```

**Output Files:**
- `validation_report.json` - Detailed validation metrics
- `hedge_decisions.json` - Daily hedge decisions for Rust comparison

---

## Integrating with Existing OptionStrategies

The strategy simulator is generic and can work with any option strategy:

```python
from simulation import StrategySimulator, StrategyConfig, OptionLeg, OptionType

# Define strategy from your domain model
def create_strategy_from_rust_trade(rust_trade_result) -> StrategyConfig:
    """Convert Rust trade result to simulation config."""

    legs = []
    for leg in rust_trade_result.legs:
        legs.append(OptionLeg(
            option_type=OptionType.CALL if leg.option_type == "call" else OptionType.PUT,
            strike=float(leg.strike),
            expiration=leg.days_to_expiry / 365,
            position_size=1.0 if leg.direction == "long" else -1.0,
            quantity=leg.quantity,
        ))

    return StrategyConfig(
        name=rust_trade_result.strategy_name,
        legs=legs,
        entry_price=float(rust_trade_result.entry_cost),
        hedging_enabled=False,  # Or set from config
    )

# Now you can simulate it
rust_trade = fetch_rust_trade(...)
config = create_strategy_from_rust_trade(rust_trade)
simulator = StrategySimulator(config)

# Run across scenarios
from core_simulator import StockSimulator, GBMConfig
gbm_config = GBMConfig(spot=100, drift_rate=0.05, volatility=0.25)
path, _ = StockSimulator.simulate_gbm(gbm_config, 30/365, 30, 100, random_seed=42)

for scenario in [SCENARIO_IV_EQUALS_RV, SCENARIO_IV_GREATER_RV, ...]:
    result = simulator.simulate(path, scenario)
    print(f"{scenario.name}: P&L = ${result.final_pnl:.2f}")
```

---

## P&L Attribution & Greeks

### Understanding the Breakdown

Each day, the simulator calculates P&L contributions:

```
Daily P&L ≈ Theta*dt + 0.5*Gamma*dS² + Vega*dIV + Delta*dS (if unhedged)
```

**Theta P&L**: `theta_greek * days_elapsed`
- Time decay contribution
- Usually positive for short options, negative for long

**Gamma P&L**: `0.5 * gamma * (spot_move)²`
- Convexity/realized volatility contribution
- Always positive (benefits from large moves in either direction)

**Vega P&L**: `vega * (iv_change)`
- Volatility change contribution
- Positive for long options (benefit from IV increase), negative for short

**Delta P&L**: `delta * spot_move` (when not hedged)
- Directional contribution
- Usually hedged to isolate vega and gamma effects

### Validation

P&L attribution error = |Observed P&L - Attributed P&L| / Observed P&L

The simulator aims for < 5% attribution error through:
- Accurate Greeks calculation using Black-Scholes
- Daily P&L decomposition
- Proper handling of expired options (intrinsic value)

---

## Performance & Scalability

### Computational Complexity

- **Path simulation**: O(num_paths * num_steps)
- **Strategy evaluation**: O(num_strategies * num_scenarios * num_steps * num_legs)
- **Greeks calculation**: O(num_legs) per day

### Typical Runtime

| Configuration | Time |
|---------------|------|
| 100 paths, 30 steps, 7 strategies, 5 scenarios | ~2 seconds |
| 500 paths, 60 steps, 7 strategies, 5 scenarios | ~10 seconds |
| 1000 paths, 90 steps, 7 strategies, 5 scenarios | ~30 seconds |

### Memory Usage

- Minimal: ~1MB per 1000 paths
- Visualizations: PNG files ~200KB each

---

## Limitations & Future Enhancements

### Current Limitations

1. **European Options Only**: No early exercise (American options)
2. **No Transaction Costs**: Hedging assumes zero bid-ask spread (can add markup)
3. **No Slippage**: Assumes execution at mark price
4. **Flat Rates**: Risk-free rate and dividends are constant
5. **No Volatility Smile**: Uses flat vol surface (can extend to SVI parameterization)

### Planned Enhancements

1. **Transaction Costs**: Add bid-ask spreads and slippage models
2. **Volatility Surface**: Parametric vol surfaces (SVI, SABR)
3. **American Options**: Early exercise optimization
4. **Jump Processes**: Merton jumps or Lévy processes
5. **Machine Learning**: Greeks via neural networks for faster computation
6. **Distributed Simulation**: Parallel Monte Carlo across clusters
7. **Exact P&L**: Path-dependent P&L instead of Greeks approximation

---

## Troubleshooting

### Issue: "No module named 'scipy'"

**Solution**: Run `uv sync` to install all dependencies

```bash
uv sync
```

### Issue: Results look wrong / P&L not realistic

**Checklist**:
1. Verify entry price matches Black-Scholes pricing
2. Check that stock path is reasonable (returns in [-50%, +50%] for 30 days)
3. Verify IV scenario makes sense (should range from 0.15x to 1.5x initial IV)
4. Check for expired legs (P&L may be unintuitive if legs expire early)

### Issue: Attribution error > 10%

**Possible causes**:
1. Very large spot moves (Greeks approximation less accurate)
2. Close to expiration (higher order Greeks matter)
3. Deep ITM/OTM options (delta near ±1)

**Solution**: Use more simulation steps for better granularity

```bash
uv run python3 simulation/trade_simulator.py --steps 60
```

---

## References

### Black-Scholes & Options Theory
- Hull, J. (2018). Options, Futures, and Other Derivatives
- Black, F., & Scholes, M. (1973). "The pricing of options and corporate liabilities"
- Merton, R. C. (1973). "Theory of rational option pricing"

### Volatility Modeling
- Heston, S. L. (1993). "A closed-form solution for options with stochastic volatility"
- Gatheral, J. (2006). The Volatility Surface

### Delta Hedging
- Derman, E. (2016). My Life as a Quant
- Nassim N. Taleb (1997). Dynamic Hedging

---

## Contributing

To add new scenarios or models:

1. **New Scenario**: Add to `core_simulator.py` in SCENARIO_* definitions
2. **New Model**: Extend `StockSimulator` with new `simulate_*` method
3. **New Greeks**: Extend `BlackScholes` class
4. **New Strategy**: Add to `TradeSimulator.create_strategies()`

---

**Questions?** Check examples in the Quick Start or see detailed docstrings in the code.
