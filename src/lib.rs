//! Bevy USPSA — a 3D first-person practical-shooting (USPSA) range. Make ready,
//! wait for the buzzer, then move around and shoot: paper targets with A/C/D
//! zones and steel poppers score, no-shoots penalize. Scored on hit factor.
//!
//! Desktop: WASD to move, mouse to look, left-click to fire.
//! Mobile: left thumb to move, drag the right side to look, FIRE to shoot.
//!
//! Targets and the range are built from primitive meshes (no art assets) and
//! every sound is synthesized at startup (no audio files).

mod audio;
mod game;

use audio::{AudioAssets, Sfx, build_audio_assets, play_sfx};
use bevy::audio::{AudioSource, Volume};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use game::{MAG_SIZE, Match, Phase, Score, TKind};

const EYE_H: f32 = 1.6;
const MOVE_SPEED: f32 = 4.2;
const BOUND_X: f32 = 8.0;
const BOUND_Z_NEAR: f32 = 12.0;
const BOUND_Z_FAR: f32 = -1.0;

// HUD button geometry (in the 2D overlay's world space).
const BTN_W: f32 = 150.0;
const BTN_H: f32 = 48.0;
const MATCH_BTN_Y: f32 = 300.0;
const MATCH_BTN_XS: [f32; 3] = [-220.0, 0.0, 220.0];
const FIRE_POS: Vec2 = Vec2::new(440.0, -250.0);
const FIRE_R: f32 = 95.0;
const JOY_CENTER: Vec2 = Vec2::new(-440.0, -250.0);
const JOY_R: f32 = 120.0;

// ---------------------------------------------------------------------------
// Markers / resources
// ---------------------------------------------------------------------------

#[derive(Component)]
struct Player {
    yaw: f32,
    pitch: f32,
}
#[derive(Component)]
struct HudCam;
#[derive(Component)]
struct TargetEntity;
#[derive(Component)]
struct SteelPlate(usize);
#[derive(Component)]
struct Hole;

#[derive(Clone, Copy, PartialEq, Eq)]
enum HudKind {
    Time,
    Shots,
    Remain,
    Status,
    Ammo,
}
#[derive(Component)]
struct HudLabel(HudKind);
#[derive(Component)]
struct ResultText;
#[derive(Component)]
struct ResultPanel;
#[derive(Component)]
struct JoyBase;
#[derive(Component)]
struct JoyKnob;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Btn {
    Ready,
    Stop,
    Next,
    Fire,
    Reload,
}
#[derive(Component)]
struct BtnLabel(Btn);

#[derive(Resource, Clone)]
struct UiFont(Handle<Font>);

/// Per-frame control intent, filled by the input systems.
#[derive(Resource, Default)]
struct Controls {
    move_vec: Vec2, // x = strafe, y = forward
    look: Vec2,     // yaw delta, pitch delta
    fire: bool,
}

/// Tracks which touches drive the move stick and the look drag.
#[derive(Resource, Default)]
struct TouchPad {
    move_id: Option<u64>,
    move_origin: Vec2,
    move_world: Vec2,
    look_id: Option<u64>,
    look_last: Vec2,
}

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

