use serenity::all::{
    Command, CommandInteraction, CommandType, CreateCommand, CreateInteractionResponse,
    CreateInteractionResponseMessage, Http, MessageId,
};

use crate::{config, constant};

use super::CommandHandler;

pub struct Handler {
    _cancel_rx: flume::Receiver<MessageId>,
    _discord_config: config::Discord,
}
impl Handler {
    pub async fn new(
        config: &config::Configuration,
        cancel_rx: flume::Receiver<MessageId>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            _cancel_rx: cancel_rx,
            _discord_config: config.discord.clone(),
        })
    }
}
#[serenity::async_trait]
impl CommandHandler for Handler {
    fn registerable_commands(&self) -> Vec<String> {
        vec![constant::commands::EXECUTE.to_string()]
    }

    async fn register(&self, http: &Http) -> anyhow::Result<()> {
        Command::create_global_command(
            http,
            CreateCommand::new(constant::commands::EXECUTE).kind(CommandType::Message),
        )
        .await?;
        Ok(())
    }

    fn can_handle_command(&self, cmd: &CommandInteraction) -> bool {
        cmd.data.name == constant::commands::EXECUTE
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
