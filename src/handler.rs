use crate::{
    config::{self, Configuration},
    constant,
    util::{self, DiscordInteraction},
};
use anyhow::Context as AnyhowContext;
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestUserMessage,
};
use serenity::{
    async_trait,
    builder::CreateComponents,
    client::{Context, EventHandler},
    futures::StreamExt,
    http::Http,
    model::{
        application::interaction::Interaction,
        prelude::{
            command::{Command, CommandOptionType},
            interaction::{
                application_command::ApplicationCommandInteraction, InteractionResponseType,
            },
            *,
        },
    },
};
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
        println!("{} is connected; registering commands...", ready.user.name);

        if let Err(err) = ready_handler(&ctx.http, &self.config, &self.models).await {
            println!("Error while registering commands: `{err}`");
            std::process::exit(1);
        }

        println!("{} is good to go!", ready.user.name);
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let http = &ctx.http;
        match interaction {
            Interaction::ApplicationCommand(cmd) => {
                let name = cmd.data.name.as_str();
                let commands = &self.config.commands;

                if let Some(command) = commands.get(name) {
                    util::run_and_report_error(
                        &cmd,
                        http,
                        hallucinate(&cmd, http, &self.client, command, self.cancel_rx.clone()),
                    )
                    .await;
                }
            }
            Interaction::MessageComponent(cmp) => {
                if let ["cancel", message_id, user_id] =
                    cmp.data.custom_id.split('#').collect::<Vec<_>>()[..]
                {
                    if let (Ok(message_id), Ok(user_id)) =
                        (message_id.parse::<u64>(), user_id.parse::<u64>())
                    {
                        if cmp.user.id == user_id {
                            self.cancel_tx.send(MessageId(message_id)).ok();
                            cmp.create_interaction_response(http, |r| {
                                r.kind(InteractionResponseType::DeferredUpdateMessage)
                            })
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

async fn ready_handler(
    http: &Http,
    config: &config::Configuration,
    models: &[String],
) -> anyhow::Result<()> {
    let registered_commands = Command::get_global_application_commands(http).await?;
    let registered_commands: HashSet<_> = registered_commands
        .iter()
        .map(|c| c.name.as_str())
        .collect();

    let our_commands: HashSet<_> = config
        .commands
        .iter()
        .filter(|(_, v)| v.enabled)
        .map(|(k, _)| k.as_str())
        .collect();

    if registered_commands != our_commands {
        // If the commands registered with Discord don't match the commands configured
        // for this bot, reset them entirely.
        Command::set_global_application_commands(http, |c| c.set_application_commands(vec![]))
            .await?;
    }

    for (name, command) in config.commands.iter().filter(|(_, v)| v.enabled) {
        Command::create_global_application_command(http, |cmd| {
            cmd.name(name)
                .description(command.description.as_str())
                .create_option(|opt| {
                    opt.name(constant::value::MODEL)
                        .description("The model to use.")
                        .kind(CommandOptionType::String)
                        .required(true);

                    for model in models {
                        opt.add_string_choice(model, model);
                    }

                    opt
                })
                .create_option(|opt| {
                    opt.name(constant::value::PROMPT)
                        .description("The prompt.")
                        .kind(CommandOptionType::String)
                        .required(true)
                });

            create_parameters(cmd)
        })
        .await?;
    }

    Ok(())
}

fn create_parameters(
    command: &mut serenity::builder::CreateApplicationCommand,
) -> &mut serenity::builder::CreateApplicationCommand {
    command.create_option(|opt| {
        opt.name(constant::value::SEED)
            .kind(CommandOptionType::Integer)
            .description("The seed to use for sampling.")
            .min_int_value(0)
            .required(false)
    })
}

async fn hallucinate(
    cmd: &ApplicationCommandInteraction,
    http: &Http,
    client: &async_openai::Client<async_openai::config::OpenAIConfig>,
    command: &config::Command,
    cancel_rx: flume::Receiver<MessageId>,
) -> anyhow::Result<()> {
    use constant::value as v;
    use util::{value_to_integer, value_to_string};

    let options = &cmd.data.options;
    let user_prompt = util::get_value(options, v::PROMPT)
        .and_then(value_to_string)
        .context("no prompt specified")?;
    let user_prompt = user_prompt.replace("\\n", "\n");

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
        model.clone(),
        user_prompt.clone(),
        std::time::Duration::from_millis(constant::config::DISCORD_MESSAGE_UPDATE_INTERVAL_MS),
    )
    .await?;

    let message = cmd.get_interaction_message(http).await?;
    let message_id = message.id;

    let mut stream = client
        .chat()
        .create_stream(
            async_openai::types::CreateChatCompletionRequestArgs::default()
                .model(model)
                .seed(seed)
                .messages([
                    ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                        content: command.system_prompt.clone().into(),
                        name: None,
                    }),
                    ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                        content: user_prompt.into(),
                        name: None,
                    }),
                ])
                .stream(true)
                .build()?,
        )
        .await?;

    let mut errored = false;
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
                    outputter.new_token(content).await?;
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

struct Outputter<'a> {
    http: &'a Http,

    user_id: UserId,
    model: String,
    messages: Vec<Message>,
    chunks: Vec<String>,

    message: String,
    user_prompt: String,

    in_terminal_state: bool,
    in_thinking_state: bool,

    last_update: std::time::Instant,
    last_update_duration: std::time::Duration,
}
impl<'a> Outputter<'a> {
    const MESSAGE_CHUNK_SIZE: usize = 1500;

