use serenity::all::{
    ButtonStyle, CommandInteraction, CreateActionRow, CreateAllowedMentions, CreateButton,
    CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, EditMessage, Http,
    Message, MessageId, UserId,
};

pub struct Outputter<'a> {
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

    pub async fn new(
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

    pub async fn update(&mut self, message: &str) -> anyhow::Result<()> {
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

    pub async fn error(&mut self, err: &str) -> anyhow::Result<()> {
        self.on_error(err).await
    }

    pub async fn cancelled(&mut self) -> anyhow::Result<()> {
        self.on_error("The generation was cancelled.").await
    }

    pub async fn finish(&mut self) -> anyhow::Result<()> {
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
