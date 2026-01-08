//! Command handler trait for extensible command execution

use anyhow::Result;

/// Trait for command handlers that can be executed
#[async_trait::async_trait]
pub trait CommandHandler: Send + Sync {
    /// Execute the command handler
    async fn execute(&self) -> Result<()>;
}
