//! Procedurally synthesized sound effects and background music.
//!
//! Everything is generated as 16-bit PCM WAV bytes at startup and handed to
//! Bevy as `AudioSource` assets, so the game ships with zero audio files.

use bevy::audio::{AudioSource, Volume};
use bevy::prelude::*;
use std::f32::consts::TAU;

const SAMPLE_RATE: u32 = 44_100;

/// One-shot sound effects.
#[derive(Clone, Copy)]
pub enum Sfx {
    Buzzer,
    Shot,
    Steel,
    Paper,
    Penalty,
    Clear,
    Reload,
    Empty,
}

#[derive(Resource)]
pub struct AudioAssets {
    buzzer: Handle<AudioSource>,
    shot: Handle<AudioSource>,
    steel: Handle<AudioSource>,
    paper: Handle<AudioSource>,
    penalty: Handle<AudioSource>,
    clear: Handle<AudioSource>,
    reload: Handle<AudioSource>,
    empty: Handle<AudioSource>,
    pub music: Handle<AudioSource>,
}

impl AudioAssets {
    fn get(&self, sfx: Sfx) -> Handle<AudioSource> {
        match sfx {
            Sfx::Buzzer => self.buzzer.clone(),
            Sfx::Shot => self.shot.clone(),
            Sfx::Steel => self.steel.clone(),
            Sfx::Paper => self.paper.clone(),
            Sfx::Penalty => self.penalty.clone(),
            Sfx::Clear => self.clear.clone(),
            Sfx::Reload => self.reload.clone(),
            Sfx::Empty => self.empty.clone(),
        }
    }
}

pub fn build_audio_assets(sources: &mut Assets<AudioSource>) -> AudioAssets {
    let mut mk = |s: Vec<f32>| sources.add(AudioSource { bytes: wav_bytes(&s).into() });
    AudioAssets {
        buzzer: mk(buzzer()),
        shot: mk(shot()),
        steel: mk(steel()),
        paper: mk(paper()),
        penalty: mk(penalty()),
        clear: mk(clear()),
        reload: mk(reload()),
        empty: mk(empty()),
        music: mk(music()),
    }
}

pub fn play_sfx(commands: &mut Commands, assets: &AudioAssets, sfx: Sfx) {
    let vol = match sfx {
        Sfx::Shot => 0.5,
        Sfx::Buzzer => 0.8,
        _ => 0.65,
    };
    commands.spawn((
        AudioPlayer::new(assets.get(sfx)),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(vol)),
    ));
}

// ---------------------------------------------------------------------------
// Synthesis primitives
// ---------------------------------------------------------------------------

fn dur(secs: f32) -> usize {
    (secs * SAMPLE_RATE as f32) as usize
}

fn add_tone(buf: &mut [f32], start: usize, freq: f32, secs: f32, amp: f32, decay: f32) {
    let n = dur(secs);
    for i in 0..n {
        let idx = start + i;
        if idx >= buf.len() {
            break;
        }
        let t = i as f32 / SAMPLE_RATE as f32;
        let env = (-decay * t).exp();
        buf[idx] += amp * env * (TAU * freq * t).sin();
    }
}

fn add_note(buf: &mut [f32], start: usize, freq: f32, secs: f32, amp: f32) {
    let n = dur(secs);
    for i in 0..n {
        let idx = start + i;
        if idx >= buf.len() {
            break;
        }
        let t = i as f32 / n as f32;
        let env = if t < 0.04 { t / 0.04 } else { (1.0 - t).powf(1.4) };
        let ph = TAU * freq * (i as f32 / SAMPLE_RATE as f32);
        let s = ph.sin() + 0.4 * (2.0 * ph).sin() + 0.2 * (3.0 * ph).sin();
        buf[idx] += amp * env * s / 1.6;
    }
}

