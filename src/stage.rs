//! A USPSA-style shooting stage: paper targets with A/C/D zones, steel poppers,
//! no-shoots, a start buzzer + timer, and hit-factor scoring.
//!
//! Scoring is USPSA **Minor**: A = 5, C = 3, D = 1; each paper needs two scoring
//! hits (best two count). A miss (unfilled required hit) or a standing steel is
//! −10; a no-shoot hit is −10. Hit factor = max(0, points) / time.

use crate::audio::Sfx;
use bevy::prelude::*;
use rand::Rng;

pub const PAPER_HW: f32 = 38.0;
pub const PAPER_HH: f32 = 58.0;
pub const STEEL_R: f32 = 34.0;

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

/// A bullet hole, stored relative to the target centre. `bad` = a no-shoot hit.
#[derive(Clone, Copy)]
pub struct Hole {
    pub off: Vec2,
    pub bad: bool,
}

#[derive(Clone)]
pub struct Target {
    pub kind: TKind,
    pub pos: Vec2,
    pub hw: f32,
    pub hh: f32,
    pub hits: Vec<Zone>,
    pub ns_hits: u32,
    pub down: bool,
    pub holes: Vec<Hole>,
}

impl Target {
    fn new(kind: TKind, pos: Vec2) -> Self {
        let (hw, hh) = match kind {
            TKind::Steel => (STEEL_R, STEEL_R),
            _ => (PAPER_HW, PAPER_HH),
        };
        Target {
            kind,
            pos,
            hw,
            hh,
            hits: Vec::new(),
            ns_hits: 0,
            down: false,
            holes: Vec::new(),
        }
    }

    fn ext(&self) -> f32 {
        self.hh.max(self.hw)
    }

    fn reset(&mut self) {
        self.hits.clear();
        self.holes.clear();
        self.ns_hits = 0;
        self.down = false;
    }

    /// If `p` lands on this target, the offset from centre, else None.
    fn contains(&self, p: Vec2) -> Option<Vec2> {
        let off = p - self.pos;
        match self.kind {
            TKind::Steel => (off.length() <= self.hw).then_some(off),
            _ => (off.x.abs() <= self.hw && off.y.abs() <= self.hh).then_some(off),
        }
    }
}

fn paper_zone(off: Vec2, hw: f32, hh: f32) -> Zone {
    // Chebyshev distance so the zones line up with the nested A/C/D rectangles
    // drawn for the target.
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

#[derive(Resource)]
pub struct Stage {
    pub phase: Phase,
    pub targets: Vec<Target>,
    pub bg_holes: Vec<Vec2>,
    pub wait_left: f32,
    pub elapsed: f32,
    pub shots: u32,
    pub stage_num: u32,
    pub result: Option<Score>,
    pub status: String,
    pub pending: Vec<Sfx>,
    pub dirty: bool,
}

impl Stage {
    pub fn new() -> Self {
        let mut s = Stage {
            phase: Phase::Idle,
            targets: Vec::new(),
            bg_holes: Vec::new(),
            wait_left: 0.0,
            elapsed: 0.0,
            shots: 0,
            stage_num: 1,
            result: None,
            status: String::new(),
            pending: Vec::new(),
            dirty: true,
        };
        s.targets = layout();
        s.status = "Press MAKE READY, wait for the buzzer, then tap the targets.".into();
        s
    }

    pub fn next_stage(&mut self) {
        self.stage_num += 1;
        self.targets = layout();
        self.bg_holes.clear();
        self.result = None;
        self.phase = Phase::Idle;
        self.elapsed = 0.0;
        self.shots = 0;
        self.status = "New stage. Press MAKE READY.".into();
        self.dirty = true;
    }

    /// Start the current stage fresh: clear hits and begin the random delay.
    pub fn make_ready(&mut self) {
        if self.phase == Phase::Running || self.phase == Phase::Waiting {
            return;
        }
        for t in &mut self.targets {
            t.reset();
        }
        self.bg_holes.clear();
        self.result = None;
        self.shots = 0;
        self.elapsed = 0.0;
        self.wait_left = rand::thread_rng().gen_range(1.5..3.5);
        self.phase = Phase::Waiting;
        self.status = "Stand by…".into();
        self.dirty = true;
    }

    pub fn tick(&mut self, dt: f32) {
        match self.phase {
            Phase::Waiting => {
                self.wait_left -= dt;
                if self.wait_left <= 0.0 {
                    self.phase = Phase::Running;
                    self.elapsed = 0.0;
                    self.pending.push(Sfx::Buzzer);
                    self.status = "GO! Tap the targets.".into();
                    self.dirty = true;
                }
            }
            Phase::Running => {
                self.elapsed += dt;
                if self.all_engaged() {
                    self.score();
                }
            }
            _ => {}
        }
    }

    fn all_engaged(&self) -> bool {
        self.targets.iter().all(|t| match t.kind {
            TKind::Paper => t.hits.len() >= 2,
            TKind::Steel => t.down,
            TKind::NoShoot => true,
        })
    }

    /// Register a shot at world position `p`. Only meaningful while running.
    pub fn shoot(&mut self, p: Vec2) {
        if self.phase != Phase::Running {
            return;
        }
        self.shots += 1;
        self.pending.push(Sfx::Shot);
        self.dirty = true;

        // topmost = the containing target whose centre is nearest the tap
        let mut best: Option<(usize, Vec2)> = None;
        for (i, t) in self.targets.iter().enumerate() {
            if let Some(off) = t.contains(p) {
                if best.is_none_or(|(_, bo)| off.length() < bo.length()) {
                    best = Some((i, off));
                }
            }
        }

        let Some((i, off)) = best else {
            self.bg_holes.push(p);
            if self.bg_holes.len() > 30 {
                self.bg_holes.remove(0);
            }
            return;
        };
        let t = &mut self.targets[i];
        match t.kind {
            TKind::Paper => {
                let zone = paper_zone(off, t.hw, t.hh);
                t.hits.push(zone);
                t.holes.push(Hole { off, bad: false });
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
                t.holes.push(Hole { off, bad: true });
                self.pending.push(Sfx::Penalty);
            }
        }
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
                    // best two scoring hits
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
        self.status = format!(
            "Stage {} complete — hit factor {:.2}",
            self.stage_num, s.hit_factor
        );
        self.pending
            .push(if s.points > 0 { Sfx::Clear } else { Sfx::Penalty });
        self.dirty = true;
    }
}

/// Lay out a fresh random stage, rejecting positions that crowd earlier targets.
fn layout() -> Vec<Target> {
    let mut rng = rand::thread_rng();
    let mut targets: Vec<Target> = Vec::new();
    let plan = [
        (TKind::Paper, PAPERS),
        (TKind::Steel, STEEL),
        (TKind::NoShoot, NOSHOOTS),
    ];
    for (kind, count) in plan {
        for _ in 0..count {
            for _try in 0..300 {
                let p = Vec2::new(rng.gen_range(-450.0..450.0), rng.gen_range(-40.0..300.0));
                let cand = Target::new(kind, p);
                let ok = targets
                    .iter()
                    .all(|t| p.distance(t.pos) > (cand.ext() + t.ext() + 24.0).max(118.0));
                if ok {
                    targets.push(cand);
                    break;
                }
            }
        }
    }
    targets
}
