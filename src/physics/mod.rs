//! Physics module — the only place in the codebase that configures or calls Rapier directly.
//!
//! # Rules (from design.md)
//!
//! - No direct Rapier calls outside `physics/`. Everything else talks to physics
//!   through components and events.
//! - Rapier handles (`RigidBodyHandle`, `ColliderHandle`) are internal to this module.
//!   Other modules never import `rapier3d` directly.
//! - The fixed timestep is 50 Hz. It is configured here and nowhere else.
//!
//! # The f32 boundary (see also `rendering/render_sync.rs`)
//!
//! Rapier is **f32 throughout**. `SimPosition` (f64) therefore cannot be the value
//! Rapier integrates. The split is:
//!
//! ```text
//!   SimPosition (f64, solar-relative)  =  WorldOrigin (f64)  +  Rapier Transform (f32, local)
//! ```
//!
//! Rapier simulates in a *local physics frame* that Krakensbane keeps within ~10 km
//! of zero, so f32 is entirely adequate there. [`readback`] converts back up to f64
//! after every physics tick.
//!
//! f32 positions are legal inside `physics/` and `rendering/`. Anything that crosses
//! a module boundary is f64.
//!
//! # What lives here (grows in Phase 1+)
//!
//! | File | Responsibility |
//! |------|---------------|
//! | `mod.rs` (this file) | Plugin definition, Rapier configuration, schedule wiring |
//! | `readback.rs` | Rapier f32 transform → `SimPosition` f64 after each tick |
//! | `joints.rs` | Part-to-part joint management, failure detection (Phase 1) |
//! | `krakensbane.rs` | Origin shifting for floating-point precision (Phase 1) |
//! | `forces.rs` | PendingForces accumulation and Rapier application (Phase 1) |

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

pub mod forces;
pub mod impact;
pub mod joints;
pub mod krakensbane;
pub mod readback;

/// The force pipeline, in order, all before Rapier reads anything.
///
/// Producers (thrust, drag, RCS, attitude control) go in
/// [`ProduceForces`](PhysicsSchedule::ProduceForces). They never need to know about each
/// other or about the sets on either side — they just add to `PendingForces` and the
/// bookends take care of the rest. See `forces.rs` for why clearing happens first.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PhysicsSchedule {
    /// Zero every accumulator.
    Clear,
    /// Thrust, drag, RCS, attitude control — anything that pushes a part.
    Produce,
    /// Hand the totals to Rapier.
    Apply,
}

/// Physics tick rate, in hertz.
///
/// design.md: "Default tick rate: 50 Hz (0.02s). Configurable per save, never dynamic
/// per-frame." Both the Bevy `FixedUpdate` accumulator and Rapier's integration step
/// are driven from this single constant so they can never disagree.
pub const PHYSICS_HZ: f64 = 50.0;

