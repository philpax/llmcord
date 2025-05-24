use std::sync::Arc;

use serenity::{
    all::{CommandInteraction, Http, MessageId},
    futures::StreamExt as _,
};

use crate::{ai::Ai, config, outputter::Outputter};

pub mod app;
pub mod slash;

#[derive(Clone)]
pub struct Handler {
    discord_config: config::Discord,
    cancel_rx: flume::Receiver<MessageId>,
    ai: Arc<Ai>,
}
impl Handler {
    pub fn new(
        discord_config: config::Discord,
        cancel_rx: flume::Receiver<MessageId>,
        ai: Arc<Ai>,
    ) -> Self {
        Self {
            discord_config,
            cancel_rx,
            ai,
        }
    }

    async fn run(
        &self,
        http: &Http,
        cmd: &CommandInteraction,
        unparsed_code: &str,
    ) -> anyhow::Result<()> {
        let mut outputter = Outputter::new(
            http,
            cmd,
            std::time::Duration::from_millis(self.discord_config.message_update_interval_ms),
            "Executing...",
        )
        .await?;
        let starting_message_id = outputter.starting_message_id();

        let code = parse_markdown_lua_block(unparsed_code).unwrap_or(unparsed_code);

        let lua = create_lua_state(&self.ai.models)?;
        let mut thread = load_async_expression::<Option<String>>(&lua, code)?;

        let mut errored = false;
        while let Some(result) = thread.next().await {
            if let Ok(cancel_message_id) = self.cancel_rx.try_recv() {
                if cancel_message_id == starting_message_id {
                    outputter.cancelled().await?;
                    errored = true;
                    break;
                }
            }

            match result {
                Ok(result) => {
                    if let Some(result) = result {
                        outputter.update(&result).await?;
                    }
                }
                Err(err) => {
                    outputter.error(&err.to_string()).await?;
                    errored = true;
                    break;
                }
            }
        }
        if !errored {
            outputter.finish().await?;
        }

        Ok(())
    }
}

fn create_lua_state(models: &[String]) -> mlua::Result<mlua::Lua> {
    let lua = mlua::Lua::new_with(
        {
            use mlua::StdLib as SL;
            SL::COROUTINE | SL::MATH | SL::STRING | SL::TABLE | SL::UTF8 | SL::VECTOR
        },
        mlua::LuaOptions::new().catch_rust_panics(true),
    )?;

    lua.globals().set(
        "sleep",
        lua.create_async_function(|_lua, ms: u32| async move {
            tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
            Ok(())
        })?,
    )?;

    lua.globals().set(
        "yield",
        lua.globals()
            .get("coroutine")
            .and_then(|c: mlua::Table| c.get::<mlua::Function>("yield"))?,
    )?;

    lua.globals().set("llm", create_llm_table(&lua, models)?)?;

    lua.globals().set(
        "inspect",
        lua.load(include_str!("../../../vendor/inspect.lua/inspect.lua"))
            .eval::<mlua::Value>()?,
    )?;

    Ok(lua)
}

fn create_llm_table(lua: &mlua::Lua, models: &[String]) -> mlua::Result<mlua::Table> {
    let llm = lua.create_table()?;
    llm.set("models", models)?;

    Ok(llm)
}

fn load_async_expression<R: mlua::FromLuaMulti>(
    lua: &mlua::Lua,
    expression: &str,
) -> anyhow::Result<mlua::AsyncThread<R>> {
    let with_return = lua
        .load(
            format!(
                r#"
coroutine.create(function()
    return {expression}
end)
"#
            )
            .trim(),
        )
        .eval::<mlua::Thread>()
        .and_then(|t| t.into_async::<R>(()));

    match with_return {
        Ok(thread) => Ok(thread),
        Err(with_return_err) => {
            let without_return = lua
                .load(
                    format!(
                        r#"
coroutine.create(function()
{expression}
end)
"#
                    )
                    .trim(),
                )
                .eval::<mlua::Thread>()
                .and_then(|t| t.into_async::<R>(()));

            match without_return {
                Ok(thread) => Ok(thread),
                Err(without_return_err) => {
                    anyhow::bail!(
                        "Failed to load expression with return: {with_return_err:?} | without return: {without_return_err:?}"
                    );
                }
            }
        }
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
