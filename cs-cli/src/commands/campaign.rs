//! Campaign command handler

use anyhow::Result;
use async_trait::async_trait;

use crate::args::{CampaignArgs, GlobalArgs};
use super::CommandHandler;

/// Campaign command handler
pub struct CampaignCommand {
    args: CampaignArgs,
    global: GlobalArgs,
}

impl CampaignCommand {
    /// Create a new campaign command handler
    pub fn new(args: CampaignArgs, global: GlobalArgs) -> Self {
        Self { args, global }
    }
}

#[async_trait]
impl CommandHandler for CampaignCommand {
    async fn execute(&self) -> Result<()> {
        // TODO: Implement campaign command execution
        println!("Running campaign for symbols: {:?}", self.args.symbols);
        println!("Strategy: {}", self.args.strategy);
        println!("Period: {} to {}", self.args.start, self.args.end);
        Ok(())
    }
}
