use std::sync::Arc;

use crate::ai::Ai;

mod globals;
mod llm;

pub fn register(
    lua: &mlua::Lua,
    ai: Arc<Ai>,
    output_tx: flume::Sender<String>,
    print_tx: flume::Sender<String>,
) -> mlua::Result<()> {
    globals::register(lua, output_tx, print_tx)?;
    llm::register(lua, ai)?;
    Ok(())
}
