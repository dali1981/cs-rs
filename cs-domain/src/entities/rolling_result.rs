use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

/// Result of a rolling strategy (any trade type)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingResult {
    pub symbol: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub roll_policy: String,
    pub trade_type: String,  // "straddle", "calendar_spread", etc.

    // Individual roll periods
    pub rolls: Vec<RollPeriod>,

    // Aggregated metrics
    pub total_option_pnl: Decimal,
    pub total_hedge_pnl: Decimal,
    pub total_transaction_cost: Decimal,
    pub total_pnl: Decimal,

    // Statistics
    pub num_rolls: usize,
    pub win_rate: f64,
    pub avg_roll_pnl: Decimal,
    pub max_drawdown: Decimal,

    // Aggregated volatility summary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volatility_summary: Option<VolatilitySummary>,

    // Aggregated capital metrics (Phase 2c)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capital_summary: Option<CapitalSummary>,

    // Aggregated P&L attribution (Phase 3a)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution_summary: Option<AttributionSummary>,
}

impl RollingResult {
    /// Create a new result from a list of roll periods
    pub fn from_rolls(
        symbol: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
        roll_policy: String,
        trade_type: String,
        rolls: Vec<RollPeriod>,
    ) -> Self {
        let num_rolls = rolls.len();

        let total_option_pnl: Decimal = rolls.iter().map(|r| r.pnl).sum();
        let total_hedge_pnl: Decimal = rolls.iter()
            .filter_map(|r| r.hedge_pnl)
            .sum();
        let total_transaction_cost: Decimal = rolls.iter()
            .map(|r| r.transaction_cost)
            .sum();
        let total_pnl = total_option_pnl + total_hedge_pnl - total_transaction_cost;

        let winners = rolls.iter().filter(|r| r.pnl > Decimal::ZERO).count();
        let win_rate = if num_rolls > 0 {
            winners as f64 / num_rolls as f64
        } else {
            0.0
        };

        let avg_roll_pnl = if num_rolls > 0 {
            total_pnl / Decimal::from(num_rolls)
        } else {
            Decimal::ZERO
        };

        // Calculate max drawdown
        let mut peak = Decimal::ZERO;
        let mut max_drawdown = Decimal::ZERO;
        let mut cumulative_pnl = Decimal::ZERO;

        for roll in &rolls {
            cumulative_pnl += roll.pnl;
            if let Some(hedge_pnl) = roll.hedge_pnl {
                cumulative_pnl += hedge_pnl;
            }
            cumulative_pnl -= roll.transaction_cost;

            if cumulative_pnl > peak {
                peak = cumulative_pnl;
            }
            let drawdown = peak - cumulative_pnl;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }

        // Compute volatility summary
        let volatility_summary = Self::compute_volatility_summary(&rolls);

        // Compute capital summary (Phase 2c)
        let capital_summary = Self::compute_capital_summary(&rolls, total_pnl, start_date, end_date);

        // Compute attribution summary (Phase 3a)
        let attribution_summary = Self::compute_attribution_summary(&rolls);

        Self {
            symbol,
            start_date,
            end_date,
            roll_policy,
            trade_type,
            rolls,
            total_option_pnl,
            total_hedge_pnl,
            total_transaction_cost,
            total_pnl,
            num_rolls,
            win_rate,
            avg_roll_pnl,
            max_drawdown,
            volatility_summary,
            capital_summary,
            attribution_summary,
        }
    }

    /// Compute volatility summary from rolls
    fn compute_volatility_summary(rolls: &[RollPeriod]) -> Option<VolatilitySummary> {
        let rolls_with_vol: Vec<_> = rolls.iter()
            .filter_map(|r| r.realized_vol_metrics.as_ref())
            .collect();

        if rolls_with_vol.is_empty() {
            return None;
        }

        let avg_entry_iv = {
            let ivs: Vec<f64> = rolls_with_vol.iter()
                .filter_map(|m| m.entry_iv)
                .collect();
            if ivs.is_empty() {
                None
            } else {
                Some(ivs.iter().sum::<f64>() / ivs.len() as f64)
            }
        };

        let avg_entry_hv = {
            let hvs: Vec<f64> = rolls_with_vol.iter()
                .filter_map(|m| m.entry_hv)
                .collect();
            if hvs.is_empty() {
                None
            } else {
                Some(hvs.iter().sum::<f64>() / hvs.len() as f64)
            }
        };

        let avg_realized_vol = {
            let rvs: Vec<f64> = rolls_with_vol.iter()
                .map(|m| m.realized_vol)
                .collect();
            if rvs.is_empty() {
                None
            } else {
                Some(rvs.iter().sum::<f64>() / rvs.len() as f64)
            }
        };

        let avg_iv_premium = {
            let prems: Vec<f64> = rolls_with_vol.iter()
                .filter_map(|m| m.iv_premium_at_entry)
                .collect();
            if prems.is_empty() {
                None
            } else {
                Some(prems.iter().sum::<f64>() / prems.len() as f64)
            }
        };

        let avg_realized_vs_implied = {
            let diffs: Vec<f64> = rolls_with_vol.iter()
                .filter_map(|m| m.realized_vs_implied)
                .collect();
            if diffs.is_empty() {
                None
            } else {
                Some(diffs.iter().sum::<f64>() / diffs.len() as f64)
            }
        };

        Some(VolatilitySummary {
            avg_entry_iv,
            avg_entry_hv,
            avg_realized_vol,
            avg_iv_premium,
            avg_realized_vs_implied,
            rolls_with_vol_data: rolls_with_vol.len(),
        })
    }

