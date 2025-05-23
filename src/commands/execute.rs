use serenity::all::{
    Command, CommandInteraction, CommandType, CreateCommand, CreateInteractionResponse,
    CreateInteractionResponseMessage, Http, MessageId,
};

use crate::{config, constant};

use super::CommandHandler;

pub struct Handler {
    _discord_config: config::Discord,
    _cancel_rx: flume::Receiver<MessageId>,
}
impl Handler {
    pub fn new(discord_config: config::Discord, cancel_rx: flume::Receiver<MessageId>) -> Self {
        Self {
            _discord_config: discord_config,
            _cancel_rx: cancel_rx,
        }
    }
}
#[serenity::async_trait]
impl CommandHandler for Handler {
    fn name(&self) -> &str {
        constant::commands::EXECUTE
    }

    async fn register(&self, http: &Http) -> anyhow::Result<()> {
        Command::create_global_command(
            http,
            CreateCommand::new(constant::commands::EXECUTE).kind(CommandType::Message),
        )
        .await?;
        Ok(())
    }

    async fn run(&self, http: &Http, cmd: &CommandInteraction) -> anyhow::Result<()> {
        let messages = &cmd.data.resolved.messages;
        let Some(code) = messages.values().next().map(|v| v.content.as_str()) else {
            anyhow::bail!("no message found");
        };

        cmd.create_response(
            http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content(format!("Executing code:\n{code}")),
            ),
        )
        .await?;

        Ok(())
    }
}