#[bevy_main]
pub fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy USPSA Match".to_string(),
                resolution: (1100u32, 820u32).into(),
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.53, 0.62, 0.75))) // sky
        .insert_resource(Match::new())
        .init_resource::<Controls>()
        .init_resource::<TouchPad>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                gather_input,
                apply_look_move,
                fire_system,
                clock,
                spawn_targets,
                sync_steel,
                hud,
                play_pending_sounds,
            )
                .chain(),
        )
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    // First-person camera = the player's eye.
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            ..default()
        },
        Tonemapping::None,
        AmbientLight {
            color: Color::WHITE,
            brightness: 380.0,
            ..default()
        },
        Transform::from_xyz(0.0, EYE_H, 7.0),
        Player { yaw: 0.0, pitch: 0.0 },
    ));
    // 2D HUD camera drawn on top.
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::AutoMin {
                min_width: 1120.0,
                min_height: 840.0,
            },
            ..OrthographicProjection::default_2d()
        }),
        HudCam,
    ));

    // Sun.
    commands.spawn((
        DirectionalLight {
            illuminance: 9000.0,
            ..default()
        },
        Transform::from_xyz(4.0, 10.0, 6.0).looking_at(Vec3::new(0.0, 1.0, -6.0), Vec3::Y),
    ));

    // Ground + back berm + side walls.
    let ground_mat = materials.add(Color::srgb(0.33, 0.40, 0.25));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(40.0, 0.2, 44.0))),
        MeshMaterial3d(ground_mat.clone()),
        Transform::from_xyz(0.0, -0.1, -8.0),
    ));
    let berm_mat = materials.add(Color::srgb(0.42, 0.36, 0.27));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(40.0, 6.0, 1.0))),
        MeshMaterial3d(berm_mat.clone()),
        Transform::from_xyz(0.0, 2.5, -13.5),
    ));
    for sx in [-16.0f32, 16.0] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(1.0, 6.0, 40.0))),
            MeshMaterial3d(berm_mat.clone()),
            Transform::from_xyz(sx, 2.5, -6.0),
        ));
    }

    // Font + audio.
    let font = fonts.add(Font::try_from_bytes(FONT_BYTES.to_vec()).expect("valid font"));
    commands.insert_resource(UiFont(font.clone()));
    let assets = build_audio_assets(&mut sources);
    commands.spawn((
        AudioPlayer::new(assets.music.clone()),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.26)),
    ));
    commands.insert_resource(assets);

    build_hud(&mut commands, &font);
}

// ---------------------------------------------------------------------------
// HUD construction
// ---------------------------------------------------------------------------

fn text_font(font: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: font.clone(),
        font_size: size,
        ..default()
    }
}

#[allow(clippy::too_many_arguments)]
fn hud_text(
    commands: &mut Commands,
    font: &Handle<Font>,
    s: &str,
    size: f32,
    color: Color,
    x: f32,
    y: f32,
    marker: impl Bundle,
) {
    commands.spawn((
        Text2d::new(s),
        text_font(font, size),
        TextColor(color),
        Transform::from_xyz(x, y, 10.0),
        marker,
    ));
}

fn hud_sprite(commands: &mut Commands, color: Color, size: Vec2, pos: Vec2, z: f32, marker: impl Bundle) {
    commands.spawn((
        Sprite {
            color,
            custom_size: Some(size),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, z),
        marker,
    ));
}

