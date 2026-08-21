//! Putting the launch site back the way it started.
//!
//! Flight is one-way: fuel burns, stages separate, joints break, and a vessel that has
//! come apart cannot be flown again. Until there is a save system (Phase 2) and a vessel
//! editor (Phase 4), the only way back to the pad is to quit and start the game again —
//! which means waiting on a load for every test of a change to launch behaviour.
//!
//! So: `0` rebuilds the world. Not a save/load, not a rewind — the old vessel is thrown
//! away and the test stack is assembled from its part definitions exactly as it was at
//! startup.

use bevy::ecs::system::SystemParam;
use bevy::math::DVec3;
use bevy::prelude::*;

use crate::diagnostics::test_pilot::PilotState;
use crate::rendering::render_sync::{LocalOrigin, SyncRender, WorldOrigin};
use crate::vessel::assembly;
use crate::vessel::components::{Vessel, VesselId};

/// `0` → discard everything that is flying and rebuild the test stack on the pad.
///
/// # What gets thrown away
///
/// Every vessel entity, every part, and every visual — including debris from earlier
/// stagings, which belongs to its own vessel entity and would otherwise still be lying on
/// the pad the new stack is spawned onto. Despawning a part takes its joint and its
/// Rapier body with it, so nothing has to be unwired by hand.
///
/// The origins are reset too. Krakensbane will have shifted them if the flight got far
/// enough, and they are the frame the rebuilt vessel's positions are expressed in: leaving
/// them where a suborbital hop had pushed them would spawn the new stack at the pad in
/// simulation space and several kilometres away in render space.
pub fn reset_flight(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    flight: FlightEntities,
    mut world_origin: ResMut<WorldOrigin>,
    mut local_origin: ResMut<LocalOrigin>,
    // Only exists when KRAKEN_PILOT is set — see `diagnostics::test_pilot`.
    pilot: Option<ResMut<PilotState>>,
) {
    if !keys.just_pressed(KeyCode::Digit0) && !keys.just_pressed(KeyCode::Numpad0) {
        return;
    }

    for entity in flight.iter() {
        commands.entity(entity).despawn();
    }

    world_origin.0 = DVec3::ZERO;
    local_origin.0 = DVec3::ZERO;

    // The scripted pilot remembers the apex it reached and whether it has landed. Both are
    // about a vessel that no longer exists, and left alone they would have the pilot fly
    // the descent half of its profile into a rocket standing on the pad.
    if let Some(mut pilot) = pilot {
        *pilot = PilotState::default();
    }

    // Queued behind the despawns rather than run here: commands apply in order, so by the
    // time assembly runs the old world is already gone. Calling it directly instead would
    // build the new stack while the wreck of the old one still had colliders on the pad.
    commands.run_system_cached(assembly::spawn_test_rocket);

    info!("flight reset — rebuilding the test stack on the pad");
}

/// Everything a flight is made of, from the point of view of throwing it away.
///
/// Grouped rather than listed on [`reset_flight`] because the three queries are one idea:
/// a reset despawns all of it and cares about nothing else in the world — not the terrain,
/// not the pad, not the camera.
#[derive(SystemParam)]
pub struct FlightEntities<'w, 's> {
    vessels: Query<'w, 's, Entity, With<Vessel>>,
    parts: Query<'w, 's, Entity, With<VesselId>>,
    visuals: Query<'w, 's, Entity, With<SyncRender>>,
}

impl FlightEntities<'_, '_> {
    /// The three sets are disjoint — a vessel entity carries `Vessel`, a part carries
    /// `VesselId`, a visual carries `SyncRender` — so nothing is yielded twice and no
    /// entity is despawned twice. A visual's meshes are its children and go with it.
    fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.vessels.iter().chain(&self.parts).chain(&self.visuals)
    }
}
