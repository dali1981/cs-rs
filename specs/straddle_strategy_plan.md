# Straddle Strategy Implementation Plan

## Overview

Add a **Long Straddle** option strategy to the backtesting framework, specifically designed to capture IV expansion going into earnings announcements.

### Strategy Definition
- **Structure**: Long ATM Call + Long ATM Put at the same strike and expiration
- **Expiration Selection**: First expiration AFTER earnings date (front-week post-earnings)
- **Entry**: 5 trading days (1 week) before earnings
- **Exit**: 1 trading day before earnings
- **Profit Driver**: IV expansion as earnings approach (opposite of IV crush strategies)
- **Max Loss**: Limited to premium paid
- **Max Profit**: Theoretically unlimited

### Comparison with Existing Strategies

| Strategy | Structure | Entry | Exit | Profit Driver |
|----------|-----------|-------|------|---------------|
| Calendar Spread | Short near + Long far | Day before/same day | Day after | IV crush (short leg) |
| Iron Butterfly | Short straddle + wings | Day before/same day | Day after | IV crush + theta |
| **Straddle (NEW)** | Long ATM call + put | 5 days before | 1 day before | IV expansion |

---

## Architecture Analysis

### Current Codebase Structure

```
cs-domain/src/
├── entities.rs          # CalendarSpread, IronButterfly, EarningsEvent
├── value_objects.rs     # Strike, SpotPrice, TimingConfig
├── strategies/
│   ├── mod.rs           # SelectionStrategy trait, OptionChainData
│   ├── atm.rs           # ATMStrategy (strike selection)
│   ├── delta.rs         # DeltaStrategy
│   └── iron_butterfly.rs
└── services/
    ├── earnings_timing.rs  # EarningsTradeTiming (entry/exit per EarningsTime)
    └── trading_calendar.rs # TradingCalendar (weekday checks)

cs-backtest/src/
├── backtest_use_case.rs    # Main orchestrator, TradeResult enum
├── trade_executor.rs       # CalendarSpread executor
├── iron_butterfly_executor.rs
├── spread_pricer.rs        # Option pricing with Greeks
└── config.rs               # SpreadType enum, BacktestConfig

cs-analytics/src/
├── straddle.rs             # StraddlePriceComputer (ALREADY EXISTS!)
└── black_scholes.rs        # BS pricing and Greeks
```

### Key Design Decisions

1. **Straddle uses different timing logic** - Not tied to BMO/AMC, but to N trading days before earnings
2. **Expiration selection**: First expiration AFTER earnings (so options still have value at exit)
3. **Pricing at exit**: Use minute data if available, otherwise use `PricingModel` (Black-Scholes) to price options
4. **Reuse StraddlePriceComputer** from cs-analytics for pricing calculations
5. **New timing service** for straddle-specific entry/exit computation
6. **Same SelectionStrategy trait** - Add `select_straddle` method

### Data Handling Strategy

**Entry (5 trading days before earnings):**
- Fetch spot price at entry time
- Fetch option chain for entry date
- Select ATM strike based on spot
- Select expiration: First available expiration AFTER earnings date
- Price using market data (mid-price or last trade)

**Exit (1 trading day before earnings):**
- Fetch spot price at exit time
- Attempt to fetch option prices from minute/daily data
- **Fallback**: If options at the selected strike don't have market prices (e.g., spot moved significantly and those strikes are no longer traded), use `PricingModel` (Sticky Strike/Sticky Moneyness/Sticky Delta) to compute theoretical prices via Black-Scholes

---

## Implementation Plan

### Phase 1: Domain Layer (cs-domain)

#### 1.1 Add Straddle Entity (`cs-domain/src/entities.rs`)

```rust
/// Long straddle = Long ATM Call + Long ATM Put
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Straddle {
    pub call_leg: OptionLeg,
    pub put_leg: OptionLeg,
}

impl Straddle {
    pub fn new(call_leg: OptionLeg, put_leg: OptionLeg) -> Result<Self, ValidationError> {
        // Validate same symbol
        if call_leg.symbol != put_leg.symbol {
            return Err(ValidationError::SymbolMismatch(
                call_leg.symbol.clone(),
                put_leg.symbol.clone(),
            ));
        }

        // Validate same expiration
        if call_leg.expiration != put_leg.expiration {
            return Err(ValidationError::ExpirationMismatch {
                short: call_leg.expiration,
                long: put_leg.expiration,
            });
        }

        // Validate same strike
        if call_leg.strike != put_leg.strike {
            return Err(ValidationError::StrikeMismatch {
                call: call_leg.strike,
                put: put_leg.strike,
            });
        }

        // Validate option types
        if call_leg.option_type != OptionType::Call {
            return Err(ValidationError::InvalidOptionType(
                "Call leg must be a Call".to_string(),
            ));
        }
        if put_leg.option_type != OptionType::Put {
            return Err(ValidationError::InvalidOptionType(
                "Put leg must be a Put".to_string(),
            ));
        }

        Ok(Self { call_leg, put_leg })
    }

    pub fn symbol(&self) -> &str { &self.call_leg.symbol }
    pub fn strike(&self) -> Strike { self.call_leg.strike }
    pub fn expiration(&self) -> NaiveDate { self.call_leg.expiration }
    pub fn dte(&self, from: NaiveDate) -> i32 {
        (self.expiration() - from).num_days() as i32
    }
}
```

#### 1.2 Add StraddleResult (`cs-domain/src/entities.rs`)

