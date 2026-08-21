//! Flight diagnostics: an always-on watchdog, and an opt-in per-tick recorder.
//!
//! # Why this module exists
//!
//! Two of the three worst bugs in Phase 1 were invisible at the resolution anybody was
//! looking at. A stationary rocket on the launch pad was bouncing at 9.5 m/s and loading its
//! joints to nine times their breaking strength — for a third of a second, then it settled,
//! and every log line sampled at 0.5 s intervals showed a vessel sitting perfectly still.
//! SAS spun a motionless vessel up to 0.68 rad/s and reversed it every two seconds, which
//! looked like a constant number in any summary and like nothing at all on screen.
//!
//! Both were found by hand-writing a throwaway per-tick logger, twice. This is that logger,
//! made permanent, plus the checks that would have shouted about each bug without anyone
//! having to suspect it first.
//!
//! # Using it
//!
//! ```text
//! KRAKEN_TRACE=2 cargo run     # one line per physics tick for the first 2 seconds
//! KRAKEN_TRACE=all cargo run   # every tick, forever
//! ```
//!
//! The watchdog needs no flag. It is a handful of comparisons per tick and it is the reason
//! the next bug of this shape gets noticed on the run that introduces it.

use bevy::platform::collections::HashSet;

pub mod test_pilot;
use bevy::prelude::*;

use crate::celestial::atmosphere::Atmosphere;
use crate::celestial::body::CelestialBody;
use crate::part_modules::engine::Engine;
use crate::part_modules::resource_container::{self, ResourceContainer};
use crate::physics::joints::PeakJointLoad;
use crate::rendering::render_sync::{SimPosition, SimRotation, SimVelocity};
use crate::vessel::components::{
    ActiveVessel, ControlState, Destroyed, PartMass, RootPart, Vessel, VesselId,
};

/// Angular rate a vessel with SAS on and no input must get below, rad/s.
///
/// Matches `control::SAS_DEADBAND_RAD_S` in spirit: above this, SAS should be actively
/// correcting, and if it still is several seconds later then it is not converging.
const SAS_CONVERGED_RAD_S: f64 = 0.05;

/// How long SAS may fail to converge before the watchdog says so, seconds.
///
/// SAS aims to null a rotation in one second, so three is comfortably past "still working
/// on it" and safely short of a player noticing their rocket will not hold still.
const SAS_CONVERGE_GRACE_SECS: f64 = 3.0;

/// Depth below the surface that means something has gone through the terrain, metres.
///
/// Generous: resting parts settle a few centimetres into a collider, and the pad is a
/// separate body from the terrain, so small negatives are normal.
const TUNNELLED_DEPTH_M: f64 = 5.0;

/// Minimum gap between repeats of the same warning, seconds. Watchdog conditions persist for
/// as long as their cause, and a warning per tick at 50 Hz is not a diagnostic, it is a wall.
const WARN_COOLDOWN_SECS: f64 = 2.0;

pub struct DiagnosticsPlugin;

impl Plugin for DiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        if let Some(profile) = test_pilot::Profile::from_env() {
            info!("scripted pilot enabled: {profile:?}");
            app.insert_resource(profile)
                .init_resource::<test_pilot::PilotState>()
                // In `Clear`, so it has written the controls before any force producer
                // reads them — the same point in the tick a player's input reaches.
                .add_systems(
                    FixedUpdate,
                    test_pilot::fly.in_set(crate::physics::PhysicsSchedule::Clear),
                );
        }

        app.init_resource::<TraceConfig>()
            .add_systems(Startup, announce_trace)
            // After Writeback so both see this tick's settled state, and after the physics
            // readback that produces it.
            .add_systems(
                FixedUpdate,
                (flight_watchdog, record_tick)
                    .chain()
                    .after(crate::physics::PhysicsSchedule::Apply)
                    .after(bevy_rapier3d::plugin::PhysicsSet::Writeback),
            );
    }
}

