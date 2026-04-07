//! Price command handler

use anyhow::Result;
use async_trait::async_trait;

use crate::args::{PriceArgs, GlobalArgs};
use super::CommandHandler;

/// Price command handler
pub struct PriceCommand {
    args: PriceArgs,
    #[allow(dead_code)]
    global: GlobalArgs,
}

impl PriceCommand {
    /// Create a new price command handler
    pub fn new(args: PriceArgs, global: GlobalArgs) -> Self {
        Self { args, global }
    }
}

#[async_trait]
impl CommandHandler for PriceCommand {
    async fn execute(&self) -> Result<()> {
        // TODO: Implement price command execution
        println!("Pricing {} strike {} on {}", self.args.symbol, self.args.strike, self.args.date);
        Ok(())
    }
}
