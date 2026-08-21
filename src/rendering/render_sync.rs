//! THE ONLY FILE THAT CONVERTS f64 SimPosition → f32 render Transform.
//!
//! No other file in this codebase is allowed to perform this conversion. Ever.
//!
//! # Why this file exists
//!
//! Simulation positions are stored as [`SimPosition`] (f64 [`DVec3`]) relative to the
//! current [`LocalOrigin`]. f32 only has ~7 significant digits of precision — at planetary
//! scale (~10^7 metres from a body centre) that leaves roughly 1 metre of precision, which
//! is completely unacceptable for a spacecraft sim.
//!
//! The safe conversion pattern is:
//! 1. Subtract [`LocalOrigin`] from the simulation position **while still in f64**.
//!    The result is a small relative value (the entity is near the local origin by design).
//! 2. **Only then** cast to f32. The small magnitude means the cast preserves full
//!    sub-millimetre precision.
//!
//! If you find yourself writing `.as_vec3()` or `as f32` on a *simulation* position outside
//! of this file, stop. You are in the wrong place.
//!
//! # Coordinate spaces (summary — full spec in design.md)
//!
//! ```text
//! SIMULATION WORLD (f64 DVec3, relative to WorldOrigin)
//!       ↓  render_sync.rs ONLY — subtract LocalOrigin, then cast to f32
//! RENDER WORLD (f32 Vec3, Bevy Transform — what the GPU sees)
//! ```
//!
//! [`LocalOrigin`] tracks the render-space origin (kept near the active vessel).
//! [`WorldOrigin`] tracks the simulation-space origin in solar-inertial f64 coordinates.
//! Krakensbane shifts both when the active vessel drifts too far from the current origin.
//!
//! # The one exception: Rapier
//!
//! Rapier is f32-only, so it physically cannot integrate `SimPosition`. It simulates in a
//! *local physics frame* that Krakensbane keeps near zero, and
//! [`crate::physics::readback`] lifts its result back into f64 every tick. That means f32
//! positions do exist inside `physics/` — but they never leave it. The rule this file
//! enforces is the one that matters: no *simulation* position is ever narrowed to f32
//! anywhere else.
//!
//! # Interpolation
//!
//! Physics runs at a fixed 50 Hz; the display does not. Rendering the raw tick position
//! would visibly stutter on any monitor whose refresh rate is not an exact multiple of
//! 50 Hz — which is most of them, and certainly a 120 Hz ProMotion display. So each entity
//! keeps its previous tick's state alongside its current one, and this file renders the
//! interpolation between them. The interpolation happens in f64, before the cast.

use bevy::math::{DQuat, DVec3};
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Coordinate-system primitives
//
// These are the canonical definitions. Other modules import from here.
// ---------------------------------------------------------------------------

/// Simulation-space position of an entity (f64, relative to [`WorldOrigin`]).
///
/// This is the authoritative position for orbital mechanics, saves, and networking.
/// It is **never** cast to f32 directly. Only [`sync_render_transforms`] may convert it.
#[derive(Component, Default, Clone, Copy, Debug)]
pub struct SimPosition(pub DVec3);

/// Simulation-space orientation of an entity (f64).
#[derive(Component, Clone, Copy, Debug)]
pub struct SimRotation(pub DQuat);

impl Default for SimRotation {
    fn default() -> Self {
        Self(DQuat::IDENTITY)
    }
}

/// Simulation-space velocity (f64), metres per second.
///
/// Derived from Rapier every tick alongside [`SimPosition`]. Lives here with the other
/// coordinate-space primitives because it is the same kind of value: authoritative,
/// f64, and read by everything outside `physics/`.
#[derive(Component, Default, Clone, Copy, Debug)]
pub struct SimVelocity(pub DVec3);

/// Previous physics tick's [`SimPosition`], used to interpolate between ticks.
///
/// Written once per fixed tick by [`store_previous_state`], *before* physics runs.
#[derive(Component, Default, Clone, Copy, Debug)]
pub struct PreviousSimPosition(pub DVec3);

/// Previous physics tick's [`SimRotation`], used to interpolate between ticks.
#[derive(Component, Clone, Copy, Debug)]
pub struct PreviousSimRotation(pub DQuat);

