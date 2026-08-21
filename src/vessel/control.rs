//! Player input → vessel control state → attitude torque.
//!
//! # Why input is sampled per frame but consumed per tick
//!
//! Keyboard state is read in `Update`, once per rendered frame, and stored on
//! [`ControlState`]. The physics systems read that stored value in `FixedUpdate` at 50 Hz.
//!
//! The alternative — reading the keyboard inside `FixedUpdate` — looks simpler and is
//! wrong: `just_pressed` is computed per frame, so on a 144 Hz display a tap that starts
//! and ends between two ticks would never be seen at all. Latching in `Update` means every
//! input reaches the simulation exactly once.

use bevy::math::DVec3;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy_rapier3d::prelude::{ReadMassProperties, Velocity};

use crate::part_modules::reaction_wheel::ReactionWheel;
use crate::physics::forces::PendingForces;
use crate::rendering::render_sync::{SimPosition, SimRotation};
use crate::vessel::components::{ActiveVessel, ControlState, RootPart, VesselId};

/// How fast the throttle ramps, in fraction per second.
///
/// Instant throttle would be twitchy and unlike any real engine. Two seconds from idle to
/// full is roughly a real gimballed engine's spool rate and, more importantly, is slow
/// enough to be flyable with a keyboard.
const THROTTLE_RATE_PER_SEC: f32 = 0.5;

/// How long SAS takes to null a rotation, in seconds.
///
/// # Why this is a time and not a gain
///
/// The obvious implementation is `torque = -gain * angular_velocity` for some tuned `gain`.
/// It does not work, and it fails in a way that looks like a physics bug rather than a
/// controller bug: the angular velocity SAS reads is one tick old, so if the commanded
/// torque can carry the rotation past zero within that tick, the next correction points the
/// other way and the vessel oscillates. Saturated against `ATTITUDE_TORQUE_NM` it becomes
/// bang-bang and settles into a steady spin instead of stopping — measured at 0.68 rad/s,
/// reversing sign every two seconds, on a vessel that was doing nothing but sitting still.
///
/// Whether a given gain is stable depends on the vessel's moment of inertia, so no constant
/// is correct for more than one rocket. Asking for `torque = I·ω/T` instead makes the
/// response independent of inertia: the correction always aims to remove the rotation over
/// `T` seconds, whether it is a probe or a full launch stack.
///
/// One second is slow enough to be stable with a wide margin (instability needs the ratio
/// `dt/T` to exceed 2, and `dt/T` here is 0.02) and fast enough to feel responsive.
const SAS_SETTLE_SECS: f64 = 1.0;

/// Rotation below which SAS stops correcting, rad/s.
///
/// The joint solver leaves a few thousandths of a radian per second of residual motion on a
/// vessel that is perfectly still. Without a deadband SAS chases that noise forever, which
/// keeps every part marked as force-changed every tick for no benefit.
const SAS_DEADBAND_RAD_S: f64 = 0.01;

/// Samples the keyboard into the active vessel's [`ControlState`].
///
/// # Controls
///
/// | Key | Action |
/// |-----|--------|
/// | `Shift` / `Ctrl` | throttle up / down |
/// | `Z` / `X` | throttle full / cut |
/// | `W` / `S` | pitch |
/// | `A` / `D` | yaw |
/// | `Q` / `E` | roll |
/// | `T` | toggle SAS |
pub fn read_control_input(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut vessels: Query<&mut ControlState, With<ActiveVessel>>,
) {
    let Ok(mut control) = vessels.single_mut() else {
        return;
    };

    let step = THROTTLE_RATE_PER_SEC * time.delta_secs();
    if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        control.throttle = (control.throttle + step).min(1.0);
    }
    if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        control.throttle = (control.throttle - step).max(0.0);
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        control.throttle = 1.0;
    }
    if keys.just_pressed(KeyCode::KeyX) {
        control.throttle = 0.0;
    }

    if keys.just_pressed(KeyCode::KeyT) {
        control.sas = !control.sas;
        info!("SAS {}", if control.sas { "on" } else { "off" });
    }

    control.pitch = axis(&keys, KeyCode::KeyS, KeyCode::KeyW);
    control.yaw = axis(&keys, KeyCode::KeyD, KeyCode::KeyA);
    control.roll = axis(&keys, KeyCode::KeyE, KeyCode::KeyQ);
}

fn axis(keys: &ButtonInput<KeyCode>, positive: KeyCode, negative: KeyCode) -> f32 {
    f32::from(keys.pressed(positive)) - f32::from(keys.pressed(negative))
}

