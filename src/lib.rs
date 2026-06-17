//! Bevy USPSA — a touch shooting game built like a practical-shooting (USPSA)
//! stage. Make ready, wait for the buzzer, then tap the targets: paper with
//! A/C/D zones and steel poppers score, no-shoots penalize. You're scored on
//! hit factor (points / time).
//!
//! Cards/targets are drawn as sprites (no art assets) and all sound is
//! synthesized at startup (no audio files).

mod audio;
mod stage;

use audio::{AudioAssets, Sfx, build_audio_assets, play_sfx};
use bevy::audio::{AudioSource, Volume};
use bevy::prelude::*;
use stage::{Phase, Stage, TKind, Target};

// ---------------------------------------------------------------------------
// Layout / colors
// ---------------------------------------------------------------------------

const BTN_W: f32 = 190.0;
const BTN_H: f32 = 54.0;
const BTN_Y: f32 = -372.0;
const BTN_XS: [f32; 3] = [-250.0, 0.0, 250.0];

const TAN_D: Color = Color::srgb(0.80, 0.67, 0.46);
const TAN_C: Color = Color::srgb(0.70, 0.55, 0.35);
const TAN_A: Color = Color::srgb(0.60, 0.45, 0.28);
const STEEL_UP: Color = Color::srgb(0.74, 0.76, 0.80);
const STEEL_DOWN: Color = Color::srgb(0.34, 0.35, 0.38);

// ---------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------

#[derive(Component)]
struct Drawn; // any sprite/text re-spawned on redraw
#[derive(Component)]
struct TimeText;
#[derive(Component)]
struct ShotsText;
#[derive(Component)]
struct RemainText;
#[derive(Component)]
struct StatusText;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Btn {
    Ready,
    Stop,
    Next,
}

#[derive(Component)]
struct BtnLabel(Btn);

#[derive(Resource, Clone)]
struct UiFont(Handle<Font>);

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
        .insert_resource(ClearColor(Color::srgb(0.16, 0.18, 0.14))) // dirt/range tan-green
        .insert_resource(Stage::new())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                keyboard_input,
                pointer_input,
                clock,
                update_hud,
                redraw,
                play_pending_sounds,
            )
                .chain(),
        )
        .run();
}

fn setup(
    mut commands: Commands,
    mut sources: ResMut<Assets<AudioSource>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::AutoMin {
                min_width: 1120.0,
                min_height: 840.0,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));

    let font = fonts.add(Font::try_from_bytes(FONT_BYTES.to_vec()).expect("valid font"));
    commands.insert_resource(UiFont(font.clone()));

    let assets = build_audio_assets(&mut sources);
    commands.spawn((
        AudioPlayer::new(assets.music.clone()),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.28)),
    ));
    commands.insert_resource(assets);

    let gold = Color::srgb(0.95, 0.85, 0.4);
    spawn_text(&mut commands, &font, "BEVY USPSA", 30.0, gold, 0.0, 392.0, 21.0, ());
    spawn_text(&mut commands, &font, "0.00s", 40.0, gold, 0.0, 345.0, 1.0, TimeText);
    spawn_text(&mut commands, &font, "Shots: 0", 22.0, Color::WHITE, -470.0, 360.0, 1.0, ShotsText);
    spawn_text(&mut commands, &font, "", 22.0, Color::WHITE, 470.0, 360.0, 1.0, RemainText);
    spawn_text(
        &mut commands,
        &font,
        "",
        21.0,
        Color::srgb(1.0, 0.96, 0.8),
        0.0,
        -305.0,
        1.0,
        StatusText,
    );

    spawn_button(&mut commands, &font, Btn::Ready, BTN_XS[0]);
    spawn_button(&mut commands, &font, Btn::Stop, BTN_XS[1]);
    spawn_button(&mut commands, &font, Btn::Next, BTN_XS[2]);

    spawn_text(
        &mut commands,
        &font,
        "Space = Make ready / Stop     N = Next stage     Tap targets to shoot",
        15.0,
        Color::srgb(0.8, 0.85, 0.8),
        0.0,
        -410.0,
        1.0,
        (),
    );
}

