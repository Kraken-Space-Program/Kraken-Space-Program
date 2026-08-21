//! Part-to-part joints. The sole owner of Rapier joint lifecycle.
//!
//! design.md: "Joint lifecycle belongs to the joints system, not to parts. A part never
//! touches Rapier directly." Parts hold `PartConnections` — who they are attached to.
//! What that attachment *is* lives here.
//!
//! # One joint per entity
//!
//! Rapier's `ImpulseJoint` is a single component, so an entity can hold exactly one. The
//! convention is that a part owns the joint to the part **below** it in the stack. A part
//! with two neighbours is joined by its own joint downward and by its upper neighbour's
//! joint upward, so every connection is owned exactly once and detaching is unambiguous.
//!
//! # Structural failure
//!
//! design.md: "Joint force limits drive structural failures — not game logic deciding to
//! explode things. Physics reports exceedance. The joints system responds."
//!
//! Rapier has no native breakable joint, so [`detect_joint_failures`] reads the constraint
//! impulses out of the solver every tick and compares them against the limits parsed from
//! Lua. What it does *not* do is decide what a failure means: it removes the joint and
//! writes a [`JointFailure`], and `vessel::staging` turns that into two vessels.

use bevy::ecs::system::SystemParam;
use bevy::math::DVec3;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::vessel::components::{ActiveVessel, VesselId};

/// Break limits for the joint an entity owns, in newtons.
///
/// Kept on the entity so [`detect_joint_failures`] never has to go back to the part
/// definitions to find the numbers.
#[derive(Component, Debug, Clone, Copy)]
pub struct JointStrength {
    pub tensile_n: f64,
    pub shear_n: f64,
}

/// A joint failed and the parts it held are no longer connected.
///
/// design.md: "The part does not know it failed. The joint system knows." Consequences —
/// the vessel split, and later the sound and the debris — are other systems' business.
#[derive(Message, Debug, Clone, Copy)]
pub struct JointFailure {
    /// The part that owned the joint. Its `ImpulseJoint` has already been removed.
    pub part_a: Entity,
    /// The part it was attached to. Carried so that consequences which need both halves —
    /// the break sound, the debris spawn, the damage report — do not have to walk the part
    /// graph to rediscover something the joints system already knew.
    #[allow(dead_code)]
    pub part_b: Entity,
}

/// Rigidly attaches `child` to `parent`.
///
/// Anchors are in each part's own local space — normally the attach node positions the two
/// parts are being joined at, so the joint sits exactly where the parts touch rather than
/// between their centres of mass.
pub fn attach(
    commands: &mut Commands,
    child: Entity,
    parent: Entity,
    child_anchor: DVec3,
    parent_anchor: DVec3,
    strength: JointStrength,
) {
    // f64 → f32 at the Rapier boundary, inside physics/. Anchors are part-local offsets
    // measured in centimetres-to-metres, nowhere near f32's limits.
    let joint = FixedJointBuilder::new()
        .local_anchor1(parent_anchor.as_vec3())
        .local_anchor2(child_anchor.as_vec3());

    commands
        .entity(child)
        .insert((ImpulseJoint::new(parent, joint), strength));
}

/// Destroys the joint an entity owns, releasing it from the part below.
pub fn detach(commands: &mut Commands, child: Entity) {
    commands
        .entity(child)
        .remove::<ImpulseJoint>()
        .remove::<JointStrength>();
}

/// Peak load a joint carried this tick, in newtons.
///
/// Split the way the Lua limits are: along the stack axis, and across it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointLoad {
    /// Force along the joint's axis — pulling the parts apart or squashing them together.
    pub axial_n: f64,
    /// Force across the axis — sliding one part sideways off the other.
    pub shear_n: f64,
}

/// Converts a tick's constraint impulses into the forces they represent.
///
/// Rapier reports what the solver had to apply to hold the joint together, as an impulse in
/// newton-seconds; dividing by the timestep gives the force. The first three components are
/// the linear degrees of freedom in the joint's own frame, which — because `attach` builds
/// the joint with an identity basis and the stack is assembled along local +Y — means index
/// 1 is axial and 0 and 2 are the two shear directions.
fn load_from_impulses(impulses: [f32; 3], dt_secs: f64) -> JointLoad {
    let scale = 1.0 / dt_secs;
    JointLoad {
        // Tension and compression are not distinguished. They are genuinely different
        // failure modes with different limits — a tube buckles long before it snaps — but
        // only one limit is authored, so treating the two alike is the honest reading of
        // the data that exists rather than an invented buckling strength.
        axial_n: f64::from(impulses[1]).abs() * scale,
        shear_n: (f64::from(impulses[0]).hypot(f64::from(impulses[2]))) * scale,
    }
}

