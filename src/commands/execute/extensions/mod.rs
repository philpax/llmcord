use std::sync::Arc;

use crate::ai::Ai;

mod globals;
mod llm;

pub fn register(
    lua: &mlua::Lua,
    ai: Arc<Ai>,
    output_tx: flume::Sender<mlua::Result<Option<String>>>,
) -> mlua::Result<()> {
    globals::register(lua, output_tx)?;
    llm::register(lua, ai)?;
    Ok(())
}
