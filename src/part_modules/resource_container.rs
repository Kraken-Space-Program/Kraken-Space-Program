//! Resource storage and the mass it contributes.
//!
//! Kilograms throughout. A "resource" is any consumable a part can hold — propellant now,
//! electric charge and life support later.

use bevy::prelude::*;

use crate::vessel::components::{PartMass, VesselId};

/// One resource slot on a part.
#[derive(Debug, Clone)]
pub struct ResourceSlot {
    pub resource: String,
    pub amount_kg: f64,
    /// Capacity. Not read until refuelling and the editor's fuel sliders exist, but it is
    /// what the Lua definition declares, so it is carried rather than discarded.
    #[allow(dead_code)]
    pub max_kg: f64,
}

/// Everything a part is currently holding.
#[derive(Component, Debug, Clone, Default)]
pub struct ResourceContainer {
    pub slots: Vec<ResourceSlot>,
}

impl ResourceContainer {
    /// Mass of the contents, kilograms.
    pub fn mass_kg(&self) -> f64 {
        self.slots.iter().map(|slot| slot.amount_kg).sum()
    }

    /// Removes up to `wanted_kg` of a resource, returning how much was actually available.
    ///
    /// Returning the shortfall rather than failing is what lets an engine detect flameout:
    /// it asks for what it needs and finds out what it got.
    pub fn draw(&mut self, resource: &str, wanted_kg: f64) -> f64 {
        let Some(slot) = self.slots.iter_mut().find(|slot| slot.resource == resource) else {
            return 0.0;
        };

        let drawn = wanted_kg.min(slot.amount_kg);
        slot.amount_kg -= drawn;
        drawn
    }
}

/// Keeps each part's Rapier mass in step with what it is carrying.
///
/// design.md: "Wet mass is recomputed from `ResourceContainer` contents whenever resources
/// change, via `Changed<ResourceContainer>`. Nothing polls for mass every frame."
///
/// This matters more than it looks: a first stage is roughly 90% propellant by mass, so a
/// rocket whose mass never updated would still weigh its launch weight when empty and would
/// stop accelerating exactly when it should start.
pub fn update_wet_mass(
    mut query: Query<
        (
            &PartMass,
            &ResourceContainer,
            &mut bevy_rapier3d::prelude::AdditionalMassProperties,
        ),
        Changed<ResourceContainer>,
    >,
) {
    for (dry, container, mut mass) in &mut query {
        // f64 → f32 at the Rapier boundary. A part's mass is a few thousand kilograms at
        // most, nowhere near f32's precision limits.
        *mass = bevy_rapier3d::prelude::AdditionalMassProperties::Mass(
            (dry.dry_kg + container.mass_kg()) as f32,
        );
    }
}

/// Total mass of a vessel in kilograms — dry parts plus everything they carry.
pub fn vessel_mass_kg(
    vessel: Entity,
    parts: &Query<(&VesselId, &PartMass, Option<&ResourceContainer>)>,
) -> f64 {
    parts
        .iter()
        .filter(|(id, _, _)| id.0 == vessel)
        .map(|(_, mass, container)| mass.dry_kg + container.map_or(0.0, ResourceContainer::mass_kg))
        .sum()
}
