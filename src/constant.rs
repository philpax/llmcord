/// names of values used in interactions
pub mod value {
    pub const PROMPT: &str = "prompt";
    pub const SEED: &str = "seed";
    pub const MODEL: &str = "model";
}

/// config-y stuff that should probably be moved (back) to the config at some point
pub mod config {
    pub const DISCORD_MESSAGE_UPDATE_INTERVAL_MS: u64 = 500;
}
