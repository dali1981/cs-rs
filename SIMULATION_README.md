## Trade Simulation System - README

A complete system for simulating option strategies across multiple market scenarios with delta hedging support. Includes validation tools to compare Python simulation results against Rust hedge implementation.

### What is This?

This is a **scenario testing and validation framework** that allows you to:

1. **Test strategies** across different market conditions
2. **Validate hedge calculations** against Rust implementation
3. **Analyze P&L attribution** (how much profit comes from theta, gamma, vega)
4. **Compare Greeks** calculations between Python and Rust
5. **Evaluate hedging effectiveness** (does delta hedging actually reduce directional risk?)

### Quick Start

#### Run Full Scenario Analysis

Test all built-in strategies (long/short calls/puts, strangles, spreads) across 5 scenarios:

```bash
# Basic run - uses defaults (spot=100, IV=25%, 30 days, GBM)
uv run python3 simulation/trade_simulator.py

# Custom parameters
uv run python3 simulation/trade_simulator.py --spot 105 --iv 0.30 --dte 45

# Use Heston volatility model instead of GBM
uv run python3 simulation/trade_simulator.py --model heston

# More accurate (slower) - 500 paths, 60 steps
uv run python3 simulation/trade_simulator.py --paths 500 --steps 60
```

**Output**:
- PNG files with 6-panel analysis plots for each strategy
- JSON files with numerical results for each scenario

#### Validate Against Rust

Compare Python Greeks and hedge decisions with your Rust implementation:

```bash
# Simple validation - no hedging
uv run python3 simulation/validate_rust_hedge.py --strategy long_call

# With delta hedging enabled
uv run python3 simulation/validate_rust_hedge.py --strategy long_strangle --hedge

# Custom market parameters
uv run python3 simulation/validate_rust_hedge.py \
    --strategy bull_call_spread \
    --spot 105 --iv 0.30 --dte 45 \
    --output ~/validation_results
```

**Output**:
- `validation_report.json` - Greeks validation, P&L attribution, hedging metrics
- `hedge_decisions.json` - Daily hedge decisions (for Rust comparison)

### How It Works

#### 1. Stock Simulation

Two models available:

**GBM (Geometric Brownian Motion)**
```
dS/S = μ dt + σ dW

Parameters:
- drift (μ) = expected return
- volatility (σ) = annualized standard deviation
```

**Heston (Stochastic Volatility)**
```
dS/S = μ dt + √v dW_S
dv = κ(θ - v) dt + ξ√v dW_v

Parameters:
- drift, initial vol, mean vol, vol-of-vol (ξ), mean reversion (κ), correlation (ρ)
```

#### 2. Black-Scholes Pricing & Greeks

Each day:
1. Calculate option price using Black-Scholes formula
2. Calculate Greeks: delta, gamma, vega, theta, rho
3. Sum across all legs to get position Greeks

#### 3. Delta Hedging

Optional automatic rehedging:
1. Calculate position delta each day
2. If delta drift > threshold, rehedge
3. Track cumulative hedge costs
4. Separate position P&L from hedge P&L

#### 4. Scenario Analysis

Run the simulation across 5 volatility scenarios:

| Scenario | IV vs RV | Use Case |
|----------|----------|----------|
| **IV = RV** | 1:1 | Fair value baseline |
| **IV > RV** | 1.2x | IV too high at entry (long vol loses) |
| **IV < RV** | 0.8x | IV too low at entry (long vol wins) |
| **IV Increases** | Linear +50% | Vol expands (long vol wins) |
| **IV Crush** | Linear -50% | Post-earnings IV collapse (short vol wins) |

#### 5. P&L Attribution

Daily P&L is decomposed into:

```
P&L ≈ Theta_daily + 0.5×Gamma×(ΔS)² + Vega×(ΔIV) + Delta×(ΔS) [if unhedged]
```