    async fn new(
        http: &'a Http,
        cmd: &ApplicationCommandInteraction,
        model: String,
        user_prompt: String,
        last_update_duration: std::time::Duration,
    ) -> anyhow::Result<Outputter<'a>> {
        cmd.create_interaction_response(http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| {
                    message
                        .content("Generating...")
                        .allowed_mentions(|m| m.empty_roles().empty_users().empty_parse())
                })
        })
        .await?;
        let starting_message = cmd.get_interaction_response(http).await?;

        Ok(Self {
            http,

            user_id: cmd.user.id,
            model,
            messages: vec![starting_message],
            chunks: vec![],

            message: String::new(),
            user_prompt,

            in_terminal_state: false,
            in_thinking_state: false,

            last_update: std::time::Instant::now(),
            last_update_duration,
        })
    }

    async fn new_token(&mut self, token: &str) -> anyhow::Result<()> {
        if self.in_terminal_state {
            return Ok(());
        }

        if self.message.is_empty() {
            // Add the cancellation button when we receive the first token
            if let Some(first) = self.messages.first_mut() {
                add_cancel_button(self.http, first.id, first, self.user_id).await?;
            }
        }

        // Handle thinking state transitions
        if token.contains("<think>") {
            self.in_thinking_state = true;
            return Ok(());
        }
        if token.contains("</think>") {
            self.in_thinking_state = false;
            // The next token should start with a newline
            if !self.message.ends_with('\n') {
                self.message += "\n";
            }
            return Ok(());
        }

        self.message += token;

        // This could be much more efficient but that's a problem for later
        self.chunks = {
            let mut chunks: Vec<String> = vec![];

            let markdown = format!(
                "**{}** (*{}*)\n{}",
                self.user_prompt, self.model, self.message
            );

            // Split into lines and handle thinking state
            let mut processed_lines = Vec::new();
            for line in markdown.split('\n') {
                if self.in_thinking_state && !line.is_empty() {
                    processed_lines.push(format!("-# {}", line));
                } else {
                    processed_lines.push(line.to_string());
                }
            }
            let processed_markdown = processed_lines.join("\n");

            // Split into chunks
            for word in processed_markdown.split(' ') {
                if let Some(last) = chunks.last_mut() {
                    if last.len() > Self::MESSAGE_CHUNK_SIZE {
                        chunks.push(word.to_string());
                    } else {
                        last.push(' ');
                        last.push_str(word);
                    }
                } else {
                    chunks.push(word.to_string());
                }
            }

            chunks
        };

        if self.last_update.elapsed() > self.last_update_duration {
            self.sync_messages_with_chunks().await?;
            self.last_update = std::time::Instant::now();
        }

        Ok(())
    }

    async fn error(&mut self, err: &str) -> anyhow::Result<()> {
        self.on_error(err).await
    }

    async fn cancelled(&mut self) -> anyhow::Result<()> {
        self.on_error("The generation was cancelled.").await
    }

    async fn finish(&mut self) -> anyhow::Result<()> {
        for msg in &mut self.messages {
            msg.edit(self.http, |m| m.set_components(CreateComponents::default()))
                .await?;
        }

        self.in_terminal_state = true;
        self.sync_messages_with_chunks().await?;

        Ok(())
    }

    async fn sync_messages_with_chunks(&mut self) -> anyhow::Result<()> {
        // Update the last message with its latest state, then insert the remaining chunks in one go
        if let Some((msg, chunk)) = self.messages.iter_mut().zip(self.chunks.iter()).next_back() {
            msg.edit(self.http, |m| m.content(chunk)).await?;
        }

        if self.chunks.len() <= self.messages.len() {
            return Ok(());
        }

        // Remove the cancel button from all existing messages
        for msg in &mut self.messages {
            msg.edit(self.http, |m| {
                m.set_components(CreateComponents::default())
                    .allowed_mentions(|m| m.empty_roles().empty_users().empty_parse())
            })
            .await?;
        }

        // Create new messages for the remaining chunks
        let Some(first_id) = self.messages.first().map(|m| m.id) else {
            return Ok(());
        };
        for chunk in self.chunks[self.messages.len()..].iter() {
            let last = self.messages.last_mut().unwrap();
            let msg = reply_to_message_without_mentions(self.http, last, chunk).await?;
            self.messages.push(msg);
        }

        // Add the cancel button to the last message
        if !self.in_terminal_state {
            if let Some(last) = self.messages.last_mut() {
                add_cancel_button(self.http, first_id, last, self.user_id).await?;
            }
        }

        Ok(())
    }

    async fn on_error(&mut self, error_message: &str) -> anyhow::Result<()> {
        for msg in &mut self.messages {
            let cut_content = format!("~~{}~~", msg.content);
            msg.edit(self.http, |m| {
                m.set_components(CreateComponents::default())
                    .allowed_mentions(|m| m.empty_roles().empty_users().empty_parse())
                    .content(cut_content)
            })
            .await?;
        }

        self.in_terminal_state = true;
        let Some(last) = self.messages.last_mut() else {
            return Ok(());
        };
        reply_to_message_without_mentions(self.http, last, error_message).await?;

        Ok(())
    }
}

async fn add_cancel_button(
    http: &Http,
    first_id: MessageId,
    msg: &mut Message,
    user_id: UserId,
) -> anyhow::Result<()> {
    Ok(msg
        .edit(http, |r| {
            let mut components = CreateComponents::default();
            components.create_action_row(|r| {
                r.create_button(|b| {
                    b.custom_id(format!("cancel#{first_id}#{user_id}"))
                        .style(component::ButtonStyle::Danger)
                        .label("Cancel")
                })
            });
            r.set_components(components)
        })
        .await?)
}

async fn reply_to_message_without_mentions(
    http: &Http,
    msg: &Message,
    content: &str,
) -> anyhow::Result<Message> {
    Ok(msg
        .channel_id
        .send_message(http, |m| {
            m.reference_message(msg)
                .content(content)
                .allowed_mentions(|m| m.empty_roles().empty_users().empty_parse())
        })
        .await?)
}
