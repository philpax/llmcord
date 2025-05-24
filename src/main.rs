use std::{collections::HashMap, collections::HashSet, sync::Arc};

use anyhow::Context as AnyhowContext;
use serenity::{
    Client,
    all::{
        Command, Context, CreateInteractionResponse, CreateInteractionResponseMessage,
        EventHandler, Http, Interaction, MessageId, Ready,
    },
    async_trait,
    model::prelude::GatewayIntents,
};

mod ai;
mod cancel;
mod commands;
mod config;
mod constant;
mod outputter;
mod util;

use config::Configuration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Configuration::load()?;
    let discord_token = config
        .authentication
        .discord_token
        .as_deref()
        .context("Expected authentication.discord_token to be filled in config")?;

    let ai = Arc::new(ai::Ai::load(&config).await?);

    let (cancel_tx, cancel_rx) = flume::unbounded::<MessageId>();
    let handlers: HashMap<String, Box<dyn commands::CommandHandler>> = config
        .commands
        .iter()
        .map(|(name, command)| {
            Box::new(commands::hallucinate::Handler::new(
                command.clone(),
                name.to_string(),
                config.discord.clone(),
                cancel_rx.clone(),
                ai.clone(),
            )) as Box<dyn commands::CommandHandler>
        })
        .chain([
            Box::new(commands::execute::app::Handler::new(
                config.discord.clone(),
                cancel_rx.clone(),
            )) as Box<dyn commands::CommandHandler>,
            Box::new(commands::execute::slash::Handler::new(
                config.discord.clone(),
                cancel_rx.clone(),
            )) as Box<dyn commands::CommandHandler>,
        ])
        .map(|handler| (handler.name().to_string(), handler))
        .collect();

    let mut client = Client::builder(discord_token, GatewayIntents::default())
        .event_handler(Handler {
            handlers,
            cancel_tx,
        })
        .await
        .context("Error creating client")?;

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }

    Ok(())
}

pub struct Handler {
    handlers: HashMap<String, Box<dyn commands::CommandHandler>>,
    cancel_tx: flume::Sender<MessageId>,
}
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        self.ready_impl(&ctx.http, ready)
            .await
            .expect("Error while registering commands");
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Some(respondable) = util::interaction_to_respondable_interaction(&interaction) else {
            panic!("Unknown interaction type: {interaction:?}");
        };

        if let Err(err) = self.interaction_create_impl(&ctx.http, &interaction).await {
            respondable
                .create_or_edit(&ctx.http, &format!("Error: {err}"))
                .await
                .unwrap();
        }
    }
}
impl Handler {
    async fn ready_impl(&self, http: &Http, ready: Ready) -> anyhow::Result<()> {
        println!("{} is connected; registering commands...", ready.user.name);

        // Check if we need to reset our registered commands
        let registered_commands: HashSet<_> = {
            let cmds = Command::get_global_commands(http).await?;
            cmds.iter().map(|c| c.name.clone()).collect()
        };
        let our_commands: HashSet<_> = self.handlers.keys().cloned().collect();
        if registered_commands != our_commands {
            Command::set_global_commands(http, vec![]).await?;
        }

        for handler in self.handlers.values() {
            handler.register(http).await?;
        }

        println!("{} is good to go!", ready.user.name);

        Ok(())
    }

    async fn interaction_create_impl(
        &self,
        http: &Http,
        interaction: &Interaction,
    ) -> anyhow::Result<()> {
        match interaction {
            Interaction::Command(cmd) => {
                let name = cmd.data.name.as_str();
                if let Some(handler) = self.handlers.get(name) {
                    handler.run(http, cmd).await?;
                } else {
                    anyhow::bail!("no handler found for command: {name}");
                }
            }
            Interaction::Component(cmp) => {
                if let Some((message_id, user_id)) = cancel::parse_id(&cmp.data.custom_id) {
                    if cmp.user.id != user_id {
                        return Ok(());
                    }

                    self.cancel_tx.send(message_id).ok();
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
            _ => {}
        };
        Ok(())
    }
}
