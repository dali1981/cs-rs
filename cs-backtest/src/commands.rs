//! Application command types for the backtest context.
//!
//! `RunBacktestCommand` is the typed, validated representation of a user's intent
//! to run a backtest. It contains only business-intent fields — no infrastructure
//! concerns like `DataSourceConfig` or `EarningsSourceConfig`, and no
//! serialization concerns like `serde` derives or deprecated backward-compat fields.
//!
//! This type is the stable handoff between the Application layer (cs-cli) and the
//! Execution layer (BacktestUseCase). When the use case is eventually migrated to
//! accept this type directly, the temporary shim in `UseCaseFactory` can be removed.
//!
//! See ADR-0003 for the decision that motivates this separation.

use chrono::NaiveDate;
use cs_analytics::{PricingModel, InterpolationMode};
use cs_domain::{
    TimingConfig, TradeSelectionCriteria, StrikeMatchMode, HedgeConfig, AttributionConfig,
    TradingCostConfig, FileRulesConfig, ReturnBasis, MarginConfig,
};

use crate::config::{SpreadType, SelectionType};

/// A validated, typed application command to run a backtest.
///
/// Produced by the CLI/config layer after parsing and normalization.
/// Consumed by [`crate::factory::UseCaseFactory`] to create a [`crate::BacktestUseCase`].
///
/// Does NOT contain:
/// - `DataSourceConfig` — infrastructure, stays in the factory
/// - `EarningsSourceConfig` — infrastructure, stays in the factory
/// - `data_dir` — deprecated field, never populated from this type
/// - `serde` derives — this type is not a config file format
#[derive(Debug, Clone)]
pub struct RunBacktestCommand {
    // ── Period ────────────────────────────────────────────────────────────────
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,

    // ── Strategy ──────────────────────────────────────────────────────────────
    pub spread: SpreadType,
    pub selection_strategy: SelectionType,
    pub selection: TradeSelectionCriteria,

    // ── Timing ────────────────────────────────────────────────────────────────
    pub timing: TimingConfig,
    /// Generic timing strategy name (e.g. "PreEarnings"). When set, overrides
    /// spread-specific legacy timing params below.
    pub timing_strategy: Option<String>,
    pub entry_days_before: Option<u16>,
    pub exit_days_before: Option<u16>,
    pub entry_offset: Option<i16>,
    pub holding_days: Option<u16>,
    pub exit_days_after: Option<u16>,

    // ── Symbols / Filters ─────────────────────────────────────────────────────
    pub symbols: Option<Vec<String>>,
    pub min_market_cap: Option<u64>,
    pub max_entry_iv: Option<f64>,
    pub min_notional: Option<f64>,
    pub min_entry_price: Option<f64>,
    pub max_entry_price: Option<f64>,

    // ── Execution ─────────────────────────────────────────────────────────────
    pub parallel: bool,
    pub pricing_model: PricingModel,
    pub vol_model: InterpolationMode,
    pub target_delta: f64,
    pub delta_range: (f64, f64),
    pub delta_scan_steps: usize,
    pub strike_match_mode: StrikeMatchMode,

    // ── Strategy-specific params ──────────────────────────────────────────────
    pub wing_width: f64,
    pub straddle_entry_days: usize,
    pub straddle_exit_days: usize,
    pub min_straddle_dte: i32,
    pub post_earnings_holding_days: usize,

    // ── Risk / Metrics ────────────────────────────────────────────────────────
    pub return_basis: ReturnBasis,
    pub margin: MarginConfig,

    // ── Optional features ─────────────────────────────────────────────────────
    pub rules: FileRulesConfig,
    pub hedge_config: HedgeConfig,
    pub attribution_config: Option<AttributionConfig>,
    pub trading_costs: TradingCostConfig,
}
