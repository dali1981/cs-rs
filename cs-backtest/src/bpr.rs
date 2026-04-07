//! BPR timeline computation for trades.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::BTreeSet;

use cs_domain::{
    BprInputs, BprSnapshot, BprTimeline, HedgeInput, MarginConfig, MarginMode, OptionLegInput,
    OptionRight, margin_engine_for_config,
    CalendarSpreadResult, CompositeTrade, HedgePosition, IronButterflyResult, OptionsDataRepository,
    EquityDataRepository, StraddleResult, CalendarStraddleResult, TradeResult,
};

use crate::composite_pricer::CompositePricing;
use crate::execution::{ExecutionError, ExecutableTrade, TradePricer};
use crate::iv_surface_builder::build_iv_surface_minute_aligned;
use crate::timing_strategy::TimingStrategy;

pub trait HasBprTimeline {
    fn bpr_timeline(&self) -> Option<&BprTimeline>;
    fn set_bpr_timeline(&mut self, timeline: Option<BprTimeline>);
}

impl HasBprTimeline for CalendarSpreadResult {
    fn bpr_timeline(&self) -> Option<&BprTimeline> {
        self.bpr_timeline.as_ref()
    }

    fn set_bpr_timeline(&mut self, timeline: Option<BprTimeline>) {
        self.bpr_timeline = timeline;
    }
}

impl HasBprTimeline for IronButterflyResult {
    fn bpr_timeline(&self) -> Option<&BprTimeline> {
        self.bpr_timeline.as_ref()
    }

    fn set_bpr_timeline(&mut self, timeline: Option<BprTimeline>) {
        self.bpr_timeline = timeline;
    }
}

impl HasBprTimeline for StraddleResult {
    fn bpr_timeline(&self) -> Option<&BprTimeline> {
        self.bpr_timeline.as_ref()
    }

    fn set_bpr_timeline(&mut self, timeline: Option<BprTimeline>) {
        self.bpr_timeline = timeline;
    }
}

impl HasBprTimeline for CalendarStraddleResult {
    fn bpr_timeline(&self) -> Option<&BprTimeline> {
        self.bpr_timeline.as_ref()
    }

    fn set_bpr_timeline(&mut self, timeline: Option<BprTimeline>) {
        self.bpr_timeline = timeline;
    }
}

#[derive(Debug, Clone)]
pub struct BprPricingContext {
    pub ts: DateTime<Utc>,
    pub spot: Decimal,
    pub pricing: CompositePricing,
}

fn hedge_shares_at(pos: &HedgePosition, ts: DateTime<Utc>) -> i32 {
    pos.hedges
        .iter()
        .filter(|h| h.timestamp <= ts)
        .map(|h| h.shares)
        .sum()
}