/// A distorted "power chord" stab (root + fifth + octave, soft-clipped).
fn add_stab(buf: &mut [f32], start: usize, root: f32, secs: f32, amp: f32) {
    let n = dur(secs);
    for i in 0..n {
        let idx = start + i;
        if idx >= buf.len() {
            break;
        }
        let t = i as f32 / n as f32;
        let env = if t < 0.02 { t / 0.02 } else { (1.0 - t).powf(1.2) };
        let tt = i as f32 / SAMPLE_RATE as f32;
        let mut s = (TAU * root * tt).sin()
            + (TAU * root * 1.5 * tt).sin()
            + (TAU * root * 2.0 * tt).sin();
        s = (s * 1.6).tanh(); // soft distortion
        buf[idx] += amp * env * s * 0.4;
    }
}

/// One-pole-filtered white noise burst with an exponential decay.
fn noise_burst(buf: &mut [f32], start: usize, secs: f32, amp: f32, lp: f32, decay: f32) {
    let n = dur(secs);
    let mut seed: u32 = 0x1234_5678 ^ (start as u32).wrapping_mul(2_654_435_761);
    let mut last = 0.0f32;
    for i in 0..n {
        let idx = start + i;
        if idx >= buf.len() {
            break;
        }
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let white = (seed >> 9) as f32 / (1u32 << 23) as f32 - 1.0;
        last = last * lp + white * (1.0 - lp);
        let t = i as f32 / SAMPLE_RATE as f32;
        let env = (-decay * t).exp();
        buf[idx] += amp * env * last;
    }
}

// ---------------------------------------------------------------------------
// Sound effects
// ---------------------------------------------------------------------------

/// Shot-timer start signal: a loud, buzzy high beep.
fn buzzer() -> Vec<f32> {
    let mut buf = vec![0.0; dur(0.45)];
    let n = buf.len();
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let tn = i as f32 / n as f32;
        let env = if tn < 0.02 { tn / 0.02 } else if tn > 0.9 { (1.0 - tn) / 0.1 } else { 1.0 };
        // a couple of detuned saw-ish tones for a harsh beep
        let mut s = 0.0;
        for h in 1..=6 {
            s += (TAU * 2300.0 * h as f32 * t).sin() / h as f32;
        }
        s += (TAU * 2308.0 * t).sin();
        buf[i] = env * s * 0.22;
    }
    buf
}

/// Gunshot: a noise crack over a low thump.
fn shot() -> Vec<f32> {
    let mut buf = vec![0.0; dur(0.16)];
    noise_burst(&mut buf, 0, 0.13, 0.9, 0.25, 55.0); // bright crack
    noise_burst(&mut buf, 0, 0.10, 0.5, 0.7, 30.0); // body
    add_tone(&mut buf, 0, 95.0, 0.12, 0.8, 40.0); // low thump
    add_tone(&mut buf, 0, 60.0, 0.12, 0.6, 35.0);
    buf
}

/// Steel hit: a bright metallic ping with inharmonic partials.
fn steel() -> Vec<f32> {
    let mut buf = vec![0.0; dur(0.5)];
    for (f, a) in [(1850.0, 0.5), (2790.0, 0.35), (3460.0, 0.25), (5200.0, 0.15)] {
        add_tone(&mut buf, 0, f, 0.5, a, 9.0);
    }
    noise_burst(&mut buf, 0, 0.04, 0.4, 0.1, 80.0); // initial tick
    buf
}

/// Paper hit: a short dull thwack.
fn paper() -> Vec<f32> {
    let mut buf = vec![0.0; dur(0.08)];
    noise_burst(&mut buf, 0, 0.07, 0.6, 0.55, 70.0);
    add_tone(&mut buf, 0, 220.0, 0.06, 0.3, 60.0);
    buf
}

/// Penalty (no-shoot / miss): a low descending honk.
fn penalty() -> Vec<f32> {
    let n = dur(0.34);
    let mut buf = vec![0.0; n];
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let tn = i as f32 / n as f32;
        let freq = 300.0 - 150.0 * tn;
        let env = (1.0 - tn).powf(1.2);
        let mut s = 0.0;
        for h in 1..=4 {
            s += (TAU * freq * h as f32 * t).sin() / h as f32;
        }
        buf[i] = env * s * 0.3;
    }
    buf
}