/// The most loaded joint this tick, as a fraction of its own limit. 1.0 is the breaking
/// point.
///
/// Published so that diagnostics can report structural margin without reaching into Rapier —
/// `physics/` is the only module allowed to do that, so it does the reading once and hands
/// out the number.
///
/// # Why there are two numbers
///
/// The world-wide peak is what the watchdog wants: any joint in trouble is worth saying so,
/// whoever owns it. It is emphatically *not* what a flight trace wants. A spent booster
/// hitting the ground loads its joints to several times their limit, and on a first ascent
/// that read as the vessel 100 km overhead carrying 308% of its breaking load — a number
/// alarming enough to chase, attached to a vessel that was doing nothing but coasting.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct PeakJointLoad {
    /// Across every joint in the simulation, debris included.
    pub fraction_of_limit: f64,
    /// Across the joints of the vessel the player is flying.
    pub on_active_vessel: f64,
}

/// Load fraction at which a joint is close enough to failing to be worth saying so.
///
/// Above the ~0.49 a healthy stack peaks at while settling onto the pad, and far enough
/// below 1.0 to be a warning rather than an obituary. The bug this exists to catch — parts'
/// colliders fighting their own joints — sat at *nine times* the limit, so the exact
/// threshold matters much less than having one at all.
const JOINT_LOAD_WARN_FRACTION: f64 = 0.7;

/// Consecutive ticks a joint must be over its limit before it breaks.
///
/// # Why a single overloaded tick is not a failure
///
/// A rigid contact at 50 Hz arrests a vessel inside one step, and the impulse that takes is
/// enormous however gently it arrived: a 2.4 t upper stage touching down at 1.8 m/s stops in
/// 20 ms, which is 9 g, which is 217 kN through a joint rated for 67. Breaking on that would
/// mean no landing was ever survivable — measured, not hypothesised; it is what the first
/// version of this did.
///
/// A real structural overload — too much thrust through too weak a node, aerodynamic load in
/// a high-speed turn — lasts as long as its cause. Three ticks is 60 ms: long enough that no
/// contact transient survives it, short enough that a genuine overload still fails promptly.
const TICKS_OVER_LIMIT_BEFORE_FAILURE: u32 = 3;

/// Per-joint overload counters, plus the warning cooldown. One `Local` rather than three.
#[derive(Default)]
pub struct FailureDetectorState {
    ticks_over_limit: HashMap<Entity, u32>,
    last_warned_secs: f64,
}

/// The live joints, and enough of the part graph to tell whose they are.
///
/// Grouped so [`detect_joint_failures`] keeps a signature somebody can read.
#[derive(SystemParam)]
pub struct JointLoads<'w, 's> {
    joints: Query<
        'w,
        's,
        (
            Entity,
            &'static JointStrength,
            &'static RapierImpulseJointHandle,
            &'static ImpulseJoint,
        ),
    >,
    owners: Query<'w, 's, &'static VesselId>,
    active: Query<'w, 's, Entity, With<ActiveVessel>>,
}