/// Fixed timestep in seconds (`1 / PHYSICS_HZ`).
pub const PHYSICS_DT: f32 = 1.0 / PHYSICS_HZ as f32;

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        // Rapier runs in FixedUpdate, NOT PostUpdate.
        //
        // This is the whole ballgame for determinism. `RapierPhysicsPlugin::default()`
        // schedules stepping in PostUpdate — once per *rendered frame* — and
        // `TimestepMode::Fixed { dt }` then advances the simulation by `dt` on every one
        // of those frames. That is not a fixed tick rate, it is "0.02 s of simulation per
        // frame": at 120 Hz the world runs 2.4x too fast, at 60 Hz 1.2x too fast, and any
        // thrust or drag value tuned by hand is really being tuned against a monitor.
        //
        // `in_fixed_schedule()` moves SyncBackend/StepSimulation/Writeback into FixedUpdate,
        // where Bevy's accumulator calls it exactly PHYSICS_HZ times per wall-clock second
        // regardless of framerate. Rendering interpolates between ticks — see
        // `rendering::render_sync`.
        // Registered *before* the Rapier plugin, and the order is load-bearing.
        //
        // bevy_rapier creates its collision-event buffer with `insert_resource`, which puts
        // the resource in the world but never registers it with Bevy's `MessageRegistry` —
        // so nothing ever calls `Messages::update()` and the buffer grows for the lifetime
        // of the process. `add_message` is what installs that cleanup, and it is a no-op if
        // the resource already exists, so it has to run first. The registry keys on the
        // resource's `ComponentId`, which does not change when Rapier later overwrites the
        // (empty) value, so both halves end up pointing at the same buffer.
        app.add_message::<CollisionEvent>()
            .add_plugins(RapierPhysicsPlugin::<NoUserData>::default().in_fixed_schedule())
            .insert_resource(Time::<Fixed>::from_hz(PHYSICS_HZ))
            .insert_resource(TimestepMode::Fixed {
                dt: PHYSICS_DT,
                substeps: 1,
            })
            .add_message::<joints::JointFailure>()
            .add_message::<impact::Impact>()
            .init_resource::<joints::PeakJointLoad>()
            .add_systems(Startup, configure_rapier)
            .add_systems(Update, debug_timestep_drift)
            .configure_sets(
                FixedUpdate,
                (
                    PhysicsSchedule::Clear,
                    PhysicsSchedule::Produce,
                    PhysicsSchedule::Apply,
                )
                    .chain()
                    // Rapier picks up ExternalForce through a Changed<> query in
                    // SyncBackend, so the whole chain has to land before it.
                    .before(PhysicsSet::SyncBackend),
            )
            .add_systems(
                FixedUpdate,
                (
                    forces::clear_pending_forces.in_set(PhysicsSchedule::Clear),
                    forces::apply_pending_forces.in_set(PhysicsSchedule::Apply),
                    crate::part_modules::resource_container::update_wet_mass
                        .in_set(PhysicsSchedule::Apply),
                ),
            )
            // Both of these must live in FixedUpdate: system ordering constraints only
            // apply within a single schedule, and PhysicsSet is now in FixedUpdate.
            .add_systems(
                FixedUpdate,
                (
                    // Snapshot last tick's position before Rapier overwrites it, so the
                    // renderer has two states to interpolate between.
                    crate::rendering::render_sync::store_previous_state
                        .before(PhysicsSet::SyncBackend),
                    // Lift Rapier's f32 result back into f64 simulation space, then shift
                    // the origin if the vessel has wandered too far from it. Chained: the
                    // shift must see this tick's settled positions.
                    (
                        readback::readback_sim_positions,
                        krakensbane::shift_world_origin,
                        // Collision events only exist once the step has run. Consumed in
                        // FixedPostUpdate by `vessel::damage`.
                        impact::detect_impacts,
                        // Same reason: the constraint impulses a joint carried are a
                        // result of the step. Consumed by `vessel::staging`.
                        joints::detect_joint_failures,
                    )
                        .chain()
                        .after(PhysicsSet::Writeback),
                ),
            );
    }
}

/// Logs simulated time against wall-clock time, at `debug` level.
///
/// Run the game with `RUST_LOG=kraken_space_program=debug` and these two numbers must
/// track 1:1 and stay that way at any framerate. If simulated time runs ahead, physics
/// has drifted back to being driven per-frame instead of per-tick, and every hand-tuned
/// thrust and drag number in the game silently becomes wrong.
///
/// This is cheap and off by default, and it catches the single most expensive category of
/// regression this codebase can have.
// `Real` is spelled out because bevy's `Time<Real>` marker and Rapier's `Real` scalar
// alias (f32) collide in the glob imports above.
fn debug_timestep_drift(fixed_time: Res<Time<Fixed>>, real_time: Res<Time<bevy::time::Real>>) {
    let simulated = fixed_time.elapsed_secs_f64();
    let wall_clock = real_time.elapsed_secs_f64();

    // Roughly every 5 wall-clock seconds, without needing a timer resource.
    if wall_clock < 1.0 || (wall_clock % 5.0) > 0.05 {
        return;
    }

    debug!(
        "timestep: simulated {simulated:.2}s vs wall clock {wall_clock:.2}s \
         (ratio {:.4}, want 1.0000)",
        simulated / wall_clock,
    );
}

/// Switches Rapier's own gravity off.
///
/// Gravity is produced by [`crate::celestial::body::apply_gravity`] as a force, like thrust
/// and drag, because Rapier's setting is a single uniform vector and gravity in a space game
/// is none of those things: it falls off with altitude, points at a body's centre rather
/// than straight down, and will eventually come from more than one body at once. Leaving
/// this at 9.81 would silently double the gravity a vessel feels.
///
/// Note: since bevy_rapier 0.22, `RapierConfiguration` is a **Component** (not a
/// Resource). It lives on the entity that owns the default `RapierContext`, which
/// the plugin spawns at startup. Query for it with `.single_mut()`.
fn configure_rapier(mut config: Query<&mut RapierConfiguration>) {
    if let Ok(mut config) = config.single_mut() {
        config.gravity = Vec3::ZERO;
    }
}