```rust
/// Long straddle trade result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StraddleResult {
    // Identification
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub strike: Strike,
    pub expiration: NaiveDate,

    // Entry (DEBIT paid)
    pub entry_time: DateTime<Utc>,
    pub call_entry_price: Decimal,
    pub put_entry_price: Decimal,
    pub entry_debit: Decimal,  // Total premium paid

    // Exit (credit received)
    pub exit_time: DateTime<Utc>,
    pub call_exit_price: Decimal,
    pub put_exit_price: Decimal,
    pub exit_credit: Decimal,

    // Pricing method used at exit
    pub exit_pricing_method: PricingSource,  // Market or Model

    // P&L
    pub pnl: Decimal,
    pub pnl_pct: Decimal,

    // Greeks at entry (net position)
    pub net_delta: Option<f64>,
    pub net_gamma: Option<f64>,
    pub net_theta: Option<f64>,
    pub net_vega: Option<f64>,

    // IV at entry/exit
    pub iv_entry: Option<f64>,
    pub iv_exit: Option<f64>,
    pub iv_change: Option<f64>,  // IV expansion (positive = good for long straddle)

    // P&L Attribution
    pub delta_pnl: Option<Decimal>,
    pub gamma_pnl: Option<Decimal>,
    pub theta_pnl: Option<Decimal>,
    pub vega_pnl: Option<Decimal>,
    pub unexplained_pnl: Option<Decimal>,

    // Spot prices
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,
    pub spot_move: f64,
    pub spot_move_pct: f64,

    // Expected move context
    pub expected_move_pct: Option<f64>,  // Straddle / Spot at entry

    // Status
    pub success: bool,
    pub failure_reason: Option<FailureReason>,
}

/// Indicates how exit prices were determined
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PricingSource {
    /// Prices from actual market data (minute bars)
    Market,
    /// Prices computed via Black-Scholes model
    Model,
}

impl StraddleResult {
    pub fn is_winner(&self) -> bool {
        self.success && self.pnl > Decimal::ZERO
    }
}
```

#### 1.3 Extend TradingCalendar (`cs-domain/src/services/trading_calendar.rs`)

```rust
impl TradingCalendar {
    /// Get N trading days before a date
    ///
    /// Example: n_trading_days_before(2025-01-10, 5)
    ///          -> 2025-01-03 (skipping weekends)
    pub fn n_trading_days_before(date: NaiveDate, n: usize) -> NaiveDate {
        let mut result = date;
        let mut count = 0;
        while count < n {
            result = Self::previous_trading_day(result);
            count += 1;
        }
        result
    }

    /// Count trading days between two dates (exclusive of start, inclusive of end)
    pub fn trading_days_count(start: NaiveDate, end: NaiveDate) -> usize {
        Self::trading_days_between(start, end).count()
    }
}
```

#### 1.4 Add StraddleTradeTiming Service (`cs-domain/src/services/straddle_timing.rs`)

```rust
/// Timing service for straddle trades around earnings
///
/// Unlike EarningsTradeTiming (which handles IV crush trades), this service
/// implements timing for IV expansion trades that profit from volatility
/// buildup BEFORE earnings.
pub struct StraddleTradeTiming {
    config: TimingConfig,
    entry_days_before: usize,  // Default: 5 (one week before)
    exit_days_before: usize,   // Default: 1 (day before earnings)
}

impl StraddleTradeTiming {
    pub fn new(config: TimingConfig) -> Self {
        Self {
            config,
            entry_days_before: 5,
            exit_days_before: 1,
        }
    }

    pub fn with_entry_days(mut self, days: usize) -> Self {
        self.entry_days_before = days;
        self
    }

    pub fn with_exit_days(mut self, days: usize) -> Self {
        self.exit_days_before = days;
        self
    }

    /// Entry date: N trading days before earnings
    pub fn entry_date(&self, event: &EarningsEvent) -> NaiveDate {
        TradingCalendar::n_trading_days_before(
            event.earnings_date,
            self.entry_days_before
        )
    }

    /// Exit date: M trading days before earnings (default: 1)
    pub fn exit_date(&self, event: &EarningsEvent) -> NaiveDate {
        TradingCalendar::n_trading_days_before(
            event.earnings_date,
            self.exit_days_before
        )
    }

    pub fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let entry_date = self.entry_date(event);
        eastern_to_utc(entry_date, self.config.entry_time())
    }

    pub fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        let exit_date = self.exit_date(event);
        eastern_to_utc(exit_date, self.config.exit_time())
    }

    /// Get holding period in trading days
    pub fn holding_period(&self) -> usize {
        self.entry_days_before - self.exit_days_before
    }
}
```

#### 1.5 Extend SelectionStrategy Trait (`cs-domain/src/strategies/mod.rs`)

```rust
/// Add Straddle variant to OptionStrategy enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionStrategy {
    CalendarSpread,
    IronButterfly,
    Straddle,  // NEW
}

/// Extend SelectionStrategy trait
pub trait SelectionStrategy: Send + Sync {
    fn select_calendar_spread(...) -> Result<CalendarSpread, StrategyError>;
    fn select_iron_butterfly(...) -> Result<IronButterfly, StrategyError>;

    /// Select a straddle opportunity
    ///
    /// Selects ATM strike and first expiration AFTER earnings date.
    /// Default implementation returns UnsupportedStrategy error.
    fn select_straddle(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
    ) -> Result<Straddle, StrategyError> {
        Err(StrategyError::UnsupportedStrategy(
            "Straddle not supported by this selection strategy".to_string()
        ))
    }
}
```

#### 1.6 Add StraddleStrategy (`cs-domain/src/strategies/straddle.rs`)

