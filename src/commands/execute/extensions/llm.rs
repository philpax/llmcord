use std::sync::Arc;

use crate::ai::Ai;

pub fn register(lua: &mlua::Lua, ai: Arc<Ai>) -> mlua::Result<()> {
    let llm = lua.create_table()?;
    llm.set("models", ai.models.clone())?;

    register_message(lua, &llm, "system")?;
    register_message(lua, &llm, "user")?;
    register_message(lua, &llm, "assistant")?;

    lua.globals().set("llm", llm)?;

    Ok(())
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
