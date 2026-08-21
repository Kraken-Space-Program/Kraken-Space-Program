# Kraken Space Program — Technical Design Document

> This document is for contributors and for keeping yourself honest. It covers architecture decisions, what to build and in what order, hard rules for the codebase, and anti-patterns to never repeat. The README covers what the project is. This covers how to actually make it.

---

## Core Philosophy

The entire codebase lives by three rules:

1. **If it's performance-critical, it's Rust.** No exceptions. Physics, rendering, networking, audio, ECS core — all Rust.
2. **If it's content or logic, it's Lua.** Parts, planets, careers, UI behavior, game rules, balance — all Lua via the Kraken SDK. A base game feature and a mod are the same thing at the engine level.
3. **If KSP1 did it a certain way, assume there's a better way.** The KSP1 source is reference material for understanding behavior, not for copying architecture. Every design decision in this codebase must be justified on its own merits, not inherited from C# spaghetti.

---

## Tech Stack

| Layer | Technology | Why |
|---|---|---|
| Language | Rust | No GC, no overhead, memory safety, fearless concurrency |
| ECS / Game Framework | Bevy | Parallel ECS, clean architecture, Rust-native, MIT licensed |
| Physics | Rapier (via bevy_rapier3d) | Rust-native, deterministic with fixed timestep, Bevy integration already exists |
| Scripting | mlua (Lua 5.4) | Safe Rust/Lua FFI, well-maintained, performant, Lua is accessible to modders |
| Graphics Backend | wgpu (via Bevy) | Vulkan, Metal, DX12, WebGPU all from one API. No vendor lock-in |
| Networking | TBD — likely either Bevy Replicon or custom on top of Quinn (QUIC) | Needs to be deterministic-physics-friendly; decide at Phase 3 |
| Audio | Bevy's built-in audio + kira backend | Sufficient for now |
| Precision | f64 throughout orbital mechanics and simulation; origin shifting (Krakensbane-style) for rendering | f32 is not enough at planetary scale |
| Save Format | TOML | Human-readable, diffable, excellent Rust ecosystem (`toml` crate), handles nested structures cleanly |

---

## Coordinate Systems

This is the most important reference section in the document. Every subtle bug that crosses a coordinate boundary comes from someone not knowing which space they were in. Read this before touching anything that involves positions.

There are four coordinate spaces in this engine. They are not interchangeable.

```
┌─────────────────────────────────────────────────────────────────┐
│  SOLAR INERTIAL (f64 DVec3)                                     │
│  Origin: system barycenter. Axes: J2000-aligned, non-rotating.  │
│  Used for: orbital elements, SOI calculations, ephemeris.       │
│  Never used for physics or rendering directly.                  │
└─────────────────────────────────────────────────────────────────┘
                          ↓ body frame transform
┌─────────────────────────────────────────────────────────────────┐
│  BODY-FIXED (f64 DVec3)                                         │
│  Origin: center of a celestial body. Rotates with the body.     │
│  Used for: surface positions, latitude/longitude/altitude,      │
│  terrain heightmap sampling, atmosphere lookups.                │
└─────────────────────────────────────────────────────────────────┘
                          ↓ subtract WorldOrigin
┌─────────────────────────────────────────────────────────────────┐
│  SIMULATION WORLD (f64 DVec3)                                   │
│  Origin: WorldOrigin resource (tracked in solar inertial f64).  │
│  Used for: Rapier physics, all vessel positions, joint anchors, │
│  collision detection. Krakensbane shifts this when needed.      │
└─────────────────────────────────────────────────────────────────┘
                          ↓ render_sync.rs ONLY — subtract local origin, cast to f32
┌─────────────────────────────────────────────────────────────────┐
│  RENDER WORLD (f32 Vec3)                                        │
│  Origin: always near the active vessel. Updated every frame.    │
│  Used for: Bevy Transform, everything the GPU sees, audio       │
│  spatialization, UI world-to-screen projection.                 │
└─────────────────────────────────────────────────────────────────┘
```

**The conversion rule: only `render_sync.rs` converts a *simulation* position to a *render* position. No other file is allowed to do this. Ever.**

There is exactly one qualification, and it is not optional: **Rapier is f32 throughout.** There
is no f64 build of it and there is not going to be one, so `SimPosition` cannot be the value
Rapier integrates — it has to be a value *derived* from what Rapier integrates:

```
SIMULATION WORLD (f64)  =  WorldOrigin (f64)  +  Rapier's local transform (f32)
```

Rapier only ever sees a **local physics frame** that Krakensbane keeps within ~10 km of zero,
where f32 has roughly millimetre precision — ample for resolving contacts. `physics/readback.rs`
lifts the result back to f64 after every tick, and `physics/krakensbane.rs` moves the frame.