```rust
/// Straddle selection strategy
///
/// Selects ATM straddle with first expiration AFTER earnings.
/// This ensures the options still have time value when we exit
/// (1 day before earnings).
pub struct StraddleStrategy {
    pub min_dte_after_earnings: i32,  // Minimum days after earnings for expiry
}

impl Default for StraddleStrategy {
    fn default() -> Self {
        Self {
            min_dte_after_earnings: 1,  // At least 1 day after earnings
        }
    }
}

impl StraddleStrategy {
    /// Create with custom minimum DTE after earnings
    pub fn with_min_dte(min_dte: i32) -> Self {
        Self { min_dte_after_earnings: min_dte }
    }

    /// Find first expiration after earnings date
    fn select_expiration(
        &self,
        expirations: &[NaiveDate],
        earnings_date: NaiveDate,
    ) -> Option<NaiveDate> {
        expirations
            .iter()
            .filter(|&&exp| {
                let days_after = (exp - earnings_date).num_days() as i32;
                days_after >= self.min_dte_after_earnings
            })
            .min()
            .copied()
    }
}

impl SelectionStrategy for StraddleStrategy {
    fn select_calendar_spread(...) -> Result<CalendarSpread, StrategyError> {
        Err(StrategyError::UnsupportedStrategy(
            "Calendar spread not supported by StraddleStrategy".into()
        ))
    }

    fn select_iron_butterfly(...) -> Result<IronButterfly, StrategyError> {
        Err(StrategyError::UnsupportedStrategy(
            "Iron butterfly not supported by StraddleStrategy".into()
        ))
    }

    fn select_straddle(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
    ) -> Result<Straddle, StrategyError> {
        // Select first expiration AFTER earnings
        let expiration = self.select_expiration(&chain_data.expirations, event.earnings_date)
            .ok_or(StrategyError::NoExpirations)?;

        // Select ATM strike
        let spot_value = spot.to_f64();
        let atm_strike = chain_data.strikes
            .iter()
            .min_by(|a, b| {
                let diff_a = (a.to_f64() - spot_value).abs();
                let diff_b = (b.to_f64() - spot_value).abs();
                diff_a.partial_cmp(&diff_b).unwrap()
            })
            .ok_or(StrategyError::NoStrikes)?;

        // Create legs
        let call_leg = OptionLeg::new(
            event.symbol.clone(),
            *atm_strike,
            expiration,
            OptionType::Call,
        );
        let put_leg = OptionLeg::new(
            event.symbol.clone(),
            *atm_strike,
            expiration,
            OptionType::Put,
        );

        Straddle::new(call_leg, put_leg)
            .map_err(StrategyError::SpreadCreation)
    }
}
```

---

### Phase 2: Backtest Layer (cs-backtest)

#### 2.1 Extend TradeResult Enum (`cs-backtest/src/backtest_use_case.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradeResult {
    CalendarSpread(CalendarSpreadResult),
    IronButterfly(IronButterflyResult),
    Straddle(StraddleResult),  // NEW
}

impl TradeResult {
    pub fn is_winner(&self) -> bool {
        match self {
            TradeResult::CalendarSpread(r) => r.is_winner(),
            TradeResult::IronButterfly(r) => r.is_winner(),
            TradeResult::Straddle(r) => r.is_winner(),
        }
    }

    pub fn success(&self) -> bool {
        match self {
            TradeResult::CalendarSpread(r) => r.success,
            TradeResult::IronButterfly(r) => r.success,
            TradeResult::Straddle(r) => r.success,
        }
    }

    pub fn pnl(&self) -> rust_decimal::Decimal {
        match self {
            TradeResult::CalendarSpread(r) => r.pnl,
            TradeResult::IronButterfly(r) => r.pnl,
            TradeResult::Straddle(r) => r.pnl,
        }
    }

    pub fn pnl_pct(&self) -> rust_decimal::Decimal {
        match self {
            TradeResult::CalendarSpread(r) => r.pnl_pct,
            TradeResult::IronButterfly(r) => r.pnl_pct,
            TradeResult::Straddle(r) => r.pnl_pct,
        }
    }

    pub fn symbol(&self) -> &str {
        match self {
            TradeResult::CalendarSpread(r) => &r.symbol,
            TradeResult::IronButterfly(r) => &r.symbol,
            TradeResult::Straddle(r) => &r.symbol,
        }
    }

    pub fn strike(&self) -> Strike {
        match self {
            TradeResult::CalendarSpread(r) => r.strike,
            TradeResult::IronButterfly(r) => r.center_strike,
            TradeResult::Straddle(r) => r.strike,
        }
    }
}
```

#### 2.2 Extend SpreadType Enum (`cs-backtest/src/config.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpreadType {
    #[default]
    Calendar,
    IronButterfly,
    Straddle,  // NEW
}

impl SpreadType {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().replace('-', "_").as_str() {
            "iron_butterfly" | "ironbutterfly" | "butterfly" => SpreadType::IronButterfly,
            "straddle" | "long_straddle" => SpreadType::Straddle,
            _ => SpreadType::Calendar,
        }
    }
}
```

#### 2.3 Add BacktestConfig Fields

```rust
pub struct BacktestConfig {
    // ... existing fields ...

    /// Straddle: Entry N trading days before earnings (default: 5)
    #[serde(default = "default_straddle_entry_days")]
    pub straddle_entry_days: usize,

    /// Straddle: Exit N trading days before earnings (default: 1)
    #[serde(default = "default_straddle_exit_days")]
    pub straddle_exit_days: usize,
}

fn default_straddle_entry_days() -> usize { 5 }
fn default_straddle_exit_days() -> usize { 1 }
```

#### 2.4 Create StraddlePricer (`cs-backtest/src/straddle_pricer.rs`)

```rust
use cs_analytics::{StraddlePriceComputer, PricingModel};
use crate::spread_pricer::{SpreadPricer, LegPricing, PricingError};

/// Pricer for straddle positions
///
/// Uses market data when available, falls back to Black-Scholes model pricing.
pub struct StraddlePricer {
    spread_pricer: SpreadPricer,
}

pub struct StraddlePricing {
    pub call: LegPricing,
    pub put: LegPricing,
    pub total_price: Decimal,
    pub source: PricingSource,
}

impl StraddlePricer {
    pub fn new(spread_pricer: SpreadPricer) -> Self {
        Self { spread_pricer }
    }

