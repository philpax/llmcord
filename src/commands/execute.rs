use anyhow::Context as _;
use serenity::all::{Command, CommandInteraction, CommandType, CreateCommand, Http, MessageId};

use crate::{config, constant, outputter::Outputter};

use super::CommandHandler;

pub struct Handler {
    discord_config: config::Discord,
    cancel_rx: flume::Receiver<MessageId>,
}
impl Handler {
    pub fn new(discord_config: config::Discord, cancel_rx: flume::Receiver<MessageId>) -> Self {
        Self {
            discord_config,
            cancel_rx,
        }
    }
}
#[serenity::async_trait]
impl CommandHandler for Handler {
    fn name(&self) -> &str {
        constant::commands::EXECUTE
    }

    async fn register(&self, http: &Http) -> anyhow::Result<()> {
        Command::create_global_command(
            http,
            CreateCommand::new(constant::commands::EXECUTE).kind(CommandType::Message),
        )
        .await?;
        Ok(())
    }

    async fn run(&self, http: &Http, cmd: &CommandInteraction) -> anyhow::Result<()> {
        let messages = &cmd.data.resolved.messages;

        let mut outputter = Outputter::new(
            http,
            cmd,
            std::time::Duration::from_millis(self.discord_config.message_update_interval_ms),
            "Executing...",
        )
        .await?;
        let starting_message_id = outputter.starting_message_id();

        let Some(code) = messages.values().next().map(|v| v.content.as_str()) else {
            anyhow::bail!("no message found");
        };
        let _code = parse_markdown_lua_block(code).with_context(|| "Invalid Lua code block")?;

        let mut errored = false;
        for i in 0..20 {
            if let Ok(cancel_message_id) = self.cancel_rx.try_recv() {
                if cancel_message_id == starting_message_id {
                    outputter.cancelled().await?;
                    errored = true;
                    break;
                }
            }

            outputter.update(&format!("Executing... {i}")).await?;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        if !errored {
            outputter.finish().await?;
        }

        Ok(())
    }
}

/// Parses a markdown code block of the form ```lua\n{CODE}\n``` and returns the code between the backticks.
/// Doesn't use regex.
fn parse_markdown_lua_block(code: &str) -> Option<&str> {
    // Find the start of the code block
    let start = code.find("```lua\n")?;
    let start = start + 7; // Skip past ```lua\n

    // Find the end of the code block
    let end = code[start..].find("\n```")?;

    // Return the slice between the markers
    Some(&code[start..start + end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_lua_block() {
        // Test basic parsing
        let input = "```lua\nprint('hello')\n```";
        assert_eq!(parse_markdown_lua_block(input), Some("print('hello')"));

        // Test with multiple lines
        let input = "```lua\nlocal x = 1\nlocal y = 2\nprint(x + y)\n```";
        assert_eq!(
            parse_markdown_lua_block(input),
            Some("local x = 1\nlocal y = 2\nprint(x + y)")
        );

        // Test with no code block
        let input = "This is not a code block";
        assert_eq!(parse_markdown_lua_block(input), None);

        // Test with wrong language
        let input = "```python\nprint('hello')\n```";
        assert_eq!(parse_markdown_lua_block(input), None);

        // Test with no closing backticks
        let input = "```lua\nprint('hello')";
        assert_eq!(parse_markdown_lua_block(input), None);
    }
}
