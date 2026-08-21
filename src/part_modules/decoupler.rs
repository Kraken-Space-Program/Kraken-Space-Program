//! Decouplers — the parts that make staging possible.
//!
//! The component is data. The *act* of decoupling lives in `vessel::staging`, because it
//! is a vessel-level event: a joint dies, a vessel may split, two things fly apart. A
//! decoupler knowing how to destroy its own joint would be the KSP1 `Part.cs` mistake in
//! miniature.

use bevy::prelude::*;

#[derive(Component, Debug, Clone)]
pub struct Decoupler {
    /// Stage number this fires on. Lower fires first.
    pub stage: u32,
    /// Name of the attach node released when it fires.
    ///
    /// Staging currently assumes a decoupler releases downward, which is what every stack
    /// decoupler does. Radial decouplers will need this read properly.
    #[allow(dead_code)]
    pub node: String,
    /// Separation impulse, newtons.
    pub ejection_force_n: f64,
    /// Set once it has fired — a decoupler is a one-shot device.
    pub fired: bool,
}
