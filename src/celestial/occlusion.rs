//! Which parts of a vessel are actually in the airstream.
//!
//! # The problem this solves
//!
//! Drag is computed per part, and the obvious version has every part present its full
//! frontal area. A five-part stack then has five times the drag of its nose cone, even
//! though four of those parts are hiding directly behind the first one. KSP1 shipped with
//! this for years, and the workaround here was the same as everyone else's: pick a drag
//! coefficient low enough that the sum comes out roughly right for one particular rocket.
//!
//! That workaround is a trap. It is wrong in opposite directions the moment somebody builds
//! a two-part probe or a twenty-part launcher, and it is wrong *invisibly*, because the
//! number it corrupts is the one you would use to notice.
//!
//! # The model
//!
//! Project every part onto the plane perpendicular to the airflow. A part is shielded by
//! anything upstream of it whose projected disc overlaps its own, and its exposed area is
//! what is left. This is the silhouette, computed the cheap way:
//!
//! - Sort parts by depth along the velocity vector, front first.
//! - For each part, find the largest overlap any single upstream part casts over it.
//! - Its drag area is its own area minus that overlap.
//!
//! Taking the largest single overlap rather than the union of all of them is the one real
//! approximation. For a stack — where each part is directly behind its neighbour — the
//! nearest upstream part dominates completely and the answer is exact. For a cluster of
//! side boosters it under-shields slightly, which errs toward more drag rather than less.
//!
//! Falling back to "everything is exposed" when the vessel is barely moving is not a special
//! case for its own sake: at low speed the drag force is negligible either way, and a
//! velocity direction computed from near-zero velocity is noise.

use bevy::math::DVec3;

/// A part as the airflow sees it.
#[derive(Debug, Clone, Copy)]
pub struct Frontal {
    /// Position in simulation space.
    pub position: DVec3,
    /// Radius of its projected disc, metres.
    pub radius_m: f64,
}

/// Speed below which shielding is not worth computing, m/s.
const NEGLIGIBLE_SPEED_MS: f64 = 0.1;

/// Computes the exposed frontal area of each part, in the same order it was given, m².
///
/// `flow` is the direction the vessel is travelling; parts further along it are upstream and
/// shield the ones behind them.
pub fn exposed_areas(parts: &[Frontal], flow: DVec3) -> Vec<f64> {
    let full_areas: Vec<f64> = parts
        .iter()
        .map(|part| std::f64::consts::PI * part.radius_m * part.radius_m)
        .collect();

    if flow.length() < NEGLIGIBLE_SPEED_MS {
        return full_areas;
    }
    let flow = flow.normalize();

    // Depth along the flow. Larger is further upstream — into the wind — so it shields.
    let depths: Vec<f64> = parts.iter().map(|part| part.position.dot(flow)).collect();

    parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let mut largest_overlap = 0.0f64;

            for (other_index, other) in parts.iter().enumerate() {
                if other_index == index || depths[other_index] <= depths[index] {
                    continue;
                }

                // Separation measured in the plane across the flow: the along-flow component
                // is exactly what does *not* matter for whether one disc covers another.
                let offset = other.position - part.position;
                let lateral = (offset - flow * offset.dot(flow)).length();

                largest_overlap =
                    largest_overlap.max(disc_overlap(part.radius_m, other.radius_m, lateral));
            }

            (full_areas[index] - largest_overlap).max(0.0)
        })
        .collect()
}

