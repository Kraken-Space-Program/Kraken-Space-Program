//! Staging — firing decouplers and splitting one vessel into two.
//!
//! design.md: "The staging system queries all `Decoupler` components matching the current
//! stage, destroys their joints via the joints system, which fires `VesselSplit` events.
//! Part tree updates, vessel reassignment, and Rapier changes all flow from events. There
//! is no method call chain inside a monolith."

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::part_modules::decoupler::Decoupler;
use crate::physics::joints;
use crate::rendering::render_sync::SimRotation;
use crate::vessel::components::{
    ActiveVessel, AttachedBelow, ControlState, CurrentStage, PartConnections, RootPart, Vessel,
    VesselId,
};

/// The player pressed stage.
#[derive(Message, Debug, Clone, Copy)]
pub struct StageActivated(pub u32);

/// A vessel became two vessels.
#[derive(Message, Debug, Clone, Copy)]
// Emitted now so the systems that will care — the vessel switcher, the tracking station,
// the networking layer — have the hook waiting for them.
#[allow(dead_code)]
pub struct VesselSplit {
    pub original: Entity,
    pub separated: Entity,
}

/// Space bar, or [`ControlState::stage`], → [`StageActivated`].
///
/// Both routes land here rather than each writing the message, so that incrementing
/// [`CurrentStage`] happens in exactly one place. A vessel that staged twice from one press
/// would skip a stage entirely, and that is the kind of bug that only shows up in the one
/// flight nobody was watching.
pub fn request_stage(
    keys: Res<ButtonInput<KeyCode>>,
    mut vessels: Query<(&mut CurrentStage, &mut ControlState), With<ActiveVessel>>,
    mut staged: MessageWriter<StageActivated>,
) {
    let Ok((mut stage, mut control)) = vessels.single_mut() else {
        return;
    };

    // Read through `Deref` and write only when the latch was actually set, so a vessel
    // nobody is staging is not marked as changed every frame.
    let requested = control.stage;
    if requested {
        control.stage = false;
    }
    if !requested && !keys.just_pressed(KeyCode::Space) {
        return;
    }

    staged.write(StageActivated(stage.0));
    stage.0 += 1;
}

