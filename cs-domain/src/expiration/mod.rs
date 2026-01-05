//! Expiration policy and cycle detection
//!
//! This module provides abstractions for selecting option expirations
//! based on various criteria: date constraints, cycle preferences (weekly/monthly),
//! or target DTE.

mod cycle;
mod policy;

pub use cycle::ExpirationCycle;
pub use policy::ExpirationPolicy;
