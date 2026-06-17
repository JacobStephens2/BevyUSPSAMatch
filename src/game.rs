//! The 3D USPSA match: a downrange array of paper/steel/no-shoot targets, a
//! start-buzzer/timer phase machine, ray-cast shooting from the camera, and
//! hit-factor scoring.
//!
//! Scoring is USPSA **Minor**: A = 5, C = 3, D = 1; each paper needs two scoring
//! hits (best two count). A miss (unfilled required hit) or a standing steel is
//! −10; a no-shoot hit is −10. Hit factor = max(0, points) / time.

use crate::audio::Sfx;
use bevy::prelude::*;
use rand::Rng;

// Target geometry, in metres. Paper/no-shoots are vertical rectangles whose
// face normal points back toward the shooter (+Z); steel are vertical discs.
pub const PAPER_HW: f32 = 0.23;
pub const PAPER_HH: f32 = 0.30;
pub const PAPER_Y: f32 = 1.42; // near eye height so a level aim hits centre
pub const STEEL_R: f32 = 0.16;
pub const STEEL_Y: f32 = 1.30;

const PAPERS: usize = 5;
const STEEL: usize = 3;
const NOSHOOTS: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TKind {
    Paper,
    Steel,
    NoShoot,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    A,
    C,
    D,
}

impl Zone {
    fn points(self) -> i32 {
        match self {
            Zone::A => 5,
            Zone::C => 3,
            Zone::D => 1,
        }
    }
}

#[derive(Clone)]
pub struct Target {
    pub kind: TKind,
    /// Centre of the target face (its plane is z = center.z, normal +Z).
    pub center: Vec3,
    pub hw: f32,
    pub hh: f32,
    pub hits: Vec<Zone>,
    pub ns_hits: u32,
    pub down: bool,
}

impl Target {
    fn new(kind: TKind, center: Vec3) -> Self {
        let (hw, hh) = match kind {
            TKind::Steel => (STEEL_R, STEEL_R),
            _ => (PAPER_HW, PAPER_HH),
        };
        Target {
            kind,
            center,
            hw,
            hh,
            hits: Vec::new(),
            ns_hits: 0,
            down: false,
        }
    }

    pub fn satisfied(&self) -> bool {
        match self.kind {
            TKind::Paper => self.hits.len() >= 2,
            TKind::Steel => self.down,
            TKind::NoShoot => true,
        }
    }

    /// Ray (origin,dir) vs this target's vertical face. Returns (distance, local
    /// offset from centre) if it hits the front of the face.
    fn ray_hit(&self, origin: Vec3, dir: Vec3) -> Option<(f32, Vec2)> {
        if dir.z >= -1.0e-4 {
            return None; // pointing away from / parallel to the downrange faces
        }
        let t = (self.center.z - origin.z) / dir.z;
        if t <= 0.0 {
            return None;
        }
        let hit = origin + dir * t;
        let off = Vec2::new(hit.x - self.center.x, hit.y - self.center.y);
        let on = match self.kind {
            TKind::Steel => off.length() <= self.hw,
            _ => off.x.abs() <= self.hw && off.y.abs() <= self.hh,
        };
        on.then_some((t, off))
    }
}

