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

### Diagnosing flight

Two of Phase 1's worst bugs were invisible at half-second logging resolution and settled
before anyone could see them. Use these before believing a physics change works:

```bash
# One log line per physics tick, for the first N seconds — or "all" for the whole session.
KRAKEN_TRACE=2 cargo run

# Fly a scripted profile so a launch or a landing is reproducible without a human at the
# keyboard. The pilot flies with the controls a player has and nothing else — including
# staging, which it requests through the same latch the space bar sets.
KRAKEN_PILOT=ascent cargo run       # straight up to space, staging on flameout (~2.5 min)
KRAKEN_PILOT=hop cargo run          # up 60 m, back down, soft landing
KRAKEN_PILOT=hop KRAKEN_PILOT_DRIFT=1 cargo run   # ...with lateral drift to cope with
KRAKEN_PILOT=ballistic cargo run    # burn to 6 km and let it fall: drag and impact damage
KRAKEN_PILOT=idle cargo run         # sit still and settle
```

The **watchdog** in `diagnostics/` needs no flag and stays on in release. If it says
something, believe it — every check in it exists because something real got past review
without it.

CI runs `fmt`, `clippy`, and `test` on every push. A red CI blocks merging. No exceptions.

**Do not try to check the Linux leg from macOS.** `cargo check --target
x86_64-unknown-linux-gnu` gets 22 crates in and stops: `wayland-sys` wants Linux system
libraries through pkg-config, and with the windowing backends disabled it stops again on
`lua-src`, which wants an `x86_64-linux-gnu-gcc` to build vendored Lua. Both are build
scripts of third-party crates, not anything this project controls, and getting past them
means installing a full cross-compilation toolchain. The ubuntu leg of CI is the check. It
costs little to be sure of that, because there is no `cfg(target_os)`, `cfg(unix)` or
`cfg(windows)` anywhere in `src/` — the only platform-specific line in the repository is the
`[patch.crates-io]` for `block`, and `block` is not in the Linux build graph at all
(`cargo tree --target x86_64-unknown-linux-gnu` confirms it, and emits no unused-patch
warning either).

---

## Currently Active — Phase 1

> Phase 0 is complete: the renderer is confirmed, the timestep is real, CI is green on Linux
> and macOS, and a rocket flies. What follows is what Phase 0 signed off on.

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
- [x] wgpu pipeline confirmation — `rendering::log_render_backend` reports the adapter at startup. Confirmed Metal on Apple Silicon (M1 Pro), no Metal-specific code required.
- [x] Rapier fixed timestep confirmed — physics runs at 50 Hz wall-clock, rendering interpolates between ticks. **This was broken and is now fixed — see the decision log.**
- [x] Module directory structure matches DESIGN.md — even if most files are just `mod.rs` stubs

---

## Up Next — Phase 1 Prep

Don't start these until Phase 0 exit criteria are met (a sphere falls under gravity, CI is green). They're here so you can think about them in the background.

### Things to figure out before writing Phase 1 code

- [x] Decide on the part definition format Rust-side — **answered:** `sdk::part_def::PartDefinition`, a plain struct loaded once at startup and turned into components by `vessel::assembly`. Lua may use engineer-friendly units (tonnes, kN); everything is normalised to SI at the boundary and is SI from then on.
- [x] Decide how `PendingForces` gets cleared — **answered: cleared at the start of the tick, before any producer runs.** See the decision log and `src/physics/forces.rs`.
- [x] Decide on vessel splitting behavior — **answered:** joint destroyed → breadth-first walk of `PartConnections` from the decoupler, refusing to cross the severed edge → everything unreachable gets a new `VesselId` and a new vessel entity with a zeroed `ControlState` → `VesselSplit` fires. See `src/vessel/staging.rs`.
- [x] Decide on the `WorldOrigin` shift threshold — **kept at 10,000 m.** At that distance an f32 ULP is about 1 mm, which is finer than any contact this engine resolves, and shifts stay rare (roughly once per 10 km of travel). Revisit if a vessel ever needs sub-millimetre precision at range — docking does not.

### Phase 1 task breakdown (rough order)

