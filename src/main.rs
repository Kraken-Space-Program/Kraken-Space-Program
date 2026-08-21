//! Kraken Space Program.
//!
//! `main` is plugin registration and nothing else. Every system belongs to the module that
//! owns its concept — see `DESIGN.md`, "Module Structure".

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

mod celestial;
mod diagnostics;
mod part_modules;
mod physics;
mod rendering;
mod sdk;
mod ui;
mod vessel;

/// Spawns the launch site: a light and a pad to stand on.
///
/// Static scenery is a plain Rapier body with its mesh attached directly. It does not need
/// the physics/visual entity split that moving parts do, because nothing but Rapier ever
/// writes its transform.
fn setup_launch_site(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
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

    // Terrain. 20 km square, 10 m thick, top face at y = -0.5.
    //
    // The size is so a suborbital hop has somewhere to come down: the original 400 m pad
    // was the *only* collider in the world, and anything that drifted off it fell forever.
    // The thickness is the backstop behind `Ccd` — at 50 Hz a fast descent covers metres
    // per tick, and a thin slab is something a body can be on both sides of between two
    // consecutive positions.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(20_000.0, 10.0, 20_000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.28, 0.30, 0.27),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -5.5, 0.0),
        RigidBody::Fixed,
        Collider::cuboid(10_000.0, 5.0, 10_000.0),
    ));

    // The pad, sitting on the terrain with its top face at y = 0.
    //
    // Kept as a separate, visually distinct body rather than merged into the terrain: from
    // altitude a featureless plane gives no sense of motion or position, and there needs to
    // be something to aim at on the way back down.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(400.0, 0.5, 400.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.35, 0.36, 0.38),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.25, 0.0),
        RigidBody::Fixed,
        Collider::cuboid(200.0, 0.25, 200.0),
    ));
}

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Kraken Space Program".into(),
                        ..default()
                    }),
                    ..default()
                })
                // Assets live under src/assets/, not Bevy's default of ./assets.
                // `sdk::loader::DefinitionRoot` must agree with this.
                .set(AssetPlugin {
                    file_path: "src/assets".into(),
                    ..default()
                }),
        )
        // Rapier registration, the 50 Hz fixed timestep and gravity all live in
        // PhysicsPlugin — design.md: no Rapier configuration outside physics/.
        .add_plugins((
            physics::PhysicsPlugin,
            celestial::CelestialPlugin,
            rendering::RenderingPlugin,
            sdk::SdkPlugin,
            part_modules::PartModulesPlugin,
            vessel::VesselPlugin,
            ui::UiPlugin,
            diagnostics::DiagnosticsPlugin,
        ))
        .add_systems(Startup, setup_launch_site)
        .run();
}