- **Theta**: Time decay (positive for short options)
- **Gamma**: Convexity profit (positive from large moves)
- **Vega**: Volatility change (positive for long options when IV increases)
- **Delta**: Directional (eliminated by hedging)

### Architecture

```
simulation/
├── core_simulator.py      # Stock price simulation (GBM, Heston)
├── black_scholes.py       # Option pricing and Greeks
├── strategy_simulator.py   # Generic multi-leg strategy engine
├── trade_simulator.py      # Main runner with visualizations
└── validate_rust_hedge.py  # Validation against Rust
```

**Key Classes**:
- `StockSimulator`: Generates price paths
- `BlackScholes`: Prices options and calculates Greeks
- `StrategySimulator`: Runs strategy over a price path
- `StrategyConfig`: Defines a strategy (legs, hedging, exit conditions)
- `SimulationResult`: Results with daily states and final P&L

### Integration with Rust Hedge Code

#### Step 1: Export Daily Decisions

The validation script exports daily hedge decisions to JSON:

```json
{
  "strategy": "Long Call (ATM)",
  "scenario": "IV equals RV",
  "decisions": [
    {
      "day": 0,
      "spot_price": 100.0,
      "option_delta": 0.3949,
      "hedge_shares": -39,  // To maintain delta-neutral position
      "greeks": {
        "delta": 0.3949,
        "gamma": 0.0414,
        "vega": 0.0873,
        "theta": -0.0001
      },
      "pnl": {
        "position": 0.0,
        "theta": 0.0,
        "gamma": 0.0,
        "vega": 0.0,
        "delta": 0.0
      }
    },
    // ... more days
  ]
}
```

#### Step 2: Load and Validate in Rust

```rust
// Load exported decisions
let decisions: HedgeDecisions = serde_json::from_str(&json)?;

// For each decision:
for decision in &decisions.decisions {
    // Calculate Rust Greeks for the same parameters
    let rust_delta = calculate_delta(
        S = decision.spot_price,
        K = strike,
        T = days_to_expiry / 365.0,
        r = risk_free_rate,
        sigma = implied_vol,
    )?;

    // Compare
    let error = (rust_delta - decision.greeks.delta).abs();
    assert!(error < 0.001, "Delta mismatch: Rust={}, Python={}", rust_delta, decision.greeks.delta);

    // Validate hedge decision
    let expected_hedge = -(rust_delta * spot * 100) as i32;  // 100 shares per contract
    assert_eq!(expected_hedge, decision.hedge_shares, "Hedge decision mismatch");
}
```

#### Step 3: Validate P&L Attribution

Compare observed P&L with Greeks-based attribution:

```rust
let attributed_pnl = theta_pnl + gamma_pnl + vega_pnl + delta_pnl;
let observed_pnl = final_position_value - entry_price;
let error = (attributed_pnl - observed_pnl).abs() / observed_pnl.abs();

assert!(error < 0.05, "P&L attribution error: {}%", error * 100);
```

### Python Usage Examples

#### Example 1: Create Custom Strategy

```python
from simulation import *

# Define long call at the money
config = StrategyConfig(
    name="Long Call",
    legs=[OptionLeg(
        option_type=OptionType.CALL,
        strike=100.0,
        expiration=30/365,     # Time to expiry from entry
        position_size=1.0,     # 1.0 = long, -1.0 = short
        quantity=1,            # 1 contract = 100 shares
    )],
    entry_price=3.50,         # Debit paid
    hedging_enabled=True,     # Enable delta hedging
    hedging_frequency=1,      # Rehedge daily
    hedging_threshold=0.05,   # If delta drift > 5%
    exit_condition=ExitCondition.HOLD_TO_EXPIRY,
)

# Simulate
from core_simulator import StockSimulator, GBMConfig
gbm = GBMConfig(spot_price=100, drift_rate=0.05, volatility=0.25)
path, _ = StockSimulator.simulate_gbm(gbm, 30/365, 30, num_paths=100)

simulator = StrategySimulator(config)
result = simulator.simulate(path, SCENARIO_IV_EQUALS_RV)

# Results
print(f"P&L: ${result.final_pnl:.2f}")
print(f"Max Loss: ${result.max_loss:.2f}")
print(f"Rehedges: {result.num_rehedges}")
```

