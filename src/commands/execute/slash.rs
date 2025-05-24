use anyhow::Context as _;
use serenity::all::{
    Command, CommandInteraction, CommandOptionType, CreateCommand, CreateCommandOption, Http,
    MessageId,
};

use crate::{config, constant, util};

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
        constant::commands::EXECUTE
    }

    async fn register(&self, http: &Http) -> anyhow::Result<()> {
        Command::create_global_command(
            http,
            CreateCommand::new(constant::commands::EXECUTE)
                .description("Execute the Lua code block from the given message ID.")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        constant::value::MESSAGE_ID,
                        "The ID of the message to execute the code block from.",
                    )
                    .required(true),
                ),
        )
        .await?;
        Ok(())
    }

    async fn run(&self, http: &Http, cmd: &CommandInteraction) -> anyhow::Result<()> {
        let message_id = util::get_value(&cmd.data.options, constant::value::MESSAGE_ID)
            .and_then(util::value_to_string)
            .context("no message ID specified")?;

        let message = cmd
            .channel_id
            .message(http, message_id.parse::<u64>()?)
            .await?;

        super::run(
            http,
            cmd,
            &self.discord_config,
            &self.cancel_rx,
            &message.content,
        )
        .await
    }
}
