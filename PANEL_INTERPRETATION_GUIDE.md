# Trade Replay Panels - Visual Interpretation Guide

## Overview

Each trade replay generates a 5-panel visualization showing different aspects of market context. This guide explains what each panel shows and how to interpret patterns.

---

## Panel 1: Stock Price with Trade Markers

### What You're Looking At

```
       Price
        ↑
        |     Entry → Blue area → Exit
        |        ↓              ↓
$105 —|——————— ┌─────────────┐
        |       │ good area   │
$100 —|——————— │             │ —— Entry strike zone
        |       │             │
$95  —|——————— └─────────────┘
        |    earnings ↑
        |             □
Time → Mon  Tue  Wed  Thu  Fri
```

### Color Code

| Element | Color | Meaning |
|---------|-------|---------|
| Stock line | Blue | Actual spot price movement |
| Entry marker | Green | Trade entry point |
| Exit marker | Red | Trade exit point |
| Earnings event | Orange | Earnings announcement date |

### Interpretation Patterns

#### Pattern 1: Calm Move (✅ GOOD for short volatility)

```
Entry at $100
Exit at $102 (+2%)
Shape: Nearly flat line
```

**What this tells you:**
- Spot stayed calm → Greeks favorable
- Short vol thesis worked → gamma positive
- Theta decay helped → time value collapse
- Result: Likely profit

**Example trade:**
```
Short strangle: 95 put / 105 call
Spot moves to 102 → both still OTM → nice profit
```

---

#### Pattern 2: Large Move (⚠️ BAD for short volatility)

```
Entry at $100
Exit at $115 (+15%)
Shape: Sharp upward line
```

**What this tells you:**
- Spot exploded → Greeks blow up
- Calls now deep ITM → gamma loss
- Theta decay helped but insufficient → overwhelmed by Greeks
- Result: Likely loss

**Example trade:**
```
Short strangle: 95 put / 105 call
Spot moves to 115 → calls blown away → large loss
```

---

#### Pattern 3: Reversal (⚠️ BAD for directional bets)

```
Entry at $100
Mid-trade: Spikes to $110
Exit: Drops back to $101
Shape: Mountain then valley
```

**What this tells you:**
- Large move but ended near entry
- Whipsaw action → gamma losses both ways
- Even if spot returns, gamma damage is real
- Result: Loss despite neutral final spot

**Example trade:**
```
Short straddle: 100 call / 100 put
Spot: 100 → 110 → 105 → 100
Greeks losses: $-1000, then $-800, then return to $0?
No! Past losses are realized → net loss
```

---

#### Pattern 4: Slow Drift (✅ GOOD for theta sellers)

```
Entry at $100
Small gradual move up to $105
Shape: Slow diagonal line
```

**What this tells you:**
- Spot moving but slowly → manageable gamma
- Theta decay accumulating daily → good for short vol
- Greeks stay in reasonable zone
- Result: Usually profitable

**Example trade:**
```
Calendar spread: short 100 call / long 100 call
Spot: 100 → 101 → 102 → 105 over 30 days
Daily theta wins → calendars love this
```

---

### Key Questions When Looking at Panel 1

1. **How big is the move relative to entry strikes?**
   - Small (< 3%) → Greeks friendly
   - Moderate (3-8%) → Greeks starting to matter
   - Large (> 8%) → Greeks dominate

2. **When did the move happen?**
   - Early (near entry) → more damage
   - Late (near exit) → less damage
   - At exit → time to close out

3. **Was it smooth or whippy?**
   - Smooth → easier for models
   - Whippy → gamma losses even if returns to entry

4. **Did earnings happen during the trade?**
   - Before entry → volatility expected
   - During hold → unexpected volatility (bad)
   - After exit → trade worked around it (good)

---

## Panel 2: IV Evolution

### What You're Looking At

```
IV
↑
35% —|—— Entry    ●
     |           ╱
30% —|——────────●  Exit
     |
25% —|
     |
Time →
```

The vertical axis shows ATM (at-the-money) implied volatility. You see snapshots at entry and exit dates.

### Interpretation Patterns

#### Pattern 1: IV Crush (✅ BEST for short volatility)

```
Entry: IV = 35% (high)
Exit:  IV = 25% (low)
Change: -10pp (CRUSH)
```

**What this tells you:**
- Sold vol at the top → perfect timing
- Vol collapsed after entry → vega profit
- This is the ideal scenario for short-vol traders
- Result: Large vega profit dominates P&L

**Example:**
```
Short straddle at 35% IV
Earnings event causes uncertainty
Market reprices: 35% → 42% (oops, expansion!)
Then: 42% → 28% (crush after earnings)
Exit at 28% → Vega profit = $$$
```

