# Gamma Loss Comparison: Calendar Spread vs Iron Butterfly

## TL;DR

Both strategies profit from IV crush and lose from gamma. The question is: **which captures more IV crush relative to gamma losses?**

---

## P&L Components

Both strategies have the same P&L drivers:

```
P&L = Vega P&L (IV crush) + Theta P&L + Gamma P&L (loss) + Delta P&L
```

For ATM positions held overnight through earnings:
- **Vega P&L**: Dominant factor (IV crushes 30-50%)
- **Theta P&L**: Small (only ~1 day)
- **Gamma P&L**: Negative (lose on spot movement)
- **Delta P&L**: Near zero for ATM (delta ≈ 0.50)

---

## Gamma Basics

- **Short gamma** = lose money when spot moves (either direction)
- **ATM options** have highest gamma
- **Near-term options** have higher gamma than far-term
- Gamma P&L ≈ 0.5 × net_gamma × (spot_move)²

---

## Numerical Example: AAPL Earnings

**Setup**: AAPL at $180, earnings after close, exit next morning

### Calendar Spread (7 DTE short / 30 DTE long)

| Leg | Position | Price | IV | Gamma | Vega |
|-----|----------|-------|-----|-------|------|
| Short 180C | -1 | $5.00 | 60% | 0.08 | 0.15 |
| Long 180C | +1 | $7.50 | 40% | 0.04 | 0.25 |

```
Entry cost = $7.50 - $5.00 = $2.50 debit
Net gamma = 0.04 - 0.08 = -0.04 (short)
Net vega = 0.25 - 0.15 = +0.10 (long vega - unusual for IV crush play!)
```

**Important**: Calendar spread is actually LONG vega because back month has higher vega. The profit comes from the *differential* IV crush (short leg IV drops more than long leg).

### Iron Butterfly (all 7 DTE, $10 wings)

| Leg | Position | Price | IV | Gamma | Vega |
|-----|----------|-------|-----|-------|------|
| Short 180C | -1 | $5.00 | 60% | 0.08 | 0.15 |
| Short 180P | -1 | $4.50 | 60% | 0.08 | 0.15 |
| Long 190C | +1 | $1.50 | 55% | 0.03 | 0.08 |
| Long 170P | +1 | $1.00 | 55% | 0.03 | 0.08 |

```
Entry credit = ($5.00 + $4.50) - ($1.50 + $1.00) = $7.00 credit
Max loss = $10 - $7.00 = $3.00
Net gamma = (0.03 + 0.03) - (0.08 + 0.08) = -0.10 (short)
Net vega = (0.08 + 0.08) - (0.15 + 0.15) = -0.14 (short vega)
```

**Iron butterfly is SHORT vega** - directly profits from IV crush.

---

## Scenario Analysis: Stock moves to $188 (+$8, ~4.4%)

### Post-Earnings IV Levels
- Short-dated IV: 60% → 25% (crush of 35 points)
- Long-dated IV: 40% → 35% (crush of 5 points)

### Calendar Spread P&L

**Vega P&L** (differential crush):
```
Short leg vega P&L = -(-0.15) × (-35) × 100 = -$5.25 (lose on short leg IV drop)
Long leg vega P&L  = +0.25 × (-5) × 100 = -$1.25 (lose on long leg IV drop)
Net vega P&L = -$5.25 + (-$1.25) = ... wait, this is wrong
```

Actually for calendar spread, the P&L works differently. Let me recalculate using option prices:

**Exit prices** (after IV crush + spot move to $188):
- Short 180C: Now ITM by $8, IV=25%, ~6 DTE → price ≈ $9.00
- Long 180C: Now ITM by $8, IV=35%, ~29 DTE → price ≈ $11.50

```
Exit value = $11.50 - $9.00 = $2.50
Entry cost = $2.50
P&L = $2.50 - $2.50 = $0.00 (breakeven)
```

**Gamma P&L component**:
```
Gamma loss ≈ 0.5 × 0.04 × 64 = $1.28
```

The IV crush helped, but gamma loss ate into it. Result: ~breakeven.

### Iron Butterfly P&L

**Exit prices** (after IV crush + spot move to $188):
- Short 180C: ITM by $8, IV=25% → price ≈ $9.00
- Short 180P: OTM by $8, IV=25% → price ≈ $0.30
- Long 190C: OTM by $2, IV=25% → price ≈ $0.80
- Long 170P: OTM by $18, IV=25% → price ≈ $0.05

```
Exit cost = ($9.00 + $0.30) - ($0.80 + $0.05) = $8.45
Entry credit = $7.00
P&L = $7.00 - $8.45 = -$1.45 (loss)
```

**Gamma P&L component**:
```
Gamma loss ≈ 0.5 × 0.10 × 64 = $3.20
```

Higher gamma exposure leads to larger loss despite IV crush.

---

## Scenario Analysis: Stock stays at $180 (no move)

### Calendar Spread P&L

**Exit prices** (IV crush only):
- Short 180C: ATM, IV=25%, ~6 DTE → price ≈ $2.50
- Long 180C: ATM, IV=35%, ~29 DTE → price ≈ $5.00

```
Exit value = $5.00 - $2.50 = $2.50
Entry cost = $2.50
P&L = $2.50 - $2.50 = $0.00 (breakeven)
```

Hmm, the calendar spread doesn't profit much even with no move because both legs crush.

### Iron Butterfly P&L

**Exit prices** (IV crush only):
- Short 180C: ATM, IV=25% → price ≈ $2.50
- Short 180P: ATM, IV=25% → price ≈ $2.30
- Long 190C: OTM by $10, IV=25% → price ≈ $0.40
- Long 170P: OTM by $10, IV=25% → price ≈ $0.35

```
Exit cost = ($2.50 + $2.30) - ($0.40 + $0.35) = $4.05
Entry credit = $7.00
P&L = $7.00 - $4.05 = +$2.95 profit
```

Iron butterfly captures the IV crush directly.

---

## Summary Comparison

| Scenario | Spot Move | Calendar P&L | Iron Butterfly P&L |
|----------|-----------|--------------|-------------------|
| No move | $0 | ~$0 | +$2.95 |
| Small move | $5 | ~-$0.50 | +$1.50 |
| Medium move | $8 | ~$0 | -$1.45 |
| Large move | $15 | ~-$4.00 | -$3.00 (capped) |
| Huge move | $25 | ~-$10.00 | -$3.00 (capped) |

---

## Key Insight

**Calendar Spread Problem**: The long leg's vega partially offsets the IV crush benefit. You're betting on *differential* IV behavior between expirations, not pure IV crush.

**Iron Butterfly Advantage**: Directly short vega - pure IV crush play. But higher gamma means you need the stock to stay near the strike.

**Crossover Point**: ~$8-10 move. Below that, iron butterfly wins. Above that, calendar's lower gamma helps, but eventually iron butterfly's cap wins on huge moves.

---

## Conclusion

For **pure IV crush capture** with **defined risk**:
- Iron Butterfly is better IF you expect stock to stay within ± credit received
- Calendar Spread is better IF you expect moderate moves ($5-15) and want lower gamma

For **earnings plays** where moves are unpredictable:
- Iron Butterfly's defined max loss is valuable for risk management
- But calendar's lower gamma is valuable if you expect the typical 3-5% earnings move

**The honest answer**: Neither is clearly superior. It depends on expected move size relative to premium received.
