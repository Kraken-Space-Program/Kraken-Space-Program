//! Low-level Lua bindings for the Kraken SDK.
//!
//! This module uses `mlua` to expose Rust functions and types to Lua scripts.
//! It provides the foundation for the high-level APIs in `api/`.

use mlua::prelude::*;

/// Creates a new Lua instance with Kraken-specific bindings.
///
/// This is called by the sandbox module to set up the Lua environment.
/// For now, it's a minimal setup — we'll expand as APIs are added.
pub fn create_lua() -> LuaResult<Lua> {
    let lua = Lua::new();

    // TODO: Add global functions like `register_part`, `register_planet`, etc.
    // For example:
    // lua.globals().set("register_part", lua.create_function(register_part)?)?;

    Ok(lua)
}

/// Placeholder for registering a part from Lua.
///
/// In the future, this will take a Lua table and convert it to a Rust `PartDefinition`.
fn register_part(_lua: &Lua, _part_table: mlua::Value) -> LuaResult<()> {
    // TODO: Deserialize Lua table into PartDefinition and store it.
    eprintln!("Part registration not yet implemented");
    Ok(())
}