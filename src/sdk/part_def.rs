//! What a Lua part definition compiles into.
//!
//! This is the answer to the EXECUTION.md question *"what struct does a Lua part
//! definition compile into at load time?"* — a plain Rust struct, loaded once at startup,
//! then turned into ECS components by `vessel::assembly`. Nothing here is a component:
//! these are the *blueprints*, not the parts. One `PartDefinition` produces many part
//! entities.
//!
//! # Units
//!
//! Lua part files are written in the units an engineer would use — tonnes, kilonewtons,
//! seconds. **Everything in this module is SI**: kilograms, newtons, metres. The
//! conversion happens exactly once, in `api::parts`, at load time.
//!
//! Mixing units is how you lose a Mars orbiter. The rule is: Lua is human-facing and may
//! use convenient units; the moment a value crosses into Rust it is SI and stays SI.

use bevy::math::DVec3;

/// A part blueprint, loaded from one `part { ... }` block in a `.lua` file.
#[derive(Debug, Clone)]
pub struct PartDefinition {
    /// Stable identity, e.g. `kraken.engine.spark`. Save files reference this string, so
    /// changing it breaks existing saves.
    pub id: String,
    // Shown by the editor's part browser (Phase 4). Parsed now because throwing away
    // authored content and re-adding the plumbing later is the more expensive order.
    #[allow(dead_code)]
    pub display_name: String,
    #[allow(dead_code)]
    pub manufacturer: String,
    #[allow(dead_code)]
    pub description: String,
    pub categories: Vec<String>,
    /// Dry mass in **kilograms** (Lua declares tonnes).
    pub dry_mass_kg: f64,
    /// Optional `.glb` **visual** override, relative to the asset root. `None` means the
    /// part is drawn from its [`shape`](Self::shape), which most parts are. A mesh never
    /// affects collision — see `sdk::shape` for why that separation matters.
    pub geometry: Option<String>,
    /// Authoritative geometry: drives the collider and the placeholder mesh alike.
    pub shape: crate::sdk::shape::PartShape,
    pub attach_nodes: Vec<AttachNode>,
    pub modules: Vec<PartModuleDef>,
    /// Drag coefficient, dimensionless. Lua may declare `drag_coefficient`; the default is
    /// a blunt cylinder, which is what most parts are and what every part looks like until
    /// somebody models a nose cone.
    pub drag_coefficient: f64,
    /// Radius of the disc the part presents to the airflow, metres.
    ///
    /// Separate from the shape radius because an open structure blocks far less air than it
    /// occupies: a landing strut assembly is 1.9 m across and needs to be, so the vessel has
    /// a wide base of support, but it is four thin legs and a hub — treating it as a solid
    /// 1.9 m disc turns it into a parachute. Lua declares `drag_radius`; the default is the
    /// shape radius, which is right for anything solid.
    pub drag_radius_m: f64,
}

/// Default drag coefficient for a part that does not declare one.
///
/// A blunt-ended cylinder in axial flow, which is what every stock part is — none of them
/// has a nose cone yet. This used to be 0.3, not because anything was that streamlined, but
/// to compensate for every part in a stack presenting its full frontal area at once. With
/// shielding modelled properly (`celestial::occlusion`) the coefficient can go back to
/// meaning what it says.
pub const DEFAULT_DRAG_COEFFICIENT: f64 = 0.8;

impl PartDefinition {
    /// Finds an attach node by name (`"top"`, `"bottom"`, ...).
    pub fn node(&self, name: &str) -> Option<&AttachNode> {
        self.attach_nodes.iter().find(|node| node.name == name)
    }
}

/// A point where another part can connect.
#[derive(Debug, Clone)]
pub struct AttachNode {
    /// Key from the Lua `attach_nodes` table — `top`, `bottom`, and later `radial_1` etc.
    pub name: String,
    /// Position in part-local space, metres.
    pub position: DVec3,
    /// Node size class. 1 = 0.625 m, 2 = 1.25 m. Parts with mismatched sizes should not
    /// connect (not enforced yet).
    pub size: u32,
    /// Force along the node axis that breaks the joint, in **newtons** (Lua declares kN).
    pub tensile_strength_n: f64,
    /// Force across the node axis that breaks the joint, in **newtons**.
    pub shear_strength_n: f64,
}

/// A module attached to a part.
///
/// design.md: "Part modules are components, not objects." This enum exists only to carry
/// data from Lua to the assembly system, which inserts the matching *component*. There is
/// no `PartModule` trait, no virtual dispatch, and no lifecycle methods — adding a module
/// type means adding a variant here, a component in `part_modules/`, and a system.
#[derive(Debug, Clone)]
pub enum PartModuleDef {
    ReactionWheel(ReactionWheelDef),
    Engine(EngineDef),
    ResourceContainer(ResourceContainerDef),
    Decoupler(DecouplerDef),
}

#[derive(Debug, Clone)]
pub struct EngineDef {
    /// Vacuum thrust in **newtons** (Lua declares kN).
    pub thrust_n: f64,
    /// Vacuum specific impulse, seconds.
    pub isp_vac_s: f64,
    /// Sea-level specific impulse, seconds.
    pub isp_sl_s: f64,
    /// Propellant mixture as `(resource, mass fraction)`. Fractions should sum to 1.
    pub propellants: Vec<(String, f64)>,
}

#[derive(Debug, Clone)]
pub struct ResourceContainerDef {
    pub resource: String,
    /// Starting quantity, kilograms.
    pub amount_kg: f64,
    /// Capacity, kilograms.
    pub max_kg: f64,
}

#[derive(Debug, Clone)]
pub struct DecouplerDef {
    /// Which stage fires this decoupler. Lower numbers fire first.
    pub stage: u32,
    /// Attach node released when it fires.
    pub node: String,
    /// Separation impulse in **newtons** (Lua declares kN).
    pub ejection_force_n: f64,
}

/// `reaction_wheel { torque = 3.0 }` — attitude authority, in kN·m in Lua.
#[derive(Debug, Clone, PartialEq)]
pub struct ReactionWheelDef {
    /// Torque at full deflection, newton-metres.
    pub torque_nm: f64,
}
