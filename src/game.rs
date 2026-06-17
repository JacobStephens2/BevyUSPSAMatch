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

/// Rounds per magazine and how long a reload takes (counted against stage time).
pub const MAG_SIZE: u32 = 10;
const RELOAD_TIME: f32 = 1.8;

// Wild-west outlaw enemies (Stage 2+). Torso is a vertical rectangle hit zone.
pub const ENEMY_HW: f32 = 0.34;
pub const ENEMY_HH: f32 = 0.52;
pub const ENEMY_Y: f32 = 1.05; // torso centre height
const ENEMY_HP: i32 = 5;
const ENEMY_SPEED: f32 = 2.6;
const ENEMY_FIRE_MIN: f32 = 1.5;
const ENEMY_FIRE_MAX: f32 = 2.8;
const ENEMY_DAMAGE: i32 = 14;
const ENEMY_HIT_CHANCE: f32 = 0.5;
pub const PLAYER_HP: i32 = 100;

/// A wild-west outlaw: strafes across the bay (moving target) and fires back.
#[derive(Clone)]
pub struct Enemy {
    pub center: Vec3, // torso centre; x strafes, z fixed
    pub home_x: f32,
    pub patrol: f32,
    pub dir: f32,
    pub hp: i32,
    pub alive: bool,
    pub fire_in: f32,
    pub flash: f32,     // muzzle-flash render timer
    pub hit_flash: f32, // got-hit tint timer
    pub fall: f32,      // death topple 0..1
}

impl Enemy {
    fn new(x: f32, z: f32, patrol: f32) -> Self {
        Enemy {
            center: Vec3::new(x, ENEMY_Y, z),
            home_x: x,
            patrol,
            dir: if rand::thread_rng().gen_bool(0.5) { 1.0 } else { -1.0 },
            hp: ENEMY_HP,
            alive: true,
            fire_in: rand::thread_rng().gen_range(1.4..2.6),
            flash: 0.0,
            hit_flash: 0.0,
            fall: 0.0,
        }
    }

    fn reset(&mut self) {
        self.center.x = self.home_x;
        self.hp = ENEMY_HP;
        self.alive = true;
        self.fire_in = rand::thread_rng().gen_range(1.4..2.6);
        self.flash = 0.0;
        self.hit_flash = 0.0;
        self.fall = 0.0;
    }

    /// Ray vs the outlaw's vertical torso rectangle (faces the shooter, +Z).
    fn ray_hit(&self, origin: Vec3, dir: Vec3) -> Option<(f32, Vec3)> {
        if !self.alive || dir.z >= -1.0e-4 {
            return None;
        }
        let t = (self.center.z - origin.z) / dir.z;
        if t <= 0.0 {
            return None;
        }
        let hit = origin + dir * t;
        let dx = (hit.x - self.center.x).abs();
        let dy = (hit.y - self.center.y).abs();
        (dx <= ENEMY_HW && dy <= ENEMY_HH).then_some((t, hit))
    }
}

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
    pub ammo: u32,
    pub reloading: bool,
    pub reload_left: f32,
    pub enemies: Vec<Enemy>,
    pub hp: i32,
    pub max_hp: i32,
    pub damage_flash: f32,
    pub failed: bool,
    pub pending: Vec<Sfx>,
    /// Set when a new layout needs its meshes (re)spawned.
    pub rebuild: bool,
    /// Set when bullet holes should be cleared (new attempt).
    pub clear_marks: bool,
}

impl Match {
    pub fn new() -> Self {
        let (targets, enemies) = build_stage(1);
        Match {
            phase: Phase::Idle,
            targets,
            wait_left: 0.0,
            elapsed: 0.0,
            shots: 0,
            stage_num: 1,
            result: None,
            status: "MAKE READY, wait for the buzzer, then move and shoot.".into(),
            ammo: MAG_SIZE,
            reloading: false,
            reload_left: 0.0,
            enemies,
            hp: PLAYER_HP,
            max_hp: PLAYER_HP,
            damage_flash: 0.0,
            failed: false,
            pending: Vec::new(),
            rebuild: true,
            clear_marks: false,
        }
    }

