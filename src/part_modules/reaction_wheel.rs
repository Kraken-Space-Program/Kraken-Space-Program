//! Reaction wheels: where a vessel's attitude authority actually comes from.
//!
//! # Why this replaced a constant
//!
//! Control torque used to be `ATTITUDE_TORQUE_NM`, one global number applied to whichever
//! part happened to be the root. Every vessel had identical authority regardless of what it
//! was built from — a probe and a fully fuelled launch stack turned at the same rate — and
//! the number was sized by hand against the one test rocket, which meant it had to be
//! retuned by a factor of ten the first time that rocket changed.
//!
//! DESIGN.md is explicit about the shape this should take: "an engine part just has an
//! `Engine` component… systems query for the specific component they care about." Attitude
//! authority is a property of the parts you bolted on, and a vessel with no wheels has none.

use bevy::prelude::*;

/// A reaction wheel, or any other source of attitude torque bolted to a part.
#[derive(Component, Debug, Clone, Copy)]
pub struct ReactionWheel {
    /// Torque at full deflection, newton-metres.
    pub torque_nm: f64,
}