fn build_hud(commands: &mut Commands, font: &Handle<Font>) {
    let gold = Color::srgb(0.95, 0.85, 0.4);
    hud_text(commands, font, "0.00s", 38.0, gold, 0.0, 392.0, HudLabel(HudKind::Time));
    hud_text(commands, font, "Shots: 0", 22.0, Color::WHITE, -510.0, 392.0, HudLabel(HudKind::Shots));
    hud_text(commands, font, "", 22.0, Color::WHITE, 510.0, 392.0, HudLabel(HudKind::Remain));
    hud_text(commands, font, "", 21.0, Color::srgb(1.0, 0.96, 0.8), 0.0, 352.0, HudLabel(HudKind::Status));
    hud_text(commands, font, "", 26.0, Color::srgb(1.0, 0.93, 0.6), 250.0, -250.0, HudLabel(HudKind::Ammo));

    // Crosshair (two thin bars).
    hud_sprite(commands, Color::srgb(0.95, 0.95, 0.95), Vec2::new(26.0, 3.0), Vec2::ZERO, 10.0, ());
    hud_sprite(commands, Color::srgb(0.95, 0.95, 0.95), Vec2::new(3.0, 26.0), Vec2::ZERO, 10.0, ());

    // Match-control buttons.
    spawn_button(commands, font, Btn::Ready, Vec2::new(MATCH_BTN_XS[0], MATCH_BTN_Y), Vec2::new(BTN_W, BTN_H));
    spawn_button(commands, font, Btn::Stop, Vec2::new(MATCH_BTN_XS[1], MATCH_BTN_Y), Vec2::new(BTN_W, BTN_H));
    spawn_button(commands, font, Btn::Next, Vec2::new(MATCH_BTN_XS[2], MATCH_BTN_Y), Vec2::new(BTN_W, BTN_H));
    // FIRE button + RELOAD button (above it).
    spawn_button(commands, font, Btn::Fire, FIRE_POS, Vec2::new(FIRE_R * 2.0, FIRE_R * 2.0));
    spawn_button(commands, font, Btn::Reload, Vec2::new(FIRE_POS.x, -110.0), Vec2::new(190.0, 54.0));

    // Move joystick (hidden until touched).
    hud_sprite(commands, Color::srgba(1.0, 1.0, 1.0, 0.12), Vec2::splat(JOY_R * 2.0), JOY_CENTER, 9.0, JoyBase);
    hud_sprite(commands, Color::srgba(1.0, 1.0, 1.0, 0.28), Vec2::splat(70.0), JOY_CENTER, 9.5, JoyKnob);

    hud_text(
        commands,
        font,
        "Move: left thumb   Look: drag right   FIRE to shoot   RELOAD when low   (Space=ready/stop, R=reload, N=next)",
        15.0,
        Color::srgb(0.85, 0.88, 0.85),
        0.0,
        -405.0,
        (),
    );

    // Results overlay (hidden until a stage is scored).
    commands.spawn((
        Sprite {
            color: Color::srgba(0.0, 0.0, 0.0, 0.62),
            custom_size: Some(Vec2::new(900.0, 470.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 30.0, 11.0),
        Visibility::Hidden,
        ResultPanel,
    ));
    commands.spawn((
        Text2d::new(""),
        text_font(font, 26.0),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 30.0, 12.0),
        Visibility::Hidden,
        ResultText,
    ));
}

fn spawn_button(commands: &mut Commands, font: &Handle<Font>, kind: Btn, pos: Vec2, size: Vec2) {
    commands
        .spawn((
            Sprite {
                color: Color::srgb(0.3, 0.3, 0.35),
                custom_size: Some(size),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 9.0),
            kind,
        ))
        .with_children(|p| {
            p.spawn((
                Text2d::new(""),
                BtnLabel(kind),
                text_font(font, if kind == Btn::Fire { 26.0 } else { 19.0 }),
                TextColor(Color::WHITE),
                Transform::from_xyz(0.0, 0.0, 1.0),
            ));
        });
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

fn gather_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    touches: Res<Touches>,
    windows: Query<&Window>,
    hud_cam: Query<(&Camera, &GlobalTransform), With<HudCam>>,
    buttons: Query<(&Btn, &Transform, &Sprite)>,
    mut controls: ResMut<Controls>,
    mut pad: ResMut<TouchPad>,
    mut m: ResMut<Match>,
) {
    *controls = Controls::default();

    // ---- Desktop: keyboard + mouse ----
    let mut mv = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        mv.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        mv.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        mv.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        mv.x += 1.0;
    }
    if mv != Vec2::ZERO {
        controls.move_vec = mv.normalize();
    }
    if mouse_motion.delta != Vec2::ZERO {
        controls.look += Vec2::new(-mouse_motion.delta.x, -mouse_motion.delta.y) * 0.0022;
    }
    if mouse.just_pressed(MouseButton::Left) {
        controls.fire = true;
    }
    if keys.just_pressed(KeyCode::Space) {
        match m.phase {
            Phase::Running => m.stop(),
            Phase::Idle | Phase::Scored => m.make_ready(),
            _ => {}
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        m.reload();
    }
    if keys.just_pressed(KeyCode::KeyN) && m.phase != Phase::Running {
        m.next_stage();
    }

    // ---- Touch ----
    let (Ok(window), Ok((cam, cam_tf))) = (windows.single(), hud_cam.single()) else {
        return;
    };
    let to_world = |p: Vec2| cam.viewport_to_world_2d(cam_tf, p).ok();
    let _ = window;

    let hit_button = |world: Vec2| -> Option<Btn> {
        for (btn, tf, sprite) in &buttons {
            let c = tf.translation.truncate();
            let half = sprite.custom_size.unwrap_or(Vec2::new(BTN_W, BTN_H)) * 0.5;
            if (world.x - c.x).abs() <= half.x && (world.y - c.y).abs() <= half.y {
                return Some(*btn);
            }
        }
        None
    };

    for t in touches.iter_just_pressed() {
        let Some(world) = to_world(t.position()) else { continue };
        if let Some(btn) = hit_button(world) {
            match btn {
                Btn::Fire if m.phase == Phase::Running => controls.fire = true,
                Btn::Reload => m.reload(),
                Btn::Ready if matches!(m.phase, Phase::Idle | Phase::Scored) => m.make_ready(),
                Btn::Stop if m.phase == Phase::Running => m.stop(),
                Btn::Next if m.phase != Phase::Running => m.next_stage(),
                _ => {}
            }
            continue;
        }
        // Otherwise: left half = move stick, right half = look.
        if world.x < 0.0 && pad.move_id.is_none() {
            pad.move_id = Some(t.id());
            pad.move_origin = world;
            pad.move_world = world;
        } else if pad.look_id.is_none() {
            pad.look_id = Some(t.id());
            pad.look_last = world;
        }
    }

    for t in touches.iter() {
        let Some(world) = to_world(t.position()) else { continue };
        if pad.move_id == Some(t.id()) {
            pad.move_world = world;
            let d = (world - pad.move_origin) / JOY_R;
            controls.move_vec = Vec2::new(d.x.clamp(-1.0, 1.0), d.y.clamp(-1.0, 1.0));
        } else if pad.look_id == Some(t.id()) {
            let delta = world - pad.look_last;
            pad.look_last = world;
            controls.look += Vec2::new(-delta.x, delta.y) * 0.0022;
        }
    }
    for t in touches.iter_just_released() {
        if pad.move_id == Some(t.id()) {
            pad.move_id = None;
        }
        if pad.look_id == Some(t.id()) {
            pad.look_id = None;
        }
    }
}

fn apply_look_move(
    time: Res<Time>,
    controls: Res<Controls>,
    mut q: Query<(&mut Transform, &mut Player)>,
) {
    let Ok((mut tf, mut player)) = q.single_mut() else { return };
    player.yaw += controls.look.x;
    player.pitch = (player.pitch + controls.look.y).clamp(-1.4, 1.4);

    let yaw_rot = Quat::from_rotation_y(player.yaw);
    let forward = yaw_rot * Vec3::NEG_Z;
    let right = yaw_rot * Vec3::X;
    let mut pos = tf.translation;
    pos += (forward * controls.move_vec.y + right * controls.move_vec.x)
        * MOVE_SPEED
        * time.delta_secs();
    pos.x = pos.x.clamp(-BOUND_X, BOUND_X);
    pos.z = pos.z.clamp(BOUND_Z_FAR, BOUND_Z_NEAR);
    pos.y = EYE_H;
    tf.translation = pos;
    tf.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
}

fn fire_system(
    controls: Res<Controls>,
    mut m: ResMut<Match>,
    cam: Query<&GlobalTransform, With<Player>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !controls.fire || m.phase != Phase::Running {
        return;
    }
    if m.reloading {
        return;
    }
    if m.ammo == 0 {
        m.dry_fire(); // click on an empty chamber
        return;
    }
    let Ok(gt) = cam.single() else { return };
    let origin = gt.translation();
    let dir = *gt.forward();
    let res = m.shoot(origin, dir);
    if res.on_target {
        let color = if res.bad {
            Color::srgb(0.85, 0.1, 0.1)
        } else {
            Color::srgb(0.05, 0.05, 0.05)
        };
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.035, 0.035, 0.01))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                ..default()
            })),
            Transform::from_translation(res.point + Vec3::new(0.0, 0.0, 0.03)),
            Hole,
        ));
    }
}

