// cs-analytics: Pure computational functions for options analytics
//
// No I/O, no side effects - highly testable and parallelizable.

pub mod black_scholes;
pub mod greeks;
pub mod iv_model;
pub mod iv_surface;
pub mod historical_iv;
pub mod math_utils;
pub mod vol_slice;
pub mod delta_surface;
pub mod opportunity;
pub mod svi;
pub mod svi_fitter;
pub mod arbitrage;

pub use black_scholes::{bs_price, bs_implied_volatility, bs_greeks, bs_delta, BSConfig, BSError};
pub use greeks::Greeks;
pub use iv_model::{
    IVInterpolator, IVModel,
    StickyStrikeInterpolator, StickyMoneynessInterpolator, StickyDeltaInterpolator,
};
pub use iv_surface::{IVSurface, IVPoint};
pub use historical_iv::{iv_percentile, iv_rank, realized_volatility};
pub use math_utils::{inv_norm_cdf, linspace};
pub use vol_slice::{VolSlice, InterpolationMode, delta_to_strike_with_iv};
pub use delta_surface::DeltaVolSurface;
pub use opportunity::{CalendarOpportunity, OpportunityAnalyzer, OpportunityAnalyzerConfig};
pub use svi::{SVIParams, SVIError};
pub use svi_fitter::{SVIFitter, SVIFitterConfig};
pub use arbitrage::{
    ArbitrageViolation, ArbitrageReport,
    check_butterfly_arbitrage, check_calendar_arbitrage, full_arbitrage_check,
};
