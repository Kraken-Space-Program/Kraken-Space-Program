# 🦑 Kraken Space Program (KSP)

> *The space game the community actually deserved.*

**Kraken Space Program** is an open-source, community-built aerospace sandbox — a spiritual successor to the game we all loved, rebuilt from scratch the right way. Fast. Moddable. Cross-platform. With multiplayer baked in from day one.

Named after the ancient evil that has claimed more rockets than any Kármán line ever will.

---

## Why does this exist?

Because we've been waiting long enough.

KSP1 was a miracle of indie development — a beautiful, physics-accurate sandbox built by a small passionate team that accidentally became one of the greatest games ever made. It was also held together with duct tape, running on an ancient Unity version, single-threaded, and slowly abandoned.

KSP2 promised to fix all of that. It didn't. It's dead now.

So we're doing it ourselves.

![ezgif-7da0d66b26a8cfd9](https://github.com/user-attachments/assets/9fc8532a-c9d7-48d5-9fc7-6d78a3af7680)

---

## What is this, exactly?

A from-scratch reimplementation of the KSP formula, built on a modern stack:

- **Rust** core engine — no garbage collector, no runtime overhead, actual multithreading
- **Bevy** game engine — ECS architecture that maps perfectly to part-based rocket simulation
- **wgpu** rendering — native Vulkan/Metal/DirectX/WebGL, cross-platform by default
- **Lua** modding API — every gameplay system scriptable, Factorio-style
- **Built-in multiplayer** — not a mod, not an afterthought, a first-class architectural decision

---

## Features (goals)

### The basics, done right
- Rocket building, orbital mechanics, atmospheric flight — all of it, accurate and satisfying
- N-body gravity as stock
- Realistic aerodynamics as stock (FAR-inspired)
- Life support as stock
- A solar system that is actually large

### The things KSP never delivered
- **Native multiplayer** — fly together, race, collaborate, destroy each other's rockets
- **Combat** — BD Armory-style weapons and vehicles built into the base game
- **A galaxy** — procedurally generated star systems, not just one tiny solar system
- **Performance** — 1000 parts without a slideshow. Multiple vessels on screen. Actual frames.

### The modding ecosystem KSP deserved
- Full Lua API — write your entire mod in Lua, no Rust required
- Stable API contracts — your mod doesn't break every update
- Open invitation — if your mod is good enough, it becomes stock

### Platform support
- Linux, Windows, macOS — all first-class, not afterthoughts
- No Unity, no .NET runtime, no garbage collector pausing your launch window

---

## The philosophy

**This project belongs to the community.**

There is no corporation here. There is no roadmap that gets abandoned. There is no early access where you pay $50 for a broken promise. The code is here, the issues are here, the decisions are made in public.

If the original contributors disappear, someone forks it and continues. That's the point.

Mod authors: your work doesn't have to live as a patch on top of a broken engine anymore. Come build it into the foundation. You'll be credited forever.

---

## Built on the shoulders of giants

This project stands on over a decade of community knowledge:

- The aerodynamics work pioneered by **Ferram Aerospace Research**
- The n-body physics of **Principia**
- The visual bar set by **Blackrack's** volumetric clouds and scatterer
- The realism pipeline built by the **Realism Overhaul** team
- Every mod author who ever loved this game enough to fix it themselves

The open-source mod ecosystem isn't a reference — it's a foundation.

---

## Current status

🚧 **Early development — Phase 0 complete, Phase 1 in progress.**

**A rocket flies, and lands.** You can throttle it up, steer it, stage it, and watch the spent booster fall
away behind you. It is five placeholder cylinders and a flat grey plane, but the parts are defined
in Lua, the physics runs at an honest fixed 50 Hz, and the origin shifts under you at 10 km without
so much as a flicker.

Gravity falls off with altitude, the air thins out above you, parts shield each other from the
airstream, and joints break when you overload them. A ballistic re-entry from 12 km peaks at
around 300 m/s and *slows down* on the way in. It lands on its legs.

**It reaches space.** Straight up on full throttle, the test stack drops its first stage at
14 km on flameout and coasts through 70,000 m at t+140 s with propellant to spare — through
seven origin shifts that leave no mark on the trace at all.

**Controls:** `Shift`/`Ctrl` throttle · `Z`/`X` full/cut · `WASD` steer · `Q`/`E` roll · `T` SAS · `Space` stage · `0` reset · right-drag orbit · scroll zoom

On a Mac trackpad, "right-drag" is usually Control-click-drag — and `Control` is throttle-down, so
orbiting that way quietly closes the throttle. Use a two-finger click-drag or a mouse.

SAS is on from the pad. It kills rotation rather than holding a heading: point the rocket where you
want it and let go, and it stays there.

`0` puts a fresh stack back on the pad — wreckage, spent stages and all thrown away. There is no
save system yet, so it is that or restart the game between attempts.

**Diagnosing a change:** `KRAKEN_TRACE=2` logs one line per physics tick, and
`KRAKEN_PILOT=ascent|hop|ballistic|idle` flies a scripted profile so a launch or a landing is
reproducible without a human at the keyboard. See EXECUTION.md.

**Phase 0 — Foundation** *(done)*
- [x] Bevy project skeleton — window opens, nothing crashes
- [x] Coordinate system types — `SimPosition` (f64), `WorldOrigin`, `LocalOrigin`, `render_sync.rs` as the single f64→f32 conversion point
- [x] Rapier physics integration — rigid bodies, genuinely fixed 50 Hz timestep, gravity
- [x] Physics interpolation — rendering interpolates between ticks, smooth at any refresh rate
- [x] wgpu pipeline confirmed — Metal on Apple Silicon, Vulkan on Linux, no platform-specific code
- [x] Module directory structure matching the full architecture in `DESIGN.md`
- [x] CI pipeline — `fmt`, `clippy -D warnings`, `test`, on Linux **and** macOS

**Phase 1 — A Rocket Goes Up** *(in progress)*
- [x] Parts loaded from Lua definitions, assembled into a jointed vessel
- [x] `PendingForces`, engine thrust, propellant consumption from the rocket equation
- [x] Staging — decouple, split into two vessels, fly on
- [x] Krakensbane origin shifting
- [x] Camera tracking and a placeholder HUD
- [x] Aerodynamic drag, with parts shielding each other from the airstream
- [x] Point-mass gravity
- [x] Crash detection
- [x] Structural failure (joints that break under load)
- [x] **Exit criterion met:** the stack reaches space — 70 km, staging on the way up

The full roadmap lives in [`DESIGN.md`](DESIGN.md). Current tasks and decisions live in [`EXECUTION.md`](EXECUTION.md). Corners cut, and why, live in [`CHECKLIST.md`](CHECKLIST.md).

---

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) first. It's short.

The quick version: the architecture is in `DESIGN.md`, the current tasks are in `EXECUTION.md`, and `CHECKLIST.md` is where you log tech debt when you cut corners. CI must be green before anything merges.

If you know Rust, Bevy, Lua, orbital mechanics, game networking, 3D art, or you just care about this existing — open an issue, start a discussion, submit a PR.

If you're a KSP mod author — your knowledge is more valuable than you know. Come talk to us.

If you're blackrack — please.

---

## Legal

Kraken Space Program is an original work. It is not a port of any existing game. It does not use assets, code, or proprietary content from any commercial product.

The name "KSP" is not owned by anyone. The Kraken is public domain. Space is free.

---

## License

This software is licensed under the MIT License

---

*The Kraken takes everything eventually. We just named the game after it.*
