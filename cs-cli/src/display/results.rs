//! Result display utilities

use tabled::Tabled;

/// Generic table row for displaying key-value result metrics
#[derive(Tabled)]
pub struct ResultRow {
    #[tabled(rename = "Metric")]
    pub metric: String,
    #[tabled(rename = "Value")]
    pub value: String,
}

impl ResultRow {
    /// Create a new result row
    pub fn new(metric: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            metric: metric.into(),
            value: value.into(),
        }
    }
}
