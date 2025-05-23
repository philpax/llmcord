use std::collections::HashMap;

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
    config, constant,
    outputter::Outputter,
    util::{self, RespondableInteraction},
};

use super::CommandHandler;

pub struct Handler {
    cancel_rx: flume::Receiver<MessageId>,
    client: async_openai::Client<async_openai::config::OpenAIConfig>,
    models: Vec<String>,
    commands: HashMap<String, config::Command>,
    discord_config: config::Discord,
}
impl Handler {
    pub async fn new(
        config: &config::Configuration,
        cancel_rx: flume::Receiver<MessageId>,
    ) -> anyhow::Result<Self> {
        let mut openai_config = async_openai::config::OpenAIConfig::default();
        if let Some(openai_api_server) = config.authentication.openai_api_server.as_deref() {
            openai_config = openai_config.with_api_base(openai_api_server);
        }
        if let Some(openai_api_key) = config.authentication.openai_api_key.as_deref() {
            openai_config = openai_config.with_api_key(openai_api_key);
        }
        let client = async_openai::Client::with_config(openai_config);

        let models: Vec<_> = client
            .models()
            .list()
            .await?
            .data
            .into_iter()
            .map(|m| m.id)
            .collect();

        Ok(Self {
            cancel_rx,
            client,
            models,
            commands: config.commands.clone(),
            discord_config: config.discord.clone(),
        })
    }
}
#[serenity::async_trait]
impl CommandHandler for Handler {
    fn registerable_commands(&self) -> Vec<String> {
        self.commands
            .iter()
            .filter(|(_, v)| v.enabled)
            .map(|(name, _)| name.clone())
            .collect()
    }

    async fn register(&self, http: &Http) -> anyhow::Result<()> {
        for (name, command) in self.commands.iter().filter(|(_, v)| v.enabled) {
            let mut model_option = CreateCommandOption::new(
                CommandOptionType::String,
                constant::value::MODEL,
                "The model to use.",
            )
            .required(true);

            for model in &self.models {
                model_option = model_option.add_string_choice(model, model);
            }

            Command::create_global_command(
                http,
                CreateCommand::new(name)
                    .description(command.description.as_str())
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
        }

        Ok(())
    }

    fn can_handle_command(&self, cmd: &CommandInteraction) -> bool {
        self.commands.contains_key(cmd.data.name.as_str())
    }

    async fn run(&self, http: &Http, cmd: &CommandInteraction) -> anyhow::Result<()> {
        let command = self
            .commands
            .get(cmd.data.name.as_str())
            .context("no command found")?;
        self.run_command(http, cmd, command).await
    }
}
impl Handler {
    async fn run_command(
        &self,
        http: &Http,
        cmd: &CommandInteraction,
        command: &config::Command,
    ) -> anyhow::Result<()> {
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
            .client
            .chat()
            .create_stream(
                async_openai::types::CreateChatCompletionRequestArgs::default()
                    .model(model.clone())
                    .seed(seed)
                    .messages([
                        ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                            content: command.system_prompt.clone().into(),
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