- [x] `Part` entity spawning — from Lua definitions rather than a hardcoded Rust struct. The Lua loader was pulled forward from Phase 2; only the *arrangement* of the test stack is hardcoded.
- [x] Part-to-part joint creation via the `joints` system
- [x] `PendingForces` component + Rapier force application system
- [x] Point gravity (single body, inverse square) — `celestial::body`, μ = g·r² about a 600 km sphere
- [x] Thrust: `Engine` component reads throttle input, writes to `PendingForces`
- [x] Basic drag: `DragSurface` component, exponential atmosphere, occlusion between parts, writes to `PendingForces`
- [x] Staging: `StageActivated` → `Decoupler` query → joint destruction → `VesselSplit`
- [x] `VesselSplit` handler: reassign `VesselId` on affected parts, spawn new vessel entity
- [x] Krakensbane: `WorldOrigin` shift when active vessel exceeds threshold
- [x] Camera: follows `ActiveVessel`, basic orbital camera controls
- [x] Crash detection: `CollisionEvent` from Rapier → impact speed from the tick before → vessel destroyed
- [x] Placeholder HUD: altitude, velocity, throttle — hardcoded positions, no Lua yet

### Phase 1 exit criteria — measured, 2026-08-20

DESIGN.md: *"A hardcoded multi-stage rocket can launch, reach space, and stage. No f32
precision artifacts visible."* Both halves, from one run of `KRAKEN_PILOT=ascent
KRAKEN_TRACE=all`, straight up on full throttle:

| | |
|---|---|
| Launch | 5,919 kg, 140 kN, TWR 2.41 |
| Max dynamic pressure | 27.7 kPa at 3,390 m, 288 m/s (t=22.3 s) |
| Staging | 14,101 m, on the first stage flaming out at 35.7 of 140.0 kN — 3 parts, 1,450 kg dropped |
| Peak joint load | 23% of limit, on the pad while settling. 22% in flight, at the staging tick |
| Origin shifts | 7, at 10 km intervals from the pad to 70 km |
| **Space** | **70,014 m at t=140.4 s, 744 m/s, 1,542 kg of propellant left** |
| After cutoff | still climbing through 83.8 km when the recording stopped |
| Watchdog | silent for the entire flight |

**On the precision half.** The seven origin shifts are the interesting part: the altitude and
speed traces are continuous across every one, tick to tick, with nothing to mark the frame
the whole world moved 10 km. That is now pinned by tests in `physics/krakensbane.rs` as well
as observed — and writing them corrected a claim this repo had been making. The module said
a shift moves nothing at all. It moves the *active vessel* nothing at all, exactly, because
the shift is taken about the mean of that vessel's own parts and subtracting two nearby f32
values never rounds. A body far from that centre does move, by up to half an f32 ULP —
0.4 mm measured at 12.6 km. Below anything this engine resolves, but a bound rather than
nothing, and worth stating honestly.

**Still labelled "in progress".** Every task box above is ticked and the exit criterion is
measured, so what is left is the ritual in "Before closing a phase" further down — chiefly
the review of provisional numbers, of which this phase accumulated a lot: `SAFE_IMPACT_SPEED_MS`,
`TICKS_OVER_LIMIT_BEFORE_FAILURE`, `SAS_SETTLE_SECS`, `ORIGIN_SHIFT_THRESHOLD_M`, the stock
parts' strengths, and the Reliant's entire balance (CHECKLIST #9). Stamping the phase closed
is that review, not this flight.

**What the flight found.** Every engine on a vessel burns whenever the throttle is open;
nothing ties ignition to a stage. The upper stage's Spark therefore runs from the pad,
burning its own propellant to lift the first stage, and "stage when thrust reaches zero"
never fires because the upper engine is still producing 20 kN. Logged as CHECKLIST #26; the
ascent profile stages on *partial* thrust loss instead, which is correct regardless.

The per-tick trace's `joint=` column was also reporting the worst joint anywhere in the
world, so the spent booster hitting the ground at 89.6 m/s read as the vessel 106 km
overhead carrying 308% of its breaking load. `PeakJointLoad` now carries both figures: the
world-wide peak for the watchdog, which should shout about any vessel, and the active
vessel's own for the trace, which claims to be about the vessel you are flying.

Pulled forward from Phase 2 because the part definitions already existed and hardcoding
them in Rust would have meant writing content twice:

- [x] mlua integration, sandboxed VM, instruction-count budget
- [x] Part definition loader (Lua → `PartRegistry` at startup)

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

### [2026-08-20] Staging is an input, not a keystroke
**Decision:** `ControlState` carries a one-shot `stage` latch. `staging::request_stage` fires on the space bar *or* that latch, clears it, and is still the only thing that writes `StageActivated` or increments `CurrentStage`.

**Why:** `StageActivated` used to be written straight from `keys.just_pressed(Space)`, which meant staging was the one action reachable only by a human hand. The scripted pilot — the fixture that exists precisely so flight behaviour can be verified without one — could not stage, so every automated profile ever flown was a single stage under 6 km. Phase 1's exit criterion is a *multi-stage* rocket reaching space, and it could not be tested by the thing built to test flights.

The latch puts staging where the throttle and the control axes already are: input, latched in one place, consumed in one place. The pilot's contract stays literally true — it writes `ControlState` and nothing else — and there is no second path to a stage event to keep in sync.

**Not a message from the pilot:** the obvious alternative is to let the pilot write `StageActivated` itself. That duplicates the `CurrentStage` increment, and a vessel that increments twice for one press skips a stage — the kind of bug that surfaces on the one flight nobody was watching.

### [2026-08-19] `block` is vendored and patched, because there is nowhere else to fix it
**Decision:** `third_party/block` holds `block` 0.1.6 with one line changed, and the root `Cargo.toml` points `[patch.crates-io]` at it.

**Why:** every macOS build printed "the following packages contain code that will be rejected by a future version of Rust: block v0.1.6". The crate is nothing to do with us — it arrives as `bevy → wgpu → wgpu-hal → metal → block` — but the lint is not cosmetic: it declares `static _NSConcreteStackBlock` with the uninhabited type `enum Class {}`, and when rust-lang/rust#74840 becomes a hard error that crate stops compiling and takes the Metal backend, and therefore the game on macOS, with it.

**Why not upstream:** 0.1.6 is the last release and it is from 2016. `metal` 0.32 is what `wgpu-hal` 27 pins and Bevy 0.18 pins that wgpu, so there is no version to move to and no maintainer to move it. The fix is an inhabited zero-sized `#[repr(C)] struct Class` — only the static's address is ever taken, as a stack block's `isa` pointer, so nothing about the behaviour changes.

