use std::sync::Arc;

use async_openai::types::{
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage,
    ChatCompletionRequestSystemMessage, ChatCompletionRequestUserMessage,
    CreateChatCompletionRequestArgs,
};
use serenity::futures::StreamExt as _;

use crate::ai::Ai;

pub fn register(lua: &mlua::Lua, ai: Arc<Ai>) -> mlua::Result<()> {
    let llm = lua.create_table()?;
    llm.set("models", ai.models.clone())?;

    register_message(lua, &llm, "system")?;
    register_message(lua, &llm, "user")?;
    register_message(lua, &llm, "assistant")?;

    llm.set(
        "by_token",
        lua.create_async_function({
            let client = ai.client.clone();
            move |_lua, args: mlua::Table| {
                let client = client.clone();
                async move {
                    let (model, seed, messages, callback) = parse_llm_args(&args)?;
                    let callback = callback.expect("by_token requires a callback");

                    let mut stream = create_chat_stream(&client, &model, seed, messages).await?;

                    while let Some(response) = stream.next().await {
                        let Ok(response) = response else { continue };
                        let Some(content) = &response.choices[0].delta.content else {
                            continue;
                        };
                        let value = callback.call::<mlua::Value>(content.clone())?;
                        if value.as_boolean().is_some_and(|b| !b) {
                            // Allow the user to cancel the stream by returning false
                            break;
                        }
                    }

                    Ok(())
                }
            }
        })?,
    )?;

    llm.set(
        "stream",
        lua.create_async_function({
            let client = ai.client.clone();
            move |_lua, args: mlua::Table| {
                let client = client.clone();
                async move {
                    let (model, seed, messages, callback) = parse_llm_args(&args)?;
                    let callback = callback.expect("stream requires a callback");

                    let mut stream = create_chat_stream(&client, &model, seed, messages).await?;

                    let mut output = String::new();

                    while let Some(response) = stream.next().await {
                        let Ok(response) = response else { continue };
                        let Some(content) = &response.choices[0].delta.content else {
                            continue;
                        };
                        output.push_str(content);
                        let value = callback.call::<mlua::Value>(output.clone())?;
                        if value.as_boolean().is_some_and(|b| !b) {
                            // Allow the user to cancel the stream by returning false
                            break;
                        }
                    }

                    Ok(())
                }
            }
        })?,
    )?;

    llm.set(
        "response",
        lua.create_async_function({
            let client = ai.client.clone();
            move |_lua, args: mlua::Table| {
                let client = client.clone();
                async move {
                    let (model, seed, messages, _) = parse_llm_args(&args)?;

                    let response = client
                        .chat()
                        .create(
                            CreateChatCompletionRequestArgs::default()
                                .model(model)
                                .seed(seed)
                                .messages(messages)
                                .build()
                                .map_err(|e| mlua::Error::ExternalError(Arc::new(e)))?,
                        )
                        .await
                        .map_err(|e| mlua::Error::ExternalError(Arc::new(e)))?;

                    Ok(response.choices[0].message.content.clone())
                }
            }
        })?,
    )?;

    lua.globals().set("llm", llm)?;

    Ok(())
}

fn parse_llm_args(
    args: &mlua::Table,
) -> mlua::Result<(
    String,
    u32,
    Vec<ChatCompletionRequestMessage>,
    Option<mlua::Function>,
)> {
    let model = args.get::<String>("model")?;
    let seed = if args.contains_key("seed")? {
        args.get::<u32>("seed")?
    } else {
        0
    };
    let messages = args.get::<mlua::Table>("messages")?;
    let callback = if args.contains_key("callback")? {
        Some(args.get::<mlua::Function>("callback")?)
    } else {
        None
    };

    let messages: Vec<ChatCompletionRequestMessage> = messages
        .sequence_values::<mlua::Table>()
        .map(|table| from_message_table_to_message(table?))
        .collect::<mlua::Result<Vec<_>>>()?;

    Ok((model, seed, messages, callback))
}

async fn create_chat_stream(
    client: &async_openai::Client<async_openai::config::OpenAIConfig>,
    model: &str,
    seed: u32,
    messages: Vec<ChatCompletionRequestMessage>,
) -> mlua::Result<
    impl serenity::futures::Stream<
        Item = Result<
            async_openai::types::CreateChatCompletionStreamResponse,
            async_openai::error::OpenAIError,
        >,
    >,
> {
    client
        .chat()
        .create_stream(
            CreateChatCompletionRequestArgs::default()
                .model(model)
                .seed(seed)
                .messages(messages)
                .stream(true)
                .build()
                .map_err(|e| mlua::Error::ExternalError(Arc::new(e)))?,
        )
        .await
        .map_err(|e| mlua::Error::ExternalError(Arc::new(e)))
}

fn register_message(lua: &mlua::Lua, table: &mlua::Table, role: &str) -> mlua::Result<()> {
    let f = lua.create_function({
        let role = role.to_string();
        move |lua, value: mlua::Value| {
            let output = lua.create_table()?;

            if let Some(table) = value.as_table() {
                output.set("content", table.get::<String>("content")?)?;
                if let Ok(name) = table.get::<String>("name") {
                    output.set("name", name)?;
                }
            } else if let Some(text) = value.as_str() {
                output.set("content", text)?;
            }

            output.set("role", role.clone())?;
            Ok(output)
        }
    })?;

    table.set(role, f)
}

fn from_message_table_to_message(table: mlua::Table) -> mlua::Result<ChatCompletionRequestMessage> {
    let role = table.get::<String>("role")?;
    let content = table.get::<String>("content")?;
    let name = if table.contains_key("name")? {
        Some(table.get::<String>("name")?)
    } else {
        None
    };

    match role.as_str() {
        "system" => Ok(ChatCompletionRequestMessage::System(
            ChatCompletionRequestSystemMessage {
                content: content.into(),
                name,
            },
        )),
        "user" => Ok(ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: content.into(),
                name,
            },
        )),
        "assistant" => Ok(ChatCompletionRequestMessage::Assistant(
            ChatCompletionRequestAssistantMessage {
                content: Some(content.into()),
                name,
                ..Default::default()
            },
        )),
        _ => Err(mlua::Error::FromLuaConversionError {
            from: "table",
            to: "ChatCompletionRequestMessage".to_string(),
            message: Some(format!("unknown role `{role}`")),
        }),
    }
}
