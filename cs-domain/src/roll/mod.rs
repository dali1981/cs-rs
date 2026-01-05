//! Roll policy for multi-period trades
//!
//! Defines when and how positions should be renewed/rolled.

mod policy;
mod event;

pub use policy::RollPolicy;
pub use event::RollEvent;
