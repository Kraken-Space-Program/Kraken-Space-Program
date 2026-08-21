# `block`, vendored

Upstream: <http://github.com/SSheldon/rust-block>, version 0.1.6, by Steven Sheldon. The
original README is kept alongside this one as `README.upstream.md`.

## Licence

MIT, as declared in upstream's own `Cargo.toml` and on crates.io. There is no licence file
to reproduce here: upstream publishes none — not in the repository, not in the packaged
`.crate` — so the declaration in the manifest is the whole of it, and this copy carries the
same declaration in its own `Cargo.toml`. Writing out an MIT notice with a copyright line
nobody upstream ever wrote would be inventing a legal notice on the author's behalf, which
is worse than pointing at what actually exists. What is reproduced instead is provenance:
name, version, author, upstream URL, and precisely what was changed.

## Why this is in the repository

`block` is not a dependency of this game. It arrives five levels down:

```
kraken-space-program → bevy → bevy_render → wgpu → wgpu-hal → metal → block
```

`metal` is the Metal backend wgpu selects on macOS, so every macOS build compiles it, and
every macOS build printed:

```
warning: the following packages contain code that will be rejected by a future version of
Rust: block v0.1.6
```

The cause is one line: upstream declares `enum Class {}` — an uninhabited type — and then
`static _NSConcreteStackBlock: Class`. A static of an uninhabited type can never be read,
so rustc is turning that into a hard error (rust-lang/rust#74840). When it does, this
crate stops compiling and takes the Metal backend, and therefore the game on macOS, with
it.

There is nowhere else to fix it. `block` 0.1.6 is the newest release and the last was in
2016; `metal` 0.32 is what `wgpu-hal` 27 pins, and Bevy 0.18 pins that wgpu. So the fix is
a `[patch.crates-io]` entry in the root `Cargo.toml` pointing at this copy.

## The patch

Three changes to `src/lib.rs`, and nothing else:

1. `enum Class {}` became a zero-sized `#[repr(C)] struct Class` — inhabited, so the lint
   does not fire. Only the static's *address* is ever used (as a stack block's `isa`
   pointer), which is unchanged.
2. The `#[cfg(test)] mod tests` block was removed, along with `mod test_utils`. They
   depend on an `objc_test_utils` C shim that is not vendored.
3. This README, `README.upstream.md`, and the `Cargo.toml` header.

## When to delete it

When wgpu moves the Metal backend onto `objc2`/`block2`, which is where that ecosystem has
gone. Cargo will say so by itself: an unused `[patch]` produces

```
warning: Patch `block v0.1.6 (.../third_party/block)` was not used in the crate graph.
```

At that point remove the `[patch.crates-io]` section and this directory.
