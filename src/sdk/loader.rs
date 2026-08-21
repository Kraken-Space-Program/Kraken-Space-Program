//! Loads `.lua` part definitions at startup.
//!
//! # Why this reads the filesystem directly instead of going through `AssetServer`
//!
//! Bevy's asset pipeline is asynchronous, and part definitions are needed *before* the
//! first vessel can be assembled. Making assembly wait on an async load would mean a state
//! machine and a loading screen for something that takes single-digit milliseconds at
//! startup. Meshes still go through `AssetServer` — those genuinely are large and
//! genuinely can stream.
//!
//! When hot-reload arrives (Phase 2), this becomes a file watcher that re-runs the same
//! parse and swaps the registry. The parse is deliberately a pure function of the file
//! contents so that stays easy.

use std::path::Path;

use bevy::prelude::*;

use crate::sdk::api::parts::{self, PartSink};
use crate::sdk::part_def::PartDefinition;
use crate::sdk::sandbox;

/// Instruction budget for a single definition file.
///
/// Generous — a definition file is data, and anything that needs a million instructions to
/// declare a fuel tank is doing something the format did not intend.
const LOAD_INSTRUCTION_BUDGET: u64 = 1_000_000;

/// Every part definition known to the game, keyed by [`PartDefinition::id`].
///
/// Sorted by id, so iteration order is stable across runs and machines.
#[derive(Resource, Default, Debug)]
pub struct PartRegistry {
    parts: Vec<PartDefinition>,
}

impl PartRegistry {
    pub fn get(&self, id: &str) -> Option<&PartDefinition> {
        self.parts
            .binary_search_by(|part| part.id.as_str().cmp(id))
            .ok()
            .map(|index| &self.parts[index])
    }

    /// Iterates every known definition. Used by the editor's part list (Phase 4) and by
    /// anything that needs to enumerate rather than look up.
    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = &PartDefinition> {
        self.parts.iter()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// Where definition files live, relative to the working directory.
///
/// Must stay in step with `AssetPlugin::file_path` in `main.rs` — mesh paths in part
/// definitions are resolved relative to the asset root, so the two have to agree on where
/// that root is.
#[derive(Resource, Debug, Clone)]
pub struct DefinitionRoot(pub String);

impl Default for DefinitionRoot {
    fn default() -> Self {
        Self("src/assets".into())
    }
}

/// A Lua definition file failed to load.
///
/// design.md: "The game must not crash because a mod's Lua file has a bug." A broken
/// definition costs you that part, logged with a message its author can act on, and
/// nothing else.
#[derive(Message, Debug, Clone)]
// Read by the error screen, which is Phase 2. The message is already emitted and logged.
#[allow(dead_code)]
pub struct LuaError {
    pub script: String,
    pub message: String,
}

/// Reads every `.lua` file under `<root>/parts/` and fills the [`PartRegistry`].
pub fn load_part_definitions(
    mut commands: Commands,
    root: Res<DefinitionRoot>,
    mut errors: MessageWriter<LuaError>,
) {
    let parts_root = Path::new(&root.0).join("parts");
    let mut files = Vec::new();

    if let Err(error) = collect_lua_files(&parts_root, &mut files) {
        error!("could not scan part definitions in {parts_root:?}: {error}");
        commands.insert_resource(PartRegistry::default());
        return;
    }

    // Sorted so that load order — and therefore any "later definition wins" behaviour —
    // does not depend on the order the filesystem happens to hand back directory entries.
    files.sort();

    let mut parts = Vec::new();

    for file in &files {
        let relative_dir = file
            .parent()
            .and_then(|dir| dir.strip_prefix(&root.0).ok())
            .map(|dir| dir.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        match load_file(file, &relative_dir) {
            Ok(loaded) => {
                debug!("loaded {} part(s) from {}", loaded.len(), file.display());
                parts.extend(loaded);
            }
            Err(message) => {
                // Log and keep going. One bad file must not cost the player every other part.
                error!("{}: {message}", file.display());
                errors.write(LuaError {
                    script: file.display().to_string(),
                    message,
                });
            }
        }
    }

    parts.sort_by(|a, b| a.id.cmp(&b.id));

    let duplicates: Vec<_> = parts
        .windows(2)
        .filter(|pair| pair[0].id == pair[1].id)
        .map(|pair| pair[0].id.clone())
        .collect();
    for id in duplicates {
        warn!("duplicate part id '{id}' — one definition is shadowing another");
    }
    parts.dedup_by(|a, b| a.id == b.id);

    info!(
        "part registry loaded: {} part(s) from {} file(s)",
        parts.len(),
        files.len()
    );
    commands.insert_resource(PartRegistry { parts });
}

/// Executes one definition file in a fresh sandbox and returns what it declared.
///
/// A fresh VM per file is deliberate: one file cannot leave globals behind for the next,
/// so load order can never change the result.
fn load_file(path: &Path, relative_dir: &str) -> Result<Vec<PartDefinition>, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;

    let lua = sandbox::create_lua_vm().map_err(|error| error.to_string())?;
    sandbox::install_instruction_budget(&lua, LOAD_INSTRUCTION_BUDGET)
        .map_err(|error| error.to_string())?;

    let env = sandbox::create_definition_environment(&lua).map_err(|error| error.to_string())?;
    let sink: PartSink = Default::default();

    parts::install(&lua, &env, sink.clone(), relative_dir.to_string())
        .map_err(|error| error.to_string())?;

    lua.load(&source)
        .set_name(path.display().to_string())
        .set_environment(env)
        .exec()
        .map_err(|error| error.to_string())?;

    let collected = sink.borrow().clone();
    Ok(collected)
}

fn collect_lua_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_lua_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "lua") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdk::part_def::PartModuleDef;

