use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Configuration {
    pub authentication: Authentication,
    pub commands: HashMap<String, Command>,
    pub discord: Discord,
}
impl Default for Configuration {
    fn default() -> Self {
        Self {
            authentication: Authentication {
                discord_token: None,
                openai_api_server: None,
                openai_api_key: None,
            },
            commands: HashMap::from_iter([(
                "ask".into(),
                Command {
                    enabled: false,
                    description: "Responds to the provided instruction.".into(),
                    system_prompt: "You are a helpful assistant.".into(),
                },
            )]),
            discord: Discord::default(),
        }
    }
}
impl Configuration {
    const FILENAME: &str = "config.toml";

    pub fn load() -> anyhow::Result<Self> {
        let config = if let Ok(file) = std::fs::read_to_string(Self::FILENAME) {
            toml::from_str(&file).context("failed to load config")?
        } else {
            Self::default()
        };
        config.save()?;

        Ok(config)
    }

    fn save(&self) -> anyhow::Result<()> {
        Ok(std::fs::write(
            Self::FILENAME,
            toml::to_string_pretty(self)?,
        )?)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Authentication {
    pub discord_token: Option<String>,
    pub openai_api_server: Option<String>,
    pub openai_api_key: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Model {
    pub path: PathBuf,
    pub context_token_length: usize,
    /// Whether or not to use GPU support. Note that `llmcord` must be
    /// compiled with GPU support for this to work.
    pub use_gpu: bool,
    /// The number of layers to offload to the GPU (if `use_gpu` is on).
    /// If not set, all layers will be offloaded.
    pub gpu_layers: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Discord {
    /// Low values will result in you getting throttled by Discord
    pub message_update_interval_ms: u64,
    /// Whether or not to replace '\n' with newlines
    pub replace_newlines: bool,
}

impl Default for Discord {
    fn default() -> Self {
        Self {
            message_update_interval_ms: 1000,
            replace_newlines: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Command {
    pub enabled: bool,
    pub description: String,
    pub system_prompt: String,
}