**Cost:** ~350 lines of MIT code we now carry, and the lint cap in that crate's manifest (path dependencies do not get `--cap-lints allow`, so its 2016-era `extern fn` declarations would otherwise add 46 warnings of their own). The removal signal is automatic: when wgpu's Metal backend moves to `objc2`/`block2`, cargo warns that the patch is unused. Delete the directory and the `[patch]` section then.

### [2026-08-19] Attitude authority comes from parts, not from a constant
**Decision:** `ReactionWheel` is a part module. A vessel's control torque is the sum of its wheels; each applies its own share; a vessel with none cannot steer.

**Why:** the constant it replaced gave every vessel identical authority regardless of what it was built from — a probe and a fuelled launch stack turned at the same rate — and it was sized by hand against the one test rocket, so it had to be retuned by a factor of ten the first time that rocket changed. It would have needed retuning again for every rocket after that.

**The consequence is deliberate:** stage away the probe core and the spent booster stops responding to input. That is correct, and it is the kind of thing that only becomes true once authority is a property of the hardware.

**Still a simplification:** wheels never saturate, need no electricity and have no gimbal to fall back on. Logged as CHECKLIST #24 and #25.

### [2026-08-19] Per-tick tracing and a flight watchdog are permanent, not ad hoc
**Decision:** `diagnostics/` ships an always-on watchdog plus `KRAKEN_TRACE=<seconds>|all` for one log line per physics tick.

**Why:** two of the three worst bugs in Phase 1 were invisible at the resolution anyone was looking at. A stationary rocket on the pad bounced at 9.5 m/s and loaded its joints to nine times breaking strength for a third of a second — every 0.5 s log line showed a vessel sitting perfectly still. SAS spun a motionless vessel to a *constant* 0.68 rad/s, which reads as "not changing" in any summary. Both were found by hand-writing a throwaway per-tick logger, twice, after already suspecting something.

The watchdog is the half that matters: it fires without anyone suspecting anything first. Each check exists because something real got past review without it — non-finite state, terrain tunnelling, SAS failing to converge, absurd velocity, and (in `joints.rs`) a joint above 70% of its breaking load. They are ordinary comparisons on data already in memory, so they stay on in release.

**Verified by breaking it on purpose:** with `SAS_SETTLE_SECS` set to an unstable 0.004, the watchdog reported "SAS on 'Test Stack' has not converged in 3 s — still rotating at 0.068 rad/s with no input". A check that has never been seen to fire is not a check.

### [2026-08-19] Gravity is a force producer, not Rapier's `gravity` setting
**Decision:** `RapierConfiguration::gravity` is `Vec3::ZERO`. `celestial::body::apply_gravity` writes `m·μ/r²` into `PendingForces` alongside thrust and drag.

**Why:** Rapier's gravity is one uniform vector. Gravity in this game is none of those things — it falls off with altitude, points at a body's centre rather than straight down, and will eventually come from more than one body at once. Producing it as a force costs one multiply per part and keeps all three of those changes inside one file. Anyone re-enabling Rapier's own gravity will silently double what every vessel feels.

