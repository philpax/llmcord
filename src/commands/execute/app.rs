use serenity::all::{Command, CommandInteraction, CommandType, CreateCommand, Http, MessageId};

use crate::{config, constant};

use crate::commands::CommandHandler;

pub struct Handler {
    discord_config: config::Discord,
    cancel_rx: flume::Receiver<MessageId>,
}
impl Handler {
    pub fn new(discord_config: config::Discord, cancel_rx: flume::Receiver<MessageId>) -> Self {
        Self {
            discord_config,
            cancel_rx,
        }
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

        super::run(
            http,
            cmd,
            &self.discord_config,
            &self.cancel_rx,
            unparsed_code,
        )
        .await
    }
}