So the rule in practice is: **f32 positions are legal inside `physics/` (Rapier's frame) and
`rendering/` (the GPU's frame). Anything crossing a module boundary is f64.** If you are writing
`as f32` on a position anywhere else, you are in the wrong place.

The cast is safe because the subtraction happens first:
```rust
// In render_sync.rs — the ONLY place this pattern is allowed
fn sync_render_transforms(
    origin: Res<LocalOrigin>,          // f64, near active vessel
    mut query: Query<(&SimPosition, &mut Transform)>,
) {
    for (sim_pos, mut transform) in &mut query {
        let relative = sim_pos.0 - origin.0;    // f64 - f64 = small f64
        transform.translation = relative.as_vec3(); // NOW cast to f32, precision is fine
    }
}
```

If you ever find yourself writing `.as_vec3()` or `as f32` outside of `render_sync.rs`, stop. That's not allowed. You're in the wrong place.

### Multiplayer and coordinate spaces

The server works exclusively in Simulation World space (f64). It never does origin shifting — that is a purely client-side rendering concern. Each client maintains its own `LocalOrigin` based on their active vessel. Network messages transmit `DVec3` positions. No `Vec3` ever goes over the wire. The server has no concept of a render world.

---

## Determinism

Rapier at a fixed timestep gives deterministic physics. This is necessary for multiplayer and for save/load consistency. There are rules that must never be broken to preserve this.

**Compiler flags:** CI builds and release builds must never enable flags that break IEEE 754 compliance. Fast-math (`-ffast-math` equivalents, or LLVM's `reassociate`/`unsafe-fp-math`) is permanently banned. This is enforced in `.cargo/config.toml`. Do not override it per-crate without a reason documented in the PR.

**FP op ordering:** Even on a single platform (x86-64), different compiler versions can reorder floating point operations in ways that produce different rounding. For anything in `orbital/` or `physics/`, prefer operations with deterministic ordering. If you're not sure, add a unit test with a known numerical result and run it on CI across multiple Rust versions.

**System ordering:** Bevy systems that affect physics state must be explicitly ordered. Do not rely on implicit parallel scheduling for anything that mutates positions, velocities, or forces. Use `.chain()` or explicit `before`/`after` constraints. Nondeterminism from race conditions is worse than from FP rounding because it's intermittent and nearly impossible to reproduce.

**Multiplayer architecture decision (Phase 5):** The networking model is TBD, but the decision must be made before Phase 5 starts, and it affects decisions made in Phases 1–4. The two realistic options are:

- **Server-authoritative with lag compensation:** Server runs simulation, clients predict locally and reconcile. More forgiving of minor FP differences. Probably the right call for a KSP-style game where precise determinism across all clients is hard to guarantee.
- **Lockstep:** All clients run the same simulation, inputs are synced, results must match. Requires true determinism. Works well for slow-paced strategy games, harder for a physics sim.

Even though the decision is deferred, write all physics code as if lockstep might be chosen. That means: no `HashMap` iteration in physics systems (non-deterministic ordering), no `thread_rng()` in simulation code (use a seeded RNG resource), no frame-rate-dependent calculations.

---

## Error Handling

Bevy systems cannot return `Result<T, E>`. This means errors need an explicit strategy or the codebase fills up with `.unwrap()` calls that silently become panics in production.

The rules:

**Fatal errors** (corrupt save, missing engine-required asset, Rapier initialization failure): write to a `FatalError` event and transition to `GameState::Error`. The error screen shows the message. Do not panic in release builds if it can be avoided.

**Non-fatal errors** (Lua script runtime error, missing optional asset, unknown part ID in save): log with `warn!()` or `error!()` and continue. The game must not crash because a mod's Lua file has a bug.

**Lua errors specifically:** All Lua calls from Rust are wrapped. A Lua error fires a `LuaError` event with the script path, error message, and stack trace. The system that called it logs it and moves on. The Lua author gets a clear message. The game keeps running.

**Assertions:** Use `debug_assert!()` for invariants that should never be violated in correct code. These fire in dev builds and are stripped in release. Use `assert!()` only for things that genuinely cannot be recovered from.

```rust
// Pattern for systems that can fail
fn load_part_asset(
    mut commands: Commands,
    mut error_events: EventWriter<LuaError>,
) {
    match try_load_part() {
        Ok(part) => { commands.spawn(part); }
        Err(e) => {
            error_events.write(LuaError { message: e.to_string(), .. });
            // continue — don't panic, don't silently swallow
        }
    }
}
```

---

## ECS Conventions

Bevy ECS is the spine of everything. Follow these rules consistently.

**Components are pure data.** No methods on components except trivial constructors and `Default`. Behavior lives in systems.

```rust
// CORRECT
#[derive(Component, Default)]
struct Fuel {
    current: f64,
    max: f64,
}

// WRONG — behavior does not belong on components
impl Fuel {
    fn consume(&mut self, amount: f64) { ... } // no
}
```

**Systems are pure functions.** They take queries and resources, they return nothing, they emit events. If a system needs to communicate a result, it writes an event.

**Use events for cross-system communication.** Never query for a component to trigger side effects in another system — that's hidden coupling. Write an event, read an event.

**Marker components over booleans.** Presence/absence of a zero-size component is cheaper and more composable than a `bool` field on another component.

```rust
#[derive(Component)]
struct Landed;        // presence means landed, absence means not

#[derive(Component)]
struct OnRails;       // physics suspended, orbit computed analytically

#[derive(Component)]
struct ActiveVessel;  // the vessel the player is currently controlling

#[derive(Component)]
struct NeedsCollider; // terrain chunk awaiting physics collider generation
```

**`Changed<T>` detection instead of polling.** If a system only needs to run when data changes, use `Query<_, Changed<T>>`. Don't run every frame if you don't need to.

---

## Part System

This is where KSP1 went most wrong. `Part.cs` at 9,400 lines was a god class that owned physics attachment, resources, module management, GUI state, joint lifecycle, drag, and more. In this engine, a "part" is not a class — it's an entity with a set of focused components, each owned by a focused system.

### What a part entity looks like

```
Entity: "Spark Engine (part_id: kraken.engine.spark)"
  ├── PartId("kraken.engine.spark")          -- immutable identity, links back to Lua definition
  ├── PartMass { dry: 0.13 }                 -- dry mass from Lua, never changes at runtime
  ├── SimPosition(DVec3)                      -- world position, f64, simulation space
  ├── PartRotation(DQuat)                     -- orientation, f64
  ├── VesselId(Entity)                        -- which vessel this part belongs to
  ├── RapierBody(RigidBodyHandle)             -- handle into Rapier, owned by physics system
  ├── PartCollider(ColliderHandle)            -- handle into Rapier collider
  ├── AttachNodes([AttachNode; N])            -- where other parts can connect
  ├── PartConnections([Entity; N])            -- entities this part is connected to
  ├── DragSurface { area: f32, cd: f32 }     -- aerodynamic properties
  ├── PendingForces(Vec<Force>)               -- forces to apply this tick, cleared after Rapier step
  │
  └── [module components — present only if applicable]
      ├── Engine { thrust, isp_vac, isp_sl, propellants }
      ├── ResourceContainer { slots: Vec<ResourceSlot> }
      ├── Decoupler { stage: u32, node: AttachNodeId }
      ├── RcsThruster { thrust, propellants }
      └── [etc. — one component per module type]
```

### Design decisions

**Part modules are components, not objects.** KSP1 had a `PartModule` base class with virtual methods (~50 empty virtuals that nobody overrides). Here, an engine part just has an `Engine` component. A fuel tank has a `ResourceContainer` component. Systems query for the specific component they care about. There is no base class, no virtual dispatch, no empty lifecycle methods.

**Joint lifecycle belongs to the joints system, not to parts.** `Part` entities hold `PartConnections` (a list of connected entities). The `joints` system is the sole owner of creating, updating, and destroying Rapier joints. A part never touches Rapier directly.

**Vessel identity is a component, not a hierarchy.** Parts belong to a vessel via a `VesselId` component. A vessel entity holds metadata (name, root part entity, etc.). When a vessel splits, affected parts get their `VesselId` updated. There is no recursive part tree at runtime — only in save files (as a flat list with parent IDs).

**Mass is computed, not stored.** `PartMass` stores dry mass only (from Lua, never changes). Wet mass is recomputed from `ResourceContainer` contents whenever resources change, via `Changed<ResourceContainer>`. The physics system reads wet mass when updating Rapier body masses. Nothing polls for mass every frame.

**`PendingForces` is the physics interface.** Any system that wants to push a part (thrust, drag, RCS, EVA jetpack) writes a `Force` into `PendingForces`. The physics integration system reads all pending forces, applies them to Rapier bodies, and clears the component. This decouples every force-producing system from Rapier entirely.

### Structural failure

Joint force limits are configured per attach node in Lua (`tensile_strength`, `shear_strength`). Rapier reports when joint force limits are exceeded. The `joints` system listens for these reports, fires a `JointFailure { part_a, part_b }` event, and the appropriate systems handle consequences (vessel split, sound, debris spawn). The part does not know it failed. The joint system knows.

This is the direct inversion of KSP1's pattern where `Part.cs` was responsible for its own destruction. Physics reports facts. Systems respond to facts. Parts are data.

### Staging

A `Decoupler` component holds the attach node it controls and the stage number it fires on. Asking to stage is *input*: it sets a one-shot `stage` latch on `ControlState`, alongside the throttle and the control axes, and `staging::request_stage` is the only thing that turns that latch — or the space bar — into a `StageActivated(u32)` event and an incremented `CurrentStage`. One path, so a scripted pilot stages exactly the way a player does and neither can skip a stage. The staging system queries all `Decoupler` components matching the current stage, destroys their joints via the joints system, which fires `VesselSplit` events. Part tree updates, vessel reassignment, and Rapier changes all flow from events. There is no method call chain inside a monolith.

---

## Module Structure

Keep modules focused. A module owns one concept.

This is the shape the code grows into, not an inventory of what exists today. Entries that
are listed and absent from the repository are planned; everything that exists is listed.

```
src/
  main.rs
  lib.rs
  
  physics/
    mod.rs
    rapier_integration.rs   -- Rapier ↔ Bevy bridge, body creation/removal
    joints.rs               -- Part-to-part joint management, failure detection
    krakensbane.rs          -- Origin shifting for floating point precision
    colliders.rs            -- Collider generation from part meshes
    forces.rs               -- PendingForces accumulation + Rapier application
    readback.rs             -- Rapier transforms → SimPosition each tick
    impact.rs               -- Contacts → impact speeds (read from the tick before)
    
  orbital/
    mod.rs
    orbit.rs                -- Orbit struct, Keplerian elements (all f64)
    propagation.rs          -- Analytical orbit propagation (on-rails)
    maneuver.rs             -- Delta-V calculations
    soi.rs                  -- Sphere of influence transitions
    
  vessel/
    mod.rs
    components.rs           -- Vessel, Part components (data only)
    assembly.rs             -- Building a vessel entity tree from a part definition
    systems.rs              -- Vessel-level systems (mass update, CoM)
    control.rs              -- Input → ControlState → attitude torque
    staging.rs              -- Stage separation logic
    damage.rs               -- Impact speed → vessel destruction
    geometry.rs             -- Colliders and placeholder meshes from a part's declared shape
    reset.rs                -- Throw the flight away and rebuild it on the pad
    symmetry.rs             -- Editor symmetry groups
    
  part_modules/
    mod.rs
    engine.rs               -- Engine component + thrust system
    resource_container.rs   -- Fuel/resource storage + flow system
    decoupler.rs            -- Decoupler component + staging integration
    reaction_wheel.rs       -- Attitude authority; a vessel's is the sum of its wheels
    rcs.rs                  -- RCS thrusters
    -- one file per module type; adding a new part type = adding a file here
    
  celestial/
    mod.rs
    body.rs                 -- CelestialBody component + data
    occlusion.rs            -- Which parts shield which from the airstream
    terrain/
      mod.rs
      quadtree.rs           -- LOD quadtree structure
      heightmap.rs          -- Procedural heightmap generation (f64 → f32 at vertex upload)
      chunk_lifecycle.rs    -- Load/unload trigger logic
      biomes.rs
    atmosphere.rs
    
  sdk/
    mod.rs
    bindings.rs             -- mlua bindings exposed to Lua
    sandbox.rs              -- Lua VM setup, tick budget enforcement
    api/
      parts.rs
      planets.rs
      vessels.rs
      ui.rs
    loader.rs               -- Loads .lua part/planet definitions at startup
    part_def.rs             -- The Rust-side shape of a part definition (SI units)
    shape.rs                -- Declared part geometry: cylinder, engine, nose cone, leg
    
  rendering/
    mod.rs
    render_sync.rs          -- THE ONLY FILE that converts f64 SimPosition → f32 Transform
    camera.rs               -- Orbit camera that follows the active vessel
    atmosphere_pipeline.rs
    terrain_pipeline.rs
    
  save/
    mod.rs
    format.rs               -- TOML serialization structures
    migrations/
      mod.rs
      v0_to_v1.rs
      -- one file per version increment, no skipping
    
  diagnostics/
    mod.rs                  -- Always-on flight watchdog + KRAKEN_TRACE per-tick recorder
    test_pilot.rs           -- Scripted profiles, flying with the controls a player has

  ui/
    mod.rs
    -- UI is primarily Lua-driven; minimal Rust here, mostly event bridges
    
  networking/               -- Phase 5, not touched until then
    mod.rs
```

**Dependency rules:**

- `orbital/` has zero dependencies on `rendering/` or `physics/`
- `physics/` has zero dependencies on `sdk/`, and none on `rendering/` beyond the
  coordinate types themselves — `SimPosition`, `SimVelocity`, `WorldOrigin`, `LocalOrigin`.
  Those are simulation types that happen to live in `render_sync.rs` because that file is
  the single conversion point, and `physics/{readback,krakensbane,impact}.rs` import them.
  Nothing else from `rendering/` — no camera, no materials, no pipelines — may be imported.
  Whether the coordinate types should move somewhere neutral instead is CHECKLIST #27.
- `sdk/` talks to the rest of the engine only through ECS events — never direct function calls into other modules
- `render_sync.rs` is the only file that imports from both `physics/` (for `SimPosition`) and touches `Transform`
- `networking/` is isolated behind a feature flag until Phase 5

If you find yourself importing across these lines to grab internals, add an event or a resource instead.

---

## The Kraken SDK (Lua API)

The SDK is the thing modders and content developers touch. It is also the thing the base game's content is written in. There is no distinction between official content and mods at the engine level.

### What Lua can do

- Define parts: geometry reference, attach nodes, modules, mass, drag coefficients, resource containers, structural strength values
- Define celestial bodies: orbit parameters, atmosphere config, terrain parameters, biomes, surface scatter
- Define resources: name, density, color, flow rules
- Define career/science progression trees
- Define UI screens and HUD elements
- Hook into game events: launch, staging, landing, docking, vessel creation/destruction
- Read vessel state: position, velocity, fuel levels, part list, crew
- Write control inputs: throttle, attitude, staging commands (for autopilot scripts)

### What Lua cannot do

- Access ECS internals directly
- Allocate Rapier bodies directly
- Call wgpu or any rendering API
- Block the main thread — all Lua runs in a sandboxed context with a tick budget

### Tick budget

Every Lua script gets **1ms per game tick** of CPU time, enforced by the mlua instruction count hook. Scripts that exceed their budget are suspended and log a warning. Scripts that repeatedly exceed it (default threshold: 10 consecutive ticks) are marked `LuaScriptStalled` and disabled until the player dismisses a notification.

This number is provisional and will be tuned once there are real scripts to benchmark against. The mechanism is not provisional — it exists from day one.

Scripts can yield explicitly (`coroutine.yield()`) to spread expensive work across ticks. The SDK documentation explains this pattern.

### Versioning

The SDK has a semantic version independent of the engine version. Mods declare which SDK version they target. The engine maintains backwards compatibility within a major version. Breaking changes bump the major version and come with a migration guide in `sdk/CHANGELOG.md`.

Any PR that changes a Lua-visible API surface must include a migration note in that file.

### Part definition example (what a modder writes)

```lua
part {
  id = "kraken.engine.spark",
  display_name = "Spark Engine",
  
  geometry = "assets/parts/engine_spark.glb",
  
  mass = 0.13,  -- tonnes, dry
  
  attach_nodes = {
    bottom = {
      position         = vec3(0, -0.5, 0),
      size             = 1,
      tensile_strength = 45.0,  -- kN before joint breaks under pull
      shear_strength   = 30.0,  -- kN before joint breaks under shear
    },
  },
  
  modules = {
    engine {
      thrust      = 20,    -- kN, vacuum
      isp_vac     = 320,   -- s
      isp_sl      = 265,   -- s
      propellants = { RP1 = 0.3, LOX = 0.7 },
    },
  },
}
```

This compiles into ECS components at load time. The Lua file is never re-parsed at runtime.

---

## Physics Design

### Fixed timestep is non-negotiable

Rapier runs at a fixed timestep. Required for deterministic physics, which is required for multiplayer and for consistent behavior regardless of framerate. Rendering interpolates between physics ticks.

Default tick rate: **50 Hz** (0.02s). Configurable per save, never dynamic per-frame.

### Joints and part connections

A vessel is a tree of parts connected by joints. Joints live in Rapier. When a part decouples, the joint is destroyed. When a vessel splits, it becomes two vessels with updated `VesselId` components on affected parts.

Joint force limits drive structural failures — not game logic deciding to explode things. Physics reports exceedance. The joints system responds. Never repeat the KSP1 pattern of `Part.cs` managing its own joint lifecycle.

### Krakensbane (origin shifting)

The rendering origin must be periodically shifted to keep the active vessel near world origin. f32 precision degrades past ~10,000 units from origin — irrelevant at human scale, fatal at planetary scale.

- The `krakensbane` system shifts the world origin when the active vessel exceeds a threshold (default: 10,000 units from current origin)
- All `SimPosition` components are relative to the current `WorldOrigin` resource
- `WorldOrigin` is tracked as an f64 solar-inertial position
- The shift is applied to all `SimPosition` components in one system pass — nothing else needs to know it happened

Implement this in Phase 1, not as a later patch.

### On-rails vs physics

Vessels not being actively controlled switch to on-rails mode. On-rails vessels are not simulated by Rapier — positions are computed analytically from orbital elements. This is how a save can have hundreds of vessels without CPU meltdown.

**Transition to on-rails** (all conditions must be true):
- Not the active vessel
- Not in atmosphere (above the body's atmosphere ceiling)
- No active engines firing
- No ongoing structural events (mid-destruction vessels stay in physics)
- Not within physics range of any active vessel (default: 2.5km)

**Transition back to physics:**
- Player switches to it
- Enters atmosphere
- Comes within physics range of the active vessel
- A forced event occurs (docking approach, debris proximity)

**Physics range** is a configurable resource (default 2.5km from the active vessel). Any on-rails vessel within this range is forced back to physics. Checked every physics tick. This prevents vessels passing through each other because one was on-rails.

**SOI boundary crossing while on-rails:** The `soi` system detects the crossing during orbital propagation, fires a `SoiTransition` event, converts state vectors to the new reference frame, and recomputes orbital elements. The vessel entity gets updated `OrbitalElements` components. Nothing else changes.

Systems that touch Rapier bodies check for the absence of `OnRails` before doing anything.

---

## Orbital Mechanics

Use standard Keplerian elements: semi-major axis, eccentricity, inclination, longitude of ascending node, argument of periapsis, mean anomaly at epoch. Expose convenient computed properties (periapsis altitude, apoapsis altitude, period, time to nodes, etc.).

`Orbit` holds f64 throughout. No f32 in orbital calculations. A planet's radius is ~6,000,000m. f32 gives 7 significant digits. You have one left. f64 gives 15. This is not optional.

SOI transitions are detected by checking vessel position against SOI radius on every physics tick while on-rails. On transition, convert state vectors to the new parent body's reference frame and recompute orbital elements.

Initial implementation: patched conics (KSP1-style). True n-body is a stretch goal.

**Tests are required** for everything in `orbital/`. Unit test against real ephemeris data or hand-calculated results. These tests run on CI across all supported Rust versions. Orbital math is easy to get subtly wrong in ways that only show up on the wrong side of a planet.

---

## Terrain System

KSP1's `PQS.cs` is 3,645 lines. The goal is a clean, async quadtree system with no synchronous stalls.

### Chunk lifecycle

Each body's surface is divided into a quadtree of chunks. Lifecycle owned by `chunk_lifecycle.rs`, driven by distance from the nearest vessel:

- **LOD 0 (farthest):** low-poly mesh, no collider, visible from orbit only
- **LOD 1–3:** increasing detail, still no collider, mid-altitude approach
- **LOD 4 (nearest, ~2km range):** highest visual detail, collider generated at ~1/4 vertex density

Chunk mesh generation is **async** — it never blocks the main thread. Chunks are requested via a channel, results applied in `PostUpdate`. A not-yet-generated chunk renders as flat placeholder. No pop-in stall.

Colliders are generated only for LOD 4 chunks, also async. A vessel cannot land on a chunk without a collider — held slightly above surface until ready (rare, descent is slow).

**Floating point:** terrain vertices are always generated relative to `WorldOrigin`. Heightmap sampling uses body-fixed f64, produces f64 height, vertex positions computed relative to origin, cast to f32 only at GPU upload.

### Procedural heightmap

Noise parameters defined in Lua per body. The terrain system exposes a Lua API for composing noise layers (simplex, ridged, domain-warped, etc.). Planets are to be defined in Lua, not hardcoded in Rust.

---

## Atmosphere

Parameters defined in Lua per body:

```lua
Atmosphere {
  height            = 70000,
  pressure_curve    = { ... },
  temperature_curve = { ... },
  composition       = "oxygen",
  scattering = {
    rayleigh_coeff = vec3(5.8e-6, 13.5e-6, 33.1e-6),
    mie_coeff      = 21e-6,
  },
}
```

Aerodynamic drag is computed per-part: cross-sectional area × drag coefficient × dynamic pressure, not CFD. KSP1's drag model is acceptable reference behavior;  drag in KrakenSP should feel similar, but be more stable and cheaper to compute.

The drag computation reads `DragSurface` components and writes to `PendingForces`. The aerodynamics system and the physics system are decoupled by this component.

---

## Rendering Pipeline

Bevy's render graph handles the pipeline. Custom render nodes are Phase 4+. Use placeholder materials until the game is playable.

Planned custom render nodes (not before Phase 4):
- Atmosphere scattering (Rayleigh + Mie, single-scattering approximation initially)
- Terrain with triplanar texturing
- Part instancing (many identical parts in one draw call)
- Depth of field and bloom (free from Bevy, just enable them)

The Lua API will eventually expose hooks for custom shaders. Not initially.

---

## Saving and Loading

Save files are TOML. Human-readable, diffable, supported by the `toml` crate. No binary formats, no XML, no ConfigNode.

A save file contains:
- Save metadata (name, timestamp, career state, SDK version the save was created with)
- All vessel records: part tree, resource levels, orbital elements or position/velocity, crew
- All dynamic celestial body states

**Vessel part trees serialize as a flat list of parts with parent IDs, not a recursive structure.** Flat lists diff cleanly and avoid stack overflows on huge vessels.

```toml
[[vessel]]
id   = "vessel-uuid-here"
name = "Kraken I"
root_part = "part-0"

[[vessels.parts]]
id          = "part-0"
part_id     = "kraken.engine.spark"
parent      = ""
attach_node = ""
position    = [0.0, 0.0, 0.0]
rotation    = [0.0, 0.0, 0.0, 1.0]

[[vessels.parts]]
id          = "part-1"
part_id     = "kraken.tank.small.shortest"
parent      = "part-0"
attach_node = "top"
resources   = { LiquidFuel = 45.0, Oxidizer = 55.0 }
```

**Versioning:** Save files include the SDK version. Loading an old save runs a migration chain. Migration functions live in `src/save/migrations/`. There must be a migration function for every version increment — no skipping.

**Unknown parts:** If a part ID no longer exists (mod removed), the part loads as an `UnknownPart` placeholder entity rather than crashing. The player is notified. The vessel loads intact minus the missing part and its subtrees (if any).

---

## What NOT to Do

Lessons from KSP1's source code, documented so they don't get repeated.

**No god classes.** KSP1's `Part.cs` is 9,393 lines. If a single file is approaching 1,000 lines, it needs splitting. A component holds data for one concern. A system handles one behavior. The word "and" in a file's description means it should be two files.

**No empty virtual methods.** KSP1 has ~50 lifecycle methods defined as empty virtuals on the base Part class, most of which are never overridden. In Bevy, lifecycle hooks are events. Systems subscribe to the events they care about. There is no base class with methods nobody calls.

**No mixed naming conventions.** `snake_case` for everything. If you find yourself typing `onFlightStart` or `GetOrbitalStateVectorsAtUT` you are in the wrong headspace.

**American english is preferred.** *meter* over *metre*, *centered* over *centred*, and so on.

**All values in metric.** Conversions should not need any value other than a power of 10. Non-SI metric values are fine.

**No `out` parameters.** This is Rust. Return `Option<T>` or `Result<T, E>`. A function that can fail says so in its signature.

**No string-keyed event lookup.** KSP1 does `Events["AimCamera"]` throughout. Events in Bevy are typed. If you're using a string to find something at runtime that should be a type at compile time, you're holding it wrong.

**No polling where change detection works.** Don't run a system every frame to check if something changed. Use `Changed<T>` or events.

**No f32 in orbital mechanics or simulation positions.** f32 at planetary scale leaves one significant digit of precision. Use f64. See the Coordinate Systems section.

**No fast-math compiler flags.** They break IEEE 754 determinism. Banned. See the Determinism section.

**No loading everything into memory at startup.** KSP1 loads all textures for all parts on game start. This is why it uses 26GB with mods. Stream assets. Load what's needed. Unload what isn't.

**No direct Rapier calls outside `physics/`.** Rapier handles are internal to the physics module. Everything else communicates through components and events. If you're importing `rapier3d` outside of `src/physics/`, stop.

**No `unwrap()` in systems.** Use the error handling patterns in the Error Handling section. `.unwrap()` is a panic waiting for edge-case input. Use `.expect("descriptive message")` at absolute minimum, proper error handling where it matters.

**No iTween.** This one should be obvious but needs to be on the list for spiritual reasons.

---

## Development Phases

### Phase 0 — Foundation (complete)
- [ ] Document KSP1 source behavior (ongoing)
- [x] Set up Bevy project skeleton
- [x] Basic Rapier integration, one rigid body falls under gravity
- [x] Basic camera
- [x] wgpu rendering pipeline confirmed working (Metal on Apple Silicon, no platform-specific code)
- [x] CI set up (fmt, clippy, tests must pass to merge)
- [x] Coordinate system types established (`SimPosition`, `WorldOrigin`, `LocalOrigin`)
- [x] `render_sync.rs` stub exists and is the only f64→f32 conversion point

**Exit criteria:** A sphere falls under gravity and renders. CI is green.

### Phase 1 — A Rocket Goes Up (in progress)
- [x] Load a vessel from part definitions — from Lua, not a hardcoded Rust struct
- [x] Part entity assembly system (definition → ECS components)
- [x] Part joint system (vessel as connected rigid bodies in Rapier)
- [x] `PendingForces` component + Rapier application system
- [x] Thrust force application
- [x] Basic atmosphere drag (flat model, no curves yet)
- [x] Gravity (point mass, single body)
- [x] Staging (decouple a joint, split into two vessels)
- [x] Krakensbane origin shifting
- [x] Camera follows active vessel
- [x] Basic placeholder UI (altitude, velocity, throttle)
- [x] Crash detection (impact velocity → vessel destruction; parts survive as debris)
- [x] Basic resource flow system — simplified: per-fuel-group, not per-node crossfeed

**Exit criteria:** A hardcoded multi-stage rocket can launch, reach space, and stage. No f32 precision artifacts visible.

### Phase 2 — Kraken SDK Online
- [ ] mlua integration in Bevy
- [ ] Part definition loader (Lua → ECS components at startup)
- [ ] Planet/body definition loader (Lua → CelestialBody at startup)
- [ ] Atmosphere parameters from Lua
- [ ] Lua tick budget enforcement (instruction count hook)
- [ ] Kraken SDK v0.1 documented (internal)
- [ ] Base game content migrated to Lua (all parts defined in .lua files)
- [ ] Hot-reload SDK files in dev mode

**Exit criteria:** Adding a new part requires zero Rust. The base game's parts are all Lua.

### Phase 3 — A Proper Solar System
- [ ] Multiple celestial bodies with real orbital relationships
- [ ] Analytical orbital propagation (on-rails)
- [ ] SOI transitions
- [ ] On-rails ↔ physics transitions with physics-range enforcement
- [ ] Time warp (runs orbital propagation at N× speed)
- [ ] Terrain quadtree with async LOD and chunk lifecycle
- [ ] Terrain colliders for landing (LOD 4 only, async)
- [ ] Procedural heightmaps from Lua parameters
- [ ] Atmosphere scattering shader (simple Rayleigh)
- [ ] Basic map view (2D orbital view)

**Exit criteria:** A vessel can launch from the starting celestial body, reach its satellite, land, and come back.

### Phase 4 — The Game
- [ ] Career mode scaffolding (Lua-driven)
- [ ] Part balancing
- [ ] Science system (Lua-driven)
- [ ] Crew EVA
- [ ] Docking
- [ ] Advanced resource flow system (fuel lines, customizable crossfeed rules)
- [ ] Save/load (TOML, migration chain, unknown part handling)
- [ ] Settings and input binding
- [ ] VAB/SPH equivalent editor
- [ ] Custom render nodes (atmosphere scattering, terrain, part instancing)

**Exit criteria:** Playable. A player can start a career, do missions, advance, and save/load.

### Phase 5 — Multiplayer
- [ ] Networking architecture decision (server-authoritative vs lockstep — decide before any code)
- [ ] Determinism audit (no HashMap iteration in physics, seeded RNG resources, no frame-rate-dependent calculations)
- [ ] Network layer (Quinn/QUIC or Bevy Replicon, based on architecture decision)
- [ ] Server runs simulation in f64 only — no origin shifting server-side
- [ ] Clients maintain independent `LocalOrigin` for their own render worlds
- [ ] `DVec3` positions only over the wire — no `Vec3`
- [ ] Vessel ownership and authority handoff
- [ ] Shared universe (all players in same solar system simultaneously)

**Exit criteria:** Two players can fly in the same solar system simultaneously.

### Phase 6+ — Community Driven
The Kraken SDK is mature enough that the community drives content. Internal dev focuses on engine improvements, performance, and graduating good mods into the base game.

---

## Contributor Guidelines

**Read the KSP1 documentation first.** Before implementing any system, check if there's a corresponding KSP1 analysis document. It contains known anti-patterns and behavioral requirements.

**One system, one file** (roughly). Systems that are hundreds of lines probably need splitting. Systems under 50 lines doing one thing are fine in a shared `systems.rs` within their module.

**No unsafe unless there's no other way.** Justify it in the PR. There are maybe five legitimate uses in this entire codebase.

**Tests for orbital mechanics.** Required for anything in `orbital/`. Unit tests with known solutions — real ephemeris data or hand-calculated results. These run on CI.

**Lua API changes need a migration note** in `sdk/CHANGELOG.md`. Modders are people, not compilers.

**Performance regressions block merging.** Before and after benchmarks for any change to physics systems, terrain generation, or asset loading. The point of this project is that it's fast.

**The tech debt list.** If you consciously cut a corner, add it to `CHECKLIST.md` under "Known Tech Debt" immediately. Future contributors will not remember it was intentional and will waste time wondering if it's a bug.

---

## Performance Targets

Targets, not guarantees for v1.0. Every architectural decision should be made with these in mind.

| Scenario | Target | KSP1 baseline |
|---|---|---|
| 300-part vessel, full physics | 60fps solid | ~15–30fps on good hardware |
| 10 vessels simultaneously active | No significant fps drop | Basically impossible |
| Base game memory usage | < 500MB | ~4GB |
| 150-mod install memory usage | < 2GB | 26GB |
| Load time (base game) | < 10 seconds | 3–5 minutes heavily modded |
| Physics tick rate | 50 Hz stable | Variable, GC-dependent |
| Terrain chunk generation | < 16ms async, never blocks frame | Synchronous, causes hitches |

---

## The Kraken Easter Egg

It exists. It is in the codebase. Parts of the code responsible for it are obfuscated and marked `// DO NOT TOUCH — this is load-bearing superstition`. Contributors should not investigate, modify, or remove these sections. They do not interact with any other system. They do not need to be understood to contribute.

If you think you found a bug that's actually the Kraken, you didn't find a bug.

---

## The Silence in Deep Space

There is something in the code. It has always been there. It does not interact with anything you will touch. Leave it alone.

Some things should not be understood. Some bugs are not bugs.