/// Fires every decoupler matching the activated stage.
///
/// # What happens, in order
///
/// 1. The decoupler's joint is destroyed, so the two halves are no longer connected.
/// 2. Everything still connected to the decoupler through remaining joints stays with it;
///    everything on the far side becomes a new vessel entity.
/// 3. Both halves get an ejection impulse, pushing them apart so they do not scrape.
///
/// The separated vessel gets a fresh, zeroed [`ControlState`], which is what stops the
/// discarded stage from continuing to burn: its engines read a throttle of zero.
/// The read-only views of the part graph that [`fire_decouplers`] needs to work out what
/// separated from what. Grouped so the system's own signature stays readable.
#[derive(SystemParam)]
pub struct PartGraph<'w, 's> {
    connections: Query<'w, 's, &'static PartConnections>,
    attached_below: Query<'w, 's, &'static AttachedBelow>,
    parts: Query<'w, 's, (Entity, &'static VesselId)>,
    roots: Query<'w, 's, Entity, With<RootPart>>,
}

/// Splits `vessel` in two at `part`, whose joint has already been removed.
///
/// Everything still reachable from `part` stays with the original vessel; everything else
/// becomes a new vessel entity, returned along with the parts that moved. `None` means the
/// joint was not actually holding anything and nothing was split.
///
/// Shared by staging and by structural failure, which are the same operation reached two
/// ways — a decoupler firing is a joint being destroyed on purpose.
fn split_vessel(
    commands: &mut Commands,
    graph: &PartGraph,
    part: Entity,
    vessel: Entity,
    name: String,
) -> Option<(Entity, Vec<Entity>)> {
    let separated_parts = parts_below(part, graph, vessel);
    if separated_parts.is_empty() {
        return None;
    }

    let separated = commands
        .spawn((
            Vessel { name },
            // Zeroed: a half that broke off must not keep flying itself.
            ControlState::default(),
            CurrentStage(0),
        ))
        .id();

    for part in &separated_parts {
        commands.entity(*part).insert(VesselId(separated));
    }

    // Both halves need exactly one root part, and which half kept the old one depends on
    // where the break was. Staging always breaks below the root, so the original keeps it;
    // a structural failure can break anywhere, and a vessel with no root is one that
    // silently stops responding to attitude input and disappears from the HUD.
    let root_went_with_the_separated_half = separated_parts
        .iter()
        .any(|part| graph.roots.contains(*part));

    if root_went_with_the_separated_half {
        let remaining = graph
            .parts
            .iter()
            .filter(|(entity, owner)| owner.0 == vessel && !separated_parts.contains(entity))
            .map(|(entity, _)| entity)
            .next();
        if let Some(remaining) = remaining {
            commands.entity(remaining).insert(RootPart);
        }
    } else if let Some(first) = separated_parts.first() {
        commands.entity(*first).insert(RootPart);
    }

    Some((separated, separated_parts))
}

/// Turns a broken joint into two vessels.
///
/// design.md: the joints system reports the failure; the consequences are somebody else's
/// job. This is the consequence.
pub fn split_on_joint_failure(
    mut commands: Commands,
    mut failures: MessageReader<joints::JointFailure>,
    graph: PartGraph,
    vessels: Query<&VesselId>,
    mut splits: MessageWriter<VesselSplit>,
) {
    for failure in failures.read() {
        let Ok(vessel_id) = vessels.get(failure.part_a) else {
            continue;
        };
        let Some((separated, parts)) = split_vessel(
            &mut commands,
            &graph,
            failure.part_a,
            vessel_id.0,
            "Debris (structural failure)".into(),
        ) else {
            continue;
        };

        info!("structural failure: {} part(s) broke away", parts.len());
        splits.write(VesselSplit {
            original: vessel_id.0,
            separated,
        });
    }
}

pub fn fire_decouplers(
    mut commands: Commands,
    mut staged: MessageReader<StageActivated>,
    mut decouplers: Query<(Entity, &mut Decoupler, &VesselId, &SimRotation)>,
    graph: PartGraph,
    mut impulses: Query<&mut ExternalImpulse>,
    mut splits: MessageWriter<VesselSplit>,
) {
    for StageActivated(stage) in staged.read().copied() {
        for (decoupler_entity, mut decoupler, vessel_id, rotation) in &mut decouplers {
            if decoupler.fired || decoupler.stage != stage {
                continue;
            }
            decoupler.fired = true;

            // The decoupler owns the joint to the part below it, so releasing its "bottom"
            // node means dropping its own joint.
            joints::detach(&mut commands, decoupler_entity);

            let Some((separated, separated_parts)) = split_vessel(
                &mut commands,
                &graph,
                decoupler_entity,
                vessel_id.0,
                format!("Debris (stage {stage})"),
            ) else {
                warn!("decoupler fired but nothing was attached below it");
                continue;
            };

            // Push the halves apart along the stack axis.
            let axis = rotation.0 * bevy::math::DVec3::Y;
            let impulse = (axis * decoupler.ejection_force_n).as_vec3();
            if let Ok(mut up) = impulses.get_mut(decoupler_entity) {
                up.impulse += impulse;
            }
            for part in &separated_parts {
                if let Ok(mut down) = impulses.get_mut(*part) {
                    down.impulse -= impulse / separated_parts.len() as f32;
                }
            }

            info!("stage {stage}: separated {} part(s)", separated_parts.len());
            splits.write(VesselSplit {
                original: vessel_id.0,
                separated,
            });
        }
    }
}

/// Finds every part that is no longer reachable from `decoupler` once its joint is gone.
///
/// A breadth-first walk of the connection graph from the decoupler, refusing to cross the
/// one edge that was just destroyed. Whatever it can still reach stays; everything else in
/// the vessel is what separated.
///
/// Walking the graph rather than assuming "everything below in the list" means this stays
/// correct for branching vessels — radial boosters, side-mounted tanks — without a rewrite.
fn parts_below(decoupler: Entity, graph: &PartGraph, vessel: Entity) -> Vec<Entity> {
    // The decoupler owns the joint downward, so the severed edge is exactly
    // decoupler <-> its AttachedBelow.
    let severed = graph
        .attached_below
        .get(decoupler)
        .map(|below| below.0)
        .ok();

    let mut still_attached = vec![decoupler];
    let mut frontier = vec![decoupler];

    while let Some(current) = frontier.pop() {
        let Ok(neighbours) = graph.connections.get(current) else {
            continue;
        };
        for neighbour in &neighbours.0 {
            let crosses_severed_joint = (current == decoupler && Some(*neighbour) == severed)
                || (*neighbour == decoupler && Some(current) == severed);
            if crosses_severed_joint {
                continue;
            }
            if !still_attached.contains(neighbour) {
                still_attached.push(*neighbour);
                frontier.push(*neighbour);
            }
        }
    }

    graph
        .parts
        .iter()
        .filter(|(entity, id)| id.0 == vessel && !still_attached.contains(entity))
        .map(|(entity, _)| entity)
        .collect()
}
