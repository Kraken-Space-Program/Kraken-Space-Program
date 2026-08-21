//! Engines: throttle in, thrust and propellant consumption out.
//!
//! The engine system is a *producer* — it writes into
//! [`PendingForces`](crate::physics::forces::PendingForces) and knows nothing about Rapier.

use bevy::math::DVec3;
use bevy::prelude::*;

use crate::celestial::atmosphere::Atmosphere;
use crate::celestial::body::CelestialBody;
use crate::part_modules::resource_container::ResourceContainer;
use crate::physics::forces::PendingForces;
use crate::rendering::render_sync::{SimPosition, SimRotation};
use crate::vessel::components::{ControlState, Destroyed, FuelGroup, VesselId};

/// Standard gravity, m/s². The constant in the rocket equation — not the local
/// gravitational acceleration, which is why it does not change when you leave Earth.
pub const G0: f64 = 9.806_65;

/// An engine.
#[derive(Component, Debug, Clone)]
pub struct Engine {
    /// Vacuum thrust, newtons.
    pub thrust_n: f64,
    /// Specific impulse in vacuum and at sea level, seconds. The engine interpolates
    /// between them on ambient pressure — see [`apply_engine_thrust`].
    pub isp_vac_s: f64,
    pub isp_sl_s: f64,
    /// `(resource, mass fraction)`, sorted by resource name for deterministic draw order.
    pub propellants: Vec<(String, f64)>,
    /// Set by [`apply_engine_thrust`] each tick — what the engine is actually doing, for
    /// the HUD and for effects.
    pub current_thrust_n: f64,
}

/// Applies thrust and burns propellant.
///
/// # Ordering
///
/// `FixedUpdate`, between `clear_pending_forces` and `apply_pending_forces`.
///
/// # Propellant
///
/// Mass flow comes straight from the rocket equation: `ṁ = F / (Isp · g₀)`. The engine
/// requests each propellant in its declared mass fraction and throttles itself down to
/// whatever fraction it actually received, so running a tank dry produces a smooth
/// spool-down rather than a step to zero.
///
/// Propellant is drawn from any tank on the same vessel. Real crossfeed rules — flow
/// through attach nodes, priority by stage — are Phase 2; this is the simplest thing that
/// makes a two-stage rocket behave correctly.
pub fn apply_engine_thrust(
    vessels: Query<(&ControlState, Has<Destroyed>)>,
    body: Res<CelestialBody>,
    atmosphere: Res<Atmosphere>,
    mut engines: Query<(
        &mut Engine,
        &VesselId,
        &FuelGroup,
        &SimPosition,
        &SimRotation,
        &mut PendingForces,
    )>,
    mut tanks: Query<(&VesselId, &FuelGroup, &mut ResourceContainer)>,
) {
    for (mut engine, vessel_id, fuel_group, position, rotation, mut forces) in &mut engines {
        // A wreck does not thrust. `vessel::damage` zeroes the throttle when a vessel is
        // destroyed, but relying on that alone makes "does the rubble fly away across the
        // pad" depend on nobody ever writing to `ControlState` afterwards — which is exactly
        // what a scripted pilot or a replayed input stream does. The invariant belongs here,
        // where the thrust is produced.
        let Ok((control, destroyed)) = vessels.get(vessel_id.0) else {
            engine.current_thrust_n = 0.0;
            continue;
        };
        if destroyed {
            engine.current_thrust_n = 0.0;
            continue;
        }

        let throttle = f64::from(control.throttle.clamp(0.0, 1.0));
        if throttle <= 0.0 {
            engine.current_thrust_n = 0.0;
            continue;
        }

        // Isp between the sea-level and vacuum figures, on ambient pressure. A rocket
        // engine is more efficient in vacuum because there is no back-pressure on the
        // nozzle exit; the linear interpolation on pressure ratio is the standard
        // approximation and is what KSP uses.
        let pressure_ratio = atmosphere.pressure_ratio_at(body.altitude_of(position.0));
        let isp = engine.isp_vac_s + (engine.isp_sl_s - engine.isp_vac_s) * pressure_ratio;
        let demanded_thrust = engine.thrust_n * throttle;
        let mass_flow_kg_s = demanded_thrust / (isp * G0);
        let demanded_kg = mass_flow_kg_s * crate::physics::PHYSICS_DT as f64;

        // Draw each propellant in its mixture fraction and keep the worst shortfall.
        let mut available_fraction: f64 = 1.0;
        for (resource, mixture_fraction) in &engine.propellants {
            let wanted = demanded_kg * mixture_fraction;
            if wanted <= 0.0 {
                continue;
            }

            let mut drawn = 0.0;
            for (tank_vessel, tank_group, mut container) in &mut tanks {
                // Same vessel and same propellant network. The fuel group is what stops a
                // second-stage engine from quietly draining the first stage's tanks
                // through a decoupler that is about to be jettisoned.
                if tank_vessel.0 != vessel_id.0 || tank_group != fuel_group {
                    continue;
                }
                drawn += container.draw(resource, wanted - drawn);
                if drawn >= wanted {
                    break;
                }
            }

            available_fraction = available_fraction.min(drawn / wanted);
        }

        let thrust = demanded_thrust * available_fraction;
        engine.current_thrust_n = thrust;

        if thrust <= 0.0 {
            continue;
        }

        // Thrust acts along the part's local +Y — the stack builds upward, so an engine
        // mounted at the bottom of a tank pushes the tank up.
        //
        // Orientation comes from the f64 `SimRotation` rather than the f32 `Transform`, so
        // this module never touches render-space data at all.
        let direction = (rotation.0 * DVec3::Y).normalize_or_zero();
        forces.add_force(direction * thrust);
    }
}