    pub fn with_pricing_model(mut self, model: PricingModel) -> Self {
        self.spread_pricer = self.spread_pricer.with_pricing_model(model);
        self
    }

    /// Price straddle - uses market data with model fallback
    pub fn price(
        &self,
        straddle: &Straddle,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
    ) -> Result<StraddlePricing, PricingError> {
        // Try to price call leg
        let call_result = self.spread_pricer.price_leg(
            &straddle.call_leg,
            chain_df,
            spot,
            timestamp,
        );

        // Try to price put leg
        let put_result = self.spread_pricer.price_leg(
            &straddle.put_leg,
            chain_df,
            spot,
            timestamp,
        );

        // Determine pricing source based on whether we used market or model
        let (call_pricing, put_pricing, source) = match (call_result, put_result) {
            (Ok(call), Ok(put)) => {
                // Both legs have market prices
                let source = if call.source == LegSource::Market && put.source == LegSource::Market {
                    PricingSource::Market
                } else {
                    PricingSource::Model
                };
                (call, put, source)
            }
            (Ok(call), Err(_)) => {
                // Call has price, put needs model
                let put = self.price_leg_with_model(&straddle.put_leg, chain_df, spot, timestamp)?;
                (call, put, PricingSource::Model)
            }
            (Err(_), Ok(put)) => {
                // Put has price, call needs model
                let call = self.price_leg_with_model(&straddle.call_leg, chain_df, spot, timestamp)?;
                (call, put, PricingSource::Model)
            }
            (Err(_), Err(_)) => {
                // Both need model pricing
                let call = self.price_leg_with_model(&straddle.call_leg, chain_df, spot, timestamp)?;
                let put = self.price_leg_with_model(&straddle.put_leg, chain_df, spot, timestamp)?;
                (call, put, PricingSource::Model)
            }
        };

        let total_price = call_pricing.price + put_pricing.price;

        Ok(StraddlePricing {
            call: call_pricing,
            put: put_pricing,
            total_price,
            source,
        })
    }

    /// Price a leg using Black-Scholes model
    fn price_leg_with_model(
        &self,
        leg: &OptionLeg,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
    ) -> Result<LegPricing, PricingError> {
        // Build IV surface from chain
        // Get IV at the leg's strike and expiration
        // Price using Black-Scholes

        self.spread_pricer.price_leg_with_model(leg, chain_df, spot, timestamp)
    }
}
```

#### 2.5 Create StraddleExecutor (`cs-backtest/src/straddle_executor.rs`)

```rust
/// Executor for straddle trades
///
/// Entry: Buy ATM call + put (debit)
/// Exit: Sell call + put (credit)
/// P&L = Exit credit - Entry debit
///
/// Uses minute data when available, falls back to PricingModel for
/// options that may not have market prices at exit (e.g., spot moved significantly).
pub struct StraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    pricer: StraddlePricer,
    max_entry_iv: Option<f64>,
}

