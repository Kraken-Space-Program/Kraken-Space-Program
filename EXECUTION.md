# Kraken Space Program — Execution Document

> DESIGN.md answers "how does this system work and why."
> README.md answers "what is this project."
> This document answers "what are we actually doing right now, and what do we do next."
>
> This is a living document. It changes constantly. When you finish a task, check it off. When you cut a corner, write it down. When you make a decision, log it. If this document is stale, it's not doing its job.

---

## How to Use This Document

**Starting a session:** Read the "Currently Active" section. Pick something. Do it.

**Finishing a task:** Check it off. If it uncovered new tasks, add them. If it required cutting a corner, add the corner to "Known Tech Debt" immediately — not later, now.

**Stuck on something:** Move it to "Blocked / Needs Decision" with a note about what's blocking it.

**Making an architectural decision:** Log it in "Decision Log" with a date and the reasoning. Future contributors should not have to reverse-engineer why things are the way they are.

**Something's broken and you're not sure if it's a bug:** Check "Known Tech Debt" first. It might be intentional. It might be the Kraken. Leave the Kraken alone.

---

## Build & Run

```bash
# Prerequisites
rustup update stable
cargo install cargo-watch  # optional, for hot reload during dev

# Run
cargo run

# Run with fast dev compilation (dynamic linking, much faster incremental builds)
cargo run --features bevy/dynamic_linking

# Tests
cargo test

# Lints (must pass before any PR)
cargo fmt --check
cargo clippy -- -D warnings
```

CI runs `fmt`, `clippy`, and `test` on every push. A red CI blocks merging. No exceptions.

---

## Currently Active — Phase 0

These are the things being worked on right now. If you're picking up a task, note it somewhere visible (GitHub issue, Discord, whatever the team is using) so two people don't do the same thing.

### Foundation
- [x] Bevy project skeleton — `main.rs`, `DefaultPlugins`, window opens, nothing crashes
- [x] Coordinate system types established:
  - [x] `SimPosition(DVec3)` component
  - [x] `WorldOrigin` resource (f64)
  - [x] `LocalOrigin` resource (f64, near active vessel, render use only)
  - [x] `render_sync.rs` stub with the f64→f32 conversion — even if it does nothing yet, the file exists and is the only place this conversion will ever happen