/// Breaks joints whose load exceeded the limits from their Lua definition.
///
/// # Ordering
///
/// `FixedUpdate`, after `PhysicsSet::Writeback` — the impulses are a result of the step, so
/// there is nothing to read before it has run.
pub fn detect_joint_failures(
    mut commands: Commands,
    context: Query<&RapierContextJoints>,
    loaded: JointLoads,
    mut failures: MessageWriter<JointFailure>,
    // Rebuilt from the live joints every tick, so a joint that goes away — through staging,
    // through a failure, through the vessel being deleted — takes its counter with it.
    mut state: Local<FailureDetectorState>,
    mut peak_load: ResMut<PeakJointLoad>,
    time: Res<Time<Fixed>>,
) {
    let FailureDetectorState {
        ticks_over_limit,
        last_warned_secs,
    } = &mut *state;
    peak_load.fraction_of_limit = 0.0;
    peak_load.on_active_vessel = 0.0;

    let JointLoads {
        joints,
        owners,
        active,
    } = &loaded;
    let active_vessel = active.single().ok();

    let Ok(context) = context.single() else {
        return;
    };

    let mut still_present: HashSet<Entity> = HashSet::new();

    for (part, strength, handle, joint) in joints {
        still_present.insert(part);

        let Some(solver_joint) = context.impulse_joints.get(handle.0) else {
            continue;
        };

        let impulses = solver_joint.impulses;
        let load = load_from_impulses(
            [impulses[0], impulses[1], impulses[2]],
            f64::from(crate::physics::PHYSICS_DT),
        );

        // A limit of zero means "not authored", not "breaks under its own weight". Every
        // stock part declares both, but a mod that forgets one should get an unbreakable
        // joint rather than a rocket that disassembles itself on the pad.
        // Fraction of whichever limit this joint is closest to breaking.
        let mut fraction: f64 = 0.0;
        if strength.tensile_n > 0.0 {
            fraction = fraction.max(load.axial_n / strength.tensile_n);
        }
        if strength.shear_n > 0.0 {
            fraction = fraction.max(load.shear_n / strength.shear_n);
        }
        peak_load.fraction_of_limit = peak_load.fraction_of_limit.max(fraction);
        if active_vessel.is_some_and(|vessel| owners.get(part).is_ok_and(|owner| owner.0 == vessel))
        {
            peak_load.on_active_vessel = peak_load.on_active_vessel.max(fraction);
        }

        let over_tension = strength.tensile_n > 0.0 && load.axial_n > strength.tensile_n;
        let over_shear = strength.shear_n > 0.0 && load.shear_n > strength.shear_n;

        let consecutive = ticks_over_limit.entry(part).or_default();
        if !over_tension && !over_shear {
            *consecutive = 0;
            continue;
        }
        *consecutive += 1;
        if *consecutive < TICKS_OVER_LIMIT_BEFORE_FAILURE {
            continue;
        }

        warn!(
            "joint failed: {:.0} kN axial / {:.0} kN shear against limits of {:.0} / {:.0}",
            load.axial_n / 1000.0,
            load.shear_n / 1000.0,
            strength.tensile_n / 1000.0,
            strength.shear_n / 1000.0,
        );

        detach(&mut commands, part);
        failures.write(JointFailure {
            part_a: part,
            part_b: joint.parent,
        });
    }

    ticks_over_limit.retain(|joint, _| still_present.contains(joint));

    // Structural margin is invisible until it is gone, so say something while there is still
    // margin left to lose. Rate-limited: the condition lasts as long as its cause.
    let now = time.elapsed_secs_f64();
    if peak_load.fraction_of_limit > JOINT_LOAD_WARN_FRACTION && now - *last_warned_secs > 2.0 {
        *last_warned_secs = now;
        // Which vessel it belongs to is most of the message. The first ascent produced
        // "308% of its breaking load" from a spent booster hitting the ground while the
        // vessel being flown was coasting through 106 km, untouched.
        let whose = if peak_load.on_active_vessel >= peak_load.fraction_of_limit {
            "the active vessel"
        } else {
            "another vessel"
        };
        warn!(
            "a joint on {whose} is carrying {:.0}% of its breaking load",
            peak_load.fraction_of_limit * 100.0
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The axis convention is the whole correctness of this file: get it wrong and a rocket
    /// under thrust reads as being sheared apart sideways.
    #[test]
    fn axial_and_shear_are_read_off_the_right_axes() {
        // 0.02 s at 50 Hz, so an impulse of 20 N·s is a force of 1000 N.
        let load = load_from_impulses([0.0, 20.0, 0.0], 0.02);
        assert!((load.axial_n - 1000.0).abs() < 1e-6);
        assert_eq!(load.shear_n, 0.0);

        let load = load_from_impulses([20.0, 0.0, 0.0], 0.02);
        assert_eq!(load.axial_n, 0.0);
        assert!((load.shear_n - 1000.0).abs() < 1e-6);

        // Shear combines both lateral axes: 3-4-5.
        let load = load_from_impulses([3.0, 0.0, 4.0], 1.0);
        assert!((load.shear_n - 5.0).abs() < 1e-6);
    }

    /// Compression must read as a load, not as a negative one that passes every check.
    #[test]
    fn compression_counts_as_axial_load() {
        let compressive = load_from_impulses([0.0, -20.0, 0.0], 0.02);
        let tensile = load_from_impulses([0.0, 20.0, 0.0], 0.02);
        assert_eq!(compressive.axial_n, tensile.axial_n);
    }
}
