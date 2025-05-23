use std::sync::Arc;

use anyhow::Context;
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestUserMessage,
};
use serenity::{
    all::{
        Command, CommandInteraction, CommandOptionType, CreateCommand, CreateCommandOption, Http,
        MessageId,
    },
    futures::StreamExt,
};

use crate::{
    ai::Ai,
    config, constant,
    outputter::Outputter,
    util::{self, RespondableInteraction},
};

use super::CommandHandler;

pub struct Handler {
    cancel_rx: flume::Receiver<MessageId>,
    name: String,
    command: config::Command,
    discord_config: config::Discord,
    ai: Arc<Ai>,
}
impl Handler {
    pub fn new(
        command: config::Command,
        name: String,
        discord_config: config::Discord,
        cancel_rx: flume::Receiver<MessageId>,
        ai: Arc<Ai>,
    ) -> Self {
        Self {
            cancel_rx,
            name,
            command,
            discord_config,
            ai,
        }
    }
}
#[serenity::async_trait]
impl CommandHandler for Handler {
    fn name(&self) -> &str {
        &self.name
    }

    async fn register(&self, http: &Http) -> anyhow::Result<()> {
        if !self.command.enabled {
            return Ok(());
        }

        let mut model_option = CreateCommandOption::new(
            CommandOptionType::String,
            constant::value::MODEL,
            "The model to use.",
        )
        .required(true);

        for model in &self.ai.models {
            model_option = model_option.add_string_choice(model, model);
        }

        Command::create_global_command(
            http,
            CreateCommand::new(self.name.clone())
                .description(self.command.description.as_str())
                .add_option(model_option)
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        constant::value::PROMPT,
                        "The prompt.",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::Integer,
                        constant::value::SEED,
                        "The seed to use for sampling.",
                    )
                    .min_int_value(0)
                    .required(false),
                ),
        )
        .await?;

        Ok(())
    }

    async fn run(&self, http: &Http, cmd: &CommandInteraction) -> anyhow::Result<()> {
        use constant::value as v;
        use util::{value_to_integer, value_to_string};

        let options = &cmd.data.options;
        let user_prompt = util::get_value(options, v::PROMPT)
            .and_then(value_to_string)
            .context("no prompt specified")?;

        let user_prompt = if self.discord_config.replace_newlines {
            user_prompt.replace("\\n", "\n")
        } else {
            user_prompt
        };

        let seed = util::get_value(options, v::SEED)
            .and_then(value_to_integer)
            .map(|i| i as u32)
            .unwrap_or(0);

        let model = util::get_value(options, v::MODEL)
            .and_then(value_to_string)
            .context("no model specified")?;

        let mut outputter = Outputter::new(
            http,
            cmd,
            std::time::Duration::from_millis(self.discord_config.message_update_interval_ms),
        )
        .await?;

        let message = cmd.get_interaction_message(http).await?;
        let message_id = message.id;

        let mut stream = self
            .ai
            .client
            .chat()
            .create_stream(
                async_openai::types::CreateChatCompletionRequestArgs::default()
                    .model(model.clone())
                    .seed(seed)
                    .messages([
                        ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                            content: self.command.system_prompt.clone().into(),
                            name: None,
                        }),
                        ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                            content: user_prompt.clone().into(),
                            name: None,
                        }),
                    ])
                    .stream(true)
                    .build()?,
            )
            .await?;

        let mut errored = false;
        let mut message = String::new();
        while let Some(response) = stream.next().await {
            if let Ok(cancel_message_id) = self.cancel_rx.try_recv() {
                if cancel_message_id == message_id {
                    outputter.cancelled().await?;
                    errored = true;
                    break;
                }
            }

            match response {
                Ok(response) => {
                    if let Some(content) = &response.choices[0].delta.content {
                        message += content;
                        outputter
                            .update(&format!("**{user_prompt}** (*{model}*)\n{message}"))
                            .await?;
                    }
                }
                Err(err) => {
                    outputter.error(&err.to_string()).await?;
                    errored = true;
                    break;
                }
            }
        }
        if !errored {
            outputter.finish().await?;
        }

        Ok(())
    }
}