/// How long the per-tick recorder runs for, from `KRAKEN_TRACE`.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub enum TraceConfig {
    Off,
    /// Trace for this many seconds of simulated time from startup.
    ForSeconds(f64),
    Forever,
}

impl Default for TraceConfig {
    fn default() -> Self {
        match std::env::var("KRAKEN_TRACE") {
            Err(_) => Self::Off,
            Ok(value) if value.eq_ignore_ascii_case("all") => Self::Forever,
            Ok(value) => match value.parse::<f64>() {
                Ok(seconds) if seconds > 0.0 => Self::ForSeconds(seconds),
                _ => Self::Off,
            },
        }
    }
}

impl TraceConfig {
    fn covers(&self, elapsed_secs: f64) -> bool {
        match self {
            Self::Off => false,
            Self::Forever => true,
            Self::ForSeconds(limit) => elapsed_secs <= *limit,
        }
    }
}

fn announce_trace(config: Res<TraceConfig>) {
    match *config {
        TraceConfig::Off => {}
        TraceConfig::Forever => info!("per-tick trace enabled for the whole session"),
        TraceConfig::ForSeconds(seconds) => info!("per-tick trace enabled for {seconds} s"),
    }
}

/// One line per physics tick, for the active vessel.
///
/// Deliberately one line with fixed-width columns rather than structured fields: the point
/// is to be able to see a transient by eye in a wall of text, and columns that move around
/// defeat that.
#[allow(clippy::too_many_arguments)]
fn record_tick(
    config: Res<TraceConfig>,
    fixed_time: Res<Time<Fixed>>,
    vessels: Query<(Entity, &ControlState, Has<Destroyed>), With<ActiveVessel>>,
    roots: Query<(&VesselId, &SimPosition, &SimVelocity), With<RootPart>>,
    engines: Query<(&VesselId, &Engine)>,
    masses: Query<(&VesselId, &PartMass, Option<&ResourceContainer>)>,
    body: Res<CelestialBody>,
    atmosphere: Res<Atmosphere>,
    peak_joint_load: Res<PeakJointLoad>,
) {
    let elapsed = fixed_time.elapsed_secs_f64();
    if !config.covers(elapsed) {
        return;
    }

    let Ok((vessel, control, destroyed)) = vessels.single() else {
        return;
    };
    let Some((_, position, velocity)) = roots.iter().find(|(id, _, _)| id.0 == vessel) else {
        return;
    };

    let altitude = body.altitude_of(position.0);
    let vertical = velocity.0.dot(body.up_at(position.0));
    let thrust_kn: f64 = engines
        .iter()
        .filter(|(id, _)| id.0 == vessel)
        .map(|(_, engine)| engine.current_thrust_n)
        .sum::<f64>()
        / 1000.0;

    info!(
        "t={elapsed:7.2} alt={altitude:10.2} vs={vertical:+8.2} spd={:7.2} thr={:.2} \
         F={thrust_kn:6.1}kN m={:7.0} g={:.3} rho={:.4} joint={:.3}{}",
        velocity.0.length(),
        control.throttle,
        resource_container::vessel_mass_kg(vessel, &masses),
        body.gravity_at(position.0).length(),
        atmosphere.density_at(altitude),
        peak_joint_load.on_active_vessel,
        if destroyed { " DESTROYED" } else { "" },
    );
}

/// Per-check cooldowns and the running SAS-convergence timer.
#[derive(Default)]
struct WatchdogState {
    last_warned: [f64; 3],
    sas_unconverged_since: Option<f64>,
    reported_non_finite: HashSet<Entity>,
}

impl WatchdogState {
    /// True at most once per [`WARN_COOLDOWN_SECS`] per check.
    fn should_warn(&mut self, check: usize, now: f64) -> bool {
        let last = self.last_warned[check];
        if now - last < WARN_COOLDOWN_SECS && last > 0.0 {
            return false;
        }
        self.last_warned[check] = now;
        true
    }
}

