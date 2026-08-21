//! Procedural part geometry, declared in Lua.
//!
//! # Why parts describe a shape instead of only naming a mesh
//!
//! Every part definition used to name a `.glb`, and four of the five stock parts named one
//! that had never been modelled. That was not only a cosmetic problem — the *collider* was
//! derived from attach-node spacing and a lookup table of stock diameters, so a part's
//! physical size was a guess that would silently change the day a real mesh arrived and
//! invalidate anything tuned against it. The Spark still carries a comment admitting its top
//! attach node is at the origin because the model does not exist.
//!
//! A shape fixes both ends. It is the authoritative geometry: the collider is built from it,
//! the placeholder mesh is built from it, and they cannot disagree. A `.glb` becomes what it
//! should always have been — an optional *visual* override that changes how a part looks and
//! nothing about how it behaves.

use mlua::Table;

/// The form of a part, for both collision and its placeholder mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartShape {
    pub kind: ShapeKind,
    /// Outer radius, metres.
    pub radius_m: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    /// A plain tube. Fuel tanks, structural sections.
    Cylinder,
    /// A body with a nozzle bell flaring out below it.
    Engine,
    /// A short, slightly oversized disc — visibly a seam in the stack.
    Decoupler,
    /// A cone, point upward.
    NoseCone,
    /// A footpad on a strut, splayed outward from the stack.
    LandingLeg,
}

impl ShapeKind {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "cylinder" => Self::Cylinder,
            "engine" => Self::Engine,
            "decoupler" => Self::Decoupler,
            "nose_cone" => Self::NoseCone,
            "landing_leg" => Self::LandingLeg,
            _ => return None,
        })
    }

    pub const NAMES: &'static str = "cylinder, engine, decoupler, nose_cone, landing_leg";
}

/// Reads `shape = { kind = "engine", radius = 0.625 }`.
///
/// `fallback_radius` comes from the part's attach-node sizes and is used when the definition
/// does not give one, so existing parts keep the diameter they already had.
pub fn parse(table: &Table, part_id: &str, fallback_radius: f64) -> mlua::Result<PartShape> {
    let Some(shape) = table.get::<Option<Table>>("shape")? else {
        return Ok(PartShape {
            kind: ShapeKind::Cylinder,
            radius_m: fallback_radius,
        });
    };

    let kind = match shape.get::<Option<String>>("kind")? {
        None => ShapeKind::Cylinder,
        Some(name) => ShapeKind::parse(&name).ok_or_else(|| {
            mlua::Error::runtime(format!(
                "part '{part_id}': unknown shape kind '{name}' — expected one of {}",
                ShapeKind::NAMES
            ))
        })?,
    };

    let radius_m = shape
        .get::<Option<f64>>("radius")?
        .unwrap_or(fallback_radius);
    if radius_m <= 0.0 {
        return Err(mlua::Error::runtime(format!(
            "part '{part_id}': shape radius must be positive, got {radius_m}"
        )));
    }

    Ok(PartShape { kind, radius_m })
}