fn clock(time: Res<Time>, mut m: ResMut<Match>) {
    m.tick(time.delta_secs());
}

// ---------------------------------------------------------------------------
// Target meshes
// ---------------------------------------------------------------------------

fn spawn_targets(
    mut m: ResMut<Match>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    old: Query<Entity, With<TargetEntity>>,
    holes: Query<Entity, With<Hole>>,
) {
    if m.clear_marks {
        for e in &holes {
            commands.entity(e).despawn();
        }
        m.clear_marks = false;
    }
    if !m.rebuild {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    for e in &holes {
        commands.entity(e).despawn();
    }

    let tan_d = materials.add(Color::srgb(0.80, 0.67, 0.46));
    let tan_c = materials.add(Color::srgb(0.70, 0.55, 0.35));
    let tan_a = materials.add(Color::srgb(0.60, 0.45, 0.28));
    let steel_mat = materials.add(Color::srgb(0.74, 0.76, 0.80));
    let post_mat = materials.add(Color::srgb(0.2, 0.2, 0.22));
    let white = materials.add(Color::srgb(0.92, 0.92, 0.9));
    let red = materials.add(Color::srgb(0.6, 0.15, 0.15));

    for (i, t) in m.targets.iter().enumerate() {
        match t.kind {
            TKind::Paper => {
                let w = t.hw * 2.0;
                let h = t.hh * 2.0;
                spawn_quad(&mut commands, &mut meshes, &tan_d, t.center, w, h, 0.04);
                spawn_quad(&mut commands, &mut meshes, &tan_c, t.center + Vec3::Z * 0.01, w * 0.7, h * 0.7, 0.03);
                spawn_quad(&mut commands, &mut meshes, &tan_a, t.center + Vec3::Z * 0.02, w * 0.34, h * 0.34, 0.03);
            }
            TKind::NoShoot => {
                let w = t.hw * 2.0;
                let h = t.hh * 2.0;
                spawn_quad(&mut commands, &mut meshes, &red, t.center, w + 0.06, h + 0.06, 0.03);
                spawn_quad(&mut commands, &mut meshes, &white, t.center + Vec3::Z * 0.01, w, h, 0.04);
            }
            TKind::Steel => {
                // post
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.05, t.center.y, 0.05))),
                    MeshMaterial3d(post_mat.clone()),
                    Transform::from_xyz(t.center.x, t.center.y / 2.0, t.center.z),
                    TargetEntity,
                ));
                // plate (rotates when knocked down)
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(t.hw * 2.0, t.hw * 2.0, 0.05))),
                    MeshMaterial3d(steel_mat.clone()),
                    Transform::from_translation(t.center),
                    TargetEntity,
                    SteelPlate(i),
                ));
            }
        }
    }
    m.rebuild = false;
}

