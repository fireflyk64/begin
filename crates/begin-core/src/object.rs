//! The object arena and ship/torpedo/probe state (AI_AND_COMBAT.md §2).
//!
//! begin2 kept intrusive linked lists over a fixed pool; we keep one
//! fixed-capacity arena of `Option<Object>` slots with stable indices.
//! Original node/body field offsets are noted for asm cross-reference.

use crate::ai::Brain;
use crate::math::Vec3;
use serde::{Deserialize, Serialize};

pub const MAX_OBJECTS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjId(pub u32);

impl ObjId {
    pub fn idx(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Ship,  // type 1
    Torp,  // type 2
    Probe, // type 3
}

/// node+6Ch detonation state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Det {
    None,      // 0
    Expire,    // 1: remove quietly (torp time-fuse fizzle)
    Detonate,  // 2: live detonation (probe expiry, self-destruct)
    Destroyed, // 3: destroyed by damage
}

/// node+6Eh helm mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelmMode {
    Course, // 0
    Pursue, // 1: re-aim at target every cycle
    Elude,  // 2: aim directly away every cycle
}

/// node+9Ch control type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Control {
    Local,          // 1: this terminal's player
    Remote(String), // 2: remote player by name
    Ai,             // 3
    None,           // 4
}

/// Per-viewing-side sensor contact state (node +0A2h..+0CCh, generalized to
/// N sides for multiplayer; the original tracked one player side).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Contact {
    pub visible: bool,    // +0A2h
    pub ever: bool,       // +0A4h
    pub last_pos: Vec3,   // +0A6h/+0AEh
    pub last_warp: f64,   // +0B6h
    pub last_course: f64, // +0BEh
    pub last_mark: f64,
    pub last_cycle: u64, // +0CCh
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Object {
    pub kind: Kind,
    pub name: String,
    pub nation: usize, // index into GameData::nations (+2 nation ptr)
    pub ballistic: bool, // +0Ah: never steers (unguided torps/kinetic rounds)
    pub warp: f64,           // +0Ch
    pub desired_warp: f64,   // +14h
    pub course: f64,         // +24h
    pub desired_course: f64, // +2Ch
    pub mark: f64,           // 3D elevation (new; ships spawn at 0)
    pub desired_mark: f64,
    pub pos: Vec3, // +34h/+3Ch (+ z)
    pub vel: Vec3, // +44h/+4Ch velocity per sub-step
    pub warp_budget: f64, // +54h "WARP POWER"
    pub pool: f64,        // +5Ch "OTHER POWER"
    pub residual: f64,    // +64h "RESIDUAL POWER"
    pub det: Det,             // +6Ch
    pub helm: HelmMode,       // +6Eh
    pub pursue: Option<ObjId>, // +78h
    pub owner: Option<ObjId>,  // +7Ch (ordnance owner)
    pub ship: Option<Ship>,        // +80h
    pub torp: Option<TorpState>,   // +84h
    pub probe: Option<ProbeState>, // +88h
    pub control: Control, // +9Ch
    pub contacts: Vec<Contact>, // per nation-side sensor state
    pub hull_integrity: f64, // +0C6h (single hit ≥ this destroys; ≈ design mass)
}

impl Object {
    pub fn is_ship(&self) -> bool {
        self.kind == Kind::Ship
    }
    pub fn alive_crew(&self) -> bool {
        self.ship.as_ref().map(|s| s.survivors >= 6).unwrap_or(true)
    }
    /// A dead hulk: ship with fewer than 6 survivors (derelict).
    pub fn is_hulk(&self) -> bool {
        self.kind == Kind::Ship && !self.alive_crew()
    }
    pub fn contact(&self, side: usize) -> &Contact {
        &self.contacts[side]
    }
    /// Effective velocity per cycle (vel is per sub-step).
    pub fn speed_per_cycle(&self) -> f64 {
        self.warp * crate::constants::UNITS_PER_WARP_PER_CYCLE
    }
}

