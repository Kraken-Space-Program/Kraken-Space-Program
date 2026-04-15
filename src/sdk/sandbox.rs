//! Lua sandbox and VM management.
//!
//! This module sets up a safe Lua execution environment with:
//! - Memory and instruction limits to prevent abuse.
//! - Error handling that logs issues without crashing the game.
//! - Restricted access to system resources.

use mlua::prelude::*;
use crate::sdk::bindings;

/// A sandboxed Lua VM for running Kraken mods.
pub struct LuaSandbox {
    lua: Lua,
}

impl LuaSandbox {
    /// Creates a new sandboxed Lua environment.
    pub fn new() -> LuaResult<Self> {
        let lua = bindings::create_lua()?;

        // TODO: Apply sandbox restrictions:
        // - Remove dangerous globals (os, io, etc.)
        // - Set memory limits
        // - Add instruction counting for CPU limits

        Ok(Self { lua })
    }

    /// Executes a Lua script string.
    ///
    /// Errors are logged but don't crash the game.
    pub fn execute(&self, script: &str) -> LuaResult<()> {
        match self.lua.load(script).exec() {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("Lua execution error: {}", e);
                // TODO: Fire a LuaError event instead of just printing.
                Ok(()) // Continue running the game
            }
        }
    }

    /// Loads and executes a Lua file.
    pub fn load_file(&self, path: &str) -> LuaResult<()> {
        // TODO: Read file from assets/mods/, then execute.
        eprintln!("File loading not yet implemented for: {}", path);
        Ok(())
    }
}