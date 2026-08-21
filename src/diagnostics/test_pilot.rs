//! A scripted pilot, so flight behaviour can be verified without a human at the keyboard.
//!
//! ```text
//! KRAKEN_PILOT=hop KRAKEN_TRACE=all cargo run
//! ```
//!
//! # Why this is permanent rather than a throwaway
//!
//! It was written from scratch three times during Phase 1 and deleted each time, and every
//! rewrite re-introduced the same mistakes — a sign-flipped descent controller once, a
//! forgotten throttle cut at touchdown another time, both of which briefly looked like
//! engine bugs. Landing is the hardest thing in the game to verify by hand and the easiest
//! to break by accident, so the fixture for it belongs in the repository.
//!
//! This is deliberately not a *good* autopilot and is not a game feature. It flies the
//! specific profiles that exercise the physics, using exactly the controls a player has:
//! it writes throttle and pitch/yaw into [`ControlState`] and nothing else.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::celestial::atmosphere::Atmosphere;
use crate::celestial::body::CelestialBody;
use crate::part_modules::engine::Engine;
use crate::part_modules::resource_container::{self, ResourceContainer};
use crate::rendering::render_sync::{SimPosition, SimRotation, SimVelocity};
use crate::vessel::components::{
    ActiveVessel, ControlState, Destroyed, PartMass, RootPart, VesselId,
};

/// Gain on the pointing error, in control deflection per radian of tilt.
const POINTING_GAIN: f64 = 3.0;
/// Damping on the body rate, so the attitude hold does not overshoot and ring.
const POINTING_DAMPING: f64 = 2.0;

/// Attitude bias used by `KRAKEN_PILOT_DRIFT` to acquire lateral velocity on the way up.
const DRIFT_BIAS: f64 = 0.35;

/// How far the descent leans into lateral drift, in radians of tilt per m/s.
const DRIFT_CANCEL_GAIN: f64 = 0.06;

/// Furthest from vertical the descent will lean to kill drift, radians (~17°).
///
/// Without a cap the aim point swings past horizontal on the first fast sideways moment and
/// the vessel thrusts itself into the ground.
const MAX_DESCENT_TILT_RAD: f64 = 0.3;

/// Apex the `hop` profile climbs to before turning around, metres.
const HOP_APEX_M: f64 = 60.0;
/// Apex the `ballistic` profile burns to, metres.
const BALLISTIC_APEX_M: f64 = 6_000.0;

/// Fraction of its rated thrust a vessel must be producing to count as running.
///
/// Not 1.0: an engine whose tank empties part-way through a tick delivers a fraction of
/// what was asked for, and that tick is exactly the one worth reacting to. Not much below
/// 1.0 either — the fraction is 1.0 to the bit while any propellant remains, so anything
/// under it means something has run out.
const FLAMEOUT_FRACTION: f64 = 0.99;

/// Root-part altitude below which the vessel counts as down, metres. The root is the top of
/// the stack, so this is the height of the stack itself plus a little.
const TOUCHDOWN_ALTITUDE_M: f64 = 6.0;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Sit still. Useful for watching a vessel settle onto the pad.
    Idle,
    /// Up to [`HOP_APEX_M`], then a controlled descent to a soft landing.
    Hop,
    /// Burn to [`BALLISTIC_APEX_M`] and let it fall back. Exercises drag and impact damage.
    Ballistic,
    /// Straight up, staging on flameout, until the atmosphere runs out. Phase 1's exit
    /// criterion: launch, reach space, and stage.
    ///
    /// # Why it does not fly a gravity turn
    ///
    /// A real ascent leans over and trades altitude for orbital velocity, and this one
    /// deliberately does not. Downrange flight leaves the 20 km terrain slab, and past a
    /// few tens of kilometres the flat pad stops being a plausible stand-in for the surface
    /// of a 600 km sphere — the ground curves away under you and the altitude readout stops
    /// agreeing with what you can see. Both are Phase 3 terrain work. Straight up measures
    /// the thing Phase 1 actually claims and nothing else.
    Ascent,
}