/// Area shared by two discs of radii `a` and `b` whose centres are `separation` apart.
fn disc_overlap(a: f64, b: f64, separation: f64) -> f64 {
    if separation >= a + b {
        return 0.0;
    }
    if separation <= (a - b).abs() {
        // One disc is entirely inside the other; the overlap is the smaller of the two.
        let smaller = a.min(b);
        return std::f64::consts::PI * smaller * smaller;
    }

    // Standard circular-segment sum. Guarded with clamps because the arguments to `acos`
    // sit exactly on ±1 at the tangent cases and rounding pushes them over.
    let d2 = separation * separation;
    let (a2, b2) = (a * a, b * b);
    let alpha = ((d2 + a2 - b2) / (2.0 * separation * a))
        .clamp(-1.0, 1.0)
        .acos();
    let beta = ((d2 + b2 - a2) / (2.0 * separation * b))
        .clamp(-1.0, 1.0)
        .acos();

    a2 * (alpha - alpha.sin() * alpha.cos()) + b2 * (beta - beta.sin() * beta.cos())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PI: f64 = std::f64::consts::PI;

    fn stack(radii: &[f64]) -> Vec<Frontal> {
        radii
            .iter()
            .enumerate()
            .map(|(index, radius)| Frontal {
                position: DVec3::new(0.0, index as f64 * 2.0, 0.0),
                radius_m: *radius,
            })
            .collect()
    }

    /// The whole point: a stack flying nose-first has the drag of its nose, not of its parts
    /// added together.
    #[test]
    fn a_stack_flying_nose_first_presents_one_cross_section() {
        let parts = stack(&[0.625; 5]);
        // Travelling +Y, so the part at the top of the stack is upstream.
        let areas = exposed_areas(&parts, DVec3::Y);

        let nose = PI * 0.625 * 0.625;
        assert!(
            (areas[4] - nose).abs() < 1e-9,
            "nose should be fully exposed"
        );
        for (index, area) in areas.iter().take(4).enumerate() {
            assert!(*area < 1e-9, "part {index} is hidden but has area {area}");
        }

        let total: f64 = areas.iter().sum();
        assert!(
            (total - nose).abs() < 1e-9,
            "stack should present {nose} m², got {total}"
        );
    }

    /// Flying backwards shields the other end. Nothing about the model may assume "up".
    #[test]
    fn shielding_follows_the_airflow_not_the_vessel() {
        let parts = stack(&[0.625; 3]);
        let falling = exposed_areas(&parts, DVec3::NEG_Y);
        assert!(falling[0] > 1e-9, "the bottom part meets the air first");
        assert!(falling[2] < 1e-9, "the top part is now in the wake");
    }

    /// Broadside, nothing is behind anything, and a long rocket really does have far more
    /// drag. This is the case a naive "just take the biggest part" model gets badly wrong.
    #[test]
    fn flying_sideways_exposes_every_part() {
        let parts = stack(&[0.625; 5]);
        let areas = exposed_areas(&parts, DVec3::X);
        for (index, area) in areas.iter().enumerate() {
            assert!(
                (*area - PI * 0.625 * 0.625).abs() < 1e-9,
                "part {index} should be fully exposed broadside, got {area}"
            );
        }
    }

    /// A wider part behind a narrower one still catches the air around the edge — this is
    /// what makes landing gear and decoupler flares cost something.
    #[test]
    fn a_wider_part_behind_a_narrow_one_shows_a_ring() {
        let parts = vec![
            Frontal {
                position: DVec3::ZERO,
                radius_m: 2.0,
            },
            Frontal {
                position: DVec3::Y,
                radius_m: 1.0,
            },
        ];
        let areas = exposed_areas(&parts, DVec3::Y);

        assert!(
            (areas[1] - PI).abs() < 1e-9,
            "the narrow nose is fully exposed"
        );
        let ring = PI * 4.0 - PI;
        assert!(
            (areas[0] - ring).abs() < 1e-9,
            "the wide part should show a ring of {ring} m², got {}",
            areas[0]
        );
    }

    #[test]
    fn a_vessel_at_rest_is_not_shielded_by_a_direction_that_does_not_exist() {
        let parts = stack(&[0.625; 3]);
        let areas = exposed_areas(&parts, DVec3::ZERO);
        assert_eq!(areas.len(), 3);
        for area in areas {
            assert!((area - PI * 0.625 * 0.625).abs() < 1e-9);
        }
    }

    #[test]
    fn partial_overlap_is_between_none_and_all() {
        let full = PI;
        let half_offset = disc_overlap(1.0, 1.0, 1.0);
        assert!(half_offset > 0.0 && half_offset < full);
        assert_eq!(disc_overlap(1.0, 1.0, 2.0), 0.0);
        assert!((disc_overlap(1.0, 1.0, 0.0) - full).abs() < 1e-9);
    }
}
