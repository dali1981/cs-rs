//! Rule evaluation errors

use std::fmt;

/// Error during rule evaluation
#[derive(Debug, Clone)]
pub enum RuleError {
    /// Required data is missing for rule evaluation
    MissingData {
        rule: &'static str,
        field: &'static str,
    },
    /// IV surface doesn't have data for requested DTE
    MissingDteData {
        rule: &'static str,
        dte: u16,
    },
    /// HV provider not available but required
    HvProviderRequired,
    /// Invalid rule configuration
    InvalidConfig {
        rule: &'static str,
        message: String,
    },
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingData { rule, field } => {
                write!(f, "Rule '{}' missing required data: {}", rule, field)
            }
            Self::MissingDteData { rule, dte } => {
                write!(f, "Rule '{}' missing IV data for DTE {}", rule, dte)
            }
            Self::HvProviderRequired => {
                write!(f, "Historical volatility provider required but not available")
            }
            Self::InvalidConfig { rule, message } => {
                write!(f, "Rule '{}' invalid config: {}", rule, message)
            }
        }
    }
}

impl std::error::Error for RuleError {}
