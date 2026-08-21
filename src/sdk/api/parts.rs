//! The `part { ... }` DSL exposed to Lua.
//!
//! This is the surface mod authors actually write against, so it is deliberately small and
//! declarative. A definition file is data with a little sugar — it declares a part, it does
//! not *do* anything.
//!
//! ```lua
//! part {
//!   id = "kraken.engine.spark",
//!   mass = 0.13,                          -- tonnes
//!   attach_nodes = {
//!     top = { position = vec3(0, 0, 0), size = 1, tensile_strength = 90.0 },
//!   },
//!   modules = {
//!     engine { thrust = 20, isp_vac = 320, propellants = { RP1 = 0.3, LOX = 0.7 } },
//!   },
//! }
//! ```
//!
//! Every value crossing into Rust is converted to SI here and nowhere else — see
//! [`super::super::part_def`].

use std::cell::RefCell;
use std::rc::Rc;

use bevy::math::DVec3;
use mlua::{Lua, Table};

use crate::sdk::part_def::{
    AttachNode, DEFAULT_DRAG_COEFFICIENT, DecouplerDef, EngineDef, PartDefinition, PartModuleDef,
    ReactionWheelDef, ResourceContainerDef,
};

/// Field used to tag a module table with its type, so `part {}` can dispatch on it.
const MODULE_TAG: &str = "__kraken_module";

const KG_PER_TONNE: f64 = 1_000.0;
const N_PER_KN: f64 = 1_000.0;

/// Collects the parts declared by the file currently being executed.
pub type PartSink = Rc<RefCell<Vec<PartDefinition>>>;

/// Installs `part`, `engine`, `resource_container`, `decoupler` and `vec3` into `env`.
///
/// `asset_dir` is the definition file's own directory, relative to the asset root. Mesh
/// paths in the file are resolved against it, so a part can say `geometry =
/// "fueltank_1.glb"` and mean "the one next to me".
pub fn install(lua: &Lua, env: &Table, sink: PartSink, asset_dir: String) -> mlua::Result<()> {
    env.set(
        "vec3",
        lua.create_function(|lua, (x, y, z): (f64, f64, f64)| {
            let table = lua.create_table()?;
            table.set("x", x)?;
            table.set("y", y)?;
            table.set("z", z)?;
            Ok(table)
        })?,
    )?;

    env.set("engine", module_constructor(lua, "engine")?)?;
    env.set(
        "resource_container",
        module_constructor(lua, "resource_container")?,
    )?;
    env.set("decoupler", module_constructor(lua, "decoupler")?)?;
    env.set("reaction_wheel", module_constructor(lua, "reaction_wheel")?)?;

    env.set(
        "part",
        lua.create_function(move |_, table: Table| {
            let definition = parse_part(&table, &asset_dir)?;
            sink.borrow_mut().push(definition);
            Ok(())
        })?,
    )?;

    Ok(())
}

/// Builds a function that tags its table argument with a module type and returns it.
///
/// `engine { ... }` is just `{ ..., __kraken_module = "engine" }`. Keeping the tag in the
/// table rather than wrapping it in userdata means authors can still inspect and compose
/// module tables in Lua.
fn module_constructor(lua: &Lua, kind: &'static str) -> mlua::Result<mlua::Function> {
    lua.create_function(move |_, table: Table| {
        table.set(MODULE_TAG, kind)?;
        Ok(table)
    })
}

// ---------------------------------------------------------------------------
// Lua → Rust
// ---------------------------------------------------------------------------

