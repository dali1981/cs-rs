//! ATM IV command handler

use anyhow::Result;
use async_trait::async_trait;

use crate::args::{AtmIvArgs, GlobalArgs};
use super::CommandHandler;

/// ATM IV command handler
pub struct AtmIvCommand {
    args: AtmIvArgs,
    global: GlobalArgs,
}

impl AtmIvCommand {
    /// Create a new ATM IV command handler
    pub fn new(args: AtmIvArgs, global: GlobalArgs) -> Self {
        Self { args, global }
    }
}

#[async_trait]
impl CommandHandler for AtmIvCommand {
    async fn execute(&self) -> Result<()> {
        // TODO: Implement ATM IV command execution
        println!("Generating ATM IV for symbols: {:?}", self.args.symbols);
        println!("Period: {} to {}", self.args.start, self.args.end);
        println!("Output: {:?}", self.args.output);
        Ok(())
    }
}
