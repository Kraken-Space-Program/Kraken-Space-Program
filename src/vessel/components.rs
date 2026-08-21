//! Vessel and part components. Data only — behaviour lives in systems.
//!
//! design.md, "Vessel identity is a component, not a hierarchy": parts belong to a vessel
//! through a [`VesselId`] component. There is no runtime part tree, only a flat set of
//! entities that agree on which vessel they are. That is what makes staging cheap — a
//! vessel splits by rewriting a component on some entities, not by re-parenting a tree.

use bevy::prelude::*;

/// A vessel: a connected group of parts under one control input.
#[derive(Component, Debug)]
pub struct Vessel {
    pub name: String,
}

/// Which vessel a part belongs to. Lives on every part entity.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct VesselId(pub Entity);

/// The vessel the player is currently flying. Exactly one should exist.
#[derive(Component, Debug)]
pub struct ActiveVessel;

/// Stable part identity, matching `PartDefinition::id`.
///
/// Read by impact damage for its log messages, and by save/load in Phase 2 — it is the
/// string a save file stores per part.
#[derive(Component, Debug, Clone)]
pub struct PartId(pub String);

/// Dry mass in kilograms. Never changes at runtime.
///
/// design.md: "Mass is computed, not stored." Wet mass is dry mass plus whatever the
/// part's [`ResourceContainer`](crate::part_modules::resource_container::ResourceContainer)
/// currently holds, recomputed only when resources change.
#[derive(Component, Debug, Clone, Copy)]
pub struct PartMass {
    pub dry_kg: f64,
}

/// Parts this part is physically connected to.
///
/// Used to work out what is still attached to what after a joint breaks, which is how a
/// vessel discovers it has become two vessels.
#[derive(Component, Debug, Default, Clone)]
pub struct PartConnections(pub Vec<Entity>);

/// The visual entity mirroring this part, so systems can find one from the other.
///
/// See `render_sync::SyncRender` for why they are separate entities at all.
#[derive(Component, Debug, Clone, Copy)]
// Written at assembly so part-destruction can despawn the matching visual. Nothing
// destroys parts yet.
#[allow(dead_code)]
pub struct PartVisual(pub Entity);

/// Player control input for a vessel, in `-1.0..1.0` (throttle is `0.0..1.0`).
///
/// Sampled every frame in `Update` and consumed at 50 Hz in `FixedUpdate` — the classic
/// latched-input arrangement. Sampling in the fixed schedule instead would drop inputs on
/// any frame that did not coincide with a tick.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct ControlState {
    pub throttle: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    /// Stability assist. When on, the vessel actively cancels its own rotation whenever the
    /// player is not steering. A damper, not a heading hold — see
    /// [`apply_attitude_control`](crate::vessel::control::apply_attitude_control).
    pub sas: bool,
    /// One-shot request to fire the next stage, cleared the moment it is consumed by
    /// [`request_stage`](crate::vessel::staging::request_stage).
    ///
    /// A latch rather than a message because staging is *input*, and input belongs here
    /// with the throttle and the control axes. The space bar sets it; so does the scripted
    /// pilot, which is the point — the pilot is meant to fly with exactly the controls a
    /// player has, and staging was the one action it could not reach, because
    /// `StageActivated` used to be written straight from the keyboard.
    pub stage: bool,
}

/// Which stage fires next when the player presses stage.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct CurrentStage(pub u32);

/// The part that carries the vessel's control authority — the one attitude torque is
/// applied to, and the reference point for "where is this vessel".
#[derive(Component, Debug)]
pub struct RootPart;

/// Which propellant network a part belongs to.
///
/// Engines draw only from tanks sharing their group. Groups are assigned at assembly time
/// and change at decoupler boundaries, which is the simple version of KSP's rule that
/// crossfeed does not pass through a decoupler.
///
/// Deciding this at assembly rather than by walking the part graph every tick is what
/// keeps propellant draw O(1) per engine — and, more importantly, deterministic: no
/// traversal means no dependence on iteration order.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuelGroup(pub u32);

/// The part directly beneath this one in the stack — the one its joint holds it to.
///
/// Connections in [`PartConnections`] are undirected, which is what you want for asking
/// "what is still attached to what". This records the *direction*, which is what you need
/// to know which single edge a decoupler destroys.
#[derive(Component, Debug, Clone, Copy)]
pub struct AttachedBelow(pub Entity);

/// Marks a vessel that has been wrecked.
///
/// Set by `vessel::damage` on a hard impact. Its parts still exist and still have physics —
/// a wreck is debris, not nothing — but its joints are gone and its controls are dead.
#[derive(Component, Debug)]
pub struct Destroyed;