---

#### Pattern 2: IV Expansion (⚠️ BAD for short volatility)

```
Entry: IV = 30% (baseline)
Exit:  IV = 38% (high)
Change: +8pp (EXPANSION)
```

**What this tells you:**
- Vol expanded instead of contracted → vega loss
- Opposite of thesis → strategy working backward
- This overwhelms theta decay
- Result: Loss despite time value

**Example:**
```
Short strangle at 30% IV
Market uncertainty → IV rises
Volatility expand even more → vega loss = $$$
Theta helped: +$50/day × 30 days = $1500
Vega hurt: -$3000
Net: -$1500 loss
```

---

#### Pattern 3: IV Stable (✓ NEUTRAL)

```
Entry: IV = 30%
Exit:  IV = 30%
Change: 0pp (NO CHANGE)
```

**What this tells you:**
- Vol didn't crush or expand
- Vega impact was zero
- Profit/loss depends entirely on theta vs gamma
- Result: Pure play on time decay vs spot move

**Example:**
```
Short strangle at 30% IV
Spot: 100 → 105 (3% move)
IV: 30% → 30% (no change)
P&L = Theta profit - Gamma loss
     = $1500 - $1200 = $300
```

---

#### Pattern 4: IV Trend (📈 LONG VOLATILITY setup)

```
Entry: IV = 20% (LOW - compress)
Exit:  IV = 28% (HIGHER)
Change: +8pp (EXPANSION)
```

**What this tells you:**
- Entered when vol was suppressed
- Vol expanded as you held
- This is GOOD for LONG vol positions
- This is BAD for SHORT vol positions
- Result: Vega profit if long vol, loss if short vol

---

### Key Questions When Looking at Panel 2

1. **Did IV crush? (< -5pp)**
   - YES → Huge vega profit for short-vol trades
   - NO → Vega didn't help

2. **Did IV expand? (> +5pp)**
   - YES → Vega loss for short-vol trades
   - NO → Vega didn't hurt

3. **When did IV change?**
   - Immediately after entry → Fast repricing
   - During hold → Gradual shift
   - Near exit → Less time to profit

