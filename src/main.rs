use anyhow::Context as AnyhowContext;
use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel},
};
use serenity::{model::prelude::*, Client};

mod config;
mod constant;
mod generation;
mod handler;
mod util;

use config::Configuration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Configuration::load()?;

    let backend = LlamaBackend::init()?;
    let mut params = LlamaModelParams::default();
    if config.model.use_gpu {
        params = params.with_n_gpu_layers(config.model.gpu_layers.unwrap_or(1000) as u32);
    }

    let model = LlamaModel::load_from_file(&backend, &config.model.path, &params)?;

    let mut client = Client::builder(
        config
            .authentication
            .discord_token
            .as_deref()
            .context("Expected authentication.discord_token to be filled in config")?,
        GatewayIntents::default(),
    )
    .event_handler(handler::Handler::new(config, backend, model))
    .await
    .context("Error creating client")?;

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }

    Ok(())
}
