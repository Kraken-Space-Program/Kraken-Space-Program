//! Vessels — assembly, control input, staging, and putting it all back on the pad.

use bevy::prelude::*;

use crate::physics::PhysicsSchedule;

pub mod assembly;
pub mod components;
pub mod control;
pub mod damage;
pub mod geometry;
pub mod reset;
pub mod staging;

pub struct VesselPlugin;

impl Plugin for VesselPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<staging::StageActivated>()
            .add_message::<staging::VesselSplit>()
            // Explicitly after the camera exists. Assembly points the camera at the
            // finished stack through `Single<&mut FlightCamera>`, and a `Single` that
            // matches nothing skips its system *silently* — so without this ordering the
            // rocket simply never gets built, with no error to explain why.
            .add_systems(
                Startup,
                assembly::spawn_test_rocket.after(crate::rendering::camera::spawn_flight_camera),
            )
            .add_systems(
                Update,
                (
                    control::read_control_input,
                    staging::request_stage,
                    reset::reset_flight,
                ),
            )
            .add_systems(
                FixedUpdate,
                control::apply_attitude_control.in_set(PhysicsSchedule::Produce),
            )
            // Staging and impact damage both edit the world (removing joints, spawning a
            // vessel entity), so they run outside the force chain, after the tick has been
            // simulated. Damage goes second: a stage separation must not be reinterpreted
            // as a crash within the same tick.
            .add_systems(
                FixedPostUpdate,
                (
                    staging::fire_decouplers,
                    staging::split_on_joint_failure,
                    damage::destroy_on_impact,
                )
                    .chain(),
            );
    }
}