/// Previous physics tick's [`SimVelocity`], in metres per second.
///
/// Not used for interpolation — it exists because impact damage needs the speed a part was
/// travelling at *before* the collision. Collision events are read after
/// `PhysicsSet::Writeback`, by which point Rapier has already resolved the contact and
/// `SimVelocity` reports the post-impact velocity, which for a hard landing is close to
/// zero. Snapshotting the pre-tick value is the only place that number still exists.
/// See [`crate::physics::impact`].
#[derive(Component, Default, Clone, Copy, Debug)]
pub struct PreviousSimVelocity(pub DVec3);

impl Default for PreviousSimRotation {
    fn default() -> Self {
        Self(DQuat::IDENTITY)
    }
}

/// Solar-inertial position of the simulation-space origin (f64).
///
/// Updated by the Krakensbane system when the active vessel drifts beyond the
/// precision threshold (~10 000 units). When this shifts, **all** [`SimPosition`]
/// values are adjusted in the same frame so relative positions are preserved.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct WorldOrigin(pub DVec3);

/// Render-space origin (f64), kept close to the active vessel.
///
/// This is subtracted from [`SimPosition`] before the f32 cast, ensuring the
/// value being cast is always small. Updated every frame by the camera / vessel
/// tracking system (Phase 1+). At Phase 0 it stays at zero, which is fine because
/// the test scene is tiny.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct LocalOrigin(pub DVec3);

/// Marks a **visual** entity whose [`Transform`] is driven by another entity's
/// [`SimPosition`].
///
/// # Why visuals are separate entities from physics bodies
///
/// The obvious design — one entity carrying the rigid body, the mesh, and `SimPosition`
/// — does not work, and fails in a way that is easy to miss. Rapier's
/// `apply_rigid_body_user_changes` picks up `Changed<GlobalTransform>` to let gameplay
/// code teleport bodies. If this system wrote an interpolated transform onto the body
/// entity every frame, Rapier would read it back on the next tick and snap the body to a
/// position that is deliberately one tick stale. The simulation would fight the renderer
/// forever.
///
/// So: the **physics entity** owns `RigidBody`, `Collider` and a `Transform` that belongs
/// to Rapier. The **visual entity** owns `Mesh3d` and a `Transform` that belongs to this
/// file, and points at the physics entity through `source`. Neither writes the other's
/// data. This also leaves room for the Phase 4 part-instancing renderer, where the
/// relationship stops being one-to-one anyway.
#[derive(Component)]
pub struct SyncRender {
    /// The simulation entity whose position this visual follows.
    pub source: Entity,
}

