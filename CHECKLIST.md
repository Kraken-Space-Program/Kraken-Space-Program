# Kraken Space Program — Checklist

> If you cut a corner, write it here **before you close the PR**. Not later. Now.
> Future contributors will not remember it was intentional and will waste time wondering if it's a bug.
> If you fix a debt item, remove it from the table and note what you did in the commit message.

---

## Known Tech Debt

| # | Description | Where | Cut in Phase | Should be fixed by Phase |
|---|-------------|-------|--------------|--------------------------|
| 1 | `eprintln!` used for logging instead of `tracing` — bypasses `RUST_LOG` filtering. Fix by adding `tracing` as an explicit Cargo dependency and replacing all `eprintln!` calls in systems with `info!`/`debug!`. | `src/main.rs` | 0 | 1 |
| 2 | `log_ball_position` runs every `PostUpdate` frame and prints to stderr — will be extremely noisy at 50Hz once the game is doing real things. Should be removed or gated behind a debug flag before Phase 1. | `src/main.rs` | 0 | 1 |
| 3 | `LocalOrigin` is never updated — it stays at `DVec3::ZERO` forever. Fine for Phase 0 (scene is tiny) but Krakensbane won't work until this is driven by the active vessel position. | `src/rendering/render_sync.rs` | 0 | 1 |
| 4 | No entities carry `SimPosition` + `SyncRender` yet — `sync_render_transforms` runs every frame and matches nothing. The render sync pipeline exists but is not connected to anything. | `src/rendering/render_sync.rs` | 0 | 1 |