/// Reload: a magazine seat "thunk" then a slide "cha-chunk".
fn reload() -> Vec<f32> {
    let mut buf = vec![0.0; dur(0.42)];
    noise_burst(&mut buf, 0, 0.07, 0.5, 0.6, 55.0); // mag seated
    add_tone(&mut buf, 0, 150.0, 0.07, 0.4, 50.0);
    add_tone(&mut buf, dur(0.18), 1400.0, 0.05, 0.4, 70.0); // slide back
    noise_burst(&mut buf, dur(0.18), 0.04, 0.3, 0.2, 90.0);
    add_tone(&mut buf, dur(0.28), 900.0, 0.06, 0.5, 60.0); // slide forward clack
    noise_burst(&mut buf, dur(0.28), 0.04, 0.4, 0.15, 90.0);
    buf
}

/// Dry-fire on an empty chamber: a single sharp click.
fn empty() -> Vec<f32> {
    let mut buf = vec![0.0; dur(0.05)];
    add_tone(&mut buf, 0, 2400.0, 0.03, 0.4, 120.0);
    noise_burst(&mut buf, 0, 0.03, 0.4, 0.1, 130.0);
    buf
}

/// Stage clear: a quick rising major arpeggio.
fn clear() -> Vec<f32> {
    let mut buf = vec![0.0; dur(0.7)];
    let notes = [523.25, 659.25, 783.99, 1046.50];
    for (i, &f) in notes.iter().enumerate() {
        add_note(&mut buf, dur(0.12 * i as f32), f, 0.28, 0.5);
    }
    buf
}

// ---------------------------------------------------------------------------
// Background music — a driving blues-rock loop in E.
// ---------------------------------------------------------------------------

fn music() -> Vec<f32> {
    let bpm = 132.0;
    let beat = 60.0 / bpm;
    let eighth = beat / 2.0;
    let bars = 4;
    let total = dur(beat * 4.0 * bars as f32);
    let mut buf = vec![0.0; total];

    // E blues-ish: bass roots per bar (E, A, E, B), chord stabs on the off-beats.
    let roots = [82.41, 110.00, 82.41, 123.47]; // E2 A2 E2 B2
    for bar in 0..bars {
        let bar_start = dur(beat * 4.0 * bar as f32);
        let root = roots[bar];
        // steady eighth-note bass walking root-fifth
        for e in 0..8 {
            let f = if e % 2 == 0 { root } else { root * 1.5 };
            add_note(&mut buf, bar_start + dur(eighth * e as f32), f, eighth * 0.9, 0.22);
        }
        // power-chord stabs on beats 1 and 3
        add_stab(&mut buf, bar_start, root * 2.0, beat * 0.45, 0.5);
        add_stab(&mut buf, bar_start + dur(beat * 2.0), root * 2.0, beat * 0.45, 0.5);
        // a simple lead riff (minor pentatonic flavor) over the bar
        let lead = [root * 4.0, root * 4.0 * 1.2, root * 4.0 * 1.5, root * 4.0 * 1.33];
        for (i, &f) in lead.iter().enumerate() {
            add_note(&mut buf, bar_start + dur(beat * i as f32), f, beat * 0.5, 0.12);
        }
    }

    // light normalize
    let peak = buf.iter().fold(0.0f32, |m, &s| m.max(s.abs())).max(1e-3);
    if peak > 0.9 {
        let g = 0.9 / peak;
        for s in &mut buf {
            *s *= g;
        }
    }
    buf
}

// ---------------------------------------------------------------------------
// WAV container
// ---------------------------------------------------------------------------

fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let channels: u16 = 1;
    let bits: u16 = 16;
    let byte_rate = SAMPLE_RATE * channels as u32 * (bits as u32 / 8);
    let block_align = channels * (bits / 8);
    let data_len = (samples.len() * 2) as u32;

    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    v.extend_from_slice(&byte_rate.to_le_bytes());
    v.extend_from_slice(&block_align.to_le_bytes());
    v.extend_from_slice(&bits.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let i = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        v.extend_from_slice(&i.to_le_bytes());
    }
    v
}
