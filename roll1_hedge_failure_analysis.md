# Roll #1 Hedge Failure Analysis
**Period:** 2025-03-03 to 2025-03-07
**Strike:** $17.50
**Entry Spot:** $20.17
**Exit Spot:** $18.32
**Total Spot Move:** -9.2% (-$1.85)

## Summary
**Total Gross Delta P&L:** -$96.56
**Total Hedge Delta P&L:** +$2.92
**Hedge Efficiency:** 3.0% ← **FAILED HEDGE**

---

## Day-by-Day Breakdown

### **Day 1: Monday 2025-03-03 (ENTRY DAY)** 🔴 UNHEDGED
```
Spot:       $20.17 → $18.81  (-$1.36, -6.7%)
Option Δ:   +70.5  (long delta, exposed to downside)
Hedge:      0 shares  ← NO HEDGE!
IV:         66.9%

P&L:
  Gross Δ:  -$95.88  (lost money on long position)
  Hedge Δ:  $0.00    (no protection)
  Net Δ:    -$95.88  (fully exposed!)
  Gamma:    +$14.24  (gamma helped a bit)
  Theta:    -$3.99   (time decay)
```
**PROBLEM:** Entered with +70.5 delta and NO HEDGE. Stock crashed -$1.36 in first day, causing -$95.88 loss (99% of total delta loss!).

---

### **Day 2: Tuesday 2025-03-04** 🔴 STILL UNHEDGED
```
Spot:       $18.485 → $18.49  (+$0.005, flat)
Option Δ:   +41.2  (delta decreased as position moved OTM)
Hedge:      0 shares  ← STILL NO HEDGE!
IV:         54.9% (dropped from 66.9%)

P&L:
  Gross Δ:  +$0.21   (tiny gain)
  Hedge Δ:  $0.00
  Net Δ:    +$0.21
  Gamma:    +$0.00   (almost no move)
  Theta:    -$4.52
```
**PROBLEM:** Still no hedge after Day 1 disaster. Lucky the stock was flat.

---

### **Day 3: Wednesday 2025-03-05** ✅ FINALLY HEDGED
```
Spot:       $18.77 → $18.95  (+$0.18, +1.0%)
Option Δ:   +49.5  (delta recovered as spot bounced)
Hedge:      -46 shares  ← HEDGE PLACED! (2 rehedges counted)
IV:         56.8%

P&L:
  Gross Δ:  +$8.91   (options made money)
  Hedge Δ:  -$8.28   (hedge cost money)
  Net Δ:    +$0.63   (almost perfectly hedged! 93% efficiency)
  Gamma:    +$0.46
  Theta:    -$4.56
```
**SUCCESS:** Hedge finally placed and worked perfectly. But too late—damage done on Day 1.

---

### **Day 4: Thursday 2025-03-06** ✅ HEDGE WORKING
```
Spot:       $18.47 → $18.07  (-$0.40, -2.2%)
Option Δ:   +37.5  (delta decreased again)
Hedge:      -46 shares  (same hedge from Day 3)
IV:         65.4% (IV spiked back up)

P&L:
  Gross Δ:  -$15.00  (options lost money on down move)
  Hedge Δ:  +$18.40  (hedge made MORE than options lost!)
  Net Δ:    +$3.41   (actually made money! 123% hedge efficiency)
  Gamma:    +$2.31   (gamma helped)
  Theta:    -$5.85
```
**SUCCESS:** Hedge overshot (46 shares vs 37.5 delta), actually made money on net delta.

---

### **Day 5: Friday 2025-03-07** ✅ HEDGE ADJUSTED
```
Spot:       $18.14 → $18.32  (+$0.18, +1.0%)
Option Δ:   +28.9  (delta further decreased)
Hedge:      -40 shares  (reduced from -46)
IV:         62.6%

P&L:
  Gross Δ:  +$5.20   (options made money)
  Hedge Δ:  -$7.20   (hedge cost money)
  Net Δ:    -$2.00   (slightly over-hedged, 138% efficiency)
  Gamma:    +$0.54
  Theta:    -$5.96
```
**MIXED:** Hedge size (-40) too large for delta (+28.9), lost a bit on net.

---

## Root Cause Analysis

### Why 3% Hedge Efficiency?
```
Total Gross Δ P&L:  -$96.56
Total Hedge Δ P&L:  +$2.92
Efficiency:         2.92 / 96.56 = 3.0%
```

**Breakdown by hedge status:**
- **Day 1-2 (Unhedged):** -$95.67 gross delta P&L, $0 hedge P&L
- **Day 3-5 (Hedged):**   -$0.89 gross delta P&L, +$2.92 hedge P&L

**The hedge worked great when it was there** (327% efficiency on Days 3-5)!
**But 99% of the loss happened BEFORE the hedge was placed.**

---

## Critical Questions

1. **Why was there no hedge on entry (Day 1)?**
   - Position entered at 9:30 AM with +70.5 delta
   - Did the hedging algorithm skip the entry time?
   - Was there a threshold that wasn't met?

2. **When were the 2 rehedges performed?**
   - The table says "2 hedges" total
   - But we see hedge changes on Day 3 and Day 5
   - What triggered the first hedge on Day 3?

3. **What was the hedge trigger threshold?**
   - Day 1: Delta +70.5, no hedge
   - Day 2: Delta +41.2, no hedge
   - Day 3: Delta +49.5, hedge placed at -46 shares

   Why did +49.5 delta trigger a hedge but +70.5 didn't?

---

## Recommendations

1. **Hedge at entry time** - Don't wait for first rehedge check
2. **Lower delta threshold** - If threshold is >70, that's too high
3. **Log hedge decisions** - Need to see why entry hedge was skipped
4. **Check rehedge schedule** - Verify hedge checks are happening at right times

---

## Verification Needed

Check the hedge configuration and timeline:
- What is `delta_threshold` set to?
- What are `rehedge_times`?
- Was entry_time included in rehedge_times?
- What was the actual hedge state machine output for this trade?
