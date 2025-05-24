use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use serenity::{
    all::{CommandInteraction, Http, MessageId},
    futures::{Stream, StreamExt as _},
};

use crate::{ai::Ai, config, outputter::Outputter};

pub mod app;
pub mod slash;

mod extensions;

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

        let (output_tx, output_rx) = flume::unbounded::<mlua::Result<Option<String>>>();

        let lua = create_lua_state(self.ai.clone(), output_tx)?;
        let thread = load_async_expression::<Option<String>>(&lua, code)?;

        let mut errored = false;
        let mut stream = SelectUntilFirstEnds::new(thread, output_rx.stream());
        while let Some(result) = stream.next().await {
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

struct SelectUntilFirstEnds<S1, S2> {
    primary: S1,
    secondary: S2,
    primary_ended: bool,
}
impl<S1, S2> SelectUntilFirstEnds<S1, S2> {
    fn new(primary: S1, secondary: S2) -> Self {
        Self {
            primary,
            secondary,
            primary_ended: false,
        }
    }
}
impl<S1, S2, T> Stream for SelectUntilFirstEnds<S1, S2>
where
    S1: Stream<Item = T> + Unpin,
    S2: Stream<Item = T> + Unpin,
{
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.primary_ended {
            return Poll::Ready(None);
        }

        // Always poll primary first for priority
        match Pin::new(&mut self.primary).poll_next(cx) {
            Poll::Ready(Some(item)) => return Poll::Ready(Some(item)),
            Poll::Ready(None) => {
                self.primary_ended = true;
                return Poll::Ready(None);
            }
            Poll::Pending => {}
        }

        // Only poll secondary if primary is pending
        match Pin::new(&mut self.secondary).poll_next(cx) {
            Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
            Poll::Ready(None) => Poll::Pending, // Don't end if only secondary ends
            Poll::Pending => Poll::Pending,
        }
    }
}

fn create_lua_state(
    ai: Arc<Ai>,
    output_tx: flume::Sender<mlua::Result<Option<String>>>,
) -> mlua::Result<mlua::Lua> {
    let lua = mlua::Lua::new_with(
        {
            use mlua::StdLib as SL;
            SL::COROUTINE | SL::MATH | SL::STRING | SL::TABLE | SL::UTF8 | SL::VECTOR
        },
        mlua::LuaOptions::new().catch_rust_panics(true),
    )?;

    extensions::register(&lua, ai, output_tx)?;

    Ok(lua)
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
