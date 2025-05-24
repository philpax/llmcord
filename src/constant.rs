/// names of values used in interactions
pub mod value {
    pub const PROMPT: &str = "prompt";
    pub const SEED: &str = "seed";
    pub const MODEL: &str = "model";

    pub const MESSAGE_ID: &str = "message_id";
}

/// names of non-user-configurable commands
pub mod commands {
    /// Used by the message command
    pub const EXECUTE_THIS_CODE_BLOCK: &str = "Execute this code block";
    /// Used by the slash command
    pub const EXECUTE: &str = "execute";
}
