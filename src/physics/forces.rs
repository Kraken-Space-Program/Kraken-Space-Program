//! [`PendingForces`] — the interface every force-producing system uses to push a part.
//!
//! design.md: "Any system that wants to push a part (thrust, drag, RCS, EVA jetpack)
//! writes a `Force` into `PendingForces`. The physics integration system reads all pending
//! forces, applies them to Rapier bodies, and clears the component. This decouples every
//! force-producing system from Rapier entirely."
//!
//! # When forces are cleared — the EXECUTION.md open question
//!
//! > *"Decide how `PendingForces` gets cleared — before or after Rapier step? What happens
//! > if two systems write to the same entity's forces in the same tick?"*
//!
//! **Cleared at the start of the tick, before any producer runs.** Three systems, chained,
//! all before `PhysicsSet::SyncBackend`:
//!
//! ```text
//!   clear_pending_forces  →  [thrust, drag, RCS, ...]  →  apply_pending_forces
//! ```
//!
//! Clearing *first* rather than after the step is what makes a missed tick safe. If a
//! producer is skipped — its vessel went on rails, its run condition failed, it panicked in
//! a previous build — its contribution disappears that tick, which is correct. Clearing
//! after the step would leave the last value in place and quietly keep thrusting from an
//! engine that had already shut down, which is exactly the kind of bug that shows up as
//! "my rocket accelerates in the map view" three months later.
//!
//! Two systems writing to the same part in one tick simply sum, which is what superposition
//! says should happen. Producers must **add**, never assign.
//!
//! # Deviation from design.md
//!
//! design.md sketches `PendingForces(Vec<Force>)`. This is a summing accumulator instead:
//! the vector's only advantage is being able to ask "which system contributed what", and
//! nothing needs that yet. Adding it later is a local change to this file plus the
//! producers. If a force-debugging overlay ever needs the breakdown, that is the moment.

use bevy::math::DVec3;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

/// Forces to apply to a part this tick, in **newtons** and **newton-metres**, world space.
///
/// f64 because producers live outside `physics/` and everything crossing a module boundary
/// is f64. The narrowing to Rapier's f32 happens in [`apply_pending_forces`], inside this
/// module, where it is allowed.
#[derive(Component, Default, Debug, Clone, Copy)]
pub struct PendingForces {
    pub linear_n: DVec3,
    pub torque_nm: DVec3,
}

impl PendingForces {
    /// Adds a world-space force. Producers use this — never assignment.
    pub fn add_force(&mut self, force_n: DVec3) {
        self.linear_n += force_n;
    }

    /// Adds a world-space torque.
    pub fn add_torque(&mut self, torque_nm: DVec3) {
        self.torque_nm += torque_nm;
    }
}

/// Zeroes every accumulator. Runs first in the chain, before any producer.
pub fn clear_pending_forces(mut query: Query<&mut PendingForces>) {
    for mut forces in &mut query {
        // Assigning through the change-detection guard on every part every tick would mark
        // them all changed forever. Only touch the ones that actually carry a force.
        if forces.linear_n != DVec3::ZERO || forces.torque_nm != DVec3::ZERO {
            *forces = PendingForces::default();
        }
    }
}

/// Hands the accumulated forces to Rapier. Runs last in the chain.
///
/// Must be before `PhysicsSet::SyncBackend`: Rapier picks up `ExternalForce` through a
/// `Changed<ExternalForce>` query in that set, so a write afterwards would not be seen
/// until the following tick.
pub fn apply_pending_forces(mut query: Query<(&PendingForces, &mut ExternalForce)>) {
    for (pending, mut external) in &mut query {
        // f64 → f32 for Rapier. Legal here: this is inside physics/, and a force in
        // newtons has nothing like the dynamic range of a planetary-scale position.
        let force = pending.linear_n.as_vec3();
        let torque = pending.torque_nm.as_vec3();

        // Same reasoning as above — avoid marking every body changed every tick.
        if external.force != force || external.torque != torque {
            external.force = force;
            external.torque = torque;
        }
    }
}
