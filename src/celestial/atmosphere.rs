//! Atmosphere, and the drag it exerts.
//!
//! # The Phase 1 model
//!
//! DESIGN.md wants pressure and temperature *curves* defined per body in Lua. This is the
//! flat model it says to build first: a single exponential, which is what an isothermal
//! atmosphere in hydrostatic equilibrium actually is, and which is within a few percent of
//! the real thing up to the altitudes a Phase 1 rocket reaches.
//!
//! ```text
//!   ρ(h) = ρ₀ · exp(-h / H)
//! ```
//!
//! Replacing it with sampled curves later changes [`Atmosphere::density_at`] and nothing
//! else — every consumer already goes through it.

use bevy::math::DVec3;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::celestial::body::CelestialBody;
use crate::celestial::occlusion;
use crate::physics::forces::PendingForces;
use crate::rendering::render_sync::{SimPosition, SimVelocity};
use crate::vessel::components::VesselId;

/// A body's atmosphere.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Atmosphere {
    /// Altitude at which the atmosphere ends, metres. Above this, density is exactly zero.
    ///
    /// An exponential never actually reaches zero, and a vessel in a 200 km orbit should not
    /// be losing energy to a millionth of a pascal for the rest of the save. A hard ceiling
    /// is also what "not in atmosphere" means for the on-rails transition in DESIGN.md.
    pub height_m: f64,
    /// Density at the surface, kg/m³.
    pub sea_level_density: f64,
    /// Pressure at the surface, pascals.
    pub sea_level_pressure_pa: f64,
    /// The altitude over which density falls by a factor of e, metres.
    pub scale_height_m: f64,
}

impl Default for Atmosphere {
    /// Earth's density and pressure, on a scale height chosen to put the ceiling somewhere
    /// a Phase 1 rocket can plausibly reach.
    fn default() -> Self {
        Self {
            height_m: 70_000.0,
            sea_level_density: 1.225,
            sea_level_pressure_pa: 101_325.0,
            scale_height_m: 5_600.0,
        }
    }
}

impl Atmosphere {
    pub fn density_at(&self, altitude_m: f64) -> f64 {
        if altitude_m >= self.height_m {
            return 0.0;
        }
        // Below sea level (in a valley, or clipped into terrain) the exponential would keep
        // growing. Clamping means the densest the air ever gets is sea level.
        self.sea_level_density * (-altitude_m.max(0.0) / self.scale_height_m).exp()
    }

    /// Ambient pressure, pascals. Engines interpolate their Isp against this.
    pub fn pressure_at(&self, altitude_m: f64) -> f64 {
        if altitude_m >= self.height_m {
            return 0.0;
        }
        self.sea_level_pressure_pa * (-altitude_m.max(0.0) / self.scale_height_m).exp()
    }

    /// Ambient pressure as a fraction of sea level, in `0.0..=1.0`.
    pub fn pressure_ratio_at(&self, altitude_m: f64) -> f64 {
        self.pressure_at(altitude_m) / self.sea_level_pressure_pa
    }
}

/// Aerodynamic properties of a part.
///
/// DESIGN.md: "Aerodynamic drag is computed per-part: cross-sectional area × drag
/// coefficient × dynamic pressure, not CFD."
#[derive(Component, Debug, Clone, Copy)]
pub struct DragSurface {
    /// Radius of the disc this part presents to the airflow, metres. The *exposed* fraction
    /// of that disc is worked out per tick by [`crate::celestial::occlusion`] — a part
    /// tucked in behind another one contributes nothing.
    pub radius_m: f64,
    /// Drag coefficient, dimensionless.
    pub cd: f64,
}