fn parse_part(table: &Table, asset_dir: &str) -> mlua::Result<PartDefinition> {
    let id: String = required(table, "id", "part")?;

    // From here on, errors name the part — "missing field 'mass'" is useless in a mod
    // folder with two hundred files in it.
    let context = format!("part '{id}'");

    let dry_mass_tonnes: f64 = required(table, "mass", &context)?;

    let geometry = table
        .get::<Option<String>>("geometry")
        .unwrap_or(None)
        .map(|path| resolve_asset_path(&path, asset_dir));

    let attach_nodes = match table.get::<Option<Table>>("attach_nodes")? {
        Some(nodes) => parse_attach_nodes(&nodes, &id)?,
        None => Vec::new(),
    };

    let modules = match table.get::<Option<Table>>("modules")? {
        Some(modules) => parse_modules(&modules, &id)?,
        None => Vec::new(),
    };

    // Attach-node size is the fallback diameter, so a part that predates `shape` keeps
    // exactly the radius it already had.
    let shape = crate::sdk::shape::parse(table, &id, stock_radius_for(&attach_nodes))?;

    Ok(PartDefinition {
        display_name: table
            .get::<Option<String>>("display_name")?
            .unwrap_or_else(|| id.clone()),
        manufacturer: table
            .get::<Option<String>>("manufacturer")?
            .unwrap_or_default(),
        description: table
            .get::<Option<String>>("description")?
            .unwrap_or_default(),
        categories: table
            .get::<Option<Vec<String>>>("categories")?
            .unwrap_or_default(),
        dry_mass_kg: dry_mass_tonnes * KG_PER_TONNE,
        drag_coefficient: table
            .get::<Option<f64>>("drag_coefficient")?
            .unwrap_or(DEFAULT_DRAG_COEFFICIENT),
        drag_radius_m: table
            .get::<Option<f64>>("drag_radius")?
            .unwrap_or(shape.radius_m),
        geometry,
        attach_nodes,
        modules,
        shape,
        id,
    })
}

/// Radius implied by a part's largest attach node, metres.
///
/// The stock diameters: size 1 is 0.625 m across, size 2 is 1.25 m. Used only when a part
/// does not declare a shape radius of its own.
fn stock_radius_for(nodes: &[AttachNode]) -> f64 {
    let size = nodes.iter().map(|node| node.size).max().unwrap_or(1);
    match size {
        1 => 0.3125,
        2 => 0.625,
        other => 0.625 * f64::from(other) / 2.0,
    }
}

/// `attach_nodes` is a table keyed by node name, not an array — the key *is* the node's
/// identity, which is what decouplers and assembly refer to.
fn parse_attach_nodes(table: &Table, part_id: &str) -> mlua::Result<Vec<AttachNode>> {
    let mut nodes = Vec::new();

    for entry in table.pairs::<String, Table>() {
        let (name, node) = entry?;
        let context = format!("part '{part_id}': attach node '{name}'");

        nodes.push(AttachNode {
            position: match node.get::<Option<Table>>("position")? {
                Some(vector) => parse_vec3(&vector, &context)?,
                None => DVec3::ZERO,
            },
            size: node.get::<Option<u32>>("size")?.unwrap_or(1),
            tensile_strength_n: node.get::<Option<f64>>("tensile_strength")?.unwrap_or(0.0)
                * N_PER_KN,
            shear_strength_n: node.get::<Option<f64>>("shear_strength")?.unwrap_or(0.0) * N_PER_KN,
            name,
        });
    }

    // pairs() over a Lua hash table has no defined order, and design.md bans
    // nondeterministic iteration from anything that reaches the simulation. Sorting by
    // name makes joint creation order identical on every machine and every run — which
    // matters the moment multiplayer or a replay needs two clients to agree.
    nodes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(nodes)
}