pub async fn build_bpr_timeline<T, Pr>(
    trade: &T,
    pricer: &Pr,
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    _timing: &TimingStrategy,
    hedge_position: Option<&HedgePosition>,
    pricing_contexts: Option<&[BprPricingContext]>,
    margin_config: &MarginConfig,
) -> Result<Option<BprTimeline>, ExecutionError>
where
    T: ExecutableTrade<Pricer = Pr> + CompositeTrade + Clone + Send + Sync,
    Pr: TradePricer<Trade = T, Pricing = CompositePricing>,
{
    if matches!(margin_config.mode, MarginMode::Off) {
        return Ok(None);
    }

    let Some(engine) = margin_engine_for_config(margin_config) else {
        return Ok(None);
    };

    let mut times = Vec::new();
    times.push(entry_time);
    times.push(exit_time);

    if let Some(pos) = hedge_position {
        for hedge in &pos.hedges {
            times.push(hedge.timestamp);
        }
    }

    times.sort();
    times.dedup();

    let mut snapshots = Vec::new();
    let symbol = ExecutableTrade::symbol(trade).to_string();
    let build_snapshot = |ts: DateTime<Utc>, spot: Decimal, pricing: &CompositePricing| {
        let trade_legs = trade.legs();
        if trade_legs.len() != pricing.legs.len() {
            tracing::debug!(
                symbol = %symbol,
                trade_legs = trade_legs.len(),
                pricing_legs = pricing.legs.len(),
                "BPR leg count mismatch"
            );
        }

        let mut option_legs = Vec::new();
        for (idx, (leg, position)) in trade_legs.iter().enumerate() {
            let Some((leg_pricing, _)) = pricing.legs.get(idx) else { break };
            let right = match leg.option_type {
                finq_core::OptionType::Call => OptionRight::Call,
                finq_core::OptionType::Put => OptionRight::Put,
            };
            let qty = match position {
                cs_domain::trade::LegPosition::Long => 1,
                cs_domain::trade::LegPosition::Short => -1,
            };

            option_legs.push(OptionLegInput {
                right,
                strike: leg.strike.value(),
                expiry: leg.expiration,
                qty,
                mark_premium: leg_pricing.price,
            });
        }

        if option_legs.is_empty() {
            return None;
        }

        let hedge = hedge_position.and_then(|pos| {
            let shares = hedge_shares_at(pos, ts);
            if shares == 0 {
                None
            } else {
                Some(HedgeInput {
                    symbol: symbol.clone(),
                    shares,
                    spot,
                })
            }
        });

        let inputs = BprInputs {
            ts,
            underlying_symbol: symbol.clone(),
            underlying_spot: spot,
            option_legs,
            hedge,
        };

        Some(engine.compute_snapshot(&inputs, margin_config))
    };

    for ts in times {
        if let Some(contexts) = pricing_contexts {
            if let Some(context) = contexts.iter().find(|ctx| ctx.ts == ts) {
                if let Some(snapshot) = build_snapshot(ts, context.spot, &context.pricing) {
                    snapshots.push(snapshot);
                }
                continue;
            }
        }

        let spot = match equity_repo.get_spot_price(&symbol, ts).await {
            Ok(s) => s.value,
            Err(e) => {
                tracing::debug!(symbol = %symbol, time = %ts, error = %e, "BPR spot fetch failed");
                continue;
            }
        };

        let chain = match options_repo.get_option_bars_at_time(&symbol, ts).await {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(symbol = %symbol, time = %ts, error = %e, "BPR option bars fetch failed");
                continue;
            }
        };

        let surface = build_iv_surface_minute_aligned(&chain, equity_repo, &symbol).await;
        let spot_f64 = spot.to_f64().unwrap_or(0.0);
        let pricing = match pricer.price_with_surface(
            trade,
            &chain,
            spot_f64,
            ts,
            surface.as_ref(),
        ) {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!(symbol = %symbol, time = %ts, error = %e, "BPR pricing failed");
                continue;
            }
        };
        if let Some(snapshot) = build_snapshot(ts, spot, &pricing) {
            snapshots.push(snapshot);
        }
    }

    if snapshots.is_empty() {
        return Ok(None);
    }

    Ok(Some(BprTimeline::new(snapshots)))
}

fn latest_snapshot_at_or_before<'a>(
    timeline: &'a BprTimeline,
    ts: DateTime<Utc>,
) -> Option<&'a BprSnapshot> {
    let mut latest = None;
    for snap in &timeline.snapshots {
        if snap.ts <= ts {
            latest = Some(snap);
        } else {
            break;
        }
    }
    latest
}

pub fn build_portfolio_bpr_timeline<R>(results: &[R]) -> Option<BprTimeline>
where
    R: TradeResult + HasBprTimeline,
{
    let mut times = BTreeSet::new();
    let mut trades = Vec::new();

    for result in results {
        if let Some(timeline) = result.bpr_timeline() {
            for snap in &timeline.snapshots {
                times.insert(snap.ts);
            }
            trades.push((result, timeline));
        }
    }

    if trades.is_empty() || times.is_empty() {
        return None;
    }

    let mut snapshots = Vec::new();
    for ts in times {
        let mut option_initial = Decimal::ZERO;
        let mut option_maint = Decimal::ZERO;
        let mut hedge_initial = Decimal::ZERO;
        let mut hedge_maint = Decimal::ZERO;

        for &(trade, timeline) in trades.iter() {
            if ts < trade.entry_time() || ts > trade.exit_time() {
                continue;
            }

            let Some(snap) = latest_snapshot_at_or_before(timeline, ts) else {
                continue;
            };

            option_initial += snap.option_initial;
            option_maint += snap.option_maint;
            hedge_initial += snap.hedge_initial;
            hedge_maint += snap.hedge_maint;
        }

        snapshots.push(BprSnapshot {
            ts,
            option_initial,
            option_maint,
            hedge_initial,
            hedge_maint,
        });
    }

    if snapshots.is_empty() {
        return None;
    }

    Some(BprTimeline::new(snapshots))
}
