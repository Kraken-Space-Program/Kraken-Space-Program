//! The celestial body a vessel is flying over, and the gravity it pulls with.
//!
//! Phase 1 is deliberately **one body, defined in Rust**. Bodies come from Lua in Phase 2
//! (`sdk/api/planets.rs`), at which point this becomes a component on a body entity rather
//! than a resource. The shape of the data does not change, only where it is loaded from.

use bevy::math::DVec3;
use bevy::prelude::*;

use bevy_rapier3d::prelude::ReadMassProperties;

use crate::physics::forces::PendingForces;
use crate::rendering::render_sync::SimPosition;

/// The body everything orbits, falls toward, and lands on.
///
/// # Why the centre is below the launch site rather than at the origin
///
/// The scene is a flat plane at `y = 0`, and altitude readouts, the camera and Krakensbane
/// all assume that. Putting the body's centre at `(0, -radius, 0)` makes the surface pass
/// exactly through the origin, so gravity becomes a point-mass field — falling off with
/// altitude, always pointing at the centre — without anything else having to change.
///
/// The curvature is real but unobservable at this scale: over the 400 m pad the surface
/// drops 0.13 mm. Terrain that is actually round is Phase 3.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CelestialBody {
    /// Surface radius, metres.
    pub radius_m: f64,
    /// Standard gravitational parameter `μ = G·M`, m³/s².
    ///
    /// Stored rather than mass because μ is what every orbital equation actually wants, and
    /// because it is the quantity that is known precisely for real bodies — `G` is the
    /// least precisely known constant in physics, and `M` is only ever derived from μ.
    pub gravitational_parameter: f64,
    /// Centre of the body in simulation space (f64, absolute).
    pub centre: DVec3,
}

impl CelestialBody {
    /// Gravitational acceleration at a simulation-space position, m/s².
    pub fn gravity_at(&self, position: DVec3) -> DVec3 {
        let to_centre = self.centre - position;
        let distance_squared = to_centre.length_squared();

        // Inside the body (or exactly at its centre) there is no meaningful "down" and the
        // inverse-square law diverges. Nothing should ever be here — it means a vessel has
        // fallen through the terrain — so return zero rather than an infinite force that
        // would launch the wreck out of the solar system.
        if distance_squared <= f64::EPSILON {
            return DVec3::ZERO;
        }

        to_centre * (self.gravitational_parameter / (distance_squared * distance_squared.sqrt()))
    }

    /// Height above the surface, metres. Negative below it.
    pub fn altitude_of(&self, position: DVec3) -> f64 {
        (position - self.centre).length() - self.radius_m
    }

    /// Unit vector pointing away from the body's centre — local "up".
    pub fn up_at(&self, position: DVec3) -> DVec3 {
        (position - self.centre).normalize_or_zero()
    }
}

impl Default for CelestialBody {
    /// The stock home world.
    ///
    /// Sized so that surface gravity is Earth's 9.81 m/s² while the radius stays small
    /// enough that orbital velocity is a few km/s rather than 7.8 — the same bargain KSP
    /// makes, and for the same reason: a real Earth is not fun to reach orbit around.
    fn default() -> Self {
        let radius_m = 600_000.0;
        let surface_gravity = 9.81;
        Self {
            radius_m,
            // μ = g·r², which is the definition rearranged. Deriving it rather than writing
            // the number keeps surface gravity exactly 9.81 if the radius is ever changed.
            gravitational_parameter: surface_gravity * radius_m * radius_m,
            centre: DVec3::new(0.0, -radius_m, 0.0),
        }
    }
}

/// Applies the body's gravity to every part.
///
/// # Why gravity is a force producer and not Rapier's `gravity` setting
///
/// Rapier's global gravity is a single uniform vector. That is exactly wrong for a space
/// game: gravity has to fall off with altitude, point at a body's centre rather than
/// straight down, and eventually come from more than one body at once. Producing it as a
/// force alongside thrust and drag costs one multiply per part and makes all three of those
/// changes local to this file. `physics::configure_rapier` therefore sets Rapier's own
/// gravity to zero.
///
/// # Ordering
///
/// `FixedUpdate`, in `PhysicsSchedule::Produce`, with the other force producers.
pub fn apply_gravity(
    body: Res<CelestialBody>,
    mut parts: Query<(&SimPosition, &ReadMassProperties, &mut PendingForces)>,
) {
    for (position, mass_properties, mut forces) in &mut parts {
        let mass_kg = f64::from(mass_properties.get().mass);
        if mass_kg <= 0.0 {
            continue;
        }
        forces.add_force(body.gravity_at(position.0) * mass_kg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_gravity_is_earth_normal() {
        let body = CelestialBody::default();
        let surface = DVec3::ZERO; // the launch pad
        let g = body.gravity_at(surface);

        assert!((g.length() - 9.81).abs() < 1e-6, "got {}", g.length());
        // Straight down at the launch site.
        assert!(g.normalize().abs_diff_eq(DVec3::NEG_Y, 1e-12));
    }

    /// The whole point of a point mass: gravity has to weaken with altitude. A uniform
    /// field is what this replaced, and it would pass every other test here.
    #[test]
    fn gravity_falls_off_with_altitude() {
        let body = CelestialBody::default();
        let low = body.gravity_at(DVec3::ZERO).length();
        let high = body.gravity_at(DVec3::new(0.0, 100_000.0, 0.0)).length();

        // (600/700)² = 0.7347
        let expected = 9.81 * (600_000.0f64 / 700_000.0).powi(2);
        assert!(
            (high - expected).abs() < 1e-6,
            "got {high}, want {expected}"
        );
        assert!(high < low);
    }

    #[test]
    fn altitude_is_measured_from_the_surface() {
        let body = CelestialBody::default();
        assert!(body.altitude_of(DVec3::ZERO).abs() < 1e-6);
        assert!((body.altitude_of(DVec3::new(0.0, 1_000.0, 0.0)) - 1_000.0).abs() < 1e-6);
    }
}