impl<O, E> StraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(options_repo: Arc<O>, equity_repo: Arc<E>) -> Self {
        let spread_pricer = SpreadPricer::new();
        Self {
            options_repo,
            equity_repo,
            pricer: StraddlePricer::new(spread_pricer),
            max_entry_iv: None,
        }
    }

    pub fn with_pricing_model(mut self, model: PricingModel) -> Self {
        self.pricer = self.pricer.with_pricing_model(model);
        self
    }

    pub fn with_max_entry_iv(mut self, max_iv: Option<f64>) -> Self {
        self.max_entry_iv = max_iv;
        self
    }

    pub async fn execute_trade(
        &self,
        straddle: &Straddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> StraddleResult {
        match self.try_execute_trade(straddle, event, entry_time, exit_time).await {
            Ok(result) => result,
            Err(e) => self.create_failed_result(straddle, event, entry_time, exit_time, e),
        }
    }

    async fn try_execute_trade(
        &self,
        straddle: &Straddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Result<StraddleResult, ExecutionError> {
        // Get spot prices
        let entry_spot = self.equity_repo
            .get_spot_price(straddle.symbol(), entry_time)
            .await?;
        let exit_spot = self.equity_repo
            .get_spot_price(straddle.symbol(), exit_time)
            .await?;

        // Get option chain data (try minute bars first, then daily)
        let entry_chain = self.get_option_chain(straddle.symbol(), entry_time).await?;
        let exit_chain = self.get_option_chain(straddle.symbol(), exit_time).await?;

        // Price at entry (debit we pay)
        let entry_pricing = self.pricer.price(
            straddle,
            &entry_chain,
            entry_spot.to_f64(),
            entry_time,
        )?;

        // Validate minimum straddle price
        let min_straddle = Decimal::new(50, 2); // $0.50 minimum
        if entry_pricing.total_price < min_straddle {
            return Err(ExecutionError::InvalidSpread(format!(
                "Straddle price too small: {} < {}",
                entry_pricing.total_price, min_straddle
            )));
        }

        // Validate entry IV
        if let Some(max_iv) = self.max_entry_iv {
            if let Some(iv) = entry_pricing.call.iv {
                if iv > max_iv {
                    return Err(ExecutionError::InvalidSpread(format!(
                        "IV too high: {:.1}% > {:.1}%",
                        iv * 100.0, max_iv * 100.0
                    )));
                }
            }
        }

        // Price at exit (credit we receive)
        // This may use model pricing if market data unavailable
        let exit_pricing = self.pricer.price(
            straddle,
            &exit_chain,
            exit_spot.to_f64(),
            exit_time,
        )?;

        // P&L = Exit value - Entry cost (profit when straddle appreciated)
        let pnl = exit_pricing.total_price - entry_pricing.total_price;
        let pnl_pct = if entry_pricing.total_price != Decimal::ZERO {
            (pnl / entry_pricing.total_price) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Net Greeks (long call + long put)
        let (net_delta, net_gamma, net_theta, net_vega) = compute_net_greeks(&entry_pricing);

        // IV change (positive = good for long straddle)
        let (iv_entry, iv_exit, iv_change) = compute_iv_change(&entry_pricing, &exit_pricing);

        // Expected move at entry
        let expected_move_pct = if entry_spot.to_f64() > 0.0 {
            Some((entry_pricing.total_price.to_f64().unwrap_or(0.0) / entry_spot.to_f64()) * 100.0)
        } else {
            None
        };

        // Spot move
        let spot_move = exit_spot.to_f64() - entry_spot.to_f64();
        let spot_move_pct = if entry_spot.to_f64() != 0.0 {
            (spot_move / entry_spot.to_f64()) * 100.0
        } else {
            0.0
        };

        // P&L attribution
        let (delta_pnl, gamma_pnl, theta_pnl, vega_pnl, unexplained_pnl) =
            calculate_pnl_attribution(
                &entry_pricing,
                &exit_pricing,
                entry_spot.to_f64(),
                exit_spot.to_f64(),
                entry_time,
                exit_time,
                pnl,
            );

        Ok(StraddleResult {
            symbol: straddle.symbol().to_string(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            strike: straddle.strike(),
            expiration: straddle.expiration(),
            entry_time,
            call_entry_price: entry_pricing.call.price,
            put_entry_price: entry_pricing.put.price,
            entry_debit: entry_pricing.total_price,
            exit_time,
            call_exit_price: exit_pricing.call.price,
            put_exit_price: exit_pricing.put.price,
            exit_credit: exit_pricing.total_price,
            exit_pricing_method: exit_pricing.source,
            pnl,
            pnl_pct,
            net_delta,
            net_gamma,
            net_theta,
            net_vega,
            iv_entry,
            iv_exit,
            iv_change,
            delta_pnl,
            gamma_pnl,
            theta_pnl,
            vega_pnl,
            unexplained_pnl,
            spot_at_entry: entry_spot.to_f64(),
            spot_at_exit: exit_spot.to_f64(),
            spot_move,
            spot_move_pct,
            expected_move_pct,
            success: true,
            failure_reason: None,
        })
    }

    /// Get option chain - tries minute bars first, then daily
    async fn get_option_chain(
        &self,
        symbol: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<DataFrame, ExecutionError> {
        // Try minute bars first for precise pricing
        if let Ok(minute_chain) = self.options_repo
            .get_minute_option_bars(symbol, timestamp)
            .await
        {
            return Ok(minute_chain);
        }

        // Fall back to daily bars
        self.options_repo
            .get_option_bars(symbol, timestamp.date_naive())
            .await
            .map_err(ExecutionError::Repository)
    }

    fn create_failed_result(
        &self,
        straddle: &Straddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        error: ExecutionError,
    ) -> StraddleResult {
        let failure_reason = match error {
            ExecutionError::NoSpotPrice => FailureReason::NoSpotPrice,
            ExecutionError::Repository(_) => FailureReason::NoOptionsData,
            ExecutionError::Pricing(_) => FailureReason::PricingError(error.to_string()),
            ExecutionError::InvalidSpread(_) => FailureReason::DegenerateSpread,
        };

        StraddleResult {
            symbol: straddle.symbol().to_string(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            strike: straddle.strike(),
            expiration: straddle.expiration(),
            entry_time,
            call_entry_price: Decimal::ZERO,
            put_entry_price: Decimal::ZERO,
            entry_debit: Decimal::ZERO,
            exit_time,
            call_exit_price: Decimal::ZERO,
            put_exit_price: Decimal::ZERO,
            exit_credit: Decimal::ZERO,
            exit_pricing_method: PricingSource::Market,
            pnl: Decimal::ZERO,
            pnl_pct: Decimal::ZERO,
            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,
            iv_entry: None,
            iv_exit: None,
            iv_change: None,
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 0.0,
            spot_at_exit: 0.0,
            spot_move: 0.0,
            spot_move_pct: 0.0,
            expected_move_pct: None,
            success: false,
            failure_reason: Some(failure_reason),
        }
    }
}

/// Compute net Greeks for long straddle (long call + long put)
fn compute_net_greeks(pricing: &StraddlePricing) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    match (pricing.call.greeks, pricing.put.greeks) {
        (Some(call_g), Some(put_g)) => {
            // Long both legs: add greeks
            let net_delta = call_g.delta + put_g.delta;  // ~0 for ATM straddle
            let net_gamma = call_g.gamma + put_g.gamma;  // Positive (long gamma)
            let net_theta = call_g.theta + put_g.theta;  // Negative (time decay)
            let net_vega = call_g.vega + put_g.vega;     // Positive (want IV expansion)

            (Some(net_delta), Some(net_gamma), Some(net_theta), Some(net_vega))
        }
        _ => (None, None, None, None),
    }
}

/// Compute IV change between entry and exit
fn compute_iv_change(
    entry_pricing: &StraddlePricing,
    exit_pricing: &StraddlePricing,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    // Use average of call and put IV
    let iv_entry = match (entry_pricing.call.iv, entry_pricing.put.iv) {
        (Some(c), Some(p)) => Some((c + p) / 2.0),
        (Some(c), None) => Some(c),
        (None, Some(p)) => Some(p),
        _ => None,
    };

    let iv_exit = match (exit_pricing.call.iv, exit_pricing.put.iv) {
        (Some(c), Some(p)) => Some((c + p) / 2.0),
        (Some(c), None) => Some(c),
        (None, Some(p)) => Some(p),
        _ => None,
    };

    let iv_change = match (iv_entry, iv_exit) {
        (Some(entry), Some(exit)) => Some(exit - entry),
        _ => None,
    };

    (iv_entry, iv_exit, iv_change)
}
```

#### 2.6 Extend BacktestUseCase (`cs-backtest/src/backtest_use_case.rs`)

```rust
impl<Earn, Opt, Eq> BacktestUseCase<Earn, Opt, Eq> {
    pub async fn execute(...) -> Result<BacktestResult, BacktestError> {
        match self.config.spread {
            SpreadType::IronButterfly => self.execute_iron_butterfly(...).await,
            SpreadType::Calendar => self.execute_calendar_spread(...).await,
            SpreadType::Straddle => self.execute_straddle(...).await,  // NEW
        }
    }

    async fn execute_straddle(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        on_progress: Option<Box<dyn Fn(SessionProgress) + Send + Sync>>,
    ) -> Result<BacktestResult, BacktestError> {
        let mut all_results: Vec<TradeResult> = Vec::new();
        let mut dropped_events: Vec<TradeGenerationError> = Vec::new();
        let mut sessions_processed = 0;
        let mut total_opportunities = 0;

        // Create straddle strategy and timing
        let strategy = StraddleStrategy::default();

        let timing = StraddleTradeTiming::new(self.config.timing)
            .with_entry_days(self.config.straddle_entry_days)
            .with_exit_days(self.config.straddle_exit_days);

        info!(
            entry_days = self.config.straddle_entry_days,
            exit_days = self.config.straddle_exit_days,
            "Starting straddle backtest"
        );

        for session_date in TradingCalendar::trading_days_between(start_date, end_date) {
            sessions_processed += 1;

            // Load earnings for wider window (need events where entry falls on session_date)
            // Entry is N days before earnings, so look for earnings N days ahead
            let lookahead = self.config.straddle_entry_days as i64 + 5;  // Buffer for weekends
            let events_end = session_date + chrono::Duration::days(lookahead);
            let events = self.earnings_repo
                .load_earnings(session_date, events_end, self.config.symbols.as_deref())
                .await
                .map_err(|e| BacktestError::Repository(e.to_string()))?;

            // Filter: Entry date == session_date
            let to_enter: Vec<_> = events
                .iter()
                .filter(|e| timing.entry_date(e) == session_date)
                .filter(|e| self.passes_market_cap_filter(e))
                .cloned()
                .collect();

            if to_enter.is_empty() {
                if let Some(ref callback) = on_progress {
                    callback(SessionProgress {
                        session_date,
                        entries_count: 0,
                        events_found: 0,
                    });
                }
                continue;
            }

            debug!(
                session_date = %session_date,
                events_count = to_enter.len(),
                "Processing straddle session"
            );

            // Process events
            let session_results: Vec<_> = if self.config.parallel {
                let futures: Vec<_> = to_enter
                    .iter()
                    .map(|event| self.process_straddle_event(event, &strategy, &timing))
                    .collect();
                futures::future::join_all(futures).await
            } else {
                let mut results = Vec::new();
                for event in &to_enter {
                    results.push(
                        self.process_straddle_event(event, &strategy, &timing).await
                    );
                }
                results
            };

            let mut session_entries = 0;
            for result in session_results {
                total_opportunities += 1;
                match result {
                    Ok(straddle_result) => {
                        all_results.push(TradeResult::Straddle(straddle_result));
                        session_entries += 1;
                    }
                    Err(e) => dropped_events.push(e),
                }
            }

            if let Some(ref callback) = on_progress {
                callback(SessionProgress {
                    session_date,
                    entries_count: session_entries,
                    events_found: to_enter.len(),
                });
            }
        }

        let total_entries = all_results.len();

        info!(
            sessions_processed,
            total_opportunities,
            results_count = total_entries,
            dropped_count = dropped_events.len(),
            "Straddle backtest completed"
        );

        Ok(BacktestResult {
            results: all_results,
            sessions_processed,
            total_entries,
            total_opportunities,
            dropped_events,
        })
    }

    async fn process_straddle_event(
        &self,
        event: &EarningsEvent,
        strategy: &StraddleStrategy,
        timing: &StraddleTradeTiming,
    ) -> Result<StraddleResult, TradeGenerationError> {
        let entry_time = timing.entry_datetime(event);
        let exit_time = timing.exit_datetime(event);
        let entry_date = entry_time.date_naive();

        // Get spot price at entry
        let spot = self.equity_repo
            .get_spot_price(&event.symbol, entry_time)
            .await
            .map_err(|_| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_SPOT_PRICE".into(),
                details: Some(format!("No spot price at {}", entry_time)),
                phase: "spot_price".into(),
            })?;

        // Get available expirations and strikes at entry
        let expirations = self.options_repo
            .get_available_expirations(&event.symbol, entry_date)
            .await
            .unwrap_or_default();

        if expirations.is_empty() {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_EXPIRATIONS".into(),
                details: None,
                phase: "chain_data".into(),
            });
        }

        // Filter expirations to those after earnings
        let valid_expirations: Vec<_> = expirations
            .iter()
            .filter(|&&exp| exp > event.earnings_date)
            .copied()
            .collect();

        if valid_expirations.is_empty() {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_POST_EARNINGS_EXPIRATION".into(),
                details: Some("Need expiration after earnings date".into()),
                phase: "chain_data".into(),
            });
        }

        let strikes = self.options_repo
            .get_available_strikes(&event.symbol, valid_expirations[0], entry_date)
            .await
            .unwrap_or_default();

        if strikes.is_empty() {
            return Err(TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "NO_STRIKES".into(),
                details: None,
                phase: "chain_data".into(),
            });
        }

        let chain_data = OptionChainData {
            expirations: valid_expirations,
            strikes,
            deltas: None,
            volumes: None,
            iv_ratios: None,
            iv_surface: None,
        };

        // Select straddle
        let straddle = strategy.select_straddle(event, &spot, &chain_data)
            .map_err(|e| TradeGenerationError {
                symbol: event.symbol.clone(),
                earnings_date: event.earnings_date,
                earnings_time: event.earnings_time,
                reason: "STRATEGY_SELECTION_FAILED".into(),
                details: Some(e.to_string()),
                phase: "strategy".into(),
            })?;

        // Execute trade
        let executor = StraddleExecutor::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
        )
        .with_pricing_model(self.config.pricing_model)
        .with_max_entry_iv(self.config.max_entry_iv);

        let result = executor.execute_trade(&straddle, event, entry_time, exit_time).await;

        if !result.success {
            return Err(TradeGenerationError {
                symbol: result.symbol,
                earnings_date: result.earnings_date,
                earnings_time: result.earnings_time,
                reason: result.failure_reason.map(|r| format!("{:?}", r)).unwrap_or("UNKNOWN".into()),
                details: None,
                phase: "execution".into(),
            });
        }

        Ok(result)
    }
}
```

---

### Phase 3: CLI Integration (cs-cli)

#### 3.1 Add CLI Arguments

```rust
// In backtest command
/// Entry N trading days before earnings (straddle only)
#[arg(long, default_value = "5")]
straddle_entry_days: usize,

