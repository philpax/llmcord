use anyhow::Context as AnyhowContext;
use serenity::{Client, model::prelude::*};

mod commands;
mod config;
mod constant;
mod handler;
mod outputter;
mod util;

use config::Configuration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Configuration::load()?;

    let mut client = Client::builder(
        config
            .authentication
            .discord_token
            .as_deref()
            .context("Expected authentication.discord_token to be filled in config")?,
        GatewayIntents::default(),
    )
    .event_handler(handler::Handler::new(config).await?)
    .await
    .context("Error creating client")?;

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }

    Ok(())
}
