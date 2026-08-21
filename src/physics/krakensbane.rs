//! Origin shifting — keeping the active vessel near the physics origin.
//!
//! design.md: "f32 precision degrades past ~10,000 units from origin — irrelevant at human
//! scale, fatal at planetary scale. Implement this in Phase 1, not as a later patch."
//!
//! # What actually moves
//!
//! Rapier simulates in a local f32 frame. When the active vessel drifts too far from that
//! frame's origin, everything in the world is translated back by the same delta and
//! [`WorldOrigin`] absorbs it:
//!
//! ```text
//!   WorldOrigin += delta          (f64, gains the offset)
//!   every Rapier transform -= delta   (f32, loses it)
//!   SimPosition = WorldOrigin + transform      →  unchanged
//! ```
//!
//! The origin gains the delta in f64, exactly. Each body loses it in f32 — and for the
//! active vessel that is exact too, because the shift is taken about the mean of its own
//! parts, so `local - centre` subtracts two f32 values of similar size, which never rounds.
//! The vessel being flown therefore comes through a shift bit-identical, joints and all.
//!
//! A body far from that centre is a different case: its subtraction produces a large result
//! that does round, and it can move by up to half an f32 ULP — 0.4 mm measured at 12.6 km.
//! That is below anything this engine resolves, but it is a bound rather than nothing, and
//! it is the reason `krakensbane.rs` has tests that say so.
//!
//! Nothing outside this file needs to know a shift happened — which is the whole point, and
//! why the shift has to be one atomic system pass rather than something systems opt into.
//!
//! # Why LocalOrigin tracks WorldOrigin exactly
//!
//! [`LocalOrigin`] is what `render_sync` subtracts, so render space is
//! `SimPosition - LocalOrigin`. Setting it equal to `WorldOrigin` makes render space
//! identical to Rapier's local frame — which means static scenery that Rapier owns
//! directly (terrain colliders, the launch pad) and interpolated vessels stay in the same
//! space without the scenery needing its own f64 position. They are kept as separate
//! resources because design.md anticipates them diverging later, when the render origin
//! follows the camera rather than the physics frame.

use bevy::math::DVec3;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::rendering::render_sync::{LocalOrigin, WorldOrigin};
use crate::vessel::components::{ActiveVessel, VesselId};

/// How far the active vessel may drift from the physics origin before everything shifts.
///
/// design.md's default. At 10 km an f32 has ~1 mm of precision, which is still far finer
/// than any collision this engine resolves.
pub const ORIGIN_SHIFT_THRESHOLD_M: f32 = 10_000.0;

/// The world origin after absorbing a shift of `delta`, in f64.
///
/// One half of a pair that has to cancel exactly; see [`shifted_local`].
fn shifted_origin(origin: DVec3, delta: DVec3) -> DVec3 {
    origin + delta
}

/// A body's position in the physics frame after the same shift, in f32.
///
/// The other half. Split out from the system purely so the cancellation can be asserted in
/// a test rather than trusted — it is the claim the whole coordinate system rests on, and
/// it is one sign flip away from silently corrupting every position in the world.
fn shifted_local(local: Vec3, centre: Vec3) -> Vec3 {
    local - centre
}

