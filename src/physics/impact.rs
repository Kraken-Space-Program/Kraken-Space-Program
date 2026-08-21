//! Turns Rapier collision events into impact speeds.
//!
//! # Why this needs the *previous* tick's velocity
//!
//! Rapier writes collision events during `PhysicsSet::StepSimulation`, and the earliest a
//! system can read them is after `PhysicsSet::Writeback` — by which point the contact has
//! already been resolved and the part's velocity is whatever it is *after* being stopped.
//! For the case that matters most, a rocket arriving at the pad, that number is close to
//! zero: the harder the landing, the less of it survives to be measured.
//!
//! So the speed comes from [`PreviousSimVelocity`], snapshotted before the tick ran. That
//! is the velocity the part carried into the collision, which is the number a damage model
//! actually wants.
//!
//! # What this module deliberately does not do
//!
//! It does not decide what an impact *means*. Thresholds, destruction and any future
//! per-part impact tolerance live in `vessel::damage`; this file only measures.

use bevy::math::DVec3;
use bevy::prelude::*;
use bevy_rapier3d::prelude::CollisionEvent;

use crate::rendering::render_sync::PreviousSimVelocity;

/// A part touched something at a measurable speed.
///
/// Emitted for every part involved in a new contact, including gentle ones — a system
/// interested in touchdown detection or landing-gear compression wants the quiet events
/// too, not just the fatal ones.
#[derive(Message, Debug, Clone, Copy)]
pub struct Impact {
    pub part: Entity,
    /// What it hit. Terrain, another vessel, or another part — consumers that care about
    /// the difference have to look the entity up themselves, because this module knows
    /// nothing about vessels.
    pub other: Entity,
    /// Closing speed between the two bodies, metres per second.
    pub speed_ms: f64,
}

/// Reads Rapier contacts and reports the closing speed of each one.
///
/// # Ordering
///
/// `FixedUpdate`, after `PhysicsSet::Writeback` — the events do not exist before the step
/// has run — but before `store_previous_state` overwrites the snapshot on the next tick.
/// Since `store_previous_state` runs at the *start* of a tick and this runs at the end of
/// one, that ordering falls out of the schedule for free.
pub fn detect_impacts(
    mut collisions: MessageReader<CollisionEvent>,
    velocities: Query<&PreviousSimVelocity>,
    mut impacts: MessageWriter<Impact>,
) {
    for event in collisions.read() {
        // Only the start of a contact carries an impact. `Stopped` is two things ceasing to
        // touch, which is a separation, not a collision.
        let CollisionEvent::Started(a, b, _) = event else {
            continue;
        };

        // Scenery has no velocity component, which is exactly right: a static body's
        // contribution to the closing speed is zero.
        let velocity_of = |entity: &Entity| {
            velocities
                .get(*entity)
                .map(|velocity| velocity.0)
                .unwrap_or(DVec3::ZERO)
        };
        let speed_ms = (velocity_of(a) - velocity_of(b)).length();

        // Report per part, not per contact: a part that hits two things in one tick took
        // two impacts, and whatever consumes these should see both.
        for (part, other) in [(a, b), (b, a)] {
            if velocities.contains(*part) {
                impacts.write(Impact {
                    part: *part,
                    other: *other,
                    speed_ms,
                });
            }
        }
    }
}