/// Generic damageable system: dmg% (0-100, 100 = destroyed) + repair progress.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sys {
    pub dmg: i32,
    pub progress: f64,
}

impl Sys {
    pub fn destroyed(&self) -> bool {
        self.dmg >= 100
    }
    pub fn health(&self) -> f64 {
        (100 - self.dmg).max(0) as f64 / 100.0
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Battery {
    pub sys: Sys,
    pub charge: f64,
}

/// Phaser bank (body+116h, stride 2Ah)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bank {
    pub sys: Sys,
    pub fire: bool,
    pub lock: Option<ObjId>,
    pub mark: f64, // relative angle off ship course (auto-tracks lock)
    pub charge: f64,
    pub spread: f64,
    pub enabled: bool, // state 7 enabled / 8 disabled
}

impl Default for Bank {
    fn default() -> Self {
        Bank {
            sys: Sys::default(),
            fire: false,
            lock: None,
            mark: 0.0,
            charge: 0.0,
            spread: crate::constants::SPREAD_DEFAULT,
            enabled: true,
        }
    }
}

/// Torpedo tube (body+268h, stride 3Ah)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tube {
    pub sys: Sys,
    pub fire: bool,
    pub lock: Option<ObjId>,
    pub lead_offset: f64, // AI 20° off-axis order (brain+8Ch)
    pub mark: f64,
    pub prox: f64,
    pub charge: f64,          // 0-100% charge toward load
    pub loading_enabled: bool, // "load tubes" starts the process
    pub loaded: Option<TorpState>,
}

/// Probe launcher (body+43Ch, stride 1Ch)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Launcher {
    pub sys: Sys,
    pub fire: bool,
    pub at_target: Option<ObjId>,
    pub course: f64,
    pub loaded: Option<ProbeState>,
    pub code: String, // probe control code, e.g. "ya101"
}

/// Railgun mount (near-future, §7.4). Hitscan cone weapon.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Rail {
    pub sys: Sys,
    pub fire: bool,
    pub lock: Option<ObjId>,
    pub mark: f64,
    pub charge: f64, // 0-100%
}

/// Warp drive (body+4AEh, stride 1Ah)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drive {
    pub sys: Sys,
    pub temp: f64,
    pub temp_delta: f64,
}

impl Default for Drive {
    fn default() -> Self {
        Drive { sys: Sys::default(), temp: crate::constants::TEMP_FLOOR, temp_delta: 0.0 }
    }
}

/// Shield states (body+518h stride 22h, state word: 7 up / 8 down / 9 reinforced)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShieldState {
    Up,         // 7
    Down,       // 8
    Reinforced, // 9
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shield {
    pub sys: Sys,
    pub strength: f64,  // regenerating strength %, ceiling 100-dmg
    pub effective: f64, // 0 when down/unpowered else strength
    pub state: ShieldState,
    pub coverage: u16, // face bitmask (§6)
    pub facing: f64,   // display: field center angle (manual: 0,60,300,120,240,180)
    pub hits: u32,     // hit-count for batched reports
}

/// TORPSTATE (§2): allocated at load, carried by the tube then the object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorpState {
    pub damage: f64,      // +0 warhead
    pub strength: f64,    // +8 time fuse / plasma remaining strength (dual use)
    pub prox: f64,        // +10h
    pub arm: f64,         // +20h arm counter
    pub salvo: u32,       // +28h salvo merge count
    pub design: usize,    // +2Ah index into GameData::torps
}

/// PROBESTATE (§2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeState {
    pub damage: f64,
    pub time: f64,
    pub prox: f64,
    pub arm: f64,
    pub design: usize, // index into GameData::probes
    pub code: String,
    pub remote_detonate: bool,
    pub deliberate_target: Option<ObjId>,
}