impl Profile {
    pub fn from_env() -> Option<Self> {
        match std::env::var("KRAKEN_PILOT").ok()?.as_str() {
            "idle" => Some(Self::Idle),
            "hop" => Some(Self::Hop),
            "ballistic" => Some(Self::Ballistic),
            "ascent" => Some(Self::Ascent),
            other => {
                error!(
                    "unknown KRAKEN_PILOT profile '{other}' — expected idle, hop, ballistic \
                     or ascent"
                );
                None
            }
        }
    }
}

/// What the pilot is flying through: the body it is climbing away from, and the air it is
/// climbing out of. Two resources, grouped so `fly`'s signature stays readable.
#[derive(SystemParam)]
pub struct Environment<'w> {
    body: Res<'w, CelestialBody>,
    atmosphere: Res<'w, Atmosphere>,
}

/// The vessel the pilot is allowed to fly: the active one, while it is still in one piece.
type FlyableVessel<'w, 's> =
    Query<'w, 's, (Entity, &'static mut ControlState), (With<ActiveVessel>, Without<Destroyed>)>;

/// The root part's full state, which is everything the pilot steers by.
type RootState<'w, 's> = Query<
    'w,
    's,
    (
        &'static VesselId,
        &'static SimPosition,
        &'static SimVelocity,
        &'static SimRotation,
        &'static bevy_rapier3d::prelude::Velocity,
    ),
    With<RootPart>,
>;

#[derive(Resource, Default)]
pub struct PilotState {
    apex_m: f64,
    landed: bool,
    /// Whether the engines have been seen producing thrust since the last stage request.
    ///
    /// Flameout is "commanded thrust, and none arriving", which is also exactly the state
    /// of a rocket in the instant before its engines light. Without this the ascent would
    /// stage itself on the launch pad, one tick after the world loads.
    thrust_seen: bool,
    /// Set once, so crossing the atmosphere's ceiling is announced once rather than fifty
    /// times a second.
    reached_space: bool,
}

/// Flies the active vessel through the configured profile.
pub fn fly(
    profile: Res<Profile>,
    mut state: ResMut<PilotState>,
    environment: Environment,
    mut vessels: FlyableVessel,
    roots: RootState,
    engines: Query<(&VesselId, &Engine)>,
    masses: Query<(&VesselId, &PartMass, Option<&ResourceContainer>)>,
) {
    let Ok((vessel, mut control)) = vessels.single_mut() else {
        return;
    };
    let Some((_, position, velocity, rotation, angular)) =
        roots.iter().find(|(id, ..)| id.0 == vessel)
    else {
        return;
    };

    let Environment { body, atmosphere } = &environment;
    let altitude = body.altitude_of(position.0);
    let up = body.up_at(position.0);
    let vertical = velocity.0.dot(up);
    state.apex_m = state.apex_m.max(altitude);

    // Where to point. Straight up while climbing; while descending, leaned into the lateral
    // drift so the same burn that slows the fall also kills the sideways motion. A vessel
    // that arrives moving sideways slides, digs a leg in and tips over — which is a real
    // failure, but not the one a landing test is trying to measure.
    let horizontal = velocity.0 - up * vertical;
    let aim = if vertical < 0.0 {
        let lean = (-horizontal * DRIFT_CANCEL_GAIN).clamp_length_max(MAX_DESCENT_TILT_RAD);
        (up + lean).normalize_or_zero()
    } else {
        up
    };

    // Hold the stack pointing at `aim`, using the player's own pitch/yaw controls.
    // Proportional on the pointing error, derivative on the body rate: manual input overrides
    // SAS entirely, so this has to supply its own damping.
    let inverse = rotation.0.inverse();
    let target = (inverse * aim).normalize_or_zero();
    let rate = inverse * angular.angvel.as_dvec3();
    control.pitch = (POINTING_GAIN * target.z - POINTING_DAMPING * rate.x).clamp(-1.0, 1.0) as f32;
    control.yaw = (-POINTING_GAIN * target.x - POINTING_DAMPING * rate.z).clamp(-1.0, 1.0) as f32;
    control.roll = 0.0;

    let max_thrust_n: f64 = engines
        .iter()
        .filter(|(id, _)| id.0 == vessel)
        .map(|(_, engine)| engine.thrust_n)
        .sum();
    // Thrust actually arriving, as opposed to what the engines could produce. Zero while the
    // throttle is open means the tanks feeding them are dry.
    let current_thrust_n: f64 = engines
        .iter()
        .filter(|(id, _)| id.0 == vessel)
        .map(|(_, engine)| engine.current_thrust_n)
        .sum();
    let mass_kg = resource_container::vessel_mass_kg(vessel, &masses);
    let gravity = body.gravity_at(position.0).length();

    control.throttle = match *profile {
        Profile::Idle => 0.0,
        Profile::Ballistic => {
            if vertical >= 0.0 && state.apex_m < BALLISTIC_APEX_M {
                1.0
            } else {
                0.0
            }
        }
        Profile::Ascent => {
            // Full throttle until the air runs out. Nothing subtler is called for: the
            // criterion is "reach space", and every kilometre of it is climbed against
            // gravity, so throttling back only spends more propellant getting there.
            if altitude < atmosphere.height_m {
                1.0
            } else {
                if !state.reached_space {
                    state.reached_space = true;
                    info!(
                        "reached space — {altitude:.0} m at {:.0} m/s, {mass_kg:.0} kg left",
                        velocity.0.length(),
                    );
                }
                0.0
            }
        }
        Profile::Hop => {
            // Cut at touchdown and stay cut. Without this the engine keeps pushing against
            // the ground, tips the vessel over and drives it across the pad — which took an
            // embarrassingly long time to recognise as the autopilot's fault and not the
            // impact model's.
            if state.landed
                || (state.apex_m > HOP_APEX_M / 2.0
                    && altitude < TOUCHDOWN_ALTITUDE_M
                    && vertical > -1.5)
            {
                state.landed = true;
                0.0
            } else if state.apex_m < HOP_APEX_M && vertical >= 0.0 {
                1.0
            } else if vertical > -0.5 {
                0.0
            } else {
                // Descend faster when high, slow to a crawl near the ground, and hold that
                // profile with a proportional term on top of the hover throttle.
                let wanted = -(1.0 + 0.12 * altitude.max(0.0)).min(30.0);
                let hover = (mass_kg * gravity / max_thrust_n) as f32;
                (hover + ((wanted - vertical) as f32) * 0.06).clamp(0.0, 1.0)
            }
        }
    };

    // Stage on flameout. `ControlState::stage` is a one-shot latch that
    // `vessel::staging::request_stage` consumes, so this is the same path the space bar
    // takes — the pilot has no privileged way to stage, by design.
    //
    // "Flameout" is *some* commanded thrust failing to arrive, not all of it. Every engine
    // on a vessel burns whenever the throttle is open — nothing ties ignition to a stage
    // number yet — so the first stage running dry shows up as the total dropping from
    // 140 kN to the 20 kN the upper engine is still producing, never as zero. Waiting for
    // zero means never staging at all, which is how this was found.
    //
    // Full thrust is also the state of a rocket in the second before its engines light,
    // hence `thrust_seen`: the comparison only counts once full thrust has been seen since
    // the last request. Clearing the flag with the request makes it self-limiting — one
    // request per flameout, not one per tick.
    if matches!(*profile, Profile::Ascent) && max_thrust_n > 0.0 {
        let lit = current_thrust_n >= max_thrust_n * FLAMEOUT_FRACTION;
        if lit {
            state.thrust_seen = true;
        } else if state.thrust_seen && control.throttle > 0.0 {
            state.thrust_seen = false;
            control.stage = true;
            info!(
                "flameout at {altitude:.0} m — {:.1} of {:.1} kN, {mass_kg:.0} kg — staging",
                current_thrust_n / 1000.0,
                max_thrust_n / 1000.0,
            );
        }
    }

    // Lean over briefly on the way up, so the landing has sideways drift to cope with — a
    // dead vertical hop never tests whether the legs do anything. A *bias* on top of the
    // attitude hold rather than a replacement for it: full deflection here tumbled the
    // stack hard enough to break a joint, which tested something else entirely.
    if matches!(*profile, Profile::Hop)
        && std::env::var("KRAKEN_PILOT_DRIFT").is_ok()
        && (3.0..9.0).contains(&altitude)
        && vertical > 0.0
    {
        control.pitch = (f64::from(control.pitch) + DRIFT_BIAS).clamp(-1.0, 1.0) as f32;
    }
}
