use crate::{
    config::{self, Configuration},
    constant,
    outputter::Outputter,
    util::{self, DiscordInteraction},
};
use anyhow::Context as AnyhowContext;
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestUserMessage,
};
use serenity::{all::*, async_trait, futures::StreamExt};
use std::collections::HashSet;

pub struct Handler {
    config: Configuration,
    client: async_openai::Client<async_openai::config::OpenAIConfig>,
    models: Vec<String>,
    cancel_tx: flume::Sender<MessageId>,
    cancel_rx: flume::Receiver<MessageId>,
}
impl Handler {
    pub fn new(
        config: Configuration,
        client: async_openai::Client<async_openai::config::OpenAIConfig>,
        models: Vec<String>,
    ) -> Self {
        let (cancel_tx, cancel_rx) = flume::unbounded::<MessageId>();

        Self {
            config,
            client,
            models,
            cancel_tx,
            cancel_rx,
        }
    }
}
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        if let Err(err) = self.ready_impl(&ctx.http, ready).await {
            println!("Error while registering commands: `{err}`");
            std::process::exit(1);
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        self.interaction_create_impl(ctx, interaction).await;
    }
}
impl Handler {
    async fn ready_impl(&self, http: &Http, ready: Ready) -> anyhow::Result<()> {
        println!("{} is connected; registering commands...", ready.user.name);

        let registered_commands = Command::get_global_commands(http).await?;
        let registered_commands: HashSet<_> = registered_commands
            .iter()
            .map(|c| c.name.as_str())
            .collect();

        let our_commands: HashSet<_> = self
            .config
            .commands
            .iter()
            .filter(|(_, v)| v.enabled)
            .map(|(k, _)| k.as_str())
            .chain(std::iter::once(constant::commands::EXECUTE))
            .collect();

        if registered_commands != our_commands {
            // If the commands registered with Discord don't match the commands configured
            // for this bot, reset them entirely.
            Command::set_global_commands(http, vec![]).await?;
        }

        for (name, command) in self.config.commands.iter().filter(|(_, v)| v.enabled) {
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

        Command::create_global_command(
            http,
            CreateCommand::new(constant::commands::EXECUTE).kind(CommandType::Message),
        )
        .await?;

        println!("{} is good to go!", ready.user.name);

        Ok(())
    }

    async fn interaction_create_impl(&self, ctx: Context, interaction: Interaction) {
        let http = &ctx.http;
        match interaction {
            Interaction::Command(cmd) => {
                let name = cmd.data.name.as_str();

                if name == constant::commands::EXECUTE {
                    util::run_and_report_error(&cmd, http, {
                        let cmd = &cmd;
                        async move {
                            let code = &cmd
                                .data
                                .resolved
                                .messages
                                .iter()
                                .next()
                                .context("no message found")?
                                .1
                                .content;
                            let data = CreateInteractionResponseMessage::new()
                                .content(format!("Executing code:\n{code}"));
                            let builder = CreateInteractionResponse::Message(data);
                            cmd.create_response(http, builder).await?;
                            Ok(())
                        }
                    })
                    .await;
                } else if let Some(command) = self.config.commands.get(name) {
                    util::run_and_report_error(
                        &cmd,
                        http,
                        hallucinate(
                            &cmd,
                            http,
                            &self.client,
                            command,
                            &self.config,
                            self.cancel_rx.clone(),
                        ),
                    )
                    .await;
                }
            }
            Interaction::Component(cmp) => {
                if let ["cancel", message_id, user_id] =
                    cmp.data.custom_id.split('#').collect::<Vec<_>>()[..]
                {
                    if let (Ok(message_id), Ok(user_id)) =
                        (message_id.parse::<u64>(), user_id.parse::<u64>())
                    {
                        if cmp.user.id == user_id {
                            self.cancel_tx.send(MessageId::new(message_id)).ok();
                            cmp.create_response(
                                http,
                                CreateInteractionResponse::UpdateMessage(
                                    CreateInteractionResponseMessage::new(),
                                ),
                            )
                            .await
                            .ok();
                        }
                    }
                }
            }
            _ => {}
        };
    }
}

async fn hallucinate(
    cmd: &CommandInteraction,
    http: &Http,
    client: &async_openai::Client<async_openai::config::OpenAIConfig>,
    command: &config::Command,
    config: &config::Configuration,
    cancel_rx: flume::Receiver<MessageId>,
) -> anyhow::Result<()> {
    use constant::value as v;
    use util::{value_to_integer, value_to_string};

    let options = &cmd.data.options;
    let user_prompt = util::get_value(options, v::PROMPT)
        .and_then(value_to_string)
        .context("no prompt specified")?;

    let user_prompt = if config.discord.replace_newlines {
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
        std::time::Duration::from_millis(config.discord.message_update_interval_ms),
    )
    .await?;

    let message = cmd.get_interaction_message(http).await?;
    let message_id = message.id;

    let mut stream = client
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
        if let Ok(cancel_message_id) = cancel_rx.try_recv() {
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
