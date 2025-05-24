use std::sync::Arc;

pub fn register(
    lua: &mlua::Lua,
    output_tx: flume::Sender<mlua::Result<Option<String>>>,
) -> mlua::Result<()> {
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

    lua.globals().set(
        "inspect",
        lua.load(include_str!("../../../../vendor/inspect.lua/inspect.lua"))
            .eval::<mlua::Value>()?,
    )?;

    lua.globals().set(
        "print",
        lua.create_function(move |_lua, value: mlua::Value| {
            let output_tx = output_tx.clone();
            output_tx
                .send(Ok(Some(value.to_string()?)))
                .map_err(|e| mlua::Error::ExternalError(Arc::new(e)))?;
            Ok(())
        })?,
    )?;

    Ok(())
}
