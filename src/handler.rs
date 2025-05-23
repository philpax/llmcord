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
            Interaction::Command(cmd) => {
                let name = cmd.data.name.as_str();
                let commands = &self.config.commands;

                if let Some(command) = commands.get(name) {
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

async fn ready_handler(
    http: &Http,
    config: &config::Configuration,
    models: &[String],
) -> anyhow::Result<()> {
    let registered_commands = Command::get_global_commands(http).await?;
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
        Command::set_global_commands(http, vec![]).await?;
    }

    for (name, command) in config.commands.iter().filter(|(_, v)| v.enabled) {
        let mut model_option = CreateCommandOption::new(
            CommandOptionType::String,
            constant::value::MODEL,
            "The model to use.",
        )
        .required(true);

        for model in models {
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

struct Outputter<'a> {
    http: &'a Http,

    user_id: UserId,
    messages: Vec<Message>,
    chunks: Vec<String>,

    in_terminal_state: bool,

    last_update: std::time::Instant,
    last_update_duration: std::time::Duration,
}
impl<'a> Outputter<'a> {
    const MESSAGE_CHUNK_SIZE: usize = 1500;

    async fn new(
        http: &'a Http,
        cmd: &CommandInteraction,
        last_update_duration: std::time::Duration,
    ) -> anyhow::Result<Outputter<'a>> {
        cmd.create_response(
            http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("Generating...")
                    .allowed_mentions(CreateAllowedMentions::new()),
            ),
        )
        .await?;
        let starting_message = cmd.get_response(http).await?;

        Ok(Self {
            http,

            user_id: cmd.user.id,
            messages: vec![starting_message],
            chunks: vec![],

            in_terminal_state: false,

            last_update: std::time::Instant::now(),
            last_update_duration,
        })
    }

    async fn update(&mut self, message: &str) -> anyhow::Result<()> {
        if self.in_terminal_state {
            return Ok(());
        }

        self.chunks = chunk_message(message, Self::MESSAGE_CHUNK_SIZE);

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
            msg.edit(self.http, EditMessage::new().components(vec![]))
                .await?;
        }

        self.in_terminal_state = true;
        self.sync_messages_with_chunks().await?;

        Ok(())
    }

    async fn sync_messages_with_chunks(&mut self) -> anyhow::Result<()> {
        // Update existing messages to match chunks
        for (msg, chunk) in self.messages.iter_mut().zip(self.chunks.iter()) {
            msg.edit(self.http, EditMessage::new().content(chunk))
                .await?;
        }

        if self.chunks.len() < self.messages.len() {
            // Delete excess messages
            for msg in self.messages.drain(self.chunks.len()..) {
                msg.delete(self.http).await?;
        }
        } else if self.chunks.len() > self.messages.len() {
        // Remove the cancel button from all existing messages
        for msg in &mut self.messages {
            msg.edit(
                self.http,
                EditMessage::new()
                    .components(vec![])
                    .allowed_mentions(CreateAllowedMentions::new()),
            )
            .await?;
        }

        // Create new messages for the remaining chunks
            for chunk in self.chunks[self.messages.len()..].iter() {
                let last = self.messages.last_mut().unwrap();
                let msg = reply_to_message_without_mentions(self.http, last, chunk).await?;
                self.messages.push(msg);
            }
        }

        let Some(first_id) = self.messages.first().map(|m| m.id) else {
            return Ok(());
        };

        // Add the cancel button to the last message
        if !self.in_terminal_state {
            if let Some(last) = self.messages.last_mut() {
                // TODO: if-let chain, 1.88
                if last.components.is_empty() {
                add_cancel_button(self.http, first_id, last, self.user_id).await?;
                }
            }
        }

        Ok(())
    }

    async fn on_error(&mut self, error_message: &str) -> anyhow::Result<()> {
        for msg in &mut self.messages {
            let cut_content = format!("~~{}~~", msg.content);
            msg.edit(
                self.http,
                EditMessage::new()
                    .components(vec![])
                    .allowed_mentions(CreateAllowedMentions::new())
                    .content(cut_content),
            )
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
        .edit(
            http,
            EditMessage::new().components(vec![CreateActionRow::Buttons(vec![
                CreateButton::new(format!("cancel#{first_id}#{user_id}"))
            .style(ButtonStyle::Danger)
                    .label("Cancel"),
            ])]),
        )
        .await?)
}

async fn reply_to_message_without_mentions(
    http: &Http,
    msg: &Message,
    content: &str,
) -> anyhow::Result<Message> {
    Ok(msg
        .channel_id
        .send_message(
            http,
            CreateMessage::new()
                .reference_message(msg)
                .content(content)
                .allowed_mentions(CreateAllowedMentions::new()),
        )
        .await?)
}

fn chunk_message(message: &str, chunk_size: usize) -> Vec<String> {
    let mut chunks: Vec<String> = vec!["".to_string()];

    for word in message.split(' ') {
        let Some(last) = chunks.last_mut() else {
            continue;
        };

        if last.len() > chunk_size {
            chunks.push(word.to_string());
        } else {
            if !last.is_empty() {
                last.push(' ');
            }
            last.push_str(word);
        }
    }

    chunks
}
