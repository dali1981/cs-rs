//! Buying power requirement (BPR) accounting types and engines.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::accounting::MarginCalculator;
use crate::CONTRACT_MULTIPLIER;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionRight {
    Call,
    Put,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionLegInput {
    pub right: OptionRight,
    pub strike: Decimal,
    pub expiry: NaiveDate,
    /// Signed contract quantity (+long, -short).
    pub qty: i32,
    /// Per-share premium (not multiplied by 100).
    pub mark_premium: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HedgeInput {
    pub symbol: String,
    /// Signed shares (+long, -short).
    pub shares: i32,
    /// Spot price for the hedge snapshot.
    pub spot: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BprInputs {
    pub ts: DateTime<Utc>,
    pub underlying_symbol: String,
    pub underlying_spot: Decimal,
    pub option_legs: Vec<OptionLegInput>,
    pub hedge: Option<HedgeInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BprSnapshot {
    pub ts: DateTime<Utc>,
    pub option_initial: Decimal,
    pub option_maint: Decimal,
    pub hedge_initial: Decimal,
    pub hedge_maint: Decimal,
}

impl BprSnapshot {
    pub fn total_initial(&self) -> Decimal {
        self.option_initial + self.hedge_initial
    }

    pub fn total_maint(&self) -> Decimal {
        self.option_maint + self.hedge_maint
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BprSummary {
    pub max_total_initial: Decimal,
    pub max_total_maint: Decimal,
    pub avg_total_initial: Decimal,
    pub avg_total_maint: Decimal,
    pub max_option_maint: Decimal,
    pub max_hedge_maint: Decimal,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BprTimeline {
    pub snapshots: Vec<BprSnapshot>,
    pub summary: BprSummary,
}

impl BprTimeline {
    pub fn new(snapshots: Vec<BprSnapshot>) -> Self {
        let mut summary = BprSummary::default();
        if snapshots.is_empty() {
            return Self { snapshots, summary };
        }

        let mut total_initial_sum = Decimal::ZERO;
        let mut total_maint_sum = Decimal::ZERO;

        for snap in &snapshots {
            let total_initial = snap.total_initial();
            let total_maint = snap.total_maint();
            total_initial_sum += total_initial;
            total_maint_sum += total_maint;

            if total_initial > summary.max_total_initial {
                summary.max_total_initial = total_initial;
            }
            if total_maint > summary.max_total_maint {
                summary.max_total_maint = total_maint;
            }
            if snap.option_maint > summary.max_option_maint {
                summary.max_option_maint = snap.option_maint;
            }
            if snap.hedge_maint > summary.max_hedge_maint {
                summary.max_hedge_maint = snap.hedge_maint;
            }
        }

        let count = Decimal::from(snapshots.len() as u32);
        summary.avg_total_initial = total_initial_sum / count;
        summary.avg_total_maint = total_maint_sum / count;

        Self { snapshots, summary }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarginMode {
    Off,
    #[serde(alias = "regt")]
    #[serde(alias = "reg-t")]
    RegT,
    Cash,
    #[serde(alias = "pm")]
    #[serde(alias = "portfolio-margin")]
    PortfolioMargin,
}

impl Default for MarginMode {
    fn default() -> Self {
        MarginMode::Off
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockMarginMode {
    #[serde(alias = "regt")]
    #[serde(alias = "reg-t")]
    RegT,
    Cash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockMarginConfig {
    pub stock_margin_mode: StockMarginMode,
    pub long_initial_rate: Decimal,
    pub short_initial_rate: Decimal,
    pub long_maint_rate: Decimal,
    pub short_maint_rate: Decimal,
}

impl Default for StockMarginConfig {
    fn default() -> Self {
        Self {
            stock_margin_mode: StockMarginMode::RegT,
            long_initial_rate: Decimal::new(50, 2),
            short_initial_rate: Decimal::new(150, 2),
            long_maint_rate: Decimal::new(50, 2),
            short_maint_rate: Decimal::new(150, 2),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionsMarginConfig {
    pub regt_variant: String,
}

impl Default for OptionsMarginConfig {
    fn default() -> Self {
        Self {
            regt_variant: "cboe_like".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarginConfig {
    pub mode: MarginMode,
    pub use_maintenance: bool,
    pub stock: StockMarginConfig,
    pub options: OptionsMarginConfig,
}

impl Default for MarginConfig {
    fn default() -> Self {
        Self {
            mode: MarginMode::Off,
            use_maintenance: true,
            stock: StockMarginConfig::default(),
            options: OptionsMarginConfig::default(),
        }
    }
}

pub trait OptionMarginEngine: Send + Sync {
    fn compute(&self, inputs: &BprInputs) -> (Decimal, Decimal);
}

pub trait StockMarginEngine: Send + Sync {
    fn compute(&self, hedge: &HedgeInput, cfg: &StockMarginConfig) -> (Decimal, Decimal);
}

pub trait MarginEngine: Send + Sync {
    fn compute_snapshot(&self, inputs: &BprInputs, cfg: &MarginConfig) -> BprSnapshot;
}

pub struct CompositeMarginEngine {
    opt: Box<dyn OptionMarginEngine>,
    stock: Box<dyn StockMarginEngine>,
}

impl CompositeMarginEngine {
    pub fn new(opt: Box<dyn OptionMarginEngine>, stock: Box<dyn StockMarginEngine>) -> Self {
        Self { opt, stock }
    }
}

impl MarginEngine for CompositeMarginEngine {
    fn compute_snapshot(&self, inputs: &BprInputs, cfg: &MarginConfig) -> BprSnapshot {
        let (opt_initial, opt_maint) = self.opt.compute(inputs);
        let (hedge_initial, hedge_maint) = match inputs.hedge.as_ref() {
            Some(hedge) => self.stock.compute(hedge, &cfg.stock),
            None => (Decimal::ZERO, Decimal::ZERO),
        };

        BprSnapshot {
            ts: inputs.ts,
            option_initial: opt_initial,
            option_maint: opt_maint,
            hedge_initial,
            hedge_maint,
        }
    }
}

pub struct OffOptionEngine;

impl OptionMarginEngine for OffOptionEngine {
    fn compute(&self, _inputs: &BprInputs) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }
}

pub struct RegTOptionEngine {
    calc: MarginCalculator,
}

impl Default for RegTOptionEngine {
    fn default() -> Self {
        Self { calc: MarginCalculator::default() }
    }
}

impl OptionMarginEngine for RegTOptionEngine {
    fn compute(&self, inputs: &BprInputs) -> (Decimal, Decimal) {
        let mut total = Decimal::ZERO;
        let contract_multiplier = Decimal::from(CONTRACT_MULTIPLIER);
        let spot = inputs.underlying_spot;

        if let Some(margin) = self.straddle_margin(&inputs.option_legs, spot) {
            return (margin, margin);
        }

        if let Some(margin) = self.iron_butterfly_margin(&inputs.option_legs) {
            return (margin, margin);
        }

        if let Some(margin) = self.calendar_spread_margin(&inputs.option_legs) {
            return (margin, margin);
        }

        if let Some(margin) = self.calendar_straddle_margin(&inputs.option_legs) {
            return (margin, margin);
        }

        for leg in &inputs.option_legs {
            let dte = (leg.expiry - inputs.ts.date_naive())
                .num_days()
                .max(0) as u32;

            let premium = leg.mark_premium.abs();
            let per_share_margin = if leg.qty >= 0 {
                self.calc.long_option_margin(premium, dte)
            } else {
                let is_call = matches!(leg.right, OptionRight::Call);
                self.calc.naked_short_equity_margin(premium, spot, leg.strike, is_call)
            };

            let qty = Decimal::from(leg.qty.abs() as u32);
            total += per_share_margin * contract_multiplier * qty;
        }

        (total, total)
    }
}

impl RegTOptionEngine {
    fn straddle_margin(&self, legs: &[OptionLegInput], spot: Decimal) -> Option<Decimal> {
        if legs.len() != 2 {
            return None;
        }

        let call = legs.iter().find(|l| matches!(l.right, OptionRight::Call))?;
        let put = legs.iter().find(|l| matches!(l.right, OptionRight::Put))?;

        if call.expiry != put.expiry || call.strike != put.strike {
            return None;
        }

        let call_sign = call.qty.signum();
        let put_sign = put.qty.signum();
        if call_sign == 0 || put_sign == 0 || call_sign != put_sign {
            return None;
        }

        if call.qty.abs() != put.qty.abs() {
            return None;
        }

        let qty = Decimal::from(call.qty.abs() as u32);
        let call_premium = call.mark_premium.abs();
        let put_premium = put.mark_premium.abs();

        let per_share = if call_sign > 0 {
            self.calc.long_straddle_margin(call_premium, put_premium)
        } else {
            self.calc.short_straddle_margin(call_premium, put_premium, spot, call.strike)
        };

        Some(per_share * Decimal::from(CONTRACT_MULTIPLIER) * qty)
    }

    fn iron_butterfly_margin(&self, legs: &[OptionLegInput]) -> Option<Decimal> {
        if legs.len() != 4 {
            return None;
        }

        let expiry = legs.first()?.expiry;
        if !legs.iter().all(|l| l.expiry == expiry) {
            return None;
        }

        let calls: Vec<&OptionLegInput> = legs.iter().filter(|l| matches!(l.right, OptionRight::Call)).collect();
        let puts: Vec<&OptionLegInput> = legs.iter().filter(|l| matches!(l.right, OptionRight::Put)).collect();
        if calls.len() != 2 || puts.len() != 2 {
            return None;
        }

        let mut center_call = None;
        let mut center_put = None;
        for call in &calls {
            if let Some(put) = puts.iter().find(|p| p.strike == call.strike && p.qty.signum() == call.qty.signum()) {
                center_call = Some(*call);
                center_put = Some(*put);
                break;
            }
        }

        let (center_call, center_put) = (center_call?, center_put?);
        if center_call.qty.abs() != center_put.qty.abs() {
            return None;
        }

        let center_strike = center_call.strike;
        let wing_call = calls.iter().find(|c| c.strike != center_strike)?;
        let wing_put = puts.iter().find(|p| p.strike != center_strike)?;

        let wing_width_call = (wing_call.strike - center_strike).abs();
        let wing_width_put = (center_strike - wing_put.strike).abs();
        let wing_width = wing_width_call.max(wing_width_put);
        if wing_width <= Decimal::ZERO {
            return None;
        }

        let qty = Decimal::from(center_call.qty.abs() as u32);
        let net_premium = legs.iter().fold(Decimal::ZERO, |acc, leg| {
            acc + (Decimal::from(leg.qty) * leg.mark_premium)
        });

        let per_share = if center_call.qty.signum() < 0 {
            let net_credit = net_premium.abs();
            self.calc.iron_butterfly_margin(net_credit, wing_width)
        } else {
            let net_debit = net_premium.abs();
            self.calc.debit_spread_margin(net_debit)
        };

        Some(per_share * Decimal::from(CONTRACT_MULTIPLIER) * qty)
    }

    fn calendar_spread_margin(&self, legs: &[OptionLegInput]) -> Option<Decimal> {
        if legs.len() != 2 {
            return None;
        }

        let left = &legs[0];
        let right = &legs[1];
        if left.right != right.right || left.expiry == right.expiry {
            return None;
        }
        if left.qty.signum() == 0 || right.qty.signum() == 0 || left.qty.signum() == right.qty.signum() {
            return None;
        }
        if left.qty.abs() != right.qty.abs() {
            return None;
        }

        let qty = Decimal::from(left.qty.abs() as u32);
        let net_premium = Decimal::from(left.qty) * left.mark_premium
            + Decimal::from(right.qty) * right.mark_premium;
        let per_share = net_premium.abs();

        Some(per_share * Decimal::from(CONTRACT_MULTIPLIER) * qty)
    }

    fn calendar_straddle_margin(&self, legs: &[OptionLegInput]) -> Option<Decimal> {
        if legs.len() != 4 {
            return None;
        }

        let mut expiries: Vec<NaiveDate> = legs.iter().map(|l| l.expiry).collect();
        expiries.sort();
        expiries.dedup();
        if expiries.len() != 2 {
            return None;
        }

        let qty = legs.iter().map(|l| l.qty.abs() as u32).max().unwrap_or(1);
        let net_premium = legs.iter().fold(Decimal::ZERO, |acc, leg| {
            acc + (Decimal::from(leg.qty) * leg.mark_premium)
        });
        let per_share = net_premium.abs();

        Some(per_share * Decimal::from(CONTRACT_MULTIPLIER) * Decimal::from(qty))
    }
}

pub struct RegTStockEngine;

impl StockMarginEngine for RegTStockEngine {
    fn compute(&self, hedge: &HedgeInput, cfg: &StockMarginConfig) -> (Decimal, Decimal) {
        let notional = Decimal::from(hedge.shares.abs() as u32) * hedge.spot;

        let (long_rate_i, short_rate_i, long_rate_m, short_rate_m) = match cfg.stock_margin_mode {
            StockMarginMode::Cash => (
                Decimal::ONE,
                Decimal::ONE,
                Decimal::ONE,
                Decimal::ONE,
            ),
            StockMarginMode::RegT => (
                cfg.long_initial_rate,
                cfg.short_initial_rate,
                cfg.long_maint_rate,
                cfg.short_maint_rate,
            ),
        };

        if hedge.shares >= 0 {
            (notional * long_rate_i, notional * long_rate_m)
        } else {
            (notional * short_rate_i, notional * short_rate_m)
        }
    }
}

pub fn margin_engine_for_config(cfg: &MarginConfig) -> Option<Box<dyn MarginEngine>> {
    match cfg.mode {
        MarginMode::Off => None,
        MarginMode::RegT | MarginMode::Cash | MarginMode::PortfolioMargin => {
            let opt: Box<dyn OptionMarginEngine> = match cfg.mode {
                MarginMode::Off => Box::new(OffOptionEngine),
                MarginMode::RegT | MarginMode::Cash | MarginMode::PortfolioMargin => Box::new(RegTOptionEngine::default()),
            };
            let stock: Box<dyn StockMarginEngine> = Box::new(RegTStockEngine);
            Some(Box::new(CompositeMarginEngine::new(opt, stock)))
        }
    }
}
