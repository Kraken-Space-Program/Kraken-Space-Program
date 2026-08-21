# Contributing to Kraken Space Program

Thanks for being here. This document covers how to contribute without stepping on anyone's toes or wasting your own time.

---

## Before you write any code

**Read `DESIGN.md`.** Not skimming — actually read it. The coordinate system section especially. The most common source of early contributor bugs is not knowing which of the four coordinate spaces an entity is in. The most common source of wasted PRs is duplicating architecture decisions that are already settled.

**Check `EXECUTION.md`.** This is the living task list. It tells you what's actively being worked on, what's coming next, and what decisions are still open. If you're not sure what to work on, start here.

**Check `CHECKLIST.md`.** If something looks wrong in the code, check here first. It might be intentional tech debt with a known fix date. If it's not listed and it looks wrong, open an issue before fixing it — there may be a reason.

---

## Getting started

```bash
# Prerequisites: Rust stable toolchain
rustup update stable

# Clone and run
git clone https://github.com/YOUR_ORG/kraken-space-program
cd kraken-space-program
cargo run

# Faster incremental builds during development
cargo run --features bevy/dynamic_linking

# Before any commit
cargo fmt
cargo clippy -- -D warnings
cargo test
```

CI runs all three on every push. A red CI blocks merging. If it passes locally it'll pass in CI.

---

## How to pick up a task

1. Look at the **Currently Active** section of `EXECUTION.md`. Pick something unassigned.
2. Note somewhere visible (GitHub issue, discussion) that you're working on it, so two people don't do the same thing.
3. If nothing in Currently Active fits your skills, the **Known Tech Debt** table in `CHECKLIST.md` is the standing list of things somebody consciously left undone, each with the phase it should be fixed by. Picking one off it is always welcome.
4. If you want to work on something that isn't listed anywhere, open a discussion first. Architecture decisions made in surprise PRs tend to get reverted.

---

## Rules that are actually enforced

These aren't guidelines — CI enforces them or reviewers will.

**`cargo fmt --check` must pass.** Run `cargo fmt` before committing. No exceptions.

**`cargo clippy -- -D warnings` must pass.** Warnings are errors. Fix them.

**No `.unwrap()` in systems.** Use `if let`, `?`, or the error event pattern from `DESIGN.md`. `.expect("descriptive message")` is acceptable when a panic genuinely means unrecoverable state. `.unwrap()` is not.

**No f32 position casts outside `render_sync.rs`.** If you find yourself writing `.as_vec3()` or `as f32` on a simulation position anywhere else, stop. Read the coordinate systems section of `DESIGN.md`.

**No Rapier imports outside `src/physics/`.** Everything else talks to physics through components and events. If you need something from Rapier in another module, the right answer is an event or a component, not an import.

**Log through `tracing`, never `println!`/`eprintln!`.** `info!`, `warn!` and `error!` from the Bevy prelude, which the `bevy_log` feature wires to a subscriber that honours `RUST_LOG`. (This rule used to say the opposite — `eprintln!` until logging was sorted. Logging is sorted.)

**Do not claim a physics change works because it looked right.** Fly it: `KRAKEN_TRACE=all` for one line per tick, `KRAKEN_PILOT=ascent|hop|ballistic|idle` to fly a profile without a human at the keyboard. Two of Phase 1's worst bugs were invisible at half-second logging resolution and settled before anyone could see them — a rocket bouncing at 9.5 m/s on the pad, SAS spinning a motionless vessel to a steady 0.68 rad/s. See the "Diagnosing flight" section of `EXECUTION.md`.

---

## When you finish something

1. Check it off in `EXECUTION.md`.
2. If your work revealed new tasks, add them.
3. If you cut a corner, add it to `CHECKLIST.md` **before you close the PR**. Not later. The whole point of that file is that "I'll add it later" never happens.
4. If you made an architectural decision, log it in the Decision Log section of `EXECUTION.md` with a date and reasoning.

---

## Code style

**snake_case everywhere.** No camelCase, no PascalCase except for type names. If you find yourself typing `onFlightStart`, you're in the wrong headspace.

**Components are pure data.** No methods on components except trivial constructors and `Default`. Behavior lives in systems.

**Systems are pure functions.** They take queries and resources, return nothing, and emit events. If a system needs to communicate a result, it writes an event — not a return value, not a shared mutable resource.

**One concept per file** (roughly). If a file is approaching 500 lines and doing more than one thing, split it. If you can't describe a file's purpose without using "and," it probably needs splitting.

**No god modules.** The reason `DESIGN.md` has a detailed module map is so nobody accidentally rebuilds `Part.cs` (9,393 lines, the thing we are explicitly not doing).

---

## What you can contribute without knowing Rust

- **Orbital mechanics:** The `orbital/` module needs correct Keplerian math with unit tests against real ephemeris data. If you know the math, the Rust is straightforward.
- **Lua part definitions:** Once Phase 2 is done, all parts are Lua files. If you want to design parts, you don't need to touch Rust.
- **Planet definitions:** Same as parts — Lua, no Rust.
- **3D assets:** `.glb` files for parts and terrain. The pipeline isn't ready yet but assets can be prepared in advance.
- **Documentation:** If something in `DESIGN.md` or `EXECUTION.md` is unclear, a PR that clarifies it is a real contribution.
- **Testing:** Play it, break it, file detailed issues. Reproducible bug reports with steps are worth as much as code.

---

## What we're specifically not doing

A few things that will be declined if proposed, not because they're bad ideas but because the decision is already made:

- **Switching away from Rust/Bevy/Rapier/Lua.** The stack is chosen and justified in `DESIGN.md`.
- **KSP1-style PartModule virtual dispatch.** Components and systems. No base classes. See the Decision Log in `EXECUTION.md`.
- **f32 in orbital mechanics or simulation positions.** f64 throughout. Non-negotiable.
- **fast-math compiler flags.** They break determinism. Banned. See `DESIGN.md`.
- **Binary save formats.** TOML. Human-readable and diffable.

If you think one of these decisions is wrong, open a discussion. The decision log exists so these can be revisited with new information — but "I prefer X" is not sufficient to reopen a settled decision.

---

## The Kraken

There is something in the codebase. It does not interact with anything you will touch. Do not investigate it, modify it, or remove it. It is marked `// DO NOT TOUCH — this is load-bearing superstition`.

If you think you found a bug that is actually the Kraken, you did not find a bug.

---

## License

By contributing, you agree that your contributions are licensed under the MIT License