    /// Writes `source` to a scratch file and loads it.
    ///
    /// `case` must be unique per test: cargo runs tests in parallel threads of one
    /// process, so a shared filename means one test reads another's script.
    fn load(case: &str, source: &str) -> Result<Vec<PartDefinition>, String> {
        let dir = std::env::temp_dir().join(format!("ksp-loader-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{case}.lua"));
        std::fs::write(&path, source).unwrap();
        let result = load_file(&path, "parts/test");
        std::fs::remove_file(&path).ok();
        result
    }

    /// The shipped definitions must actually parse. This reads the real files rather than a
    /// fixture, so a change to `fueltank_1.lua` that breaks the format fails CI.
    #[test]
    fn stock_definitions_load() {
        let root = DefinitionRoot::default();
        let mut files = Vec::new();
        collect_lua_files(Path::new(&root.0).join("parts").as_path(), &mut files)
            .expect("stock parts directory should exist");
        assert!(!files.is_empty(), "no stock part definitions found");

        for file in files {
            let dir = file
                .parent()
                .and_then(|d| d.strip_prefix(&root.0).ok())
                .map(|d| d.to_string_lossy().to_string())
                .unwrap_or_default();
            let parts = load_file(&file, &dir)
                .unwrap_or_else(|error| panic!("{} failed to load: {error}", file.display()));
            assert!(!parts.is_empty(), "{} declared no parts", file.display());
        }
    }

    /// Units are converted exactly once, here. Tonnes in, kilograms out.
    #[test]
    fn units_are_converted_to_si() {
        let parts = load(
            "units",
            r#"
            part {
              id = "test.tank",
              mass = 0.25,
              attach_nodes = {
                top = { position = vec3(0, 1, 0), size = 2, tensile_strength = 720.0 },
              },
              modules = {
                resource_container { name = LOX, amount = 1400, max = 1400 },
              },
            }
            "#,
        )
        .expect("definition should load");

        let part = &parts[0];
        assert_eq!(part.dry_mass_kg, 250.0, "0.25 t should load as 250 kg");
        assert_eq!(
            part.node("top").unwrap().tensile_strength_n,
            720_000.0,
            "720 kN should load as 720000 N"
        );

        // Resource amounts are already kilograms and must NOT be scaled.
        let PartModuleDef::ResourceContainer(container) = &part.modules[0] else {
            panic!("expected a resource container");
        };
        assert_eq!(container.amount_kg, 1400.0);
        assert_eq!(
            container.resource, "LOX",
            "bare identifier should become its own name"
        );
    }

    /// A broken file must produce a readable error, not a panic and not a silent skip.
    #[test]
    fn missing_required_field_is_a_named_error() {
        let error = load("missing_field", r#"part { id = "test.broken" }"#)
            .expect_err("should have failed");
        assert!(
            error.contains("test.broken") && error.contains("mass"),
            "error should name the part and the field, got: {error}"
        );
    }

    /// An infinite loop in a definition file must not hang the game.
    #[test]
    fn runaway_script_is_killed_by_the_instruction_budget() {
        let error = load("runaway", "while true do end").expect_err("should have been killed");
        assert!(
            error.contains("budget"),
            "expected a budget error, got: {error}"
        );
    }
}
