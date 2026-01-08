//! CLI wrapper for SelectionType with ValueEnum support

use clap::ValueEnum;
use cs_backtest::SelectionType;
use std::fmt;

/// CLI argument type for strike selection method (with clap ValueEnum)
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SelectionTypeArg {
    /// At-the-money selection (default)
    #[value(name = "atm")]
    ATM,
    /// Fixed delta strategy
    Delta,
    /// Delta scanning strategy
    #[value(name = "delta-scan")]
    DeltaScan,
}

impl From<SelectionTypeArg> for SelectionType {
    fn from(arg: SelectionTypeArg) -> Self {
        match arg {
            SelectionTypeArg::ATM => SelectionType::ATM,
            SelectionTypeArg::Delta => SelectionType::Delta,
            SelectionTypeArg::DeltaScan => SelectionType::DeltaScan,
        }
    }
}

impl Default for SelectionTypeArg {
    fn default() -> Self {
        SelectionTypeArg::ATM
    }
}

impl fmt::Display for SelectionTypeArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectionTypeArg::ATM => write!(f, "atm"),
            SelectionTypeArg::Delta => write!(f, "delta"),
            SelectionTypeArg::DeltaScan => write!(f, "delta-scan"),
        }
    }
}