fn text_font(font: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: font.clone(),
        font_size: size,
        ..default()
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_text(
    commands: &mut Commands,
    font: &Handle<Font>,
    text: &str,
    size: f32,
    color: Color,
    x: f32,
    y: f32,
    z: f32,
    marker: impl Bundle,
) {
    commands.spawn((
        Text2d::new(text),
        text_font(font, size),
        TextColor(color),
        Transform::from_xyz(x, y, z),
        marker,
    ));
}

fn spawn_button(commands: &mut Commands, font: &Handle<Font>, kind: Btn, x: f32) {
    let font = font.clone();
    commands
        .spawn((
            Sprite {
                color: Color::srgb(0.3, 0.3, 0.35),
                custom_size: Some(Vec2::new(BTN_W, BTN_H)),
                ..default()
            },
            Transform::from_xyz(x, BTN_Y, 2.0),
            kind,
        ))
        .with_children(|p| {
            p.spawn((
                Text2d::new(""),
                BtnLabel(kind),
                text_font(&font, 20.0),
                TextColor(Color::WHITE),
                Transform::from_xyz(0.0, 0.0, 1.0),
            ));
        });
}

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

fn clock(time: Res<Time>, mut stage: ResMut<Stage>) {
    stage.tick(time.delta_secs());
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

fn keyboard_input(keys: Res<ButtonInput<KeyCode>>, mut stage: ResMut<Stage>) {
    if keys.just_pressed(KeyCode::Space) {
        match stage.phase {
            Phase::Running => stage.stop(),
            Phase::Idle | Phase::Scored => stage.make_ready(),
            _ => {}
        }
    }
    if keys.just_pressed(KeyCode::KeyN) || keys.just_pressed(KeyCode::KeyR) {
        if stage.phase != Phase::Running {
            stage.next_stage();
        }
    }
}

fn pointer_input(
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    buttons: Query<(&Btn, &Transform)>,
    mut stage: ResMut<Stage>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((camera, cam_transform)) = cameras.single() else { return };

    let mut presses: Vec<Vec2> = Vec::new();
    if mouse.just_pressed(MouseButton::Left) {
        if let Some(c) = window.cursor_position() {
            presses.push(c);
        }
    }
    for t in touches.iter_just_pressed() {
        presses.push(t.position());
    }

    for screen in presses {
        let Ok(world) = camera.viewport_to_world_2d(cam_transform, screen) else {
            continue;
        };
        // Buttons take priority over shooting.
        let mut hit_button = false;
        for (btn, transform) in &buttons {
            let pos = transform.translation.truncate();
            let half = Vec2::new(BTN_W, BTN_H) * 0.5;
            if (world.x - pos.x).abs() <= half.x && (world.y - pos.y).abs() <= half.y {
                hit_button = true;
                if btn_active(&stage, *btn) {
                    match btn {
                        Btn::Ready => stage.make_ready(),
                        Btn::Stop => stage.stop(),
                        Btn::Next => stage.next_stage(),
                    }
                }
            }
        }
        if !hit_button && stage.phase == Phase::Running {
            stage.shoot(world);
        }
    }
}

fn btn_active(stage: &Stage, kind: Btn) -> bool {
    match kind {
        Btn::Ready => matches!(stage.phase, Phase::Idle | Phase::Scored),
        Btn::Stop => stage.phase == Phase::Running,
        Btn::Next => matches!(stage.phase, Phase::Idle | Phase::Scored),
    }
}

// ---------------------------------------------------------------------------
// HUD (every frame)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn update_hud(
    stage: Res<Stage>,
    mut texts: ParamSet<(
        Query<&mut Text2d, With<TimeText>>,
        Query<&mut Text2d, With<ShotsText>>,
        Query<&mut Text2d, With<RemainText>>,
        Query<&mut Text2d, With<StatusText>>,
        Query<(&BtnLabel, &mut Text2d)>,
    )>,
    mut btn_sprites: Query<(&Btn, &mut Sprite)>,
) {
    let time_str = match stage.phase {
        Phase::Waiting => "Stand by…".to_string(),
        _ => format!("{:.2}s", stage.elapsed),
    };
    if let Ok(mut t) = texts.p0().single_mut() {
        t.0 = time_str;
    }
    if let Ok(mut t) = texts.p1().single_mut() {
        t.0 = format!("Shots: {}", stage.shots);
    }
    if let Ok(mut t) = texts.p2().single_mut() {
        let remaining = stage.targets.iter().filter(|t| !t.satisfied()).count();
        t.0 = match stage.phase {
            Phase::Running | Phase::Waiting => format!("To go: {}", remaining),
            _ => String::new(),
        };
    }
    if let Ok(mut t) = texts.p3().single_mut() {
        t.0 = stage.status.clone();
    }
    for (label, mut text) in &mut texts.p4() {
        text.0 = match label.0 {
            Btn::Ready => "MAKE READY".into(),
            Btn::Stop => "STOP".into(),
            Btn::Next => "NEXT STAGE".into(),
        };
    }
    for (kind, mut sprite) in &mut btn_sprites {
        sprite.color = button_color(*kind, btn_active(&stage, *kind));
    }
}

fn button_color(kind: Btn, on: bool) -> Color {
    if !on {
        return Color::srgb(0.22, 0.22, 0.26);
    }
    match kind {
        Btn::Ready => Color::srgb(0.30, 0.62, 0.38),
        Btn::Stop => Color::srgb(0.78, 0.30, 0.26),
        Btn::Next => Color::srgb(0.30, 0.50, 0.78),
    }
}

// ---------------------------------------------------------------------------
// Rendering the stage (on dirty)
// ---------------------------------------------------------------------------

fn redraw(
    mut commands: Commands,
    mut stage: ResMut<Stage>,
    old: Query<Entity, With<Drawn>>,
    ui_font: Res<UiFont>,
) {
    if !stage.dirty {
        return;
    }
    let font = &ui_font.0;
    for e in &old {
        commands.entity(e).despawn();
    }

    for t in &stage.targets {
        draw_target(&mut commands, font, t);
    }
    for &p in &stage.bg_holes {
        rect(&mut commands, Color::srgb(0.1, 0.1, 0.09), Vec2::splat(7.0), p, 0.5);
    }

    if stage.phase == Phase::Scored {
        if let Some(s) = stage.result {
            draw_results(&mut commands, font, &s, stage.stage_num);
        }
    }

    stage.dirty = false;
}

fn rect(commands: &mut Commands, color: Color, size: Vec2, pos: Vec2, z: f32) {
    commands.spawn((
        Sprite {
            color,
            custom_size: Some(size),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, z),
        Drawn,
    ));
}

fn draw_target(commands: &mut Commands, font: &Handle<Font>, t: &Target) {
    match t.kind {
        TKind::Paper => {
            rect(commands, TAN_D, Vec2::new(t.hw * 2.0, t.hh * 2.0), t.pos, 0.0);
            rect(commands, TAN_C, Vec2::new(t.hw * 2.0 * 0.7, t.hh * 2.0 * 0.7), t.pos, 0.1);
            rect(commands, TAN_A, Vec2::new(t.hw * 2.0 * 0.34, t.hh * 2.0 * 0.34), t.pos, 0.2);
            draw_holes(commands, t);
        }
        TKind::NoShoot => {
            rect(commands, Color::srgb(0.12, 0.1, 0.08), Vec2::new(t.hw * 2.0 + 10.0, t.hh * 2.0 + 10.0), t.pos, 0.0);
            rect(commands, Color::srgb(0.92, 0.92, 0.9), Vec2::new(t.hw * 2.0, t.hh * 2.0), t.pos, 0.1);
            commands.spawn((
                Text2d::new("NO\nSHOOT"),
                text_font(font, 16.0),
                TextColor(Color::srgb(0.6, 0.2, 0.2)),
                Transform::from_xyz(t.pos.x, t.pos.y, 0.2),
                Drawn,
            ));
            draw_holes(commands, t);
        }
        TKind::Steel => {
            // stand
            rect(commands, Color::srgb(0.2, 0.2, 0.22), Vec2::new(10.0, 26.0), t.pos + Vec2::new(0.0, -t.hw - 8.0), -0.1);
            let color = if t.down { STEEL_DOWN } else { STEEL_UP };
            let mut tf = Transform::from_xyz(t.pos.x, t.pos.y, 0.0);
            if t.down {
                tf.rotation = Quat::from_rotation_z(1.3);
                tf.translation.y -= 18.0;
            }
            commands.spawn((
                Sprite {
                    color,
                    custom_size: Some(Vec2::splat(t.hw * 2.0)),
                    ..default()
                },
                tf,
                Drawn,
            ));
            // bolt highlight in the middle
            rect(commands, Color::srgb(0.55, 0.57, 0.6), Vec2::splat(12.0), t.pos, if t.down { -0.2 } else { 0.1 });
        }
    }
}

fn draw_holes(commands: &mut Commands, t: &Target) {
    for h in &t.holes {
        let color = if h.bad {
            Color::srgb(0.85, 0.12, 0.12)
        } else {
            Color::srgb(0.06, 0.06, 0.06)
        };
        rect(commands, color, Vec2::splat(8.0), t.pos + h.off, 0.4);
    }
}

fn draw_results(commands: &mut Commands, font: &Handle<Font>, s: &stage::Score, stage_num: u32) {
    // dim overlay over the play area (leaves the bottom button row visible)
    rect(commands, Color::srgba(0.0, 0.0, 0.0, 0.62), Vec2::new(2600.0, 720.0), Vec2::new(0.0, 60.0), 5.0);

    let body = format!(
        "Hits   A:{}   C:{}   D:{}\nMisses: {}    No-shoots: {}\nSteel: {}/{}\nPoints: {}     Time: {:.2}s",
        s.a, s.c, s.d, s.mikes, s.ns, s.steel_down, s.steel_total, s.points, s.time
    );

    commands.spawn((
        Text2d::new(format!("STAGE {} CLEAR", stage_num)),
        text_font(font, 34.0),
        TextColor(Color::srgb(0.95, 0.85, 0.4)),
        Transform::from_xyz(0.0, 150.0, 6.0),
        Drawn,
    ));
    commands.spawn((
        Text2d::new(body),
        text_font(font, 24.0),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 20.0, 6.0),
        Drawn,
    ));
    commands.spawn((
        Text2d::new(format!("HIT FACTOR  {:.2}", s.hit_factor)),
        text_font(font, 40.0),
        TextColor(Color::srgb(0.5, 0.9, 0.55)),
        Transform::from_xyz(0.0, -120.0, 6.0),
        Drawn,
    ));
    commands.spawn((
        Text2d::new("NEXT STAGE →"),
        text_font(font, 20.0),
        TextColor(Color::srgb(0.85, 0.9, 0.85)),
        Transform::from_xyz(0.0, -185.0, 6.0),
        Drawn,
    ));
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

fn play_pending_sounds(mut commands: Commands, assets: Res<AudioAssets>, mut stage: ResMut<Stage>) {
    if stage.pending.is_empty() {
        return;
    }
    let sounds: Vec<Sfx> = stage.pending.drain(..).collect();
    for sfx in sounds {
        play_sfx(&mut commands, &assets, sfx);
    }
}

/// Whether a target no longer needs to be engaged (used by the HUD counter).
impl Target {
    fn satisfied(&self) -> bool {
        match self.kind {
            TKind::Paper => self.hits.len() >= 2,
            TKind::Steel => self.down,
            TKind::NoShoot => true,
        }
    }
}
