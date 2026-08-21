//! What an impact does to a vessel.
//!
//! # Why a hard landing breaks the stack instead of deleting parts
//!
//! The obvious model — despawn the part that hit too hard — has three failure modes that
//! all cost more than the effect is worth right now. A despawned part leaves neighbouring
//! `ImpulseJoint`s pointing at a dead entity; despawning the root part silently disables
//! attitude control and blanks the HUD, because both find the vessel through `RootPart`;
//! and two contacts in one tick despawn the same entity twice.
//!
//! Breaking every joint in the vessel has none of those problems, is one command per part,
//! and looks like what it is: the stack comes apart and the pieces tumble and scatter.
//! Removing an `ImpulseJoint` from a live entity is something the joints module already
//! supports and Rapier handles cleanly.
//!
//! Actual part destruction — debris meshes, explosions, a part disappearing — belongs with
//! the effects work in a later phase.

use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::physics::impact::Impact;
use crate::physics::joints;
use crate::vessel::components::{ControlState, Destroyed, PartId, VesselId};

/// Impact speed a part survives, metres per second.
///
/// One global number standing in for per-part impact tolerance declared in Lua, which is
/// where it belongs — a fuel tank and a landing leg should not have the same limit. Chosen
/// so that a landing has to be flown: fast enough that you cannot simply fall the last
/// hundred metres, forgiving enough that a reasonable suicide burn is survivable.
pub const SAFE_IMPACT_SPEED_MS: f64 = 8.0;

/// Breaks a vessel apart when any of its parts is hit too hard.
pub fn destroy_on_impact(
    mut commands: Commands,
    mut impacts: MessageReader<Impact>,
    parts: Query<(Entity, &PartId, &VesselId)>,
    mut vessels: Query<(&mut ControlState, Has<Destroyed>)>,
) {
    // A crash produces a contact per part per tick for as long as the wreck is settling, so
    // the same vessel arrives here many times over. `Has<Destroyed>` catches the repeats on
    // later ticks but *not* the ones in this same batch: the marker is inserted through
    // `Commands` and is not visible until the buffer flushes. Hence the local set as well.
    let mut destroyed_now: HashSet<Entity> = HashSet::new();

    for impact in impacts.read() {
        if impact.speed_ms <= SAFE_IMPACT_SPEED_MS {
            continue;
        }

        let Ok((_, part_id, vessel_id)) = parts.get(impact.part) else {
            continue;
        };

        // Two parts of the same vessel touching is not a crash.
        //
        // The joint solver leaves neighbouring parts in permanent contact, and every
        // attitude input makes them grind against each other — an early version of this
        // system destroyed the rocket in mid-air the first time it was asked to turn. Real
        // self-inflicted damage is structural failure (CHECKLIST #1), which compares joint
        // forces against the limits already parsed from Lua; it is not this.
        if parts
            .get(impact.other)
            .is_ok_and(|(_, _, other_vessel)| other_vessel.0 == vessel_id.0)
        {
            continue;
        }
        let Ok((mut control, already_destroyed)) = vessels.get_mut(vessel_id.0) else {
            continue;
        };
        if already_destroyed || !destroyed_now.insert(vessel_id.0) {
            continue;
        }

        warn!(
            "{} hit at {:.1} m/s — vessel destroyed (limit {SAFE_IMPACT_SPEED_MS:.0} m/s)",
            part_id.0, impact.speed_ms,
        );

        commands.entity(vessel_id.0).insert(Destroyed);
        // Engines read throttle from here, so this is also what stops a crashed vessel
        // from continuing to burn while it lies in pieces on the pad.
        *control = ControlState::default();

        for (part, _, owner) in &parts {
            if owner.0 == vessel_id.0 {
                joints::detach(&mut commands, part);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs one update with a single impact against a one-part vessel and reports whether
    /// the vessel survived.
    ///
    /// No Rapier: the system under test only reads components and issues commands, so a
    /// bare `App` exercises exactly the logic in question and nothing else.
    fn survives(speed_ms: f64) -> bool {
        let mut app = App::new();
        app.add_message::<Impact>()
            .add_systems(Update, destroy_on_impact);

        let vessel = app
            .world_mut()
            .spawn(ControlState {
                throttle: 1.0,
                ..default()
            })
            .id();
        let part = app
            .world_mut()
            .spawn((PartId("test.part".into()), VesselId(vessel)))
            .id();

        app.world_mut().write_message(Impact {
            part,
            other: Entity::PLACEHOLDER,
            speed_ms,
        });
        app.update();

        app.world().get::<Destroyed>(vessel).is_none()
    }

    #[test]
    fn a_gentle_touchdown_is_survivable() {
        assert!(survives(SAFE_IMPACT_SPEED_MS - 1.0));
    }

    #[test]
    fn a_hard_landing_destroys_the_vessel() {
        assert!(!survives(SAFE_IMPACT_SPEED_MS + 1.0));
    }

    /// A crash must also stop the engines. Otherwise the wreck keeps thrusting along the
    /// ground at whatever throttle it hit with, which looks exactly like a physics bug.
    #[test]
    fn destruction_cuts_the_throttle() {
        let mut app = App::new();
        app.add_message::<Impact>()
            .add_systems(Update, destroy_on_impact);

        let vessel = app
            .world_mut()
            .spawn(ControlState {
                throttle: 1.0,
                ..default()
            })
            .id();
        let part = app
            .world_mut()
            .spawn((PartId("test.part".into()), VesselId(vessel)))
            .id();

        app.world_mut().write_message(Impact {
            part,
            other: Entity::PLACEHOLDER,
            speed_ms: SAFE_IMPACT_SPEED_MS * 4.0,
        });
        app.update();

        assert_eq!(
            app.world().get::<ControlState>(vessel).unwrap().throttle,
            0.0
        );
    }
}