**The body is centred below the launch site,** at `(0, -radius, 0)`, so its surface passes through the origin. The flat ground plane, the camera and Krakensbane all assume the surface is at `y = 0`, and this makes point-mass gravity work without changing any of them. The curvature is real but unobservable: over the 400 m pad the surface drops 0.13 mm.

### [2026-08-19] Part colliders stop 2 cm short of their attach nodes
**Decision:** `COLLIDER_GAP_M` shortens every part's collider so neighbouring parts in a stack never touch.

**What was wrong:** parts are stacked so one's bottom node sits exactly on the next one's top node. With colliders reaching all the way to those nodes, every neighbouring pair was in permanent exact contact — and the contact solver was pushing them apart at the same moment the joint was holding them together. The two fought, and the fight injected energy: a stationary five-part stack on the launch pad bounced at up to **9.5 m/s** for the first third of a second and momentarily loaded its joints to **nine times** their breaking strength.

**Why it went unnoticed for so long:** it settles within about 0.3 s, and nothing logged at finer resolution than 0.5 s. It was found only because structural failure went in and immediately disassembled the rocket on the pad. The joint is what holds parts at the right distance; the collider never needed to.

### [2026-08-19] Structural failure needs a *sustained* overload
**Decision:** a joint breaks after `TICKS_OVER_LIMIT_BEFORE_FAILURE` (3) consecutive ticks over its limit, not on the first.

**Why:** a rigid contact at 50 Hz arrests a vessel inside one step, and the impulse that takes is enormous however gently it arrived — a 2.4 t upper stage touching down at **1.8 m/s** stops in 20 ms, which is 9 g, which is 217 kN through a joint rated for 67. The first version broke on that, which made every landing fatal regardless of how well it was flown. A genuine structural overload lasts as long as its cause; 60 ms is longer than any contact transient and short enough to still fail promptly.

### [2026-08-19] SAS sizes its torque from measured inertia, not a tuned gain
**Decision:** `torque = I·ω / SAS_SETTLE_SECS`, where `I` comes from `vessel_inertia()` summing every part's own inertia plus its `m·d²` offset from the vessel's centre of mass. Clamped to `ATTITUDE_TORQUE_NM`, with a small deadband.

**What was tried first:** the obvious `torque = -gain · ω`, with `gain` picked to look reasonable. It made the problem it was meant to solve. The vessel SAS was holding *spun itself up* to 0.68 rad/s and reversed direction every two seconds, sitting on the pad, doing nothing else.

**Why:** the angular velocity SAS reads is one tick old. If the commanded torque can carry the rotation past zero inside that tick, the next correction points the other way — and once the correction saturates against `ATTITUDE_TORQUE_NM` the controller is bang-bang with a one-tick delay, which has no stable fixed point. Whether any particular gain is stable depends on the vessel's moment of inertia, so no constant is right for more than one rocket. `I·ω/T` removes the dependency: `dt/T` is 0.02 against an instability threshold of 2, a margin wide enough that the inertia estimate can be off by an order of magnitude and still settle.

**The second half of the same bug:** the first working version read the inertia off the *root part* alone. That is 20× too small for a five-part stack, because the parallel-axis terms are most of it. SAS then reported that it was correcting while the rocket slowly tipped over across a two-minute coast — the torque was real, it was just nowhere near enough. `vessel_inertia()` sums the whole vessel.

**Consequence:** `ATTITUDE_TORQUE_NM` dropped from 30 kN·m to 3 kN·m in the same pass. 30 kN·m was not control authority, it was a weapon: one second of full deflection reached 11 rad/s, and the joint solver then came apart entirely — the stack hit 847 rad/s before the parts scattered.

### [2026-08-19] Impact speed is read from the tick *before* the contact
**Decision:** `PreviousSimVelocity`, snapshotted by `store_previous_state` alongside the interpolation state, is what `physics::impact` measures.

**Why:** collision events cannot be read until after `PhysicsSet::Writeback`, and by then Rapier has resolved the contact — the part's velocity is what survived the impact, not what caused it. For the case that matters most, a rocket arriving at the pad, the harder the landing the less of it is left to measure. A 230 m/s arrival would have been recorded as a gentle touchdown.

