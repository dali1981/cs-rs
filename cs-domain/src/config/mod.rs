//! Domain configuration types
//!
//! This module contains configuration types for trading strategies,
//! filters, and position specifications.

mod filter_criteria;
mod position_spec;

pub use filter_criteria::FilterCriteria;
pub use position_spec::{PositionSpec, PositionStructure, StrikeSelection};
