use serenity::all::{Command, CommandInteraction, CommandType, CreateCommand, Http};

use crate::constant;

use crate::commands::CommandHandler;

pub struct Handler {
    base: super::Handler,
}
impl Handler {
    pub fn new(base: super::Handler) -> Self {
        Self { base }
    }
}
#[serenity::async_trait]
impl CommandHandler for Handler {
    fn name(&self) -> &str {
        constant::commands::EXECUTE_THIS_CODE_BLOCK
    }

    async fn register(&self, http: &Http) -> anyhow::Result<()> {
        Command::create_global_command(
            http,
            CreateCommand::new(constant::commands::EXECUTE_THIS_CODE_BLOCK)
                .kind(CommandType::Message),
        )
        .await?;
        Ok(())
    }

    async fn run(&self, http: &Http, cmd: &CommandInteraction) -> anyhow::Result<()> {
        let messages = &cmd.data.resolved.messages;
        let Some(unparsed_code) = messages.values().next().map(|v| v.content.as_str()) else {
            anyhow::bail!("no message found");
        };

        self.base.run(http, cmd, unparsed_code).await
    }
}