/// Exit N trading days before earnings (straddle only)
#[arg(long, default_value = "1")]
straddle_exit_days: usize,
```

#### 3.2 Extend SpreadType Parsing

```rust
let spread = match spread_str.to_lowercase().as_str() {
    "calendar" | "cal" => cs_backtest::SpreadType::Calendar,
    "iron-butterfly" | "iron_butterfly" | "butterfly" => cs_backtest::SpreadType::IronButterfly,
    "straddle" | "long-straddle" => cs_backtest::SpreadType::Straddle,
    _ => anyhow::bail!("Invalid spread type: {}. Must be 'calendar', 'iron-butterfly', or 'straddle'", spread_str),
};

// Validate incompatible options
if spread == "straddle" && strike_match_mode_str.is_some() {
    anyhow::bail!("--strike-match-mode is not applicable to straddle strategy");
}
if spread == "straddle" && wing_width.is_some() {
    anyhow::bail!("--wing-width is not applicable to straddle strategy");
}
```

#### 3.3 Add Output Formatting

```rust
// In results display
cs_backtest::SpreadType::Straddle => {
    println!("  Spread:        Straddle (Long Volatility)");
    println!("  Entry:         {} trading days before earnings", config.straddle_entry_days);
    println!("  Exit:          {} trading day(s) before earnings", config.straddle_exit_days);
    println!("  Expiration:    First expiry after earnings");
}
```

---

### Phase 4: Testing

#### 4.1 Unit Tests

```rust
// cs-domain/src/entities.rs
#[test]
fn test_straddle_valid() {
    let call = OptionLeg::new(
        "AAPL".to_string(),
        Strike::new(Decimal::new(180, 0)).unwrap(),
        NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
        OptionType::Call,
    );
    let put = OptionLeg::new(
        "AAPL".to_string(),
        Strike::new(Decimal::new(180, 0)).unwrap(),
        NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
        OptionType::Put,
    );

    let straddle = Straddle::new(call, put);
    assert!(straddle.is_ok());
    let s = straddle.unwrap();
    assert_eq!(s.symbol(), "AAPL");
    assert_eq!(s.strike().value(), Decimal::new(180, 0));
}

