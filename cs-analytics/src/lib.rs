// cs-analytics: Pure computational functions for options analytics
//
// No I/O, no side effects - highly testable and parallelizable.

pub mod black_scholes;
pub mod greeks;
pub mod iv_model;
pub mod iv_surface;
pub mod historical_iv;

pub use black_scholes::{bs_price, bs_implied_volatility, bs_greeks, bs_delta, BSConfig, BSError};
pub use greeks::Greeks;
pub use iv_model::{
    IVInterpolator, IVModel,
    StickyStrikeInterpolator, StickyMoneynessInterpolator, StickyDeltaInterpolator,
};
pub use iv_surface::{IVSurface, IVPoint};
pub use historical_iv::{iv_percentile, iv_rank, realized_volatility};