fn spawn_quad(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<StandardMaterial>,
    center: Vec3,
    w: f32,
    h: f32,
    depth: f32,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(w, h, depth))),
        MeshMaterial3d(mat.clone()),
        Transform::from_translation(center),
        TargetEntity,
    ));
}

/// Knocked-down steel tips back and drops.
fn sync_steel(m: Res<Match>, mut plates: Query<(&SteelPlate, &mut Transform)>) {
    for (plate, mut tf) in &mut plates {
        let t = &m.targets[plate.0];
        if t.down {
            tf.rotation = Quat::from_rotation_x(-1.3);
            tf.translation = t.center + Vec3::new(0.0, -t.hw, -t.hw);
        } else {
            tf.rotation = Quat::IDENTITY;
            tf.translation = t.center;
        }
    }
}

// ---------------------------------------------------------------------------
// HUD update
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn hud(
    m: Res<Match>,
    pad: Res<TouchPad>,
    mut q: ParamSet<(
        Query<(&HudLabel, &mut Text2d)>,
        Query<(&BtnLabel, &mut Text2d)>,
        Query<(&mut Text2d, &mut Visibility), With<ResultText>>,
        Query<&mut Visibility, With<ResultPanel>>,
        Query<&mut Visibility, With<JoyBase>>,
        Query<(&mut Transform, &mut Visibility), With<JoyKnob>>,
    )>,
    mut btn_sprites: Query<(&Btn, &mut Sprite)>,
) {
    let remaining = m.targets.iter().filter(|t| !t.satisfied()).count();
    for (label, mut text) in &mut q.p0() {
        text.0 = match label.0 {
            HudKind::Time => match m.phase {
                Phase::Waiting => "Stand by…".into(),
                _ => format!("{:.2}s", m.elapsed),
            },
            HudKind::Shots => format!("Shots: {}", m.shots),
            HudKind::Remain => match m.phase {
                Phase::Running | Phase::Waiting => format!("To go: {}", remaining),
                _ => String::new(),
            },
            HudKind::Status => m.status.clone(),
            HudKind::Ammo => match m.phase {
                Phase::Running if m.reloading => "RELOADING…".into(),
                Phase::Running if m.ammo == 0 => "RELOAD! (R)".into(),
                Phase::Running => format!("Ammo {} / {}", m.ammo, MAG_SIZE),
                _ => String::new(),
            },
        };
    }
    for (label, mut text) in &mut q.p1() {
        text.0 = match label.0 {
            Btn::Ready => "MAKE READY".into(),
            Btn::Stop => "STOP".into(),
            Btn::Next => "NEXT STAGE".into(),
            Btn::Fire => "FIRE".into(),
            Btn::Reload => "RELOAD".into(),
        };
    }
    for (kind, mut sprite) in &mut btn_sprites {
        sprite.color = button_color(*kind, btn_active(&m, *kind));
    }

    // Results overlay.
    let show = m.phase == Phase::Scored;
    let vis = if show { Visibility::Visible } else { Visibility::Hidden };
    if let Ok((mut text, mut v)) = q.p2().single_mut() {
        *v = vis;
        if let Some(s) = m.result {
            text.0 = result_text(&s, m.stage_num);
        }
    }
    if let Ok(mut v) = q.p3().single_mut() {
        *v = vis;
    }

    // Joystick visual.
    let active = pad.move_id.is_some();
    let jvis = if active { Visibility::Visible } else { Visibility::Hidden };
    if let Ok(mut v) = q.p4().single_mut() {
        *v = jvis;
    }
    if let Ok((mut tf, mut v)) = q.p5().single_mut() {
        *v = jvis;
        let knob = if active {
            JOY_CENTER + (pad.move_world - pad.move_origin).clamp_length_max(JOY_R)
        } else {
            JOY_CENTER
        };
        tf.translation.x = knob.x;
        tf.translation.y = knob.y;
    }
}