#### Example 2: Multi-Leg Strategy (Short Strangle)

```python
# Short strangle = sell call + sell put (both OTM)
config = StrategyConfig(
    name="Short Strangle",
    legs=[
        OptionLeg(OptionType.CALL, strike=105, expiration=30/365, position_size=-1.0),
        OptionLeg(OptionType.PUT, strike=95, expiration=30/365, position_size=-1.0),
    ],
    entry_price=-2.50,  # Credit received
    hedging_enabled=False,  # Typically not hedged
)

# Run across scenarios
results = {}
for scenario in [SCENARIO_IV_EQUALS_RV, SCENARIO_IV_CRUSH]:
    result = simulator.simulate(path, scenario)
    results[scenario.name] = result

# Short strangle should profit in IV crush scenario
print(f"IV=RV: ${results['IV equals RV'].final_pnl:.2f}")
print(f"IV Crush: ${results['IV Crush'].final_pnl:.2f}")  # Should be higher
```

#### Example 3: Take Profit / Stop Loss

```python
# Exit when P&L reaches $200 (take profit)
config = StrategyConfig(
    name="Long Call - Take Profit",
    legs=[OptionLeg(OptionType.CALL, 100, 30/365, 1.0)],
    entry_price=3.50,
    exit_condition=ExitCondition.PROFIT_TARGET,
    exit_param=200,  # Target profit
)

# Or stop loss at $100
config = StrategyConfig(
    name="Long Call - Stop Loss",
    legs=[OptionLeg(OptionType.CALL, 100, 30/365, 1.0)],
    entry_price=3.50,
    exit_condition=ExitCondition.STOP_LOSS,
    exit_param=100,  # Max loss
)

# Exit after 10 days
config = StrategyConfig(
    name="Long Call - 10 Day Exit",
    legs=[OptionLeg(OptionType.CALL, 100, 30/365, 1.0)],
    entry_price=3.50,
    exit_condition=ExitCondition.TIME_TARGET,
    exit_param=10,  # Days
)
```

### Interpreting Results

#### Validation Report (validation_report.json)

```json
{
  "final_pnl": -3.06,           // Total P&L at end
  "max_loss": -3.06,            // Max drawdown
  "max_gain": 3.99,             // Max gain before exit

  "greeks_validation": {        // Compare Python vs Rust
    "delta": {
      "python_value": 0.7829,
      "rust_value": 0.7829,     // Should match
      "percent_error": 0.0      // Should be < 0.1%
    },
    // ... gamma, vega, theta
  },

  "pnl_attribution": {          // P&L breakdown
    "observed_pnl": -3.06,      // Actual P&L
    "theta_pnl": 0.00,          // From time decay
    "gamma_pnl": 1.48,          // From price moves
    "vega_pnl": 0.00,           // From IV changes
    "delta_pnl": -2.68,         // From direction (hedged = 0)
    "attributed_pnl": -1.21,    // Sum of above
    "attribution_error_pct": -60 // Error - check for large moves
  }
}
```

**Interpretation**:
- **Greeks validation**: All should be ~0% error. If > 0.1%, check Rust implementation
- **P&L Attribution Error**: < 5% is good. > 10% suggests large moves where Greeks approx breaks down
- **Theta P&L**: Should be negative for long options, positive for short
- **Gamma P&L**: Always positive (benefits from moves)
- **Vega P&L**: Positive for long vol, negative for short vol

#### Strategy Analysis Plots

6-panel visualization saved for each strategy:

1. **Stock Price Path**: Entry to exit with key levels
2. **Final P&L by Scenario**: Bar chart comparing scenarios
3. **P&L Over Time**: Evolution of position value
4. **Delta Evolution**: Changes as spot moves
5. **Max Loss/Gain**: Range by scenario
6. **P&L Components**: Cumulative theta, gamma, vega

