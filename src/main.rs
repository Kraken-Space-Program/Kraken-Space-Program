use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

mod physics;
mod rendering;
mod sdk;

// SimPosition and SyncRender are defined in render_sync but unused until Phase 1
// (no entities carry SimPosition + SyncRender yet). Import only what's needed.
use rendering::render_sync::{LocalOrigin, RenderInterpolation, WorldOrigin};

// ---------------------------------------------------------------------------
// Marker components
// ---------------------------------------------------------------------------

/// The ball used for Phase 0 validation. Filters debug queries so they don't
/// accidentally match other entities.
#[derive(Component)]
struct DebugBall;

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Orbits the camera around a fixed target when the right mouse button is held.
///
/// Uses [`AccumulatedMouseMotion`] which already accumulates a full per-frame
/// delta — do NOT multiply by `delta_secs`, that would double-scale the input.
fn camera_orbit(
    mut camera: Single<&mut Transform, With<Camera>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
) {
    if !mouse_buttons.pressed(MouseButton::Right) {
        return;
    }

    let delta = mouse_motion.delta;
    let yaw = -delta.x * 0.005;
    let pitch = -delta.y * 0.005;

    let target = Vec3::new(0.0, 2.0, 0.0);
    let offset = camera.translation - target;

    let new_offset = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * offset;
    camera.translation = target + new_offset;
    camera.look_at(target, Vec3::Y);
}

/// Logs the ball's Y position after physics has run for the frame.
///
/// Runs in [`PostUpdate`] so it reads Rapier's final position for the tick,
/// not the stale value from the previous frame.
fn log_ball_position(ball: Query<&Transform, With<DebugBall>>, time: Res<Time>) {
    for transform in &ball {
        eprintln!(
            "[{:.2}s] Ball Y = {:.3} m",
            time.elapsed_secs(),
            transform.translation.y,
        );
    }
}

/// Spawns the Phase 0 scene: camera, light, ground plane, and a falling ball.
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 15.0, 35.0).looking_at(Vec3::new(0.0, 2.0, 0.0), Vec3::Y),
    ));

    // Sun
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(30.0, 60.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.75, 0.85),
        brightness: 0.55,
        ..default()
    });

    // Ground placeholder — a much larger planet-scale sphere.
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(10_000.0).mesh().ico(10).unwrap())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.55, 0.55),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -10_000.0, 0.0),
        RigidBody::Fixed,
        Collider::ball(10_000.0),
    ));

    // Ball — dropped from height to validate gravity + restitution.
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(0.5).mesh().ico(5).unwrap())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.75, 0.65),
            perceptual_roughness: 0.6,
            ..default()
        })),
        Transform::from_xyz(0.0, 50.0, 0.0),
        DebugBall,
        RenderInterpolation::default(),
    ));

    eprintln!("Phase 0 scene spawned — ball at Y = 50.0 m");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    eprintln!("Kraken Space Program starting");

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Kraken Space Program".into(),
                ..default()
            }),
            ..default()
        }))
        // Physics — gravity is configured in physics::PhysicsPlugin, not here.
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
        .add_plugins(physics::PhysicsPlugin)
        // Coordinate-system resources (zero until Krakensbane is implemented in Phase 1).
        .init_resource::<WorldOrigin>()
        .init_resource::<LocalOrigin>()
        // Fixed timestep: 50 Hz as specified in design.md.
        // "Fixed tick rate: 50 Hz (0.02s). Never dynamic per-frame."
        .insert_resource(TimestepMode::Fixed {
            dt: physics::FIXED_TIMESTEP as f32,
            substeps: 1,
        })
        .add_systems(Startup, setup)
        .add_systems(Update, camera_orbit)
        // log_ball_position runs after Rapier writes its final transforms for the tick.
        .add_systems(PostUpdate, log_ball_position)
        .add_systems(PostUpdate, rendering::render_sync::cache_physics_transforms.after(log_ball_position))
        .add_systems(PostUpdate, rendering::render_sync::interpolate_render_transforms.after(rendering::render_sync::cache_physics_transforms))
        // render_sync runs last in PostUpdate, after all simulation writes,
        // before Bevy's transform propagation and render extract.
        .add_systems(
            PostUpdate,
            rendering::render_sync::sync_render_transforms.after(rendering::render_sync::interpolate_render_transforms),
        )
        .run();
}
