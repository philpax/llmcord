use serenity::all::{CommandInteraction, Http};

pub mod execute;
pub mod hallucinate;

#[serenity::async_trait]
pub trait CommandHandler: Send + Sync {
    fn registerable_commands(&self) -> Vec<String>;
    async fn register(&self, http: &Http) -> anyhow::Result<()>;
    fn can_handle_command(&self, cmd: &CommandInteraction) -> bool;
    async fn run(&self, http: &Http, cmd: &CommandInteraction) -> anyhow::Result<()>;
}