### Command Reference

#### trade_simulator.py

```bash
Usage: uv run python3 simulation/trade_simulator.py [options]

Options:
  --spot SPOT              Initial spot price (default: 100)
  --iv IV                  Implied volatility (default: 0.25)
  --dte DTE                Days to expiration (default: 30)
  --rate RATE              Risk-free rate (default: 0.05)
  --model {gbm,heston}     Model (default: gbm)
  --steps STEPS            Simulation steps (default: 30)
  --paths PATHS            Monte Carlo paths (default: 100)
  --seed SEED              Random seed (default: 42)
  --output OUTPUT          Output directory

Examples:
  uv run python3 simulation/trade_simulator.py
  uv run python3 simulation/trade_simulator.py --spot 105 --iv 0.30 --paths 500
  uv run python3 simulation/trade_simulator.py --model heston --dte 45
```

#### validate_rust_hedge.py

```bash
Usage: uv run python3 simulation/validate_rust_hedge.py [options]

Options:
  --strategy {long_call,long_put,short_call,short_put,long_strangle,short_strangle,bull_call_spread}
  --spot SPOT              Spot price
  --iv IV                  Initial IV
  --dte DTE                Days to expiration
  --hedge                  Enable delta hedging
  --output OUTPUT          Output directory

Examples:
  uv run python3 simulation/validate_rust_hedge.py --strategy long_call
  uv run python3 simulation/validate_rust_hedge.py --strategy long_strangle --hedge
  uv run python3 simulation/validate_rust_hedge.py --strategy bull_call_spread --spot 105 --iv 0.30
```

### Files & Outputs

#### Input Files
- Strategy config (defined in code, not files)
- Random seed (for reproducibility)

#### Output Files
- `{strategy}_analysis.png` - 6-panel visualization
- `{strategy}_results.json` - Numerical results for all scenarios
- `validation_report.json` - Validation metrics
- `hedge_decisions.json` - Daily decisions for Rust comparison

### Troubleshooting

#### Q: P&L attribution error is huge (> 50%)

**A**: Large moves (>2%) in a day break the Greeks approximation. This is normal near expiration or with extreme moves.

#### Q: Delta doesn't match between Python and Rust

**A**: Check:
1. Same strike price (handle rounding)
2. Same time to expiry (account for day count conventions)
3. Same volatility (could be smile effects in Rust)
4. Same interest rate and dividend assumptions

#### Q: Hedge decisions don't make sense

**A**: Check:
1. Delta is calculated correctly (validate against Black-Scholes)
2. Hedge target = -delta * spot * 100 (100 shares per contract)
3. Only hedge if enabled in config

### Limitations

1. **European Options Only**: No early exercise
2. **Black-Scholes Model**: Flat IV surface (no volatility smile)
3. **No Transaction Costs**: Assumes perfect execution
4. **Constant Rates**: Interest rates, dividends don't change
5. **Daily Rebalancing**: Can't do intraday hedging

### Performance

- **100 paths, 30 steps**: ~2 seconds
- **500 paths, 60 steps**: ~10 seconds
- **1000 paths, 90 steps**: ~30 seconds

Use `--steps 15 --paths 50` for quick tests, `--steps 60 --paths 500` for accurate results.

### Next Steps

1. **Run Full Analysis**: `uv run python3 simulation/trade_simulator.py`
2. **Review Plots**: Open PNG files in `/tmp/simulation_results/`
3. **Validate**: `uv run python3 simulation/validate_rust_hedge.py --strategy long_call --hedge`
4. **Compare with Rust**: Load `hedge_decisions.json` into Rust validator
5. **Adjust Parameters**: Test different IV, spots, expirations

### Questions?

See `SIMULATION_GUIDE.md` for detailed documentation, examples, and API reference.
