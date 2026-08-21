//! Placeholder flight HUD.
//!
//! design.md: "UI is primarily Lua-driven; minimal Rust here, mostly event bridges."
//! This is the Phase 1 placeholder it describes — hardcoded positions, no Lua. It exists
//! because flying without an altimeter is flying blind, not because the layout is a design
//! anyone should keep.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::celestial::atmosphere::Atmosphere;
use crate::celestial::body::CelestialBody;
use crate::part_modules::engine::Engine;
use crate::part_modules::resource_container::{self, ResourceContainer};
use crate::rendering::render_sync::{SimPosition, SimVelocity};
use crate::vessel::components::{
    ActiveVessel, ControlState, Destroyed, PartMass, RootPart, Vessel, VesselId,
};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, update_hud);
    }
}

/// Size of the flight readout, in logical pixels.
///
/// Sized to be read at a glance from across a desk while flying, not to be efficient with
/// screen space: the readout is the instrument panel, and numbers small enough to need
/// looking *at* get ignored during the ten seconds when they matter. 24 px is roughly the
/// smallest that stays legible on a 4K display without the window being scaled up.
const READOUT_FONT_SIZE: f32 = 24.0;

/// Size of the controls hint, in logical pixels.
///
/// Deliberately smaller than the readout — it is a reminder of the keys, read once and
/// then ignored — but no longer small enough to be unreadable, which is what 13 px was.
const HINT_FONT_SIZE: f32 = 17.0;

/// Marks the HUD's readout text.
#[derive(Component)]
struct HudText;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(16.0),
            left: Val::Px(16.0),
            ..default()
        },
        Text::new("altitude   0 m"),
        TextFont {
            font_size: READOUT_FONT_SIZE,
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.95, 1.0)),
        HudText,
    ));

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(16.0),
            left: Val::Px(16.0),
            ..default()
        },
        Text::new(
            "Shift/Ctrl throttle  ·  Z full  ·  X cut  ·  WASD steer  ·  QE roll  ·  \
             T SAS  ·  Space stage  ·  0 reset  ·  right-drag orbit  ·  scroll zoom",
        ),
        TextFont {
            font_size: HINT_FONT_SIZE,
            ..default()
        },
        TextColor(Color::srgb(0.65, 0.68, 0.75)),
    ));
}

/// Vertical speed below which "you are descending" stops being interesting, m/s.
///
/// Contacts and solver jitter leave a resting vessel with a few cm/s of residual motion, so
/// a strict `< 0.0` test would flash the landing readouts at a rocket sitting on the pad.
const DESCENT_THRESHOLD_MS: f64 = 1.0;

/// The per-part readings the HUD sums up. Grouped so `update_hud`'s signature stays legible
/// as the readout grows.
#[derive(SystemParam)]
struct VesselReadings<'w, 's> {
    roots: Query<
        'w,
        's,
        (
            &'static VesselId,
            &'static SimPosition,
            &'static SimVelocity,
        ),
        With<RootPart>,
    >,
    engines: Query<'w, 's, (&'static VesselId, &'static Engine)>,
    tanks: Query<'w, 's, (&'static VesselId, &'static ResourceContainer)>,
    masses: Query<
        'w,
        's,
        (
            &'static VesselId,
            &'static PartMass,
            Option<&'static ResourceContainer>,
        ),
    >,
}

fn update_hud(
    vessels: Query<(Entity, &Vessel, &ControlState, Has<Destroyed>), With<ActiveVessel>>,
    readings: VesselReadings,
    body: Res<CelestialBody>,
    atmosphere: Res<Atmosphere>,
    mut hud: Query<&mut Text, With<HudText>>,
) {
    let VesselReadings {
        roots,
        engines,
        tanks,
        masses,
    } = &readings;
    let (Ok((vessel, vessel_data, control, destroyed)), Ok(mut text)) =
        (vessels.single(), hud.single_mut())
    else {
        return;
    };

    let Some((_, position, velocity)) = roots.iter().find(|(id, _, _)| id.0 == vessel) else {
        return;
    };

    let altitude_m = body.altitude_of(position.0);
    let speed_ms = velocity.0.length();
    // Vertical is "away from the body's centre", not "+Y". Identical at the launch site and
    // increasingly not identical the further downrange a vessel gets.
    let vertical_ms = velocity.0.dot(body.up_at(position.0));
    let gravity_ms2 = body.gravity_at(position.0).length();

    let thrust_n: f64 = engines
        .iter()
        .filter(|(id, _)| id.0 == vessel)
        .map(|(_, engine)| engine.current_thrust_n)
        .sum();
    // What the engines could produce at full throttle, which is the number that decides
    // whether a landing is possible at all.
    let max_thrust_n: f64 = engines
        .iter()
        .filter(|(id, _)| id.0 == vessel)
        .map(|(_, engine)| engine.thrust_n)
        .sum();

    let propellant_kg: f64 = tanks
        .iter()
        .filter(|(id, _)| id.0 == vessel)
        .map(|(_, container)| container.mass_kg())
        .sum();

    let mass_kg = resource_container::vessel_mass_kg(vessel, masses);

    // Thrust-to-weight. Below 1.0 the vessel cannot arrest a fall no matter how it is
    // flown, which is invisible without a readout and reads as a bug — the Spark upper
    // stage separates at 0.86 and only climbs past 1.0 once it has burned a few hundred kg.
    let weight_n = mass_kg * gravity_ms2;
    let twr = if weight_n > 0.0 {
        max_thrust_n / weight_n
    } else {
        0.0
    };

    // Height at which a full-throttle burn *just* arrests the current descent:
    // v² / 2a, with `a` the net deceleration once gravity has been paid for. Undefined
    // while climbing, and undefined when TWR ≤ 1 because there is no net deceleration to
    // integrate — both cases print as a dash rather than a misleading number.
    let net_decel = max_thrust_n / mass_kg - gravity_ms2;
    let descending = vertical_ms < -DESCENT_THRESHOLD_MS;
    let burn_line = if !descending || net_decel <= 0.0 {
        "burn alt          — ".to_string()
    } else {
        let burn_altitude_m = vertical_ms.powi(2) / (2.0 * net_decel);
        let marker = if altitude_m <= burn_altitude_m {
            "   << BURN NOW"
        } else {
            ""
        };
        format!("burn alt   {burn_altitude_m:>8.1} m{marker}")
    };

    let status = if destroyed {
        "  — DESTROYED"
    } else if control.sas {
        "  [SAS]"
    } else {
        ""
    };

    text.0 = format!(
        "{}{status}\n\
         altitude   {altitude_m:>8.1} m\n\
         speed      {speed_ms:>8.1} m/s   (vertical {vertical_ms:>+7.1})\n\
         throttle   {:>8.0} %\n\
         thrust     {:>8.1} kN   (TWR {twr:.2})\n\
         propellant {propellant_kg:>8.0} kg\n\
         mass       {mass_kg:>8.0} kg\n\
         air        {:>8.3} kg/m³  (g {gravity_ms2:.2})\n\
         {burn_line}",
        vessel_data.name,
        control.throttle * 100.0,
        thrust_n / 1000.0,
        atmosphere.density_at(altitude_m),
    );
}