4. **What caused IV change?**
   - Earnings event → Expected (plan for it)
   - Market stress → Unexpected (strategy didn't account for it)
   - Normal mean reversion → Should recover later

---

## Panel 3: Volatility Comparison (HV vs IV)

### What You're Looking At

```
Volatility (%)
↑
40 —|——— Entry IV (horizontal line)
    |    ╱╲
35 —|———╱  ╲  ← Historical Volatility (wavy line)
    |  │    │
30 —|──┘    └──
    |
Time →
```

- **Horizontal line:** IV at time of entry
- **Wavy line:** Realized (Historical) Volatility during trade

### Interpretation Patterns

#### Pattern 1: IV > HV (✅ SHORT VOLATILITY advantage)

```
Entry IV:   40%
Realized HV: 25%
Gap:        +15pp
```

**What this tells you:**
- Sold vol above realized level → win!
- Options were overpriced relative to actual moves
- Spot didn't move as much as IV implied
- Result: Profit from vol premium

**Example:**
```
Entry: Sell straddle at 40% IV
Reality: Spot only moves to cause 25% realized vol
Buyer overpaid → seller profits
This is the ideal short-vol scenario
```

---

#### Pattern 2: IV < HV (⚠️ SHORT VOLATILITY trap)

```
Entry IV:   20%
Realized HV: 35%
Gap:        -15pp
```

**What this tells you:**
- Sold vol below realized level → loss!
- Options were underpriced relative to actual moves
- Spot moved much more than IV implied
- Result: Loss from vol premium getting crushed by reality

**Example:**
```
Entry: Sell strangle at 20% IV
Reality: Spot moves wildly → 35% realized vol
Seller underpriced → large loss
Greeks blowup overwhelms theta
This is the short-vol nightmare scenario
```

---

#### Pattern 3: IV ≈ HV (EFFICIENT pricing)

```
Entry IV:   30%
Realized HV: 30%
Gap:        0pp
```

**What this tells you:**
- Options fairly priced at entry
- No vol premium/discount
- P&L depends on theta vs gamma
- Result: "Expected value" scenario

**Example:**
```
Entry: Sell at fair value (30% IV = 30% HV)
P&L = Theta decay (good) - Greeks slippage (bad)
No windfall from mispricing
```

---

#### Pattern 4: HV Spikes During Hold (⚠️ BAD for Greeks)

```
Entry IV: 30%
HV trajectory:
  - Day 1-5:  25% (calm) ✓
  - Day 6-10: 40% (spike!) ✗
  - Day 11+:  30% (return) ✓
```

**What this tells you:**
- Entry IV was reasonable
- But spot moved more than expected mid-hold
- Greeks losses during spike aren't recovered
- Result: Theta profit overcome by gamma loss during spike

**Example:**
```
Entry: 30% IV, Greeks look balanced
Days 1-5: Calm, theta accumulating
Day 6: Earnings → spot jumps → gamma loss $$$
Days 7+: Calm returns but damage is done
Net P&L: Negative despite "fair value" entry
```

---

### Key Questions When Looking at Panel 3

1. **Is IV above or below realized HV?**
   - Above → Good for short vol (won the vol bet)
   - Below → Bad for short vol (lost the vol bet)
   - Equal → Neutral (no premium/discount)

2. **How big is the gap?**
   - 10pp+ difference → This explains a lot of P&L
   - 0-5pp difference → Other factors dominated

3. **Did HV move during the trade?**
   - Stayed flat → Greeks predictable
   - Spiked → Greeks blew up (unexpected)

4. **When did HV spike?**
   - Early → More damage
   - Late → Less damage
   - Never → Lucky scenario

---

## Panel 4: Greeks Analysis

### What You're Looking At

A table showing Greeks at trade entry:

```
DELTA:  +0.08    Position gains $0.08 if spot +$1
GAMMA:  +0.001   Delta will increase by 0.001 if spot +$1
THETA:  +0.08    Position gains $0.08 per day from decay
VEGA:   +0.30    Position gains $0.30 if IV +1%
```

### Interpretation Patterns

#### Pattern 1: Vega-Negative (✓ SHORT VOL)

```
VEGA: -0.45
This is CORRECT for short volatility
```

**What this tells you:**
- Position loses money if vol expands
- Position makes money if vol crushes
- Setup matches short-vol thesis
- Result: Strategy working as intended

**Example:**
```
Short strangle vega: -0.45
If IV: 30% → 35% → Loss of $0.45 × 100 = $45
If IV: 30% → 25% → Gain of $0.45 × 100 = $45
```

---

#### Pattern 2: Vega-Positive (⚠️ LONG VOL)

```
VEGA: +0.45
This is WRONG for short volatility trades!
```

**What this tells you:**
- Position makes money if vol expands
- Position loses money if vol crushes
- Thesis is reversed!
- Result: Opposite direction bet

**Example:**
```
Should be short vol but vega is positive
IV: 30% → 35% → Gain of $0.45 × 100 = $45 (lucky!)
IV: 30% → 25% → Loss of $0.45 × 100 = $45 (unlucky!)
Strike selection error or mismatched position
```

---

#### Pattern 3: High Theta, Low Gamma (✓ IDEAL)

```
DELTA: ≈0      Neutral directionally
GAMMA: ≈0      Not punished by moves
THETA: +0.12   Earning $0.12/day
VEGA:  -0.20   Winning on vol crush
```

**What this tells you:**
- Direction neutral → safe
- Big moves won't kill you → safe
- Earning daily from decay → profit
- Benefiting from vol crush → bonus
- Result: This is the ideal short-vol setup

---

#### Pattern 4: High Gamma, Low Theta (⚠️ RISKY)

```
DELTA: ≈0      Neutral directionally
GAMMA: +0.004  Punished by moves
THETA: -0.02   Losing daily to decay
VEGA:  +0.10   Winning on vol expansion
```

**What this tells you:**
- Direction neutral but fragile
- Big moves will hurt → dangerous
- Losing daily from decay → costly
- Needs vol expansion to win → speculative
- Result: High risk, requires perfect conditions

---

### Key Questions When Looking at Panel 4

1. **Does vega match strategy?**
   - Short vol trade → should be NEGATIVE vega
   - Long vol trade → should be POSITIVE vega
   - If backwards → fundamental problem!

2. **Is theta positive? (for short vol)**
   - YES → Earning from decay (good)
   - NO → Paying for decay (bad)

3. **Is gamma/gamma exposure acceptable?**
   - Small → Greeks safe, small spot moves fine
   - Large → Greeks risky, big moves hurt

4. **Are Greeks balanced?**
   - Balanced → Position resilient
   - Imbalanced → Position fragile to specific scenarios

---

## Panel 5: P&L Summary

### What You're Looking At

```
P&L:        +$450
P&L %:      +12.0%
IV Entry:   35%
IV Exit:    28%
IV Change:  -700bp
Status:     ✓ Success
```

### Interpretation Patterns

#### Pattern 1: IV Crush Profit

```
P&L:        +$450
IV Change:  -700bp (huge crush)
Status:     ✓ Success
Analysis:   Vega profit explains most of P&L
            Short-vol thesis confirmed
```

**What to look for in panels:**
- Panel 2 shows big IV decline
- Panel 3 might show IV < HV
- Vega should be negative in Panel 4

---

#### Pattern 2: Theta Profit

```
P&L:        +$120
IV Change:  -100bp (modest)
Status:     ✓ Success
Analysis:   Theta decay explains most of P&L
            Position stayed neutral
```

**What to look for in panels:**
- Panel 1 shows flat price (no gamma losses)
- Panel 4 shows positive theta
- V small spot move relative to strike width

---

#### Pattern 3: Greeks Profit (Volatility Realization)

```
P&L:        +$300
IV Change:  -50bp (tiny)
Status:     ✓ Success
Analysis:   Spot moved within Greeks limits
            Favorable gamma from long position
```

**What to look for in panels:**
- Panel 1 shows spot movement
- Panel 4 shows positive gamma (unusual for short-vol)
- Indicates long position, not short

---

#### Pattern 4: Vega Loss Despite Profit

```
P&L:        +$200
IV Change:  +400bp (expansion)
Status:     ✓ Success (barely)
Analysis:   Vega hurt but compensated by:
            - Strong theta accumulation
            - Spot movement favorable to position
```

**What to look for in panels:**
- Panel 2 shows IV going up
- Panel 4 shows large positive theta
- P&L still positive (thesis partially worked)

---

#### Pattern 5: Greeks Failure

```
P&L:        -$1200
IV Change:  -200bp (modest crush)
Status:     ✗ Failed
Analysis:   Spot movement blew up Greeks
            IV crush wasn't enough to overcome
```

**What to look for in panels:**
- Panel 1 shows big spot move
- Panel 3 shows HV spiked
- Panel 4 shows high gamma (lost on moves)
- IV crush (Panel 2) didn't compensate

---

### Key Questions When Looking at Panel 5

1. **Does P&L make sense given panels 1-4?**
   - Yes → Strategy thesis confirmed
   - No → Hidden factor or modeling error

2. **What was the dominant P&L driver?**
   - Panel 2 IV crush → Vega profit
   - Panel 1 flat price → Theta profit
   - Panel 3 HV high → Greeks losses
   - Combination → Multiple factors

3. **Could this trade be improved?**
   - If vega lost: Need better entry timing for IV
   - If gamma lost: Need wider strikes or position sizing
   - If theta didn't help: Need longer duration
   - If spot was wrong: Need earnings awareness

4. **Is this result repeatable?**
   - Yes → Add to strategy rules
   - No → Just lucky or unlucky outlier

---

## Summary: Quick Pattern Recognition

### ✅ Signs of Successful Trade

```
Panel 1: Flat or slow move        → Greeks safe
Panel 2: IV crush                 → Vega profit
Panel 3: IV > HV at entry         → Won vol bet
Panel 4: Positive theta, neg vega → Thesis correct
Panel 5: Green P&L, IV crush      → Typical winner
```

### ⚠️ Signs of Failed Trade

```
Panel 1: Big spot move            → Greeks blow up
Panel 2: IV expansion             → Vega loss
Panel 3: HV spike, IV < HV        → Lost vol bet
Panel 4: High gamma, low theta    → Risky setup
Panel 5: Red P&L, IV expansion    → Typical loser
```

### 📊 What to Track Across Trades

**For winners:**
- Which symbols most profitable?
- Common IV entry levels? (e.g., > 60th percentile)
- Common spot move ranges?
- Typical theta/vega contribution to P&L?

**For losers:**
- Failure modes (IV expansion? Spot explosion? Greeks?)
- Common symbol problems?
- Entry timing mistakes?
- Position sizing issues?

**For improvement:**
- Add filters based on winner characteristics
- Avoid loser patterns
- Adjust position sizing for high-gamma scenarios
- Monitor earnings events

---

## Next Steps

1. **Analyze your first trade:**
   ```bash
   uv run python3 replay_trade.py --result results.json --index 0
   ```

2. **Refer back to this guide** when interpreting each panel

3. **Batch analyze winners** to find repeatable patterns:
   ```bash
   uv run python3 batch_replay_trades.py --result results.parquet --success-only
   ```

4. **Refine strategy rules** based on patterns

5. **Backtest new rules** to validate improvements

---

For full documentation, see `REPLAY_ANALYSIS_COMPLETE_GUIDE.md`
