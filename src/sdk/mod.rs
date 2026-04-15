//! Kraken SDK — Lua modding API.
//!
//! This module provides the Lua scripting interface for mods. All gameplay content
//! (parts, planets, vessels, UI, rules) is defined in Lua for maximum moddability.
//!
//! The SDK is designed to be:
//! - **Safe**: Sandboxed Lua execution with memory/CPU limits.
//! - **Stable**: API contracts that don't break between versions.
//! - **Performant**: Lua loaded at startup, cached as Rust data structures.
//!
//! # Architecture
//!
//! - `bindings.rs`: Low-level mlua bindings exposing Rust functions to Lua.
//! - `sandbox.rs`: Lua VM setup, error handling, and security restrictions.
//! - `loader.rs`: Loads and validates Lua mod files from disk.
//! - `api/`: High-level Lua APIs (parts, planets, etc.).

pub mod bindings;
pub mod sandbox;
pub mod loader;

pub mod api {
    pub mod parts;
    pub mod planets;
    pub mod vessels;
    pub mod ui;
}