fn parse_modules(table: &Table, part_id: &str) -> mlua::Result<Vec<PartModuleDef>> {
    let mut modules = Vec::new();

    for entry in table.sequence_values::<Table>() {
        let module = entry?;
        let kind: String = module.get::<Option<String>>(MODULE_TAG)?.ok_or_else(|| {
            mlua::Error::runtime(format!(
                "part '{part_id}': entry in 'modules' is a plain table — modules must be \
                 built with a constructor, e.g. engine {{ ... }} or resource_container {{ ... }}"
            ))
        })?;

        let context = format!("part '{part_id}': module '{kind}'");

        modules.push(match kind.as_str() {
            "engine" => PartModuleDef::Engine(EngineDef {
                thrust_n: required::<f64>(&module, "thrust", &context)? * N_PER_KN,
                isp_vac_s: required::<f64>(&module, "isp_vac", &context)?,
                // Sea-level Isp defaults to vacuum Isp: a part that never declares it is
                // a vacuum-only engine, and silently pretending otherwise would make it
                // mysteriously weak in atmosphere.
                isp_sl_s: module
                    .get::<Option<f64>>("isp_sl")?
                    .unwrap_or(required::<f64>(&module, "isp_vac", &context)?),
                propellants: parse_propellants(&module, &context)?,
            }),
            "resource_container" => {
                let max = required::<f64>(&module, "max", &context)?;
                PartModuleDef::ResourceContainer(ResourceContainerDef {
                    resource: required::<String>(&module, "name", &context)?,
                    amount_kg: module.get::<Option<f64>>("amount")?.unwrap_or(max),
                    max_kg: max,
                })
            }
            "reaction_wheel" => PartModuleDef::ReactionWheel(ReactionWheelDef {
                torque_nm: required::<f64>(&module, "torque", &context)? * N_PER_KN,
            }),
            "decoupler" => PartModuleDef::Decoupler(DecouplerDef {
                stage: module.get::<Option<u32>>("stage")?.unwrap_or(0),
                node: required::<String>(&module, "node", &context)?,
                ejection_force_n: module.get::<Option<f64>>("ejection_force")?.unwrap_or(0.0)
                    * N_PER_KN,
            }),
            other => {
                return Err(mlua::Error::runtime(format!(
                    "part '{part_id}': unknown module type '{other}'"
                )));
            }
        });
    }

    Ok(modules)
}

/// `propellants = { RP1 = 0.3, LOX = 0.7 }` — resource name to mass fraction.
fn parse_propellants(module: &Table, context: &str) -> mlua::Result<Vec<(String, f64)>> {
    let Some(table) = module.get::<Option<Table>>("propellants")? else {
        return Ok(Vec::new());
    };

    let mut propellants: Vec<(String, f64)> = table
        .pairs::<String, f64>()
        .collect::<mlua::Result<Vec<_>>>()
        .map_err(|error| type_error(&format!("{context}: propellants"), error))?;

    // Same determinism reasoning as attach nodes: this drives the order propellant is
    // drained in, which must not depend on Lua's hash iteration order.
    propellants.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(propellants)
}

fn parse_vec3(table: &Table, context: &str) -> mlua::Result<DVec3> {
    Ok(DVec3::new(
        table
            .get::<Option<f64>>("x")
            .map_err(|error| type_error(context, error))?
            .unwrap_or(0.0),
        table.get::<Option<f64>>("y")?.unwrap_or(0.0),
        table.get::<Option<f64>>("z")?.unwrap_or(0.0),
    ))
}

/// Resolves a mesh path against the definition file's own directory.
///
/// Authors have written both `"fueltank_1.glb"` and `"assets/parts/engine_spark.glb"`.
/// The first is what the format means; the second is someone writing a path relative to
/// the repo root out of habit. Strip a leading `assets/` so both land in the same place
/// rather than one of them silently failing to load.
fn resolve_asset_path(path: &str, asset_dir: &str) -> String {
    let path = path.trim_start_matches('/');
    let path = path.strip_prefix("assets/").unwrap_or(path);

    // A path with a separator was written from the asset root and is taken as-is; a bare
    // filename is resolved next to the definition file.
    if path.contains('/') || asset_dir.is_empty() {
        path.to_string()
    } else {
        format!("{asset_dir}/{path}")
    }
}

/// Reads a field that must be present, with an error message that names the part.
///
/// `get::<Option<T>>` yields `Ok(None)` for a missing field and `Err` for a present one of
/// the wrong type, which lets "you forgot this" and "you typed the wrong thing here" stay
/// distinguishable in the message the mod author reads.
fn required<T: mlua::FromLua>(table: &Table, field: &str, context: &str) -> mlua::Result<T> {
    table
        .get::<Option<T>>(field)
        .map_err(|error| type_error(&format!("{context}: '{field}'"), error))?
        .ok_or_else(|| mlua::Error::runtime(format!("{context}: missing required field '{field}'")))
}

fn type_error(context: &str, error: mlua::Error) -> mlua::Error {
    mlua::Error::runtime(format!("{context}: {error}"))
}
