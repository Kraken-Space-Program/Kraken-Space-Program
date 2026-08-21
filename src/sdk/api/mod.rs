//! The Kraken SDK's Lua-facing API surface.
//!
//! design.md: "There is no distinction between official content and mods at the engine
//! level." The base game's parts are loaded through exactly these functions, which is the
//! only way to be sure the modding API is actually good enough to build a game with.
//!
//! One file per API area, matching `sdk/api/` in the module structure: `parts.rs` today,
//! `planets.rs`, `vessels.rs` and `ui.rs` as later phases need them.

pub mod parts;
