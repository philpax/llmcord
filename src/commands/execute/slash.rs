use serenity::all::{
    Command, CommandInteraction, CommandOptionType, CreateCommand, CreateCommandOption, Http,
};

use crate::{constant, util};

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
        constant::commands::EXECUTE
    }

    async fn register(&self, http: &Http) -> anyhow::Result<()> {
        Command::create_global_command(
            http,
            CreateCommand::new(constant::commands::EXECUTE)
                .description("Execute the Lua code block from the given code snippet or message ID.")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        constant::value::CODE,
                        "The Lua code block to execute. Mutually exclusive with message ID.",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        constant::value::MESSAGE_ID,
                        "The ID of the message to execute the code block from. Mutually exclusive with code.",
                    )
                    .required(false),
                )
        )
        .await?;
        Ok(())
    }

    async fn run(&self, http: &Http, cmd: &CommandInteraction) -> anyhow::Result<()> {
        let options = &cmd.data.options;

        let message_id =
            util::get_value(options, constant::value::MESSAGE_ID).and_then(util::value_to_string);

        let code = util::get_value(options, constant::value::CODE).and_then(util::value_to_string);

        let unparsed_code = match (message_id, code) {
            (Some(message_id), None) => {
                let message = cmd
                    .channel_id
                    .message(http, message_id.parse::<u64>()?)
                    .await?;

                message.content
            }
            (None, Some(code)) => code,
            (Some(_), Some(_)) => anyhow::bail!("message ID and code are mutually exclusive"),
            (None, None) => anyhow::bail!("no message ID or code specified"),
        };

        self.base.run(http, cmd, &unparsed_code).await
    }
}
