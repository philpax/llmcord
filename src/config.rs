use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Configuration {
    pub authentication: Authentication,
    pub model: Model,
    pub inference: Inference,
    pub commands: HashMap<String, Command>,
}
impl Default for Configuration {
    fn default() -> Self {
        Self {
            authentication: Authentication {
                discord_token: None,
            },
            model: Model {
                path: "your_model.gguf".into(),
                context_token_length: 2048,
                use_gpu: true,
                gpu_layers: None,
            },
            inference: Inference {
                discord_message_update_interval_ms: 250,
                replace_newlines: true,
                show_prompt_template: true,
            },
            commands: HashMap::from_iter([(
                "ask".into(),
                Command {
                    enabled: false,
                    description: "Responds to the provided instruction.".into(),
                    system_prompt: "You are a helpful assistant.".into(),
                },
            )]),
        }
    }
}
impl Configuration {
    const FILENAME: &str = "config.toml";

    pub fn load() -> anyhow::Result<Self> {
        let config = if let Ok(file) = std::fs::read_to_string(Self::FILENAME) {
            toml::from_str(&file).context("failed to load config")?
        } else {
            let config = Self::default();
            config.save()?;
            config
        };

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
pub struct Inference {
    /// Low values will result in you getting throttled by Discord
    pub discord_message_update_interval_ms: u64,
    /// Whether or not to replace '\n' with newlines
    pub replace_newlines: bool,
    /// Whether or not to show the entire prompt template, or just
    /// what the user specified
    pub show_prompt_template: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Command {
    pub enabled: bool,
    pub description: String,
    pub system_prompt: String,
}