/// Applies aerodynamic drag to every part with a [`DragSurface`].
///
/// `F = ½ · ρ · v² · Cd · A`, opposing the velocity vector.
///
/// # Shielding
///
/// Only the exposed part of each disc counts — see [`crate::celestial::occlusion`]. Drag is
/// therefore a per-*vessel* calculation even though the force is applied per part, because
/// whether a part is in the wake depends on what its neighbours are doing.
///
/// # What this still does not model
///
/// Lift, angle of attack, and the aerodynamic torque that makes a real rocket weathervane.
/// All three need a model of *where* the force acts, not just how big it is. Drag here acts
/// at each part's own centre, which does produce some turning moment on an asymmetric
/// vessel, but nothing that deserves to be called an aerodynamics model.
///
/// # Ordering
///
/// `FixedUpdate`, in `PhysicsSchedule::Produce`, alongside thrust and gravity.
pub fn apply_drag(
    body: Res<CelestialBody>,
    atmosphere: Res<Atmosphere>,
    parts: Query<(Entity, &VesselId, &SimPosition, &SimVelocity, &DragSurface)>,
    mut forces: Query<&mut PendingForces>,
    mut by_vessel: Local<HashMap<Entity, Vec<Entity>>>,
    mut lookup: Local<HashMap<Entity, (DVec3, DVec3, f64, f64)>>,
) {
    by_vessel.clear();
    lookup.clear();

    for (part, vessel, position, velocity, surface) in &parts {
        by_vessel.entry(vessel.0).or_default().push(part);
        lookup.insert(part, (position.0, velocity.0, surface.radius_m, surface.cd));
    }

    for members in by_vessel.values() {
        // The flow direction is the vessel's motion, not each part's: parts of one rigid
        // stack differ only by rotation, and using per-part velocity would let a slowly
        // tumbling vessel disagree with itself about which end is facing the wind.
        let mean_velocity: DVec3 = members
            .iter()
            .filter_map(|part| lookup.get(part))
            .map(|(_, velocity, _, _)| *velocity)
            .sum::<DVec3>()
            / members.len() as f64;

        let frontals: Vec<occlusion::Frontal> = members
            .iter()
            .filter_map(|part| lookup.get(part))
            .map(|(position, _, radius, _)| occlusion::Frontal {
                position: *position,
                radius_m: *radius,
            })
            .collect();

        let areas = occlusion::exposed_areas(&frontals, mean_velocity);

        for (index, part) in members.iter().enumerate() {
            let Some((position, velocity, _, cd)) = lookup.get(part) else {
                continue;
            };
            let speed = velocity.length();
            if speed <= 0.0 || areas[index] <= 0.0 {
                continue;
            }

            let density = atmosphere.density_at(body.altitude_of(*position));
            if density <= 0.0 {
                continue;
            }

            let magnitude = 0.5 * density * speed * speed * cd * areas[index];
            if let Ok(mut pending) = forces.get_mut(*part) {
                pending.add_force(-*velocity / speed * magnitude);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_falls_off_exponentially_and_stops_at_the_ceiling() {
        let atmosphere = Atmosphere::default();

        assert!((atmosphere.density_at(0.0) - 1.225).abs() < 1e-9);

        // One scale height up is exactly 1/e of sea level. This is the property the whole
        // model rests on, so it is worth pinning rather than assuming.
        let one_scale_height = atmosphere.density_at(atmosphere.scale_height_m);
        assert!((one_scale_height - 1.225 / std::f64::consts::E).abs() < 1e-9);

        assert_eq!(atmosphere.density_at(atmosphere.height_m), 0.0);
        assert_eq!(atmosphere.density_at(200_000.0), 0.0);
    }

    /// Terminal velocity is the number a player actually feels, so it is the one worth
    /// asserting: a 5 t stack must not be falling at supersonic speeds all the way down,
    /// and must not be floating either.
    #[test]
    fn terminal_velocity_is_in_a_sane_range() {
        let atmosphere = Atmosphere::default();
        let mass_kg = 5_000.0;
        let cd_a = 0.3 * 1.23 * 5.0; // five 1.25 m parts

        let terminal = (2.0 * mass_kg * 9.81 / (atmosphere.sea_level_density * cd_a)).sqrt();
        assert!(
            (100.0..400.0).contains(&terminal),
            "terminal velocity {terminal} m/s is outside anything plausible"
        );
    }

    #[test]
    fn pressure_ratio_is_one_at_sea_level_and_zero_in_space() {
        let atmosphere = Atmosphere::default();
        assert!((atmosphere.pressure_ratio_at(0.0) - 1.0).abs() < 1e-12);
        assert_eq!(atmosphere.pressure_ratio_at(100_000.0), 0.0);
    }
}
