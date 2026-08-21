//! The Kraken SDK — Lua definitions in, engine data out.
//!
//! # Scope right now
//!
//! Loading declarative *definitions* (parts today, celestial bodies later). Scripted
//! runtime behaviour — the part of the SDK that runs Lua every tick — is Phase 2, and the
//! tick-budget machinery in [`sandbox`] is already here waiting for it.
//!
//! # Dependency rule (design.md)
//!
//! `sdk/` talks to the rest of the engine only through ECS resources and events. It never
//! calls into `physics/`, `vessel/` or `rendering/`. The loader produces a
//! [`PartRegistry`](loader::PartRegistry); `vessel::assembly` consumes it. Neither knows
//! the other exists.

use bevy::prelude::*;

pub mod api;
pub mod loader;
pub mod part_def;
pub mod sandbox;
pub mod shape;

pub struct SdkPlugin;

impl Plugin for SdkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<loader::DefinitionRoot>()
            .add_message::<loader::LuaError>()
            .add_systems(PreStartup, loader::load_part_definitions);
    }
}