/// Shifts the world origin when the active vessel drifts past the threshold.
///
/// # Ordering
///
/// `FixedUpdate`, after `PhysicsSet::Writeback` and after the readback that derives
/// `SimPosition`, so it sees settled positions and its correction lands before the next
/// tick's `SyncBackend` reads transforms back into Rapier.
pub fn shift_world_origin(
    mut world_origin: ResMut<WorldOrigin>,
    mut local_origin: ResMut<LocalOrigin>,
    active_vessels: Query<Entity, With<ActiveVessel>>,
    // One query, iterated twice: reading `&Transform` in a second query while this one
    // holds `&mut Transform` is a access conflict Bevy rejects at run time, and splitting
    // it with `Without` would exclude exactly the bodies that need shifting.
    mut bodies: Query<(&mut Transform, Option<&VesselId>), With<RigidBody>>,
) {
    let Ok(active) = active_vessels.single() else {
        return;
    };

    // Reference point: the mean of the active vessel's parts, so a long stack shifts
    // around its middle instead of around whichever part happened to be the root.
    let mut sum = Vec3::ZERO;
    let mut count = 0.0f32;
    for (transform, vessel_id) in &bodies {
        if vessel_id.is_some_and(|id| id.0 == active) {
            sum += transform.translation;
            count += 1.0;
        }
    }
    if count == 0.0 {
        return;
    }
    let centre = sum / count;

    if centre.length() < ORIGIN_SHIFT_THRESHOLD_M {
        return;
    }

    // Widening f32 → f64 is lossless; this is the direction that is always safe.
    let delta = centre.as_dvec3();
    world_origin.0 = shifted_origin(world_origin.0, delta);
    local_origin.0 = world_origin.0;

    for (mut transform, _) in &mut bodies {
        transform.translation = shifted_local(transform.translation, centre);
    }

    // SimPosition is WorldOrigin + local, and both sides just moved by the same amount, so
    // every simulation position is unchanged and nothing needs rewriting. PreviousSimPosition
    // is equally unaffected, which is why interpolation does not jump on the frame a shift
    // happens — that would otherwise be a 10 km visual snap once per shift.
    info!(
        "Krakensbane: origin shifted by {:.1} m, world origin now {:.1?}",
        delta.length(),
        world_origin.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulation position of a body, the way the rest of the engine derives it.
    fn sim_position(origin: DVec3, local: Vec3) -> DVec3 {
        origin + local.as_dvec3()
    }

    /// The invariant this module exists to preserve, for the bodies it matters for: the
    /// active vessel's parts come through a shift bit-identical.
    ///
    /// Not by luck. The shift is taken about the mean of those parts, so each part's
    /// `local - centre` subtracts two f32 values within a factor of two of each other, and
    /// that is exact — no rounding exists to accumulate. A jointed stack that moved even a
    /// fraction of a millimetre relative to itself would be a stack whose joints were told
    /// their anchors had changed, once every 10 km, forever.
    ///
    /// Verified in flight as well: a straight-up ascent shifts the origin seven times
    /// between the pad and 70 km, and the altitude trace is continuous across every one.
    #[test]
    fn a_shift_leaves_the_active_vessels_parts_exactly_where_they_were() {
        let world_origin = DVec3::new(1.0e6, -2.5e6, 7.5e5);
        // A stack straddling the shift threshold, plus the terrain still at the old origin.
        let parts = [
            Vec3::new(10_004.4, 9_971.7, -233.6),
            Vec3::new(10_004.4, 9_968.2, -233.6),
            Vec3::new(10_004.4, 9_975.1, -233.6),
        ];
        let centre = (parts[0] + parts[1] + parts[2]) / 3.0;

        for local in parts.iter().chain([Vec3::ZERO].iter()) {
            let was = sim_position(world_origin, *local);
            let now = sim_position(
                shifted_origin(world_origin, centre.as_dvec3()),
                shifted_local(*local, centre),
            );
            assert_eq!(
                now, was,
                "a shift moved a simulation position: {was:?} became {now:?}"
            );
        }
    }

    /// What the shift does to a body a long way from the vessel being shifted around —
    /// debris a few kilometres downrange, say. Here `local - centre` is a subtraction of
    /// two large f32 values whose difference is also large, so the result rounds, and the
    /// position moves by up to half an f32 ULP at that distance.
    ///
    /// This is the honest bound and it is worth stating rather than claiming exactness: at
    /// 12.6 km the measured error is 0.4 mm, against an ULP of 1.0 mm. Fine for debris.
    /// Not fine if anything ever needs sub-millimetre precision far from the active vessel
    /// — docking a second vessel would, which is Phase 4, and which is the point at which
    /// this test should start failing on purpose.
    #[test]
    fn a_distant_body_stays_within_one_f32_ulp() {
        let world_origin = DVec3::new(1.0e6, -2.5e6, 7.5e5);
        let centre = Vec3::new(10_004.4, 9_971.7, -233.6);
        let distant = Vec3::new(-1.0, 0.125, 12_345.678);

        let was = sim_position(world_origin, distant);
        let now = sim_position(
            shifted_origin(world_origin, centre.as_dvec3()),
            shifted_local(distant, centre),
        );

        let moved_m = (now - was).length();
        let ulp_m = f64::from(f32::EPSILON) * f64::from((distant - centre).length());
        assert!(
            moved_m > 0.0,
            "if this is exact now, the rounding it documents has gone away and the comment \
             above is stale"
        );
        assert!(
            moved_m < ulp_m,
            "a distant body moved {:.4} mm, more than the {:.4} mm an f32 can resolve there",
            moved_m * 1000.0,
            ulp_m * 1000.0
        );
    }

    /// The shift is taken in f32 and applied to the origin in f64, so the delta the origin
    /// gains must be the delta the transforms lost — not a rounded version of it. Widening
    /// is the safe direction; this pins that it is the direction used.
    #[test]
    fn the_delta_survives_the_f32_to_f64_widening() {
        let centre = Vec3::new(10_000.5, -9_999.75, 0.125);
        let origin = DVec3::ZERO;

        let gained = shifted_origin(origin, centre.as_dvec3()) - origin;
        let lost = (Vec3::ZERO - shifted_local(Vec3::ZERO, centre)).as_dvec3();

        assert_eq!(gained, lost);
    }
}
