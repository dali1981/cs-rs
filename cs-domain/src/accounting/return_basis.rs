//! Return denominator selection for reporting and analytics.

use serde::{Deserialize, Serialize};

/// Denominator used for return calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReturnBasis {
    /// Premium magnitude (absolute entry cash flow).
    Premium,
    /// Capital requirement (margin/buying power reduction).
    CapitalRequired,
    /// Maximum loss (defined-risk strategies).
    MaxLoss,
    /// Peak BPR over the trade lifetime.
    #[serde(rename = "bpr-peak")]
    BprPeak,
    /// Average BPR over the trade lifetime.
    #[serde(rename = "bpr-avg")]
    BprAvg,
}

impl Default for ReturnBasis {
    fn default() -> Self {
        ReturnBasis::CapitalRequired
    }
}

impl ReturnBasis {
    pub fn label(&self) -> &'static str {
        match self {
            ReturnBasis::Premium => "premium",
            ReturnBasis::CapitalRequired => "capital-required",
            ReturnBasis::MaxLoss => "max-loss",
            ReturnBasis::BprPeak => "bpr-peak",
            ReturnBasis::BprAvg => "bpr-avg",
        }
    }
}