/// Convenience bundle: everything an entity needs to participate in the f64 pipeline.
#[derive(Bundle, Default)]
pub struct SimTransformBundle {
    pub sim_position: SimPosition,
    pub sim_rotation: SimRotation,
    pub sim_velocity: SimVelocity,
    pub previous_position: PreviousSimPosition,
    pub previous_rotation: PreviousSimRotation,
    pub previous_velocity: PreviousSimVelocity,
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Snapshots the current simulation state as "previous" before physics overwrites it.
///
/// # Ordering
///
/// `FixedUpdate`, before `PhysicsSet::SyncBackend`. After a tick completes,
/// `previous` holds the state at the end of tick N-1 and the live components hold
/// the state at the end of tick N — the two endpoints [`sync_render_transforms`]
/// interpolates between.
pub fn store_previous_state(
    mut query: Query<(
        &SimPosition,
        &SimRotation,
        &SimVelocity,
        &mut PreviousSimPosition,
        &mut PreviousSimRotation,
        &mut PreviousSimVelocity,
    )>,
) {
    for (pos, rot, vel, mut prev_pos, mut prev_rot, mut prev_vel) in &mut query {
        prev_pos.0 = pos.0;
        prev_rot.0 = rot.0;
        prev_vel.0 = vel.0;
    }
}

/// Copies interpolated simulation positions into Bevy [`Transform`]s for rendering.
///
/// This is the **only** system in the codebase that crosses the simulation f64 → render
/// f32 boundary.
///
/// # Scheduling
///
/// Runs in [`PostUpdate`], after all simulation systems have written their final
/// positions, but before Bevy's transform propagation and the render extract step.
///
/// # Precision guarantee
///
/// Both the interpolation and the `sim - local_origin` subtraction happen entirely in
/// f64. The resulting relative vector is small (the entity is near the local origin by
/// design), so the subsequent `.as_vec3()` cast preserves full precision.
///
/// # Interpolation
///
/// `overstep_fraction()` is how far Bevy's fixed-timestep accumulator has advanced
/// toward the *next* tick, in `0.0..1.0`. Rendering `lerp(previous, current, fraction)`
/// therefore trails the simulation by exactly one tick (20 ms) and is perfectly smooth
/// at any refresh rate. Trading 20 ms of latency for the absence of stutter is the
/// standard bargain and the right one here — a rocket is not a competitive shooter.
pub fn sync_render_transforms(
    local_origin: Res<LocalOrigin>,
    fixed_time: Res<Time<Fixed>>,
    sources: Query<(
        &SimPosition,
        &SimRotation,
        &PreviousSimPosition,
        &PreviousSimRotation,
    )>,
    mut visuals: Query<(&SyncRender, &mut Transform)>,
) {
    let alpha = fixed_time.overstep_fraction_f64();

    for (sync, mut transform) in &mut visuals {
        // A visual whose source has despawned (destroyed part) simply stops updating
        // until something removes it. Not an error worth logging every frame.
        let Ok((sim_pos, sim_rot, prev_pos, prev_rot)) = sources.get(sync.source) else {
            continue;
        };

        // Step 1: interpolate between the two most recent physics ticks, in f64.
        let position = prev_pos.0.lerp(sim_pos.0, alpha);
        let rotation = prev_rot.0.slerp(sim_rot.0, alpha);

        // Step 2: subtract origin FIRST, while still in f64.
        // This keeps the value being cast small, preserving precision.
        let relative = position - local_origin.0;

        // Step 3: THIS IS THE ONLY SIMULATION-POSITION CAST TO f32 IN THE CODEBASE.
        // All other f64→f32 position casts are forbidden. If you are adding one
        // elsewhere, you are breaking the coordinate-system contract.
        transform.translation = relative.as_vec3();
        transform.rotation = rotation.as_quat();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The load-bearing claim of the entire coordinate system: an object 10 000 km from
    /// the world origin still renders at full precision, because the origin is subtracted
    /// before the cast.
    ///
    /// The naive version — casting first, subtracting after — is what this test exists to
    /// forbid. At 1e7 m one f32 ULP is exactly 1 metre, so every position snaps to a whole
    /// metre: a vessel would jitter by up to half a metre per frame and a part stack would
    /// tear itself apart as its joints were told their anchors had moved.
    #[test]
    fn subtract_before_cast_preserves_precision_at_planetary_scale() {
        // 10 000 km up-range, plus sub-metre offsets chosen to straddle f32's 1 m grid
        // at this magnitude (rounds down / rounds up / exact tie).
        let local_origin = DVec3::new(1.0e7, 1.0e7, 1.0e7);
        let offset = DVec3::new(0.75, 1.25, 2.5);
        let sim = local_origin + offset;

        // What render_sync does: subtract in f64, then cast. Exact — all three offsets
        // are representable in f32 once they are small numbers.
        let correct = (sim - local_origin).as_vec3();
        assert_eq!(
            correct,
            offset.as_vec3(),
            "subtract-then-cast must be exact at this scale"
        );

        // What happens if someone casts first — the failure mode this file prevents.
        let naive = sim.as_vec3() - local_origin.as_vec3();
        assert!(
            (naive - correct).length() > 0.2,
            "cast-then-subtract should have quantized the offset to whole metres, but got \
             {naive:?} against {correct:?} — if this ever stops holding, f32 has more \
             precision than the coordinate-system design assumes and the docs need revisiting"
        );
        // Concretely: every component landed on a whole metre.
        for component in [naive.x, naive.y, naive.z] {
            assert_eq!(
                component,
                component.round(),
                "expected metre-quantized {naive:?}"
            );
        }
    }

    /// Interpolation must happen in f64 too. Lerping at planetary distances in f32
    /// would reintroduce exactly the error the subtraction was there to avoid.
    #[test]
    fn interpolation_is_exact_at_tick_boundaries() {
        let previous = DVec3::new(1.0e7, 0.0, 0.0);
        let current = DVec3::new(1.0e7 + 4.0, 0.0, 0.0);

        assert_eq!(previous.lerp(current, 0.0), previous);
        assert_eq!(previous.lerp(current, 1.0), current);

        // Halfway through a tick, 4 m of travel should read as exactly 2 m.
        let mid = previous.lerp(current, 0.5);
        assert!((mid.x - (1.0e7 + 2.0)).abs() < 1.0e-9, "got {mid:?}");
    }
}