    /// Compute capital summary from rolls (Phase 2d)
    fn compute_capital_summary(
        rolls: &[RollPeriod],
        total_pnl: Decimal,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Option<CapitalSummary> {
        if rolls.is_empty() {
            return None;
        }

        // Total option premium = sum of entry debits
        let total_option_premium: Decimal = rolls.iter()
            .map(|r| r.entry_debit)
            .sum();

        // Peak hedge capital across all rolls
        let peak_hedge_capital: Decimal = rolls.iter()
            .filter_map(|r| r.hedge_capital.as_ref())
            .map(|c| c.long_capital)
            .max()
            .unwrap_or(Decimal::ZERO);

        // Peak hedge margin across all rolls
        let peak_hedge_margin: Decimal = rolls.iter()
            .filter_map(|r| r.hedge_capital.as_ref())
            .map(|c| c.short_margin)
            .max()
            .unwrap_or(Decimal::ZERO);

        // Total capital = option premium + max(peak_long_capital, peak_short_margin)
        let hedge_capital = peak_hedge_capital.max(peak_hedge_margin);
        let total_capital_required = total_option_premium + hedge_capital;

        // Holding period in days
        let holding_days = (end_date - start_date).num_days();

        // Return on capital
        let return_on_capital = if total_capital_required > Decimal::ZERO {
            (total_pnl / total_capital_required)
                .to_f64()
                .unwrap_or(0.0) * 100.0
        } else {
            0.0
        };

        // Annualized return
        let annualized_return = if holding_days > 0 {
            return_on_capital * 365.0 / holding_days as f64
        } else {
            0.0
        };

        Some(CapitalSummary {
            total_option_premium,
            peak_hedge_capital,
            peak_hedge_margin,
            total_capital_required,
            return_on_capital,
            annualized_return,
            holding_days,
        })
    }

    /// Compute attribution summary from rolls (Phase 3b)
    fn compute_attribution_summary(rolls: &[RollPeriod]) -> Option<AttributionSummary> {
        // Collect all rolls with position attribution
        let rolls_with_attr: Vec<_> = rolls.iter()
            .filter_map(|r| r.position_attribution.as_ref())
            .collect();

        if rolls_with_attr.is_empty() {
            return None;
        }

        // Sum up all the components
        let total_gross_delta_pnl: Decimal = rolls_with_attr.iter()
            .map(|a| a.total_gross_delta_pnl)
            .sum();

        let total_hedge_delta_pnl: Decimal = rolls_with_attr.iter()
            .map(|a| a.total_hedge_delta_pnl)
            .sum();

        let total_net_delta_pnl: Decimal = rolls_with_attr.iter()
            .map(|a| a.total_net_delta_pnl)
            .sum();

        let total_gamma_pnl: Decimal = rolls_with_attr.iter()
            .map(|a| a.total_gamma_pnl)
            .sum();

        let total_theta_pnl: Decimal = rolls_with_attr.iter()
            .map(|a| a.total_theta_pnl)
            .sum();

        let total_vega_pnl: Decimal = rolls_with_attr.iter()
            .map(|a| a.total_vega_pnl)
            .sum();

        let total_unexplained: Decimal = rolls_with_attr.iter()
            .map(|a| a.total_unexplained)
            .sum();

        // Average hedge efficiency
        let avg_hedge_efficiency = if !rolls_with_attr.is_empty() {
            let sum: f64 = rolls_with_attr.iter()
                .map(|a| a.hedge_efficiency)
                .sum();
            sum / rolls_with_attr.len() as f64
        } else {
            0.0
        };

        Some(AttributionSummary {
            total_gross_delta_pnl,
            total_hedge_delta_pnl,
            total_net_delta_pnl,
            total_gamma_pnl,
            total_theta_pnl,
            total_vega_pnl,
            total_unexplained,
            avg_hedge_efficiency,
            rolls_with_attribution: rolls_with_attr.len(),
        })
    }
}

/// A single roll period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollPeriod {
    pub entry_date: NaiveDate,
    pub exit_date: NaiveDate,
    pub strike: Decimal,
    pub expiration: NaiveDate,

