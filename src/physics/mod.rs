//! Physics module — the only place in the codebase that configures or calls Rapier directly.
//!
//! # Rules (from design.md)
//!
//! - No direct Rapier calls outside `physics/`. Everything else talks to physics
//!   through components and events.
//! - Rapier handles (`RigidBodyHandle`, `ColliderHandle`) are internal to this module.
//!   Other modules never import `rapier3d` directly.
//! - The fixed timestep is 50 Hz. This is set in `main.rs` via `TimestepMode` and
//!   must never be changed to per-frame.
//!
//! # What lives here (Phase 0 stub — grows in Phase 1+)
//!
//! | File | Responsibility |
//! |------|---------------|
//! | `mod.rs` (this file) | Plugin definition, Rapier configuration |
//! | `rapier_integration.rs` | Rapier ↔ Bevy bridge, body creation/removal (Phase 1) |
//! | `joints.rs` | Part-to-part joint management, failure detection (Phase 1) |
//! | `krakensbane.rs` | Origin shifting for floating-point precision (Phase 1) |
//! | `forces.rs` | PendingForces accumulation and Rapier application (Phase 1) |

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

/// Fixed physics timestep duration, shared across main and render interpolation.
pub const FIXED_TIMESTEP: f64 = 1.0 / 50.0;

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, configure_rapier);
    }
}

/// Sets Rapier's global gravity.
///
/// Gravity is a Rapier-internal concern, so it belongs here in `physics/` rather
/// than in `main.rs`. The value mirrors real Earth gravity for Phase 0 testing;
/// per-body gravity will be applied procedurally once celestial bodies exist.
///
/// Note: since bevy_rapier 0.22, `RapierConfiguration` is a **Component** (not a
/// Resource). It lives on the entity that owns the default `RapierContext`, which
/// the plugin spawns at startup. Query for it with `.single_mut()`.
fn configure_rapier(mut config: Query<&mut RapierConfiguration>) {
    if let Ok(mut config) = config.single_mut() {
        config.gravity = Vec3::new(0.0, -9.81, 0.0);
    }
}
