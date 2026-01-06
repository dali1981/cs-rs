//! Position attribution module for P&L decomposition
//!
//! Collects daily snapshots of position Greeks and computes P&L attribution
//! by delta, gamma, theta, and vega contributions.

mod greeks_computer;
mod snapshot_collector;

pub use greeks_computer::GreeksComputer;
pub use snapshot_collector::SnapshotCollector;