    pub entry_debit: Decimal,
    pub exit_credit: Decimal,
    pub pnl: Decimal,

    pub spot_at_entry: f64,
    pub spot_at_exit: f64,
    pub spot_move_pct: f64,

    pub iv_entry: Option<f64>,
    pub iv_exit: Option<f64>,
    pub iv_change: Option<f64>,

    pub net_delta: Option<f64>,
    pub net_gamma: Option<f64>,
    pub net_theta: Option<f64>,
    pub net_vega: Option<f64>,

    // P&L Attribution (legacy - kept for non-hedged strategies)
    pub delta_pnl: Option<Decimal>,
    pub gamma_pnl: Option<Decimal>,
    pub theta_pnl: Option<Decimal>,
    pub vega_pnl: Option<Decimal>,
    pub unexplained_pnl: Option<Decimal>,

    pub hedge_pnl: Option<Decimal>,
    pub hedge_count: usize,
    pub transaction_cost: Decimal,

    pub roll_reason: RollReason,

    // Integrated position attribution (when hedging is enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_attribution: Option<crate::position::PositionAttribution>,

    // Volatility metrics (when hedging with track_realized_vol=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realized_vol_metrics: Option<crate::hedging::RealizedVolatilityMetrics>,

    // Capital metrics (Phase 2b)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hedge_capital: Option<HedgeCapitalMetrics>,
}

/// Reason why a position was rolled
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RollReason {
    /// Normal scheduled roll (weekly/monthly/days)
    Scheduled,
    /// Option expired before roll date
    Expiry,
    /// Reached campaign end date
    EndOfCampaign,
}

impl std::fmt::Display for RollReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scheduled => write!(f, "Scheduled"),
            Self::Expiry => write!(f, "Expiry"),
            Self::EndOfCampaign => write!(f, "End"),
        }
    }
}

/// Aggregated volatility metrics across all rolls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilitySummary {
    /// Average entry IV across all rolls (annualized)
    pub avg_entry_iv: Option<f64>,
    /// Average entry HV across all rolls (annualized)
    pub avg_entry_hv: Option<f64>,
    /// Average realized vol across all rolls (annualized)
    pub avg_realized_vol: Option<f64>,
    /// Average IV premium at entry: (IV - HV) / HV
    pub avg_iv_premium: Option<f64>,
    /// Average realized vs implied: (RV - IV) / IV
    pub avg_realized_vs_implied: Option<f64>,
    /// Number of rolls with valid volatility data
    pub rolls_with_vol_data: usize,
}

/// Capital metrics for a single roll period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HedgeCapitalMetrics {
    /// Peak long shares held
    pub peak_long_shares: i32,
    /// Peak short shares held (absolute)
    pub peak_short_shares: i32,
    /// Average hedge price
    pub avg_hedge_price: f64,
    /// Capital required for long hedge
    pub long_capital: Decimal,
    /// Margin required for short hedge
    pub short_margin: Decimal,
}

/// Aggregated capital metrics across all rolls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapitalSummary {
    /// Total option premium (sum of entry_debit)
    pub total_option_premium: Decimal,
    /// Peak hedge capital (max across rolls)
    pub peak_hedge_capital: Decimal,
    /// Peak hedge margin (max across rolls)
    pub peak_hedge_margin: Decimal,
    /// Total capital required = option_premium + max(hedge_capital, hedge_margin)
    pub total_capital_required: Decimal,
    /// Return on capital: total_pnl / total_capital_required
    pub return_on_capital: f64,
    /// Annualized return
    pub annualized_return: f64,
    /// Holding period in days
    pub holding_days: i64,
}

/// Aggregated P&L attribution summary across all rolls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionSummary {
    pub total_gross_delta_pnl: Decimal,
    pub total_hedge_delta_pnl: Decimal,
    pub total_net_delta_pnl: Decimal,
    pub total_gamma_pnl: Decimal,
    pub total_theta_pnl: Decimal,
    pub total_vega_pnl: Decimal,
    pub total_unexplained: Decimal,
    pub avg_hedge_efficiency: f64,
    pub rolls_with_attribution: usize,
}
