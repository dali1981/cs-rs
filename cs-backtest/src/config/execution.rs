use serde::{Serialize, Deserialize};

/// Execution configuration (runtime concern)
///
/// Specifies how to run the backtest (parallel vs sequential).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    pub parallel: bool,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self { parallel: true }
    }
}