/// The active vessel, if it is still in one piece. A wreck has no attitude to hold and no
/// controls to obey, so every check below would be reporting on something that is by
/// definition no longer flying.
type FlyableVessel<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Vessel, &'static ControlState),
    (With<ActiveVessel>, Without<Destroyed>),
>;

/// Shouts when the simulation is doing something physically implausible.
///
/// Each check exists because something real got past review without it. They are ordinary
/// comparisons on data already in memory — cheap enough to leave on in release, which is the
/// point: a check nobody has to remember to enable is one that actually fires.
fn flight_watchdog(
    fixed_time: Res<Time<Fixed>>,
    body: Res<CelestialBody>,
    vessels: FlyableVessel,
    roots: Query<(&VesselId, &SimVelocity), With<RootPart>>,
    parts: Query<(Entity, &SimPosition, &SimVelocity, &SimRotation)>,
    angular: Query<(&VesselId, &bevy_rapier3d::prelude::Velocity), With<RootPart>>,
    mut state: Local<WatchdogState>,
) {
    let now = fixed_time.elapsed_secs_f64();

    // 1. Non-finite state. A NaN propagates through every subsequent tick and turns into
    //    "the rocket vanished", which is a miserable thing to debug from the far end.
    for (entity, position, velocity, rotation) in &parts {
        let finite = position.0.is_finite() && velocity.0.is_finite() && rotation.0.is_finite();
        if finite || state.reported_non_finite.contains(&entity) {
            continue;
        }
        state.reported_non_finite.insert(entity);
        error!(
            "part {entity} has non-finite state: position {:?}, velocity {:?} — the \
             simulation is corrupt from this tick on",
            position.0, velocity.0
        );
    }

    // 2. Something went through the terrain. Continuous collision detection is meant to make
    //    this impossible; if it happens, CCD has been dropped from a part or the timestep has
    //    grown.
    if let Some((entity, position, _, _)) = parts
        .iter()
        .find(|(_, position, _, _)| body.altitude_of(position.0) < -TUNNELLED_DEPTH_M)
        && state.should_warn(0, now)
    {
        warn!(
            "part {entity} is {:.0} m below the surface — collision detection let it through",
            -body.altitude_of(position.0)
        );
    }

    let Ok((vessel, vessel_data, control)) = vessels.single() else {
        state.sas_unconverged_since = None;
        return;
    };

    // 3. SAS is on, the player is not steering, and the vessel still will not hold still.
    //    This is the check that would have caught SAS driving its own oscillation: the rate
    //    it settled at was constant, so it read as "not changing" in every summary.
    let steering = control.pitch != 0.0 || control.yaw != 0.0 || control.roll != 0.0;
    let rate = angular
        .iter()
        .find(|(id, _)| id.0 == vessel)
        .map(|(_, velocity)| f64::from(velocity.angvel.length()))
        .unwrap_or(0.0);

    if !control.sas || steering || rate <= SAS_CONVERGED_RAD_S {
        state.sas_unconverged_since = None;
    } else {
        let since = *state.sas_unconverged_since.get_or_insert(now);
        if now - since > SAS_CONVERGE_GRACE_SECS && state.should_warn(1, now) {
            warn!(
                "SAS on '{}' has not converged in {:.0} s — still rotating at {rate:.3} rad/s \
                 with no input. Expect it to null a rotation in about a second.",
                vessel_data.name,
                now - since,
            );
        }
    }

    // 4. Absurd speed. Catches a solver explosion in the tick it happens rather than when
    //    the vessel is already past the edge of the world.
    if let Some((_, velocity)) = roots.iter().find(|(id, _)| id.0 == vessel)
        && velocity.0.length() > 100_000.0
        && state.should_warn(2, now)
    {
        warn!(
            "'{}' is travelling at {:.0} m/s — that is not flight, that is a solver failure",
            vessel_data.name,
            velocity.0.length()
        );
    }
}
