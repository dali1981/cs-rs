// cs-analytics: Pure computational functions for options analytics
//
// No I/O, no side effects - highly testable and parallelizable.

pub mod black_scholes;
pub mod greeks;
pub mod iv_surface;
pub mod historical_iv;
pub mod price_interpolation;

pub use black_scholes::{bs_price, bs_implied_volatility, bs_greeks, BSConfig, BSError};
pub use greeks::Greeks;
pub use iv_surface::{IVSurface, IVPoint};
pub use historical_iv::{iv_percentile, iv_rank, realized_volatility};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder() {
        // TODO: Implement when modules are ready
        assert!(true);
    }
}
