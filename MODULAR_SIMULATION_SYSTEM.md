# Modular Simulation System - Complete Documentation

**Date**: January 8, 2026
**Status**: ✅ Complete and Production-Ready
**Version**: 2.0 - Refactored for Modularity and Extensibility

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Module Reference](#module-reference)
4. [Quick Start](#quick-start)
5. [Examples](#examples)
6. [Advanced Usage](#advanced-usage)
7. [Configuration](#configuration)
8. [Visualization](#visualization)
9. [API Reference](#api-reference)

---

## Overview

The Modular Simulation System is a comprehensive framework for running Monte Carlo simulations of option strategies with support for:

✅ **Multiple Strategies**: Single legs (calls, puts) and multi-leg strategies (straddles, spreads, condors)
✅ **Scenario Testing**: IV equals RV, IV > RV, IV < RV, IV changes (crush/spike)
✅ **Hedging Modes**: No hedge, daily hedging, weekly hedging, threshold-based hedging
✅ **P&L Distribution Analysis**: Mean, std dev, percentiles, win rates, Sharpe ratios
✅ **Visualization**: Histograms, box plots, comparisons, CDF plots
✅ **Easy Configuration**: Presets for common setups, or custom configurations
✅ **Proper Scaling**: All P&L and Greeks in contract terms (×100)

### Key Improvements Over v1

| Aspect | v1 | v2 |
|--------|----|----|
| **Architecture** | Single monolithic file | 5 focused modules |
| **Configuration** | Hard to change | Flexible config classes + presets |
| **Scenarios** | Limited options | Fully configurable with presets |
| **Hedging** | Fixed parameters | Multiple modes with adjustable parameters |
| **Visualization** | None | Full matplotlib integration |
| **P&L Scaling** | Incorrect (per-share) | Correct (contract terms ×100) |
| **Greeks Scaling** | Per-share (0.54) | Contract terms (54) |
| **Extensibility** | Hard to extend | Easy to add strategies/scenarios |

---

## Architecture

### Module Dependency Graph

```
┌─────────────┐
│   config.py │  ← Configuration classes, presets, builders
├─────────────┴─────────────────────────────────────────┐
│                                                         │
├─────────────┬──────────────┬──────────────┬───────────┤
│  engine.py  │aggregator.py │  plotter.py  │run_sim.py │
│  (Runner)   │ (Analysis)   │ (Visual)     │ (CLI)     │
└─────────────┴──────────────┴──────────────┴───────────┘
```

### File Organization

```
simulation/
├── config.py                 (600 lines) - Configuration system
├── engine.py                 (650 lines) - Simulation engine
├── aggregator.py             (400 lines) - Results analysis
├── plotter.py                (500 lines) - Visualization
├── run_simulation.py          (300 lines) - CLI interface
│
├── (existing v1 files)
├── strategy_simulator_v2.py   (refactored, correct formula)
├── pedagogical_reporter.py    (reporting)
└── black_scholes.py          (pricing & Greeks)
```

---

## Module Reference

### 1. `config.py` - Configuration System

**Purpose**: Define all simulation parameters in a structured, reusable way.

**Key Classes**:

#### `MarketConfig`
```python
market = MarketConfig(
    spot_price=100.0,
    risk_free_rate=0.05,
    dividend_yield=0.0,
    entry_iv=0.25,
)
```

#### `OptionLegConfig`
```python
leg = OptionLegConfig(
    option_type=OptionType.CALL,
    direction=PositionDirection.LONG,
    quantity=1,
    expiration_days=30,
    strike_pct=1.0,  # ATM (1.0 = 100% of spot)
)
```

#### `StrategyConfig`
```python
strategy = StrategyConfig(
    name="Long Call (ATM)",
    legs=(leg1, leg2),
    entry_price=3.06,  # Optional; computed from BS if None
)
```

#### `ScenarioConfig`
```python
scenario = ScenarioConfig(
    name="IV > RV (25% less RV)",
    scenario_type=ScenarioType.IV_GREATER_RV,
    realized_vol_multiplier=0.75,  # RV = IV × 0.75
)
```

#### `HedgingConfig`
```python
hedging = HedgingConfig(
    mode=HedgingMode.DAILY,
    threshold=0.05,        # 5% delta drift threshold
    frequency=1,           # Rehedge daily
)
```

#### `SimulationConfig`
```python
config = SimulationConfig(
    market=market,
    strategy=strategy,
    scenarios=(scenario1, scenario2),
    hedging_modes=(hedge_none, hedge_daily),
    simulation=SimulationParams(num_simulations=1000, num_days=30),
)
```

**Presets Available**:

```python
# Strategies
StrategyPresets.long_call_atm()
StrategyPresets.short_call_atm()
StrategyPresets.long_straddle_atm()
StrategyPresets.bull_call_spread(width_pct=0.05)
StrategyPresets.iron_condor(wing_width_pct=0.10)

# Scenarios
ScenarioPresets.iv_equals_rv()
ScenarioPresets.iv_greater_rv(multiplier=0.75)
ScenarioPresets.iv_less_rv(multiplier=1.25)
ScenarioPresets.iv_crush(crush_rate=0.5)
ScenarioPresets.all_standard()

# Hedging
HedgingPresets.no_hedge()
HedgingPresets.daily_hedge()
HedgingPresets.weekly_hedge()
HedgingPresets.threshold_hedge(threshold=0.05)
HedgingPresets.all_modes()
```

**Quick Config Builder**:

```python
config = quick_config(
    strategy="long_call",
    spot=100.0,
    iv=0.25,
    days=30,
    num_sims=1000,
    scenarios="all",        # or "standard", "iv_equals_rv", etc.
    hedging="both",         # or "none", "daily", "all"
    seed=42,
)
```

---

### 2. `engine.py` - Simulation Engine

**Purpose**: Run Monte Carlo simulations with Black-Scholes pricing and delta hedging.

**Key Classes**:

#### `SimulationEngine`
```python
engine = SimulationEngine(progress_bar=True)
results = engine.run(config)  # Returns List[AggregatedResults]
```

**Features**:
- Generates GBM stock price paths with scenario-specific volatility
- Prices options using Black-Scholes formula
- Calculates Greeks (delta, gamma, vega, theta)
- Simulates delta hedging with configurable modes
- Tracks P&L through entire position
- Computes realized volatility from paths

**Output**:

Each run produces an `AggregatedResults` object:

```python
result.mean_pnl              # Average P&L across simulations
result.std_pnl               # Standard deviation
result.median_pnl            # Median P&L
result.min_pnl, result.max_pnl  # Range
result.pnl_5th, pnl_95th     # Percentiles
result.win_rate              # Probability of profit
result.sharpe_ratio          # Risk-adjusted return
result.pnls                  # Array of all individual P&Ls
```

**Internal Scaling** (Important!):

- **Stock paths**: Generated with scenario-specific realized volatility
- **Option prices**: Computed per-share from Black-Scholes
- **P&L reported**: In contract terms (×100 shares)
- **Greeks reported**: In contract terms (delta × 100, etc.)
- **Hedge shares**: Integer number of shares to short/long

---

### 3. `aggregator.py` - Results Analysis

**Purpose**: Aggregate and analyze simulation results across different configurations.

**Key Class**:

#### `ResultsAggregator`
```python
agg = ResultsAggregator(results)

# Summary statistics
agg.summary_table()

# Compare configurations
comparisons = agg.compare_hedging()
comparisons = agg.compare_scenarios()

# Find best/worst
best = agg.best_configuration("sharpe_ratio")
worst = agg.worst_configuration("sharpe_ratio")

# Rank by metric
ranked = agg.rank_by_metric("win_rate")

# Detailed distribution stats
for stats in agg.distribution_stats():
    print(stats.summary())
```

**Distribution Statistics** (`DistributionStats`):

```
- Central tendency: mean, median, mode
- Dispersion: std, variance, IQR, range
- Shape: skewness, kurtosis
- Percentiles: 1%, 5%, 10%, 25%, 50%, 75%, 90%, 95%, 99%
- Risk metrics: VaR (95%), CVaR, P(Loss), E[Loss|Loss]
```

**Grouping & Comparison**:

```python
# Get P&Ls grouped by category
pnls_by_hedging = agg.get_pnls_by_group("hedging")
pnls_by_scenario = agg.get_pnls_by_group("scenario")
pnls_by_strategy = agg.get_pnls_by_group("strategy")
```

---

### 4. `plotter.py` - Visualization

**Purpose**: Generate publication-quality plots of simulation results.

**Key Class**:

#### `SimulationPlotter`
```python
plotter = SimulationPlotter(results, figsize=(12, 8))

# Individual plots
plotter.plot_pnl_distribution()
plotter.plot_pnl_comparison(group_by="hedging")
plotter.plot_boxplot(group_by="scenario")
plotter.plot_hedging_comparison(scenario="IV = RV")
plotter.plot_scenario_comparison(hedging_mode="none")
plotter.plot_summary_metrics()
plotter.plot_cumulative_distribution(group_by="hedging")
plotter.plot_percentile_comparison()

# Generate and save all plots
plotter.plot_all(output_dir="./plots", show=True)
plotter.save_all("./plots", format="png", dpi=150)
```

**Plot Types**:
1. **Histograms**: P&L distributions with mean/median lines
2. **Box Plots**: Quartiles and outliers
3. **Comparisons**: Side-by-side hedging/scenario analysis
4. **Summary Metrics**: Multi-panel overview (mean, win rate, Sharpe, std)
5. **CDF Plots**: Cumulative distribution functions
6. **Percentile Charts**: 5th/25th/50th/75th/95th percentiles

**Color Schemes**:
- Hedging modes: Red (none), Green (daily), Blue (weekly)
- Scenarios: Specific colors for each scenario type
- Automatic palette generation for custom categories

---

### 5. `run_simulation.py` - CLI Interface

**Purpose**: Command-line interface for running simulations with various options.

**Usage**:

```bash
# Quick simulation
uv run simulation/run_simulation.py --strategy long_call --num-sims 1000

# Full comparison
uv run simulation/run_simulation.py \
    --strategy long_call \
    --num-sims 5000 \
    --scenarios all \
    --hedging both \
    --output-dir ./results \
    --save-plots

# Custom parameters
uv run simulation/run_simulation.py \
    --strategy iron_condor \
    --spot 150 \
    --iv 0.30 \
    --days 45 \
    --num-sims 2000 \
    --seed 42
```

**Arguments**:

```
Strategy Options:
  --strategy {long_call, short_call, long_put, short_put,
              long_straddle, short_straddle, bull_call_spread, iron_condor}

Market Parameters:
  --spot FLOAT              Initial spot price (default: 100)
  --iv FLOAT                Entry IV (default: 0.25)
  --days INT                Days to expiration (default: 30)

Simulation:
  --num-sims INT            Number of Monte Carlo simulations (default: 1000)
  --seed INT                Random seed for reproducibility

Scenarios:
  --scenarios {all, standard, iv_equals_rv, iv_greater_rv, iv_less_rv}

Hedging:
  --hedging {none, daily, weekly, both, all}

Output:
  --output-dir PATH         Save results to directory
  --plot                    Display plots
  --save-plots              Save plots to output directory
  --quiet                   Suppress progress output
```

**Example Functions** (in code):

```python
# Run examples
uv run simulation/run_simulation.py --examples
```

Available examples:
1. Quick simulation
2. Custom configuration
3. Strategy comparison
4. Full simulation with plots

---

## Quick Start

### Minimal Example (5 lines)

```python
from config import quick_config
from engine import SimulationEngine
from aggregator import ResultsAggregator

config = quick_config(strategy="long_call", num_sims=1000)
engine = SimulationEngine()
results = engine.run(config)
agg = ResultsAggregator(results)
print(agg.summary_table())
```

### With Hedging Comparison

```python
from engine import run_quick_simulation
from aggregator import ResultsAggregator

results = run_quick_simulation(
    strategy="long_call",
    num_sims=1000,
    scenarios="standard",
    hedging="both",
)

agg = ResultsAggregator(results)
print(agg.summary_table())

# Compare
for comp in agg.compare_hedging():
    print(comp.summary())
```

### With Visualization

```python
from config import quick_config
from engine import SimulationEngine
from plotter import SimulationPlotter

config = quick_config(strategy="long_call", num_sims=1000, scenarios="all", hedging="both")
engine = SimulationEngine()
results = engine.run(config)
plotter = SimulationPlotter(results)
plotter.plot_all(show=True)
```

---

## Examples

### Example 1: Long Call vs Short Call

```python
from engine import run_quick_simulation
from aggregator import ResultsAggregator

# Run both
long_results = run_quick_simulation(
    strategy="long_call",
    num_sims=2000,
    scenarios="all",
)

short_results = run_quick_simulation(
    strategy="short_call",
    num_sims=2000,
    scenarios="all",
)

# Combine and analyze
all_results = long_results + short_results
agg = ResultsAggregator(all_results)

# Display summary
print(agg.summary_table())

# Best performer
best = agg.best_configuration("sharpe_ratio")
print(f"Best: {best.strategy_name} | {best.scenario_name} | Sharpe: {best.sharpe_ratio:.2f}")
```

### Example 2: Hedging Effectiveness

```python
from config import quick_config
from engine import SimulationEngine
from aggregator import ResultsAggregator

# Long call with and without hedging
config = quick_config(
    strategy="long_call",
    num_sims=5000,
    scenarios="all",
    hedging="both",  # Both hedged and unhedged
)

results = SimulationEngine().run(config)
agg = ResultsAggregator(results)

# Compare
print("Hedging Impact Analysis:")
print("=" * 70)

for scenario in agg.scenarios:
    scenario_results = agg.by_scenario[scenario]
    unhedged = [r for r in scenario_results if r.hedging_mode == "none"][0]
    hedged = [r for r in scenario_results if r.hedging_mode != "none"][0]

    print(f"\n{scenario}:")
    print(f"  Unhedged: Mean ${unhedged.mean_pnl:.2f}, Std ${unhedged.std_pnl:.2f}")
    print(f"  Hedged:   Mean ${hedged.mean_pnl:.2f}, Std ${hedged.std_pnl:.2f}")
    print(f"  Variance Reduction: {(1 - hedged.std_pnl/unhedged.std_pnl)*100:.1f}%")
```

### Example 3: Strategy Comparison

```python
from config import StrategyPresets, ScenarioPresets, quick_config
from engine import SimulationEngine
from aggregator import ResultsAggregator

strategies = [
    ("Long Call", StrategyPresets.long_call_atm()),
    ("Short Call", StrategyPresets.short_call_atm()),
    ("Long Straddle", StrategyPresets.long_straddle_atm()),
    ("Bull Call Spread", StrategyPresets.bull_call_spread()),
]

all_results = []

for name, strategy_config in strategies:
    config = SimulationConfig(
        market=MarketConfig(spot_price=100, entry_iv=0.25),
        strategy=strategy_config,
        scenarios=(ScenarioPresets.iv_equals_rv(),),
        hedging_modes=(HedgingPresets.no_hedge(),),
        simulation=SimulationParams(num_simulations=2000, num_days=30),
    )

    results = SimulationEngine().run(config)
    all_results.extend(results)

agg = ResultsAggregator(all_results)

print("\nStrategy Ranking (by Sharpe Ratio):")
for rank, result in agg.rank_by_metric("sharpe_ratio"):
    print(f"  {rank}. {result.strategy_name}: {result.sharpe_ratio:.2f}")
```

---

## Advanced Usage

### Custom Scenario with IV Evolution

```python
from config import SimulationConfig, ScenarioConfig, ScenarioType

# IV gradually increases during trade
iv_spike = ScenarioConfig(
    name="IV Spike (50%)",
    scenario_type=ScenarioType.IV_INCREASES,
    realized_vol_multiplier=1.0,
    iv_evolution_rate=0.5,  # IV increases 50% by expiry
)

config = SimulationConfig(
    # ... other params ...
    scenarios=(iv_spike,),
)
```

### Fine-Grained Control

```python
from config import (
    MarketConfig, OptionLegConfig, StrategyConfig, ScenarioConfig,
    HedgingConfig, SimulationParams, SimulationConfig, HedgingMode,
    ScenarioType, OptionType, PositionDirection,
)

# Custom market
market = MarketConfig(
    spot_price=150.0,
    entry_iv=0.30,
    risk_free_rate=0.04,
    dividend_yield=0.01,
)

# Custom legs
long_call = OptionLegConfig(
    option_type=OptionType.CALL,
    direction=PositionDirection.LONG,
    quantity=1,
    expiration_days=60,
    strike_pct=1.0,  # ATM
)

short_call = OptionLegConfig(
    option_type=OptionType.CALL,
    direction=PositionDirection.SHORT,
    quantity=1,
    expiration_days=60,
    strike_pct=1.10,  # 10% OTM
)

# Bull call spread
strategy = StrategyConfig(
    name="Bull Call Spread 10%",
    legs=(long_call, short_call),
)

# Custom scenarios
scenarios = (
    ScenarioConfig(
        name="Low Vol",
        scenario_type=ScenarioType.IV_GREATER_RV,
        realized_vol_multiplier=0.60,
    ),
    ScenarioConfig(
        name="High Vol",
        scenario_type=ScenarioType.IV_LESS_RV,
        realized_vol_multiplier=1.40,
    ),
)

# Custom hedging
hedging = HedgingConfig(
    mode=HedgingMode.THRESHOLD,
    threshold=0.03,  # Rehedge if delta drifts 3%
)

config = SimulationConfig(
    market=market,
    strategy=strategy,
    scenarios=scenarios,
    hedging_modes=(hedging,),
    simulation=SimulationParams(
        num_simulations=5000,
        num_days=60,
        random_seed=42,
    ),
)

results = SimulationEngine().run(config)
```

### Batch Processing Multiple Strategies

```python
from config import quick_config
from engine import SimulationEngine
from aggregator import ResultsAggregator
from plotter import SimulationPlotter
from pathlib import Path

strategies = ["long_call", "short_call", "long_straddle", "iron_condor"]
output_dir = Path("./batch_results")
output_dir.mkdir(exist_ok=True)

all_results = []

for strategy in strategies:
    print(f"Running {strategy}...")
    config = quick_config(
        strategy=strategy,
        num_sims=3000,
        scenarios="all",
        hedging="both",
    )
    results = SimulationEngine().run(config)
    all_results.extend(results)

# Analyze
agg = ResultsAggregator(all_results)
with open(output_dir / "summary.txt", "w") as f:
    f.write(agg.summary_table())

# Plot
plotter = SimulationPlotter(all_results)
plotter.save_all(output_dir / "plots")

print(f"Results saved to {output_dir}")
```

---

## Configuration

### Key Parameters

**Market**:
- `spot_price`: Initial stock price (default: 100)
- `entry_iv`: Implied volatility at entry (default: 0.25 = 25%)
- `risk_free_rate`: Risk-free rate (default: 0.05 = 5%)
- `dividend_yield`: Dividend yield (default: 0 = no dividends)

**Strategy**:
- `legs`: List of option legs
- `entry_price`: Cost to enter (if None, computed from Black-Scholes)

**Simulation**:
- `num_simulations`: Number of Monte Carlo paths (default: 1000)
- `num_days`: Trading days to simulate (default: 30)
- `random_seed`: Seed for reproducibility (default: None = random)

**Hedging**:
- `mode`: NONE, DAILY, WEEKLY, THRESHOLD (default: NONE)
- `frequency`: Rehedge every N days (for DAILY/WEEKLY)
- `threshold`: Delta drift % to trigger rehedge (for THRESHOLD)

**Scenarios**:
- `scenario_type`: Type of IV evolution
- `realized_vol_multiplier`: RV = entry_iv × this value
- `iv_evolution_rate`: Rate of IV change for crush/spike scenarios

---

## Visualization

### Quick Plot

```python
from plotter import quick_plot

fig = quick_plot(results, plot_type="distribution")
fig = quick_plot(results, plot_type="comparison", group_by="hedging")
fig = quick_plot(results, plot_type="boxplot", group_by="scenario")
```

### Save Plots

```python
plotter = SimulationPlotter(results)
plotter.save_all("./plots", format="png", dpi=150)

# Or individual plots
fig = plotter.plot_pnl_distribution()
fig.savefig("pnl_dist.png", dpi=150, bbox_inches='tight')
```

---

## API Reference

### Core Classes

- `SimulationConfig`: Main configuration container
- `SimulationEngine`: Runs simulations
- `AggregatedResults`: Results for one scenario/hedging combo
- `ResultsAggregator`: Analysis of multiple results
- `SimulationPlotter`: Visualization

### Enums

- `OptionType`: CALL, PUT
- `PositionDirection`: LONG, SHORT
- `HedgingMode`: NONE, DAILY, WEEKLY, THRESHOLD
- `ScenarioType`: IV_EQUALS_RV, IV_GREATER_RV, IV_LESS_RV, IV_INCREASES, IV_CRUSH
- `ExitCondition`: HOLD_TO_EXPIRY, PROFIT_TARGET, STOP_LOSS, TIME_TARGET

### Key Functions

- `quick_config()`: Build config with one call
- `run_quick_simulation()`: Run simulation with one call
- `quick_plot()`: Plot results with one call

---

## Files & Line Count

```
simulation/config.py              ~600 lines  (Configuration system)
simulation/engine.py              ~650 lines  (Simulation engine)
simulation/aggregator.py          ~400 lines  (Results analysis)
simulation/plotter.py             ~500 lines  (Visualization)
simulation/run_simulation.py       ~300 lines  (CLI interface)
───────────────────────────────────────────
Total New Code:                 ~2,450 lines
```

---

## Next Steps & Future Enhancements

### Possible Enhancements
1. **Parallel execution**: Speed up large simulations with multiprocessing
2. **Advanced Greeks**: Add vanna, volga, charm
3. **Real option data**: Integration with market data APIs
4. **Optimization**: Auto-optimize hedging frequency vs cost
5. **Machine learning**: Learn optimal hedging policy from simulations
6. **Interactive plots**: Plotly/Dash for interactive visualization
7. **Report generation**: Auto-generate PDF reports

### Current Limitations
- Single instrument (no correlation/spreads across symbols)
- Fixed strike selection (could optimize wing positioning)
- No transaction costs (optional parameter)
- No bid-ask spread modeling

---

## Support

For issues or questions:
1. Check the examples in `run_simulation.py`
2. Review the docstrings in each module
3. Run `uv run simulation/run_simulation.py --examples`
4. Check individual class docstrings: `help(SimulationEngine)`, etc.

---

**Documentation Generated**: January 8, 2026
**Status**: Complete and Production-Ready
