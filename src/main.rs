use anyhow::Context as AnyhowContext;
use serenity::{Client, model::prelude::*};

mod config;
mod constant;
mod handler;
mod outputter;
mod util;

use config::Configuration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Configuration::load()?;

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

    let mut client = Client::builder(
        config
            .authentication
            .discord_token
            .as_deref()
            .context("Expected authentication.discord_token to be filled in config")?,
        GatewayIntents::default(),
    )
    .event_handler(handler::Handler::new(config, client, models))
    .await
    .context("Error creating client")?;

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }

    Ok(())
}
