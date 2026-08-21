//! Rapier (f32, local physics frame) → [`SimPosition`] (f64, simulation space).
//!
//! # Why this file exists
//!
//! Rapier is f32 throughout. There is no f64 build of it, and there is not going to be.
//! So `SimPosition(DVec3)` cannot be the value Rapier integrates — it has to be a value
//! *derived* from what Rapier integrates:
//!
//! ```text
//!   SimPosition (f64)  =  WorldOrigin (f64)  +  Rapier Transform (f32, local frame)
//! ```
//!
//! Rapier only ever sees the local physics frame, which Krakensbane keeps within ~10 km
//! of zero. At that magnitude f32 has roughly millimetre precision, which is fine for
//! contact resolution. The f64 value is what everything outside `physics/` and
//! `rendering/` reads: orbital mechanics, save files, and the network layer.
//!
//! This system is the counterpart to [`crate::rendering::render_sync`]:
//!
//! ```text
//!   Rapier f32 local  --readback.rs-->  SimPosition f64  --render_sync.rs-->  Transform f32
//! ```
//!
//! It runs in `FixedUpdate` after `PhysicsSet::Writeback`, so it reads Rapier's final
//! transform for the tick.

use bevy::prelude::*;

use crate::rendering::render_sync::{SimPosition, SimRotation, SimVelocity, WorldOrigin};

/// Marker: this entity's [`SimPosition`] is produced by Rapier, not by an analytic orbit.
///
/// On-rails vessels (Phase 3) will *not* carry this — their `SimPosition` is computed
/// from orbital elements instead, and Rapier knows nothing about them.
#[derive(Component)]
pub struct PhysicsDriven;

/// Lifts Rapier's f32 local transform into f64 simulation space.
///
/// # Ordering
///
/// `FixedUpdate`, after [`PhysicsSet::Writeback`](bevy_rapier3d::plugin::PhysicsSet::Writeback).
/// Anything that reads `SimPosition` should run after this or in a later schedule.
pub fn readback_sim_positions(
    world_origin: Res<WorldOrigin>,
    mut query: Query<
        (
            &Transform,
            &bevy_rapier3d::prelude::Velocity,
            &mut SimPosition,
            &mut SimRotation,
            &mut SimVelocity,
        ),
        With<PhysicsDriven>,
    >,
) {
    for (transform, velocity, mut sim_pos, mut sim_rot, mut sim_vel) in &mut query {
        // f32 → f64 widening. This is always lossless: every f32 is exactly
        // representable as an f64. The precision-critical direction is the *other*
        // one, and that lives in render_sync.rs behind the subtract-first rule.
        sim_pos.0 = world_origin.0 + transform.translation.as_dvec3();
        sim_rot.0 = transform.rotation.as_dquat();
        // Velocity needs no origin correction: shifting the origin moves positions, not
        // their derivatives.
        sim_vel.0 = velocity.linvel.as_dvec3();
    }
}