fn result_text(s: &Score, stage_num: u32) -> String {
    format!(
        "STAGE {} CLEAR\n\nHits  A:{}  C:{}  D:{}\nMisses: {}    No-shoots: {}\nSteel: {}/{}\nPoints: {}    Time: {:.2}s\n\nHIT FACTOR  {:.2}\n\nNEXT STAGE \u{2192}",
        stage_num, s.a, s.c, s.d, s.mikes, s.ns, s.steel_down, s.steel_total, s.points, s.time, s.hit_factor
    )
}

fn btn_active(m: &Match, kind: Btn) -> bool {
    match kind {
        Btn::Ready => matches!(m.phase, Phase::Idle | Phase::Scored),
        Btn::Stop => m.phase == Phase::Running,
        Btn::Fire => m.can_fire(),
        Btn::Reload => m.phase == Phase::Running && !m.reloading && m.ammo < MAG_SIZE,
        Btn::Next => m.phase != Phase::Running,
    }
}

fn button_color(kind: Btn, on: bool) -> Color {
    if !on {
        return Color::srgba(0.22, 0.22, 0.26, 0.6);
    }
    match kind {
        Btn::Ready => Color::srgb(0.30, 0.62, 0.38),
        Btn::Stop => Color::srgb(0.78, 0.30, 0.26),
        Btn::Next => Color::srgb(0.30, 0.50, 0.78),
        Btn::Fire => Color::srgb(0.85, 0.45, 0.2),
        Btn::Reload => Color::srgb(0.80, 0.70, 0.22),
    }
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

fn play_pending_sounds(mut commands: Commands, assets: Res<AudioAssets>, mut m: ResMut<Match>) {
    if m.pending.is_empty() {
        return;
    }
    let sounds: Vec<Sfx> = m.pending.drain(..).collect();
    for sfx in sounds {
        play_sfx(&mut commands, &assets, sfx);
    }
}