#[test]
fn test_straddle_strike_mismatch() {
    let call = OptionLeg::new(
        "AAPL".to_string(),
        Strike::new(Decimal::new(180, 0)).unwrap(),
        NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
        OptionType::Call,
    );
    let put = OptionLeg::new(
        "AAPL".to_string(),
        Strike::new(Decimal::new(175, 0)).unwrap(),  // Different strike
        NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
        OptionType::Put,
    );

    let straddle = Straddle::new(call, put);
    assert!(matches!(straddle.unwrap_err(), ValidationError::StrikeMismatch { .. }));
}

#[test]
fn test_straddle_expiration_mismatch() {
    let call = OptionLeg::new(
        "AAPL".to_string(),
        Strike::new(Decimal::new(180, 0)).unwrap(),
        NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
        OptionType::Call,
    );
    let put = OptionLeg::new(
        "AAPL".to_string(),
        Strike::new(Decimal::new(180, 0)).unwrap(),
        NaiveDate::from_ymd_opt(2025, 1, 24).unwrap(),  // Different expiry
        OptionType::Put,
    );

    let straddle = Straddle::new(call, put);
    assert!(matches!(straddle.unwrap_err(), ValidationError::ExpirationMismatch { .. }));
}

// cs-domain/src/services/trading_calendar.rs
#[test]
fn test_n_trading_days_before() {
    // Friday Jan 10, 2025 -> 5 trading days before = Fri Jan 3
    let date = NaiveDate::from_ymd_opt(2025, 1, 10).unwrap();
    let result = TradingCalendar::n_trading_days_before(date, 5);
    assert_eq!(result, NaiveDate::from_ymd_opt(2025, 1, 3).unwrap());
}

#[test]
fn test_n_trading_days_before_with_weekend() {
    // Monday Jan 13, 2025 -> 5 trading days before = Mon Jan 6
    let date = NaiveDate::from_ymd_opt(2025, 1, 13).unwrap();
    let result = TradingCalendar::n_trading_days_before(date, 5);
    assert_eq!(result, NaiveDate::from_ymd_opt(2025, 1, 6).unwrap());
}

#[test]
fn test_n_trading_days_before_zero() {
    let date = NaiveDate::from_ymd_opt(2025, 1, 10).unwrap();
    let result = TradingCalendar::n_trading_days_before(date, 0);
    assert_eq!(result, date);
}