**Also decided here:** a contact between two parts of the *same* vessel is not an impact. The joint solver keeps neighbouring parts permanently in contact and every attitude input grinds them together; the first version of this destroyed the rocket in mid-air the moment it was asked to turn. Self-inflicted damage is structural failure (CHECKLIST #1), which compares joint forces against the limits already parsed from Lua.

### [2026-08-19] A hard landing breaks the vessel apart rather than deleting parts
**Decision:** on an impact above `SAFE_IMPACT_SPEED_MS`, mark the vessel `Destroyed`, zero its controls, and remove every joint. Parts stay in the world as debris.

**Why:** despawning the part that hit has three failure modes that all cost more than the effect is worth — a despawned part leaves neighbouring `ImpulseJoint`s pointing at a dead entity, despawning the root part silently disables attitude control and blanks the HUD because both find the vessel through `RootPart`, and two contacts in one tick despawn the same entity twice. Breaking the joints has none of them, is one command per part, and looks like what it is. The cost is logged as CHECKLIST #13: debris is never cleaned up.

### [2026-08-19] Physics runs in `FixedUpdate`, not `PostUpdate`
**Decision:** `RapierPhysicsPlugin::default().in_fixed_schedule()`, with `Time::<Fixed>::from_hz(50.0)` and `TimestepMode::Fixed { dt: 1/50 }`.
**What was wrong:** `RapierPhysicsPlugin::default()` schedules stepping in `PostUpdate` — once per *rendered frame*. Combined with `TimestepMode::Fixed { dt }`, that advances the simulation by `dt` on every frame, which is not a fixed tick rate at all: it is "0.02 s of simulation per frame". On a 120 Hz display the world ran 2.4× too fast; on 60 Hz, 1.2×. The tick rate was silently the monitor's refresh rate.
**Why it matters more than it sounds:** every hand-tuned number — thrust, drag coefficients, joint strengths, reaction wheel authority — would have been tuned against whatever display the author happened to have. It also makes determinism impossible, which is a Phase 5 blocker.
**How to check it stayed fixed:** run with `RUST_LOG=kraken_space_program=debug` and watch `debug_timestep_drift`. Simulated and wall-clock time must track 1:1 at any framerate.

### [2026-08-19] Rapier is f32, so `SimPosition` is derived, not simulated
**Decision:** Rapier simulates in a **local f32 physics frame** that Krakensbane keeps near zero. `SimPosition` (f64) is derived after every tick as `WorldOrigin + transform`. The rule "only `render_sync.rs` crosses the f64→f32 boundary" is restated as: **f32 positions are legal inside `physics/` and `rendering/`; anything crossing a module boundary is f64.**
**Alternatives considered:** an f64 physics engine (does not exist for Rust, and writing one is not this project), or keeping `SimPosition` authoritative and pushing it into Rapier each tick (fights Rapier's own integration and destroys determinism).
**Reasoning:** Rapier has no f64 build and is not getting one. Something had to give, and the honest version is that f32 is fine for *contact resolution within 10 km of an origin* — which is all Rapier is ever asked to do — while f64 carries everything that leaves that box. Without writing this down, the first contributor to touch `physics/forces.rs` would either break the stated rule or try to build an f64 integrator.

### [2026-08-19] `PendingForces` is cleared at the start of the tick
**Decision:** three chained sets in `FixedUpdate`, all before `PhysicsSet::SyncBackend`: `Clear` → `Produce` → `Apply`. Producers **add**; they never assign.
**Alternatives considered:** clearing after the Rapier step.
**Reasoning:** clearing first is what makes a missed tick safe. If a producer does not run — its vessel went on rails, its run condition failed — its contribution vanishes that tick, which is correct. Clearing afterwards leaves the last value in place, so a shut-down engine keeps thrusting; that surfaces months later as "my rocket accelerates in the map view". Two producers on one part simply sum, which is what superposition says should happen.

### [2026-08-19] Visual entities are separate from physics entities
**Decision:** a part is two entities — a physics body Rapier owns, and a visual carrying `SyncRender { source }` that `render_sync` owns.
**Alternatives considered:** one entity holding both.
**Reasoning:** not a preference, a necessity. Rapier's `apply_rigid_body_user_changes` picks up `Changed<GlobalTransform>` so gameplay code can teleport bodies. An interpolated transform written onto the body entity every frame is read back by Rapier on the next tick, snapping the body to a position that is deliberately one tick stale — the renderer and the simulation fight forever. Splitting them also leaves room for Phase 4 part instancing, where the relationship stops being one-to-one anyway.

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

**Lives in [`CHECKLIST.md`](CHECKLIST.md).** It started here as an empty table waiting for
its first entry; by the time it had eighteen, the same list existed in two files and only
one of them was being kept up. One list, in the file whose entire job is to hold it.

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