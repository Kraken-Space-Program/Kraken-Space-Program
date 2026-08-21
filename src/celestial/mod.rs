//! Celestial bodies: their gravity, and their atmospheres.
//!
//! Phase 1 is one body and one atmosphere, both resources with hardcoded stock values.
//! Phase 2 loads them from Lua and they become components on body entities — see
//! `sdk/api/planets.rs` in DESIGN.md's module plan. Nothing outside this module reads the
//! numbers directly; everything goes through [`body::CelestialBody`] and
//! [`atmosphere::Atmosphere`], so that change stays contained.

use bevy::prelude::*;

use crate::physics::PhysicsSchedule;

pub mod atmosphere;
pub mod body;
pub mod occlusion;

pub struct CelestialPlugin;

impl Plugin for CelestialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<body::CelestialBody>()
            .init_resource::<atmosphere::Atmosphere>()
            // Both are force producers, so they sit in the same set as engine thrust and
            // never touch Rapier themselves.
            .add_systems(
                FixedUpdate,
                (body::apply_gravity, atmosphere::apply_drag).in_set(PhysicsSchedule::Produce),
            );
    }
}
