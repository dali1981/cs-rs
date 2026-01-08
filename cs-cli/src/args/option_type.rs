//! CLI wrapper for OptionType with ValueEnum support

use clap::ValueEnum;
use finq_core::OptionType;

/// CLI argument type for option type (with clap ValueEnum)
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OptionTypeArg {
    /// Call option
    Call,
    /// Call option (short alias)
    #[value(alias = "c")]
    C,
    /// Put option
    Put,
    /// Put option (short alias)
    #[value(alias = "p")]
    P,
}

impl From<OptionTypeArg> for OptionType {
    fn from(arg: OptionTypeArg) -> Self {
        match arg {
            OptionTypeArg::Call | OptionTypeArg::C => OptionType::Call,
            OptionTypeArg::Put | OptionTypeArg::P => OptionType::Put,
        }
    }
}
