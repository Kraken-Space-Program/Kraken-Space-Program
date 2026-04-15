//! Lua mod file loading and validation.
//!
//! This module scans for mod files (e.g., in `assets/mods/`) and loads them
//! into the sandbox. It validates syntax and basic structure before execution.

use crate::sdk::sandbox::LuaSandbox;

/// Loads all available Kraken mods.
///
/// For now, this is a placeholder. In the future, it will:
/// - Scan directories for .lua files.
/// - Load and validate each mod.
/// - Handle dependencies and load order.
pub fn load_mods() -> Vec<LuaSandbox> {
    // TODO: Implement actual loading logic.
    eprintln!("Mod loading not yet implemented");
    vec![]
}

/// Validates a Lua script without executing it.
///
/// Returns true if the script is syntactically valid.
pub fn validate_script(script: &str) -> bool {
    // TODO: Use mlua to check syntax.
    eprintln!("Script validation not yet implemented for: {}", script);
    true // Placeholder
}