fn paper_zone(off: Vec2, hw: f32, hh: f32) -> Zone {
    let d = (off.x / hw).abs().max((off.y / hh).abs());
    if d < 0.34 {
        Zone::A
    } else if d < 0.7 {
        Zone::C
    } else {
        Zone::D
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Phase {
    Idle,
    Waiting,
    Running,
    Scored,
}

#[derive(Clone, Copy, Default)]
pub struct Score {
    pub a: u32,
    pub c: u32,
    pub d: u32,
    pub mikes: u32,
    pub ns: u32,
    pub steel_down: u32,
    pub steel_total: u32,
    pub points: i32,
    pub time: f32,
    pub hit_factor: f32,
}

/// What a fired shot produced, for the renderer to place a bullet hole.
pub struct ShotResult {
    pub on_target: bool,
    pub point: Vec3,
    pub bad: bool,
}

#[derive(Resource)]
pub struct Match {
    pub phase: Phase,
    pub targets: Vec<Target>,
    pub wait_left: f32,
    pub elapsed: f32,
    pub shots: u32,
    pub stage_num: u32,
    pub result: Option<Score>,
    pub status: String,
    pub pending: Vec<Sfx>,
    /// Set when a new layout needs its meshes (re)spawned.
    pub rebuild: bool,
    /// Set when bullet holes should be cleared (new attempt).
    pub clear_marks: bool,
}

impl Match {
    pub fn new() -> Self {
        let mut m = Match {
            phase: Phase::Idle,
            targets: layout(),
            wait_left: 0.0,
            elapsed: 0.0,
            shots: 0,
            stage_num: 1,
            result: None,
            status: "MAKE READY, wait for the buzzer, then move and shoot.".into(),
            pending: Vec::new(),
            rebuild: true,
            clear_marks: false,
        };
        m.result = None;
        m
    }

    pub fn next_stage(&mut self) {
        if self.phase == Phase::Running {
            return;
        }
        self.stage_num += 1;
        self.targets = layout();
        self.result = None;
        self.phase = Phase::Idle;
        self.elapsed = 0.0;
        self.shots = 0;
        self.status = "New stage. MAKE READY.".into();
        self.rebuild = true;
    }

    pub fn make_ready(&mut self) {
        if matches!(self.phase, Phase::Running | Phase::Waiting) {
            return;
        }
        for t in &mut self.targets {
            t.hits.clear();
            t.ns_hits = 0;
            t.down = false;
        }
        self.result = None;
        self.shots = 0;
        self.elapsed = 0.0;
        self.wait_left = rand::thread_rng().gen_range(1.5..3.5);
        self.phase = Phase::Waiting;
        self.status = "Stand by…".into();
        self.clear_marks = true;
    }

    pub fn tick(&mut self, dt: f32) {
        match self.phase {
            Phase::Waiting => {
                self.wait_left -= dt;
                if self.wait_left <= 0.0 {
                    self.phase = Phase::Running;
                    self.elapsed = 0.0;
                    self.pending.push(Sfx::Buzzer);
                    self.status = "GO! Move and shoot.".into();
                }
            }
            Phase::Running => {
                self.elapsed += dt;
                if self.targets.iter().all(|t| t.satisfied()) {
                    self.score();
                }
            }
            _ => {}
        }
    }

    /// Fire a ray from `origin` along `dir`; update scoring and return where to
    /// draw the bullet hole.
    pub fn shoot(&mut self, origin: Vec3, dir: Vec3) -> ShotResult {
        self.shots += 1;
        self.pending.push(Sfx::Shot);

        let mut best: Option<(usize, f32, Vec2)> = None;
        for (i, t) in self.targets.iter().enumerate() {
            if let Some((dist, off)) = t.ray_hit(origin, dir) {
                if best.is_none_or(|(_, bd, _)| dist < bd) {
                    best = Some((i, dist, off));
                }
            }
        }

        let Some((i, dist, off)) = best else {
            return ShotResult { on_target: false, point: origin + dir * 30.0, bad: false };
        };
        let point = origin + dir * dist;
        let t = &mut self.targets[i];
        let mut bad = false;
        match t.kind {
            TKind::Paper => {
                t.hits.push(paper_zone(off, t.hw, t.hh));
                self.pending.push(Sfx::Paper);
            }
            TKind::Steel => {
                if !t.down {
                    t.down = true;
                    self.pending.push(Sfx::Steel);
                }
            }
            TKind::NoShoot => {
                t.ns_hits += 1;
                bad = true;
                self.pending.push(Sfx::Penalty);
            }
        }
        ShotResult { on_target: true, point, bad }
    }

    pub fn stop(&mut self) {
        if self.phase == Phase::Running {
            self.score();
        }
    }

    fn score(&mut self) {
        let mut s = Score {
            time: self.elapsed.max(0.05),
            ..Default::default()
        };
        for t in &self.targets {
            match t.kind {
                TKind::Paper => {
                    let mut hits = t.hits.clone();
                    hits.sort_by_key(|z| -z.points());
                    for z in hits.iter().take(2) {
                        s.points += z.points();
                        match z {
                            Zone::A => s.a += 1,
                            Zone::C => s.c += 1,
                            Zone::D => s.d += 1,
                        }
                    }
                    let missing = 2u32.saturating_sub(t.hits.len() as u32);
                    s.mikes += missing;
                    s.points -= missing as i32 * 10;
                }
                TKind::Steel => {
                    s.steel_total += 1;
                    if t.down {
                        s.steel_down += 1;
                        s.points += 5;
                    } else {
                        s.mikes += 1;
                        s.points -= 10;
                    }
                }
                TKind::NoShoot => {
                    s.ns += t.ns_hits;
                    s.points -= t.ns_hits as i32 * 10;
                }
            }
        }
        s.hit_factor = (s.points.max(0) as f32) / s.time;
        self.result = Some(s);
        self.phase = Phase::Scored;
        self.status = format!("Stage {} — hit factor {:.2}", self.stage_num, s.hit_factor);
        self.pending
            .push(if s.points > 0 { Sfx::Clear } else { Sfx::Penalty });
    }
}

/// A random downrange layout: targets spread across x, set back in −z.
fn layout() -> Vec<Target> {
    let mut rng = rand::thread_rng();
    let mut out: Vec<Target> = Vec::new();
    let plan = [
        (TKind::Paper, PAPERS, PAPER_Y),
        (TKind::Steel, STEEL, STEEL_Y),
        (TKind::NoShoot, NOSHOOTS, PAPER_Y),
    ];
    for (kind, count, y) in plan {
        for _ in 0..count {
            for _try in 0..300 {
                let x = rng.gen_range(-5.5..5.5);
                let z = rng.gen_range(-11.0..-3.0);
                let c = Vec3::new(x, y, z);
                let ok = out
                    .iter()
                    .all(|t| (t.center.x - x).abs() > 1.1 || (t.center.z - z).abs() > 1.1);
                if ok {
                    out.push(Target::new(kind, c));
                    break;
                }
            }
        }
    }
    out
}