// cs-domain/src/services/straddle_timing.rs
#[test]
fn test_straddle_timing_entry_exit() {
    let timing = StraddleTradeTiming::new(TimingConfig::default())
        .with_entry_days(5)
        .with_exit_days(1);

    let event = EarningsEvent::new(
        "AAPL".into(),
        NaiveDate::from_ymd_opt(2025, 1, 30).unwrap(),  // Thursday earnings
        EarningsTime::AfterMarketClose,
    );

    // Entry: 5 trading days before Jan 30 = Jan 23 (Thursday)
    let entry = timing.entry_date(&event);
    assert_eq!(entry, NaiveDate::from_ymd_opt(2025, 1, 23).unwrap());

    // Exit: 1 trading day before Jan 30 = Jan 29 (Wednesday)
    let exit = timing.exit_date(&event);
    assert_eq!(exit, NaiveDate::from_ymd_opt(2025, 1, 29).unwrap());

    // Holding period: 4 trading days
    assert_eq!(timing.holding_period(), 4);
}
```

#### 4.2 Integration Tests

```rust
#[tokio::test]
async fn test_straddle_backtest_basic() {
    let config = BacktestConfig {
        spread: SpreadType::Straddle,
        straddle_entry_days: 5,
        straddle_exit_days: 1,
        ..Default::default()
    };

    let use_case = BacktestUseCase::new(
        mock_earnings_repo(),
        mock_options_repo(),
        mock_equity_repo(),
        config,
    );

    let result = use_case.execute(
        NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2025, 3, 31).unwrap(),
        OptionType::Call,  // Not used for straddle
        None,
    ).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_straddle_selects_post_earnings_expiry() {
    // Mock earnings on Jan 30
    // Should select first expiry AFTER Jan 30 (e.g., Feb 7)

    let strategy = StraddleStrategy::default();
    let event = EarningsEvent::new(
        "AAPL".into(),
        NaiveDate::from_ymd_opt(2025, 1, 30).unwrap(),
        EarningsTime::AfterMarketClose,
    );

    let expirations = vec![
        NaiveDate::from_ymd_opt(2025, 1, 24).unwrap(),  // Before earnings
        NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),  // After earnings (closest)
        NaiveDate::from_ymd_opt(2025, 2, 7).unwrap(),   // After earnings
    ];

    let chain_data = OptionChainData {
        expirations,
        strikes: vec![Strike::new(Decimal::new(180, 0)).unwrap()],
        deltas: None,
        volumes: None,
        iv_ratios: None,
        iv_surface: None,
    };

    let spot = SpotPrice::new(Decimal::new(180, 0), Utc::now());
    let straddle = strategy.select_straddle(&event, &spot, &chain_data).unwrap();

    // Should select Jan 31 (first after earnings)
    assert_eq!(straddle.expiration(), NaiveDate::from_ymd_opt(2025, 1, 31).unwrap());
}
```

---

## File Changes Summary

### New Files
| File | Purpose |
|------|---------|
| `cs-domain/src/services/straddle_timing.rs` | Straddle-specific timing service |
| `cs-domain/src/strategies/straddle.rs` | Straddle selection strategy |
| `cs-backtest/src/straddle_pricer.rs` | Straddle pricing with model fallback |
| `cs-backtest/src/straddle_executor.rs` | Straddle trade execution |

### Modified Files
| File | Changes |
|------|---------|
| `cs-domain/src/entities.rs` | Add `Straddle`, `StraddleResult`, `PricingSource` |
| `cs-domain/src/services/mod.rs` | Export `StraddleTradeTiming` |
| `cs-domain/src/services/trading_calendar.rs` | Add `n_trading_days_before`, `trading_days_count` |
| `cs-domain/src/strategies/mod.rs` | Add `Straddle` to `OptionStrategy`, `select_straddle` to trait |
| `cs-backtest/src/lib.rs` | Export new modules |
| `cs-backtest/src/config.rs` | Add `Straddle` to `SpreadType`, straddle config fields |
| `cs-backtest/src/backtest_use_case.rs` | Add `Straddle` to `TradeResult`, `execute_straddle` method |
| `cs-cli/src/main.rs` | Add CLI arguments, output formatting |

---

## Risk Considerations

### Theta Decay
- Long straddle loses money to theta over 4-day holding period
- IV expansion must overcome theta decay for profit
- Consider tracking theta P&L attribution separately

### Pricing at Exit
- If spot moves significantly, the original ATM options may become illiquid
- `PricingModel` fallback ensures we can always price the position
- Track `exit_pricing_method` to analyze model vs market pricing accuracy

### Expiration Selection
- Must select expiration AFTER earnings to ensure options retain value at exit
- Avoid weekly expirations if they might expire exactly on earnings day

### Weekend Effects
- Entry 5 trading days before Monday earnings = previous Monday (not weekend)
- `TradingCalendar::n_trading_days_before` handles this correctly

---

## Usage Examples

### CLI Usage

```bash
# Basic straddle backtest
./target/release/cs backtest \
  --spread straddle \
  --start 2024-01-01 \
  --end 2024-12-31 \
  --output straddle_results.json

# Custom entry/exit timing
./target/release/cs backtest \
  --spread straddle \
  --straddle-entry-days 7 \
  --straddle-exit-days 2 \
  --start 2024-01-01 \
  --end 2024-12-31

# With market cap filter
./target/release/cs backtest \
  --spread straddle \
  --min-market-cap 10000000000 \
  --start 2024-01-01 \
  --end 2024-12-31
```

---

## Future Enhancements

1. **Strangle variant** - OTM call + OTM put (cheaper premium, needs larger move)
2. **IV rank filtering** - Only enter when IV is below historical average
3. **Earnings magnitude analysis** - Compare expected move to historical earnings moves
4. **Variable holding period** - Exit when IV expansion target reached
5. **Delta hedging** - Track delta exposure and suggest hedging adjustments