/// Repair priority classes (body+678h; §11 codes 1..12)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepairClass {
    Launchers = 1,
    Banks = 2,
    Tubes = 3,
    Drives = 4,
    Shields = 5,
    Transporter = 6,
    Cloak = 7,
    Reactors = 8,
    Batteries = 9,
    Scanner = 10,
    Impulse = 11,
    Tractor = 12,
}

/// SHIP body (§2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ship {
    pub design: usize, // +60Eh index into GameData::ships
    pub survivors: i32, // +4
    pub captain: String,
    pub brain: Brain, // +22h (0xA0 bytes); used only when control == Ai
    pub reactors: Vec<Sys>,        // +32h
    pub batteries: Vec<Battery>,   // +84h
    pub banks: Vec<Bank>,          // +116h
    pub tubes: Vec<Tube>,          // +268h
    pub launchers: Vec<Launcher>,  // +43Ch
    pub rails: Vec<Rail>,          // near-future
    pub drives: Vec<Drive>,        // +4AEh
    pub shields: Vec<Shield>,      // +518h
    pub transporters: Vec<Sys>,    // +61Ah
    pub hull_hits: u32,     // +5E4h report counter
    pub armour: f64,        // +5E8h
    pub destruct_countdown: f64, // +5EEh (-1 = off)
    pub scanner: Sys,       // +5F6h
    pub cloak_capable: bool, // +600h
    pub cloaked: bool,       // +602h
    pub cloak: Sys,          // +604h
    pub boarders: i32,        // +612h
    pub boarders_nation: usize, // +614h
    pub hits_this_cycle: i32,  // +66Ah (cloak reveal)
    pub partner: Option<ObjId>, // +66Ch tow/dock latch (position glued)
    pub docked_to: Option<ObjId>,  // +674h list links
    pub docked_ships: Vec<ObjId>,  // +670h
    pub repair_priority: Option<RepairClass>, // +678h
    pub impulse: Option<Sys>, // +67Ch/+67Eh
    pub tractor: Option<Sys>, // +688h.. exists/health
    pub tractor_engaged: bool,
    pub tractor_target: Option<ObjId>, // +696h
    pub tow_bearing: f64,              // +69Ah display
    pub group: Option<u32>,            // +6A4h/+6A6h
    pub max_warp_attainable: f64,      // +6A8h cached per cycle
    pub torps_left: i32,
    pub probes_left: i32,
    pub rail_rounds_left: i32,
    pub life_failures: i32, // consecutive life-support failures (brain+9Eh)
    pub life_accum: f64,
    pub fighters_left: i32,
}

impl Ship {
    pub fn operational_banks(&self) -> usize {
        self.banks.iter().filter(|b| !b.sys.destroyed()).count()
    }
    pub fn charged_banks(&self, charge_needed: f64) -> usize {
        self.banks
            .iter()
            .filter(|b| !b.sys.destroyed() && b.enabled && b.charge >= charge_needed)
            .count()
    }
    pub fn operational_tubes(&self) -> usize {
        self.tubes.iter().filter(|t| !t.sys.destroyed()).count()
    }
    pub fn loaded_tubes(&self) -> usize {
        self.tubes
            .iter()
            .filter(|t| !t.sys.destroyed() && t.loaded.is_some() && t.charge >= 100.0)
            .count()
    }
    pub fn operational_launchers(&self) -> usize {
        self.launchers.iter().filter(|l| !l.sys.destroyed()).count()
    }
    pub fn loaded_launchers(&self) -> usize {
        self.launchers
            .iter()
            .filter(|l| !l.sys.destroyed() && l.loaded.is_some())
            .count()
    }
    pub fn scanner_works(&self) -> bool {
        !self.scanner.destroyed()
    }
    pub fn docked(&self) -> bool {
        self.docked_to.is_some()
    }
    pub fn max_drive_temp(&self) -> f64 {
        self.drives.iter().map(|d| d.temp).fold(0.0, f64::max)
    }
}
