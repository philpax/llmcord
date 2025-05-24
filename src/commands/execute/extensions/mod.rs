use std::sync::Arc;

use crate::ai::Ai;

mod globals;
mod llm;

pub fn register(lua: &mlua::Lua, ai: Arc<Ai>) -> mlua::Result<()> {
    globals::register(lua)?;
    llm::register(lua, ai)?;
    Ok(())
}
