//! Part modules — one file per module type.
//!
//! design.md, "Part modules are components, not objects": an engine part has an
//! [`Engine`](engine::Engine) component, a tank has a
//! [`ResourceContainer`](resource_container::ResourceContainer). There is no `PartModule`
//! base class, no virtual dispatch, and no lifecycle methods to leave empty.
//!
//! Adding a new module type means: add a file here with a component and a system, add a
//! variant to `sdk::part_def::PartModuleDef`, and insert it in `vessel::assembly`. Nothing
//! inherits from anything.

use bevy::prelude::*;

use crate::physics::PhysicsSchedule;

pub mod decoupler;
pub mod engine;
pub mod reaction_wheel;
pub mod resource_container;

pub struct PartModulesPlugin;

impl Plugin for PartModulesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            // A force producer: runs inside the clear → produce → apply chain.
            // `update_wet_mass` is scheduled by PhysicsPlugin alongside the force
            // application, since it writes to Rapier's mass properties rather than to
            // PendingForces.
            engine::apply_engine_thrust.in_set(PhysicsSchedule::Produce),
        );
    }
}