    pub fn next_stage(&mut self) {
        if self.phase == Phase::Running {
            return;
        }
        self.stage_num += 1;
        let (targets, enemies) = build_stage(self.stage_num);
        self.targets = targets;
        self.enemies = enemies;
        self.result = None;
        self.failed = false;
        self.hp = self.max_hp;
        self.damage_flash = 0.0;
        self.phase = Phase::Idle;
        self.elapsed = 0.0;
        self.shots = 0;
        self.status = if self.stage_num >= 2 {
            "Stage 2 — outlaws downrange! MAKE READY.".into()
        } else {
            "New stage. MAKE READY.".into()
        };
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
        for e in &mut self.enemies {
            e.reset();
        }
        self.hp = self.max_hp;
        self.damage_flash = 0.0;
        self.failed = false;
        self.result = None;
        self.shots = 0;
        self.elapsed = 0.0;
        self.ammo = MAG_SIZE;
        self.reloading = false;
        self.reload_left = 0.0;
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
                self.elapsed += dt; // reload time counts against the clock
                if self.reloading {
                    self.reload_left -= dt;
                    if self.reload_left <= 0.0 {
                        self.reloading = false;
                        self.ammo = MAG_SIZE;
                    }
                }
                self.tick_enemies(dt);
                if self.failed {
                    return;
                }
                let targets_done = self.targets.iter().all(|t| t.satisfied());
                let enemies_done = self.enemies.iter().all(|e| !e.alive);
                if targets_done && enemies_done {
                    self.score();
                }
            }
            _ => {}
        }
    }

    /// Move the outlaws and let them shoot back; may down the player.
    fn tick_enemies(&mut self, dt: f32) {
        self.damage_flash = (self.damage_flash - dt).max(0.0);
        let mut rng = rand::thread_rng();
        let mut shot_player = false;

        for i in 0..self.enemies.len() {
            if !self.enemies[i].alive {
                let e = &mut self.enemies[i];
                if e.fall < 1.0 {
                    e.fall = (e.fall + dt / 0.4).min(1.0);
                }
                continue;
            }

            // Strafe + tick render timers (all pure-enemy, no self.* access).
            let fired;
            {
                let e = &mut self.enemies[i];
                e.center.x += e.dir * ENEMY_SPEED * dt;
                if (e.center.x - e.home_x).abs() >= e.patrol {
                    e.center.x = e.home_x + e.patrol * e.dir.signum();
                    e.dir = -e.dir;
                }
                e.flash = (e.flash - dt).max(0.0);
                e.hit_flash = (e.hit_flash - dt).max(0.0);
                e.fire_in -= dt;
                fired = e.fire_in <= 0.0;
                if fired {
                    e.fire_in = rng.gen_range(ENEMY_FIRE_MIN..ENEMY_FIRE_MAX);
                    e.flash = 0.06;
                }
            }

            if fired {
                self.pending.push(Sfx::Shot);
                if rng.gen_range(0.0f32..1.0) < ENEMY_HIT_CHANCE {
                    self.hp -= ENEMY_DAMAGE;
                    self.damage_flash = 0.55;
                    self.pending.push(Sfx::Hurt);
                    if self.hp <= 0 {
                        self.hp = 0;
                        shot_player = true;
                    }
                }
            }
        }

        if shot_player {
            self.fail();
        }
    }

    fn fail(&mut self) {
        self.failed = true;
        self.result = Some(Score {
            time: self.elapsed.max(0.05),
            ..Default::default()
        });
        self.phase = Phase::Scored;
        self.status = "DOWNED — the outlaws got you.".into();
        self.pending.push(Sfx::Penalty);
    }

    /// Whether a shot can be fired right now (running, loaded, not reloading).
    pub fn can_fire(&self) -> bool {
        self.phase == Phase::Running && !self.reloading && self.ammo > 0
    }

    /// Begin a reload (only useful while running and not already full/reloading).
    pub fn reload(&mut self) {
        if self.phase == Phase::Running && !self.reloading && self.ammo < MAG_SIZE {
            self.reloading = true;
            self.reload_left = RELOAD_TIME;
            self.pending.push(Sfx::Reload);
        }
    }

    /// Dry-fire click when the trigger is pulled on an empty chamber.
    pub fn dry_fire(&mut self) {
        self.pending.push(Sfx::Empty);
    }

    /// Fire a ray from `origin` along `dir`; update scoring and return where to
    /// draw the bullet hole.
    pub fn shoot(&mut self, origin: Vec3, dir: Vec3) -> ShotResult {
        self.shots += 1;
        self.ammo = self.ammo.saturating_sub(1);
        self.pending.push(Sfx::Shot);

        // Nearest paper/steel/no-shoot plane hit.
        let mut best: Option<(usize, f32, Vec2)> = None;
        for (i, t) in self.targets.iter().enumerate() {
            if let Some((dist, off)) = t.ray_hit(origin, dir) {
                if best.is_none_or(|(_, bd, _)| dist < bd) {
                    best = Some((i, dist, off));
                }
            }
        }
        // Nearest outlaw hit.
        let mut ebest: Option<(usize, f32, Vec3)> = None;
        for (i, e) in self.enemies.iter().enumerate() {
            if let Some((dist, pt)) = e.ray_hit(origin, dir) {
                if ebest.is_none_or(|(_, bd, _)| dist < bd) {
                    ebest = Some((i, dist, pt));
                }
            }
        }

        let enemy_first = match (best, ebest) {
            (Some((_, td, _)), Some((_, ed, _))) => ed <= td,
            (None, Some(_)) => true,
            _ => false,
        };
        if enemy_first {
            let (ei, _, pt) = ebest.unwrap();
            let e = &mut self.enemies[ei];
            e.hp -= 1;
            e.hit_flash = 0.12;
            let died = e.hp <= 0;
            if died {
                e.alive = false;
                e.fall = 0.0;
            }
            self.pending.push(Sfx::Paper); // body thwack
            if died {
                self.pending.push(Sfx::Down);
            }
            return ShotResult { on_target: true, point: pt, bad: false };
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

/// A random downrange stage. Stage 1 is paper/steel/no-shoot only; Stage 2+
/// adds wild-west outlaws (and keeps the targets shallow so the outlaws, set
/// further back, read clearly).
fn build_stage(stage: u32) -> (Vec<Target>, Vec<Enemy>) {
    let mut rng = rand::thread_rng();
    let has_enemies = stage >= 2;
    let (npaper, nsteel, nnoshoot) = if has_enemies {
        (3, 2, 1)
    } else {
        (PAPERS, STEEL, NOSHOOTS)
    };
    let z_far = if has_enemies { -6.5 } else { -11.0 };

    let mut targets: Vec<Target> = Vec::new();
    let plan = [
        (TKind::Paper, npaper, PAPER_Y),
        (TKind::Steel, nsteel, STEEL_Y),
        (TKind::NoShoot, nnoshoot, PAPER_Y),
    ];
    for (kind, count, y) in plan {
        for _ in 0..count {
            for _try in 0..300 {
                let x = rng.gen_range(-5.5..5.5);
                let z = rng.gen_range(z_far..-3.0);
                let c = Vec3::new(x, y, z);
                let ok = targets
                    .iter()
                    .all(|t| (t.center.x - x).abs() > 1.1 || (t.center.z - z).abs() > 1.1);
                if ok {
                    targets.push(Target::new(kind, c));
                    break;
                }
            }
        }
    }

    let mut enemies: Vec<Enemy> = Vec::new();
    if has_enemies {
        for k in 0..2 {
            let x = if k == 0 { -3.0 } else { 3.0 };
            let z = -8.5 - k as f32 * 1.4;
            enemies.push(Enemy::new(x, z, 4.0));
        }
    }
    (targets, enemies)
}
