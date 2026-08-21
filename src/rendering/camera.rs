//! Flight camera — orbits a target entity, follows it, and stays out of the way.
//!
//! # Why the angles are stored, not derived
//!
//! The Phase 0 camera rotated its own transform by `Quat::from_rotation_y(yaw) *
//! Quat::from_rotation_x(pitch)` each frame and then called `look_at`. That has two
//! problems that get worse the more you use it: the pitch is applied around the *global*
//! X axis, so once you have yawed 90° the vertical drag rotates the camera around what is
//! now its forward axis; and there is no clamp, so dragging past vertical puts the offset
//! parallel to `Vec3::Y` and `look_at` — which needs a non-parallel up vector — flips the
//! view over.
//!
//! Keeping `yaw`/`pitch` as the authoritative state and *deriving* the transform from them
//! fixes both. The pitch clamp then has somewhere to live.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

/// Radians of rotation per pixel of mouse movement.
const ORBIT_SENSITIVITY: f32 = 0.005;

/// How far a single scroll notch moves the camera, as a fraction of current distance.
///
/// Proportional rather than absolute so that zooming feels the same when you are 5 m
/// from a probe core and when you are 2 km from a launch stack.
const ZOOM_SENSITIVITY: f32 = 0.12;

const MIN_DISTANCE: f32 = 2.0;
const MAX_DISTANCE: f32 = 5_000.0;

/// Pitch limit. Just short of vertical, so the up vector never becomes parallel to the
/// view direction.
const MAX_PITCH: f32 = 1.54; // ~88.2°

/// Orbital camera state.
///
/// The camera looks at [`target`](Self::target) from `distance` metres away, at the
/// spherical angles `yaw`/`pitch`. All values are in render space — this component is
/// downstream of the f64→f32 conversion and never sees a `SimPosition`.
#[derive(Component)]
pub struct FlightCamera {
    /// Entity to follow. `None` orbits the render-space origin, which is what happens
    /// before a vessel exists.
    pub target: Option<Entity>,
    /// Offset from the target's origin, in render space — lets the camera frame the
    /// middle of a tall rocket rather than its root part.
    pub target_offset: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for FlightCamera {
    fn default() -> Self {
        Self {
            target: None,
            target_offset: Vec3::ZERO,
            distance: 40.0,
            yaw: 0.0,
            pitch: 0.35,
        }
    }
}

/// Spawns the single flight camera at startup.
pub fn spawn_flight_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        FlightCamera::default(),
        Transform::default(),
    ));
}

/// Applies mouse input and re-derives the camera transform from its target.
///
/// # Controls
///
/// - **Right-click drag** — orbit
/// - **Scroll** — zoom
///
/// # Ordering
///
/// `PostUpdate`, after `render_sync::sync_render_transforms`, so the target's transform
/// is this frame's interpolated position. Running before it would make the camera chase a
/// stale value and undo the smoothing.
pub fn update_flight_camera(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
    targets: Query<&Transform, Without<FlightCamera>>,
    mut camera: Single<(&mut FlightCamera, &mut Transform)>,
) {
    let (camera_state, camera_transform) = &mut *camera;

    // Orbit. AccumulatedMouseMotion is already a per-frame delta — multiplying by
    // delta_secs here would double-scale the input and make the camera framerate-dependent.
    if mouse_buttons.pressed(MouseButton::Right) {
        let delta = mouse_motion.delta;
        camera_state.yaw -= delta.x * ORBIT_SENSITIVITY;
        camera_state.pitch =
            (camera_state.pitch + delta.y * ORBIT_SENSITIVITY).clamp(-MAX_PITCH, MAX_PITCH);
    }

    // Zoom, proportional to current distance.
    if mouse_scroll.delta.y != 0.0 {
        let factor = 1.0 - mouse_scroll.delta.y.clamp(-3.0, 3.0) * ZOOM_SENSITIVITY;
        camera_state.distance = (camera_state.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    // Follow. A target that has despawned (destroyed vessel) silently falls back to the
    // origin rather than leaving the camera frozen mid-scene.
    let focus = camera_state
        .target
        .and_then(|entity| targets.get(entity).ok())
        .map(|transform| transform.translation)
        .unwrap_or(Vec3::ZERO)
        + camera_state.target_offset;

    // Derive the transform from the stored angles rather than accumulating rotations
    // into it — see the module docs for why.
    let rotation = Quat::from_euler(EulerRot::YXZ, camera_state.yaw, -camera_state.pitch, 0.0);
    camera_transform.translation = focus + rotation * Vec3::new(0.0, 0.0, camera_state.distance);
    camera_transform.look_at(focus, Vec3::Y);
}
