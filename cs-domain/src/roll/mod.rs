//! Roll policy for multi-period trades
//!
//! Defines when and how positions should be renewed/rolled.

mod event;
mod policy;

pub use event::RollEvent;
pub use policy::RollPolicy;