/// Turns control input into torque on the vessel's root part.
///
/// Applying the whole vessel's control torque at one part is a simplification: real
/// attitude authority comes from reaction wheels and gimballed engines distributed through
/// the stack. Because the parts are rigidly joined, the joints carry the torque to the rest
/// of the vessel anyway, so the flight behaviour is close and the bookkeeping is trivial.
///
/// # SAS
///
/// With no key held and SAS on, the same torque budget is spent cancelling the vessel's own
/// rotation. This is a **damper, not a heading hold**: it brings you to a stop wherever you
/// are pointing, it does not fly back to a heading you left. That distinction matters for a
/// landing — you flip retrograde by hand, release, and SAS keeps you there rather than
/// letting the residual rate walk you off it over the next thirty seconds.
///
/// A real attitude controller belongs with reaction wheels as a part module (CHECKLIST #2);
/// this shares `ATTITUDE_TORQUE_NM` with manual control precisely so it cannot be stronger
/// than the vessel's actual authority.
pub fn apply_attitude_control(
    vessels: Query<&ControlState>,
    parts: Query<(&VesselId, &SimPosition, &ReadMassProperties)>,
    rates: Query<(&VesselId, &Velocity), With<RootPart>>,
    mut wheels: Query<(&ReactionWheel, &VesselId, &SimRotation, &mut PendingForces)>,
) {
    // What each vessel wants done, worked out once, then divided among its wheels.
    let mut demand: HashMap<Entity, AttitudeDemand> = HashMap::new();

    for (_, vessel_id, _, _) in &wheels {
        if demand.contains_key(&vessel_id.0) {
            continue;
        }
        let Ok(control) = vessels.get(vessel_id.0) else {
            continue;
        };

        // Total authority is whatever is bolted to this vessel. A stack that has lost its
        // probe core to staging has none, and stops responding — which is the point.
        let available_nm: f64 = wheels
            .iter()
            .filter(|(_, owner, _, _)| owner.0 == vessel_id.0)
            .map(|(wheel, _, _, _)| wheel.torque_nm)
            .sum();
        if available_nm <= 0.0 {
            continue;
        }

        // Control axes are vessel-local: pitch about its right axis, yaw about its up axis,
        // roll about the axis it points along. Using world axes instead would make the
        // controls swap meaning as soon as the rocket tipped over.
        let local = DVec3::new(
            f64::from(control.pitch),
            f64::from(control.roll),
            f64::from(control.yaw),
        );

        let wanted = if local != DVec3::ZERO {
            // Manual input wins outright. Blending SAS in here would fight the player on
            // every deliberate turn and make the rocket feel like it is resisting them.
            Wanted::Local(local)
        } else if !control.sas {
            continue;
        } else {
            let rate = rates
                .iter()
                .find(|(owner, _)| owner.0 == vessel_id.0)
                .map(|(_, velocity)| velocity.angvel.as_dvec3())
                .unwrap_or(DVec3::ZERO);
            if rate.length() < SAS_DEADBAND_RAD_S {
                continue;
            }
            // `τ = I·ω/T` — the torque that removes this rotation over SAS_SETTLE_SECS.
            let correction = -rate * (vessel_inertia(vessel_id.0, &parts) / SAS_SETTLE_SECS);
            Wanted::World(correction.clamp_length_max(available_nm))
        };

        demand.insert(
            vessel_id.0,
            AttitudeDemand {
                wanted,
                available_nm,
            },
        );
    }

    for (wheel, vessel_id, rotation, mut forces) in &mut wheels {
        let Some(demand) = demand.get(&vessel_id.0) else {
            continue;
        };

        // Each wheel does its share of the work, in proportion to how much of the vessel's
        // authority it is. Applying the total at one part would work too — the joints carry
        // it either way — but this puts the torque where the hardware is, which is what
        // makes a vessel with wheels at one end behave differently from one with them in
        // the middle once flexible joints exist.
        let share = wheel.torque_nm / demand.available_nm;

        match demand.wanted {
            Wanted::Local(axes) => forces.add_torque(rotation.0 * axes * wheel.torque_nm),
            Wanted::World(torque) => forces.add_torque(torque * share),
        }
    }
}

/// What a vessel's wheels are being asked to do this tick.
struct AttitudeDemand {
    wanted: Wanted,
    /// Total torque the vessel's wheels can produce, newton-metres.
    available_nm: f64,
}

enum Wanted {
    /// Player deflection, in vessel-local axes, as a fraction of each wheel's own torque.
    Local(DVec3),
    /// An absolute world-space torque to be divided among the wheels — the SAS correction.
    World(DVec3),
}

/// Approximate moment of inertia of a whole vessel about its centre of mass, kg·m².
///
/// # Why the whole vessel and not just the root part
///
/// SAS applies its torque at the root part, but what it has to rotate is the entire stack.
/// Sizing the correction from the root part's own inertia looks reasonable and is wrong by
/// the parallel-axis terms — for the five-part test stack, about twenty times too small.
/// The visible symptom is a vessel that tips over during a two-minute coast while SAS
/// reports that it is correcting: the commanded torque is real, it is just nowhere near
/// enough to matter.
///
/// # The approximation
///
/// A scalar, not a tensor. Each part contributes its own largest principal inertia plus
/// `m·d²` for its offset from the vessel's centre of mass. For a stack — a line of masses —
/// the parallel-axis term dominates and this is close. For rotation *about* the stack axis
/// it overestimates, because the `d²` offsets are along that axis and should not count; the
/// consequence is that roll is corrected faster than pitch and yaw, which is harmless.
///
/// Exact inertia belongs with the reaction-wheel part module (CHECKLIST #2), where there is
/// a real torque budget to spend and the tensor is worth carrying.
fn vessel_inertia(
    vessel: Entity,
    parts: &Query<(&VesselId, &SimPosition, &ReadMassProperties)>,
) -> f64 {
    let mut total_mass = 0.0;
    let mut weighted_position = DVec3::ZERO;

    for (owner, position, mass_properties) in parts {
        if owner.0 != vessel {
            continue;
        }
        let mass = f64::from(mass_properties.get().mass);
        total_mass += mass;
        weighted_position += position.0 * mass;
    }

    if total_mass <= 0.0 {
        return 0.0;
    }
    let centre_of_mass = weighted_position / total_mass;

    parts
        .iter()
        .filter(|(owner, _, _)| owner.0 == vessel)
        .map(|(_, position, mass_properties)| {
            let properties = mass_properties.get();
            let offset = (position.0 - centre_of_mass).length_squared();
            f64::from(properties.principal_inertia.max_element())
                + f64::from(properties.mass) * offset
        })
        .sum()
}
