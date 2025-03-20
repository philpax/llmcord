use anyhow::Context as AnyhowContext;
use serenity::{model::prelude::*, Client};

mod config;
mod constant;
mod handler;
mod util;

use config::Configuration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Configuration::load()?;

    // Split the model path into directory and filename
    let model_path = &config.model.path;
    let (directory, filename) = if let Some(parent) = model_path.parent() {
        (
            parent.to_string_lossy().to_string(),
            model_path
                .file_name()
                .map_or_else(String::new, |f| f.to_string_lossy().to_string()),
        )
    } else {
        // If there's no parent directory, the entire path is the filename
        (String::new(), model_path.to_string_lossy().to_string())
    };

    let model = mistralrs::GgufModelBuilder::new(&directory, vec![&filename])
        // .with_paged_attn(|| mistralrs::PagedAttentionMetaBuilder::default().build())?
        .build()
        .await?;

    let mut client = Client::builder(
        config
            .authentication
            .discord_token
            .as_deref()
            .context("Expected authentication.discord_token to be filled in config")?,
        GatewayIntents::default(),
    )
    .event_handler(handler::Handler::new(config, model))
    .await
    .context("Error creating client")?;

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }

    Ok(())
}
