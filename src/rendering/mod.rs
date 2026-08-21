//! Rendering module.
//!
//! # Rules (from design.md)
//!
//! - `render_sync.rs` is the **only** file in the entire codebase that converts
//!   a f64 [`SimPosition`](render_sync::SimPosition) to a f32 [`Transform`].
//! - No other file may call `.as_vec3()` on a simulation position.
//! - Custom render nodes (atmosphere scattering, terrain, part instancing) are
//!   Phase 4+ — nothing custom lives here yet.

use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::settings::Backends;

pub mod camera;
pub mod render_sync;

pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<render_sync::WorldOrigin>()
            .init_resource::<render_sync::LocalOrigin>()
            .add_systems(Startup, (log_render_backend, camera::spawn_flight_camera))
            .add_systems(
                PostUpdate,
                (
                    // The f64 → f32 boundary. Everything simulation-side has finished
                    // writing by now; nothing render-side has read yet.
                    render_sync::sync_render_transforms,
                    // The camera follows the *interpolated* render position, so it must
                    // run after the sync or it would chase a one-frame-stale target and
                    // reintroduce the jitter interpolation just removed.
                    camera::update_flight_camera,
                )
                    .chain()
                    .before(TransformSystems::Propagate),
            );
    }
}

/// Logs which wgpu backend and GPU the game actually got.
///
/// This is the Phase 0 "wgpu pipeline confirmation" exit criterion. There is no
/// Metal-specific code anywhere in this project and there does not need to be:
/// wgpu selects Metal on macOS, Vulkan on Linux and DX12 on Windows by itself.
/// Confirming that is a log line, not a subsystem — but it is worth having, because
/// "which backend am I on" is otherwise invisible until something renders wrong.
///
/// `RenderAdapterInfo` is inserted into the main app world (not just the render
/// sub-app) by `RenderPlugin::finish`, so an ordinary `Startup` system can read it.
fn log_render_backend(adapter_info: Res<RenderAdapterInfo>) {
    info!(
        "Render backend: {:?} | GPU: {} | driver: {} {}",
        adapter_info.backend, adapter_info.name, adapter_info.driver, adapter_info.driver_info,
    );

    // `Backend` (singular) is not re-exported by bevy_render::settings, only `Backends`
    // (the bitflag set), so compare through the From<Backend> conversion.
    if cfg!(target_os = "macos") && Backends::from(adapter_info.backend) != Backends::METAL {
        warn!(
            "Expected the Metal backend on macOS but got {:?}. Rendering will still work, \
             but performance characteristics will not match the target platform.",
            adapter_info.backend,
        );
    }
}
