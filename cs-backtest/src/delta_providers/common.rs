//! Common utilities shared across delta providers
//!
//! This module extracts duplicated leg-by-leg computation logic
//! to avoid repetition across multiple delta provider implementations.

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use cs_analytics::bs_delta;
use cs_domain::trade::CompositeTrade;
use finq_core::OptionType;

/// Compute position delta from all legs using a uniform volatility
///
/// This helper is used by delta providers that compute delta using a single
/// volatility value for all legs (e.g., entry HV, current HV, entry IV).
///
/// # Arguments
/// * `trade` - Composite trade with all option legs
/// * `spot` - Current spot price
/// * `timestamp` - Current time (used to compute time-to-expiration)
/// * `volatility` - Volatility to use for all legs (in decimal form, e.g., 0.30 = 30%)
/// * `risk_free_rate` - Risk-free rate for Black-Scholes
///
/// # Returns
/// Per-share position delta (no multiplier applied)
pub fn compute_position_delta_uniform_vol<T: CompositeTrade>(
    trade: &T,
    spot: f64,
    timestamp: DateTime<Utc>,
    volatility: f64,
    risk_free_rate: f64,
) -> f64 {
    trade.legs()
        .iter()
        .map(|(leg, position)| {
            let tte = (leg.expiration - timestamp.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 {
                return 0.0; // Expired option has zero delta
            }

            let is_call = leg.option_type == OptionType::Call;
            let strike = leg.strike.value().to_f64().unwrap_or(0.0);

            // Per-share delta from Black-Scholes
            let leg_delta = bs_delta(spot, strike, tte, volatility, is_call, risk_free_rate);

            // Apply position sign (long = +1, short = -1)
            // NO multiplier here - we return per-share delta
            leg_delta * position.sign()
        })
        .sum()
}

/// Compute position delta from all legs using per-leg volatility lookup
///
/// This helper is used by delta providers that compute delta using volatility
/// that varies by leg (e.g., current market IV surface, historical average IV).
///
/// # Arguments
/// * `trade` - Composite trade with all option legs
/// * `spot` - Current spot price
/// * `timestamp` - Current time (used to compute time-to-expiration)
/// * `mut vol_lookup` - Callback to get volatility for each leg
///   - Takes `(&OptionLeg, f64)` and returns volatility in decimal form
///   - The f64 parameter is spot price (for convenience in IV surface lookups)
/// * `risk_free_rate` - Risk-free rate for Black-Scholes
///
/// # Returns
/// Per-share position delta (no multiplier applied)
pub fn compute_position_delta_with_vol_lookup<T: CompositeTrade>(
    trade: &T,
    spot: f64,
    timestamp: DateTime<Utc>,
    mut vol_lookup: impl FnMut(&cs_domain::entities::OptionLeg, f64) -> f64,
    risk_free_rate: f64,
) -> f64 {
    trade.legs()
        .iter()
        .map(|(leg, position)| {
            let tte = (leg.expiration - timestamp.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 {
                return 0.0; // Expired option has zero delta
            }

            let is_call = leg.option_type == OptionType::Call;
            let strike = leg.strike.value().to_f64().unwrap_or(0.0);

            // Get volatility specific to this leg
            let leg_iv = vol_lookup(leg, spot);

            // Per-share delta from Black-Scholes
            let leg_delta = bs_delta(spot, strike, tte, leg_iv, is_call, risk_free_rate);

            // Apply position sign (long = +1, short = -1)
            // NO multiplier here - we return per-share delta
            leg_delta * position.sign()
        })
        .sum()
}
