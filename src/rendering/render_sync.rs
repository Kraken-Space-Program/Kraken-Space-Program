//! THE ONLY FILE THAT CONVERTS f64 SimPosition → f32 Transform.
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
//! If you find yourself writing `.as_vec3()` or `as f32` outside of this file, stop.
//! You are in the wrong place. Add a note to the PR explaining why and get it reviewed.
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

use bevy::math::DVec3;
use bevy::prelude::*;

use crate::physics;

// ---------------------------------------------------------------------------
// Coordinate-system primitives
//
// These are the canonical definitions. Other modules import from here.
// ---------------------------------------------------------------------------

/// Simulation-space position of an entity (f64, relative to [`WorldOrigin`]).
///
/// This is the authoritative position for physics, collision, and orbital mechanics.
/// It is **never** cast to f32 directly. Only [`sync_render_transforms`] may convert it.
#[derive(Component, Default, Clone, Copy, Debug)]
pub struct SimPosition(pub DVec3);

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

/// Marker component — entity should have its [`Transform`] driven by [`SimPosition`].
///
/// Add this to any entity whose render position should track its simulation position.
/// Entities without this component (e.g. UI, purely-visual effects) are unaffected.
#[derive(Component)]
pub struct SyncRender;

/// Enables render interpolation for a physics-driven entity.
#[derive(Component, Default, Clone, Copy, Debug)]
pub struct RenderInterpolation {
    pub previous: Transform,
    pub current: Transform,
    pub initialized: bool,
}

// ---------------------------------------------------------------------------
// The systems
// ---------------------------------------------------------------------------

/// Copies simulation positions into Bevy [`Transform`]s for rendering.
///
/// This is the **only** system in the codebase that crosses the f64→f32 boundary.
///
/// # Scheduling
///
/// Runs in [`PostUpdate`], after all physics and simulation systems have written
/// their final positions for the frame, but before Bevy's transform propagation
/// and the render extract step.
///
/// # Precision guarantee
///
/// The subtraction `sim_pos.0 - local_origin.0` happens entirely in f64.
/// The resulting relative vector is small (entity is near the local origin),
/// so the subsequent `.as_vec3()` cast to f32 preserves full precision.
pub fn sync_render_transforms(
    local_origin: Res<LocalOrigin>,
    mut query: Query<(&SimPosition, &mut Transform), With<SyncRender>>,
) {
    for (sim_pos, mut transform) in &mut query {
        // Step 1: subtract origin FIRST, while still in f64.
        // This keeps the value being cast small, preserving precision.
        let relative = sim_pos.0 - local_origin.0;

        // Debug assertion to ensure relative position is reasonable
        debug_assert!(relative.length() < 1e6, "Relative position too large, LocalOrigin may need updating");

        // Step 2: THIS IS THE ONLY .as_vec3() IN THE ENTIRE CODEBASE.
        // All other f64→f32 position casts are forbidden. If you are adding
        // one elsewhere, you are breaking the coordinate-system contract.
        transform.translation = relative.as_vec3();
    }
}

/// Captures the latest physics transform snapshot so rendering can interpolate.
///
/// This runs after Rapier has synced physics transforms to Bevy, but before the
/// final interpolated render transform is written.
pub fn cache_physics_transforms(
    mut query: Query<(&Transform, &mut RenderInterpolation)>,
) {
    for (transform, mut interp) in &mut query {
        if interp.initialized {
            interp.previous = interp.current;
        } else {
            interp.previous = *transform;
            interp.initialized = true;
        }
        interp.current = *transform;
    }
}

/// Interpolates the visible transform between the last two physics ticks.
///
/// This reduces render jitter when physics runs at a fixed rate and the
/// render frame rate does not exactly match it.
pub fn interpolate_render_transforms(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &RenderInterpolation)>,
) {
    let alpha = ((time.elapsed_seconds_f64() % physics::FIXED_TIMESTEP)
        / physics::FIXED_TIMESTEP) as f32;

    for (mut transform, interp) in &mut query {
        if interp.initialized {
            transform.translation = interp
                .previous
                .translation
                .lerp(interp.current.translation, alpha);
            transform.rotation = interp
                .previous
                .rotation
                .slerp(interp.current.rotation, alpha);
            transform.scale = interp
                .previous
                .scale
                .lerp(interp.current.scale, alpha);
        }
    }
}