- [x] Basic Rapier integration: a sphere with a `RigidBody` falls under a point gravity field
- [x] Basic camera: orbits the scene, doesn't clip through origin
- [x] CI pipeline: GitHub Actions, runs `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- [x] `CHECKLIST.md` exists and has the "Known Tech Debt" section ready to be filled

### (Phase 0 backlog)
- [ ] wgpu pipeline confirmation — Bevy's default renderer is working, custom wgpu node can be registered (smoke test only, no custom shaders yet)
- [x] Rapier fixed timestep confirmed — physics running at 50 Hz, rendering interpolating between ticks (`TimestepMode::Interpolated` + `TransformInterpolation` on dynamic bodies)
- [x] Module directory structure matches DESIGN.md — even if most files are just `mod.rs` stubs

---

## Up Next — Phase 1 Prep

Don't start these until Phase 0 exit criteria are met (a sphere falls under gravity, CI is green). They're here so you can think about them in the background.

### Things to figure out before writing Phase 1 code

- [ ] Decide on the part definition format Rust-side — what struct does a Lua part definition compile into at load time? Sketch it out before Phase 2 forces the answer.
- [ ] Decide how `PendingForces` gets cleared — before or after Rapier step? What happens if two systems write to the same entity's forces in the same tick? Document the answer.
- [ ] Decide on vessel splitting behavior — when a joint breaks and a vessel becomes two, what's the exact sequence of events? Who fires what? Map it out so Phase 1 staging code doesn't get it wrong.
- [ ] Decide on the `WorldOrigin` shift threshold — 10,000 units is the current default in DESIGN.md. Is that right for our solar system scale? Check it before Krakensbane gets implemented.

### Phase 1 task breakdown (rough order)

- [ ] `Part` entity spawning from a hardcoded Rust struct (no Lua yet)
- [ ] Part-to-part joint creation via the `joints` system
- [ ] `PendingForces` component + Rapier force application system
- [ ] Point gravity (single body, inverse square)
- [ ] Thrust: `Engine` component reads throttle input, writes to `PendingForces`
- [ ] Basic drag: `DragSurface` component, flat drag coefficient, writes to `PendingForces`
- [ ] Staging: `StageActivated` event → `Decoupler` query → joint destruction → `VesselSplit` event
- [ ] `VesselSplit` handler: reassign `VesselId` on affected parts, spawn new vessel entity
- [ ] Krakensbane: `WorldOrigin` shift when active vessel exceeds threshold
- [ ] Camera: follows `ActiveVessel`, basic orbital camera controls
- [ ] Crash detection: `CollisionEvent` from Rapier → check impact velocity → `PartDestroyed` event
- [ ] Placeholder HUD: altitude, velocity, throttle — hardcoded positions, no Lua yet

---

## Blocked / Needs Decision

Things that can't move forward until a call is made. If you're unblocking one of these, move it out of here and into the appropriate section, and log the decision below.

### Networking architecture (Phase 5)
**What's needed:** A call on server-authoritative vs lockstep. This affects every physics system written between now and Phase 5 — specifically, how much we care about strict determinism (HashMap ordering, RNG seeding) in systems that don't need to be multiplayer-ready yet but need to not be rewritten when they do.

**Current stance:** Write physics code as if lockstep might be chosen (no HashMap iteration in simulation systems, seeded RNG as a resource). This is the conservative option — it costs almost nothing now and saves a potential audit pass later.

**Decision needed by:** Before any Phase 3 code is written. The on-rails system particularly depends on this.

### N-body vs patched conics (long-term)
**What's needed:** The README promises n-body as stock. DESIGN.md says patched conics for initial implementation. These are not in conflict — conics first, n-body later — but "later" needs a phase assignment so it doesn't become "never."

**Current stance:** Patched conics through Phase 3. N-body is a Phase 4/5 consideration. Open question: is n-body required for the Phase 4 "playable" exit criteria, or is it Phase 6+ community-driven?

### Combat and galaxy (long-term scope)
**What's needed:** The README lists combat and procedural star systems as goals. Neither is in any phase in DESIGN.md. These need either a phase assignment or an explicit "this is a post-v1 community feature" decision, because they affect whether the Lua API needs to expose weapon/damage hooks and whether the celestial body system needs to handle multi-star configurations.

---

## Decision Log

Decisions made, with dates and reasoning. If you're wondering why something is the way it is, check here before asking.

### [DATE TBD] Save format: TOML
**Decision:** TOML for all save files.
**Alternatives considered:** Custom format, MessagePack (binary), JSON.
**Reasoning:** TOML is human-readable (players can hand-edit saves), diffable (version control friendly), and has first-class Rust support via the `toml` crate. The only reason to go binary is performance, and save files are read/written infrequently enough that this doesn't matter. Custom formats are maintenance burden with no upside.

### [DATE TBD] Lua tick budget: 1ms per tick
**Decision:** 1ms CPU time per Lua script per game tick, enforced by mlua instruction count hook.
**Alternatives considered:** No budget (trust modders), coroutine-only (cooperative, not preemptive).
**Reasoning:** No budget means one bad mod tanks everyone's framerate. Cooperative-only means one infinite loop hangs the game. 1ms is generous for most scripts and can be tuned once real benchmarks exist. The mechanism (instruction count hook) is the non-negotiable part; the number is provisional.

### [DATE TBD] Part modules as components, not objects
**Decision:** An engine part has an `Engine` component. A tank has a `ResourceContainer` component. There is no `PartModule` base class.
**Alternatives considered:** KSP1-style virtual dispatch with a `PartModule` trait, enum-based module types.
**Reasoning:** KSP1's virtual dispatch approach is the direct cause of ~50 empty lifecycle methods that nobody overrides. Bevy's ECS already gives us composability for free — a part is just an entity with whatever components describe it. Systems query for the components they care about. No base class, no virtual dispatch, no empty methods. Adding a new module type means adding a component and a system, not inheriting from anything.

### [DATE TBD] Coordinate system: four spaces, one conversion point
**Decision:** Solar Inertial (f64) → Body-Fixed (f64) → Simulation World (f64) → Render World (f32). Only `render_sync.rs` crosses the f64→f32 boundary.
**Alternatives considered:** KSP1-style single coordinate space with precision hacks, double-precision rendering (wgpu doesn't support this natively).
**Reasoning:** Floating point precision issues at planetary scale are not optional to solve — they must be solved. The cleanest solution is keeping all simulation in f64 and only converting to f32 at the last possible moment (render time), with the subtraction-before-cast pattern ensuring the cast is always to a small number. Making this a single-file responsibility (render_sync.rs) means the boundary can't accidentally spread.

---

## Known Tech Debt

Corners cut consciously. Every item here was intentional. Future contributors: these are not bugs, they are decisions. If you fix one, remove it from this list and note what you did in the commit message.

*This section starts empty. The first entry gets written the first time someone types "TODO: fix this later" in the codebase.*

| # | Description | Where | Cut in Phase | Should be fixed by Phase |
|---|-------------|-------|--------------|--------------------------|
| — | — | — | — | — |

---

## Recurring Checks

Things to verify periodically that aren't tied to a specific task.

**Before any PR merges:**
- `cargo fmt --check` passes
- `cargo clippy -- -D warnings` passes
- `cargo test` passes
- No new `.unwrap()` calls in systems (use `?`, `if let`, or the error event pattern)
- No f32/Vec3 positions outside `render_sync.rs`
- No Rapier imports outside `src/physics/`

**Before closing a phase:**
- All phase exit criteria are met and verifiable (not just "looks right")
- Tech debt incurred during the phase is logged above
- Any provisional decisions (numbers, thresholds, limits) are reviewed — are they still right?
- DESIGN.md reflects any architectural decisions made during the phase

**Before starting multiplayer (Phase 5):**
- Full determinism audit: no HashMap iteration in physics systems, no unseeded RNG in simulation code, no frame-rate-dependent calculations anywhere in the physics path
- Networking architecture decision made and logged
- All `SimPosition` data uses DVec3, confirmed no Vec3 leaking into network-relevant code

---

## Contributor Quick Reference

**I want to pick up a task:** Look at "Currently Active." If nothing fits your skills, look at "Up Next." If you're not sure what to work on, open a discussion.

**I finished something:** Check it off here and in any linked GitHub issue. Add any new tasks it revealed. If you cut a corner, log it in "Known Tech Debt" before you close the PR.

**I found something that seems wrong but isn't in any issue:** Check "Known Tech Debt" first. If it's not there, open an issue before fixing it — there might be a reason.

**I want to make an architectural change:** Open a discussion, not a PR. Get the decision logged here before writing code. Architecture decisions made in PRs without discussion tend to get reverted.

**I want to add a new part module type:** Add a component in `src/part_modules/your_module.rs`. Add a system that reads it. Expose it in the Lua API via `src/sdk/api/parts.rs`. Write a test part definition in Lua. That's it — no base class, no registration, no inheritance.

**I want to add a new celestial body:** Write a Lua definition file. The body loader picks it up automatically. No Rust changes needed (after Phase 2).

**I think I found the Kraken:** You didn't.