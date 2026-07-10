//! Game state: the object arena, spawn logic, side bookkeeping.

use crate::constants::*;
use crate::data::{GameData, ShipDesign};
use crate::events::{Reporter, ReportKind};
use crate::math::{Rng, Vec3};
use crate::object::*;
use crate::ai::Brain;

#[derive(Debug, Clone, PartialEq)]
pub enum EndState {
    /// Winner side (nation idx) or None for mutual destruction/quit.
    Over { winner: Option<usize> },
}

pub struct Game {
    pub data: GameData,
    pub tuning: crate::constants::Tuning,
    pub rng: Rng,
    pub cycle: u64,
    pub objects: Vec<Option<Object>>,
    pub reporter: Reporter,
    /// Sensor fog-of-war active (begin2 `word_37806`; the `random` setup
    /// command spreads fleets beyond scanner range and enables this).
    pub fog: bool,
    pub over: Option<EndState>,
    /// Strength totals at game start, per nation index (endgame rating).
    pub start_strength: Vec<f64>,
    /// Environment (planets, stations, rings); pure scenery + spawn anchors.
    pub env: crate::env::Environment,
}

impl Game {
    pub fn new(data: GameData, tuning: Tuning, seed: u64) -> Game {
        let nations = data.nations.len();
        Game {
            data,
            tuning,
            rng: Rng::new(seed),
            cycle: 0,
            objects: Vec::new(),
            reporter: Reporter::default(),
            fog: false,
            over: None,
            start_strength: vec![0.0; nations],
            env: crate::env::Environment::default(),
        }
    }

    // ---- arena ----

    pub fn get(&self, id: ObjId) -> Option<&Object> {
        self.objects.get(id.idx()).and_then(|o| o.as_ref())
    }
    pub fn get_mut(&mut self, id: ObjId) -> Option<&mut Object> {
        self.objects.get_mut(id.idx()).and_then(|o| o.as_mut())
    }
    pub fn obj(&self, id: ObjId) -> &Object {
        self.objects[id.idx()].as_ref().unwrap()
    }
    pub fn obj_mut(&mut self, id: ObjId) -> &mut Object {
        self.objects[id.idx()].as_mut().unwrap()
    }

    pub fn ids(&self) -> Vec<ObjId> {
        (0..self.objects.len() as u32)
            .filter(|&i| self.objects[i as usize].is_some())
            .map(ObjId)
            .collect()
    }
    pub fn ship_ids(&self) -> Vec<ObjId> {
        self.ids().into_iter().filter(|&i| self.obj(i).kind == Kind::Ship).collect()
    }
    pub fn torp_ids(&self) -> Vec<ObjId> {
        self.ids().into_iter().filter(|&i| self.obj(i).kind == Kind::Torp).collect()
    }
    pub fn probe_ids(&self) -> Vec<ObjId> {
        self.ids().into_iter().filter(|&i| self.obj(i).kind == Kind::Probe).collect()
    }
    pub fn any_ordnance(&self) -> bool {
        self.ids()
            .iter()
            .any(|&i| matches!(self.obj(i).kind, Kind::Torp | Kind::Probe))
    }

    pub fn insert(&mut self, o: Object) -> Option<ObjId> {
        if let Some(slot) = self.objects.iter().position(|s| s.is_none()) {
            self.objects[slot] = Some(o);
            return Some(ObjId(slot as u32));
        }
        if self.objects.len() >= MAX_OBJECTS {
            return None;
        }
        self.objects.push(Some(o));
        Some(ObjId(self.objects.len() as u32 - 1))
    }

    pub fn remove(&mut self, id: ObjId) {
        self.objects[id.idx()] = None;
    }

    /// Find a live object by name (case-insensitive, prefix allowed).
    pub fn find_by_name(&self, name: &str) -> Option<ObjId> {
        let lower = name.to_ascii_lowercase();
        let ids = self.ids();
        ids.iter()
            .copied()
            .find(|&i| self.obj(i).name.to_ascii_lowercase() == lower)
            .or_else(|| {
                ids.into_iter()
                    .find(|&i| self.obj(i).name.to_ascii_lowercase().starts_with(&lower))
            })
    }

    // ---- reports ----

    pub fn say(&mut self, side: Option<usize>, speaker: &str, text: String, kind: ReportKind) {
        self.reporter.say(side, speaker, text, kind, self.cycle);
    }
    /// Record a cosmetic scope flash (front-ends may render it briefly).
    pub fn flash(&mut self, f: crate::events::Flash) {
        if self.reporter.flashes.len() < 256 {
            self.reporter.flashes.push(f);
        }
    }
    pub fn take_flashes(&mut self) -> Vec<crate::events::Flash> {
        self.reporter.take_flashes()
    }
    /// A line from one of the side's bridge officers (Sulu, Worf, ...).
    pub fn officer_say(&mut self, side: usize, text: String, kind: ReportKind) {
        let officers = &self.data.nations[side].officers;
        let pick = self.rng.irange(0, officers.len() as i32 - 1).clamp(0, officers.len() as i32 - 1);
        let name = officers[pick as usize].clone();
        self.say(Some(side), &name, text, kind);
    }

    // ---- spawn ----

    /// Instantiate a ship of `design_idx` at a position. Returns its id.
    pub fn spawn_ship(
        &mut self,
        design_idx: usize,
        name: String,
        pos: Vec3,
        course: f64,
        control: Control,
    ) -> Option<ObjId> {
        let d = self.data.ships[design_idx].clone();
        let nation = self.data.nation_idx(&d.nation).expect("design nation exists");
        let captain = if d.captain_names.is_empty() {
            String::from("Captain")
        } else {
            let k = self.rng.irange(0, d.captain_names.len() as i32 - 1) as usize;
            d.captain_names[k.min(d.captain_names.len() - 1)].clone()
        };
        let brain = Brain::roll(&self.data.nations[nation], &mut self.rng);
        let ship = self.build_ship_body(design_idx, &d, captain, brain);
        let o = Object {
            kind: Kind::Ship,
            name,
            nation,
            ballistic: false,
            warp: 0.0,
            desired_warp: 0.0,
            course,
            desired_course: course,
            mark: 0.0,
            desired_mark: 0.0,
            pos,
            vel: Vec3::ZERO,
            warp_budget: 0.0,
            pool: 0.0,
            residual: 0.0,
            det: Det::None,
            helm: HelmMode::Course,
            pursue: None,
            owner: None,
            ship: Some(ship),
            torp: None,
            probe: None,
            control,
            contacts: vec![Contact::default(); self.data.nations.len()],
            hull_integrity: d.mass,
        };
        self.insert(o)
    }

    fn build_ship_body(
        &mut self,
        design_idx: usize,
        d: &ShipDesign,
        captain: String,
        brain: Brain,
    ) -> Ship {
        // Shield fields: manual numbering 1..6 = 0,60,300,120,240,180 degrees;
        // coverage bitmask per §6 (front=1, 30-90=2, 270-330=4, 90-150=8,
        // 210-270=0x10, 150-210=0x20).
        const FACING: [f64; 6] = [0.0, 60.0, 300.0, 120.0, 240.0, 180.0];
        const MASK: [u16; 6] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20];
        let shields = (0..d.shields)
            .map(|i| {
                let coverage = if d.shields >= 6 {
                    MASK[i % 6]
                } else {
                    // fewer than 6 shields: distribute the faces round-robin
                    (0..6).filter(|f| f % d.shields.max(1) == i).map(|f| MASK[f]).fold(0, |a, m| a | m)
                };
                Shield {
                    sys: Sys::default(),
                    strength: 100.0,
                    effective: 100.0,
                    state: ShieldState::Up,
                    coverage,
                    facing: FACING[i % 6],
                    hits: 0,
                }
            })
            .collect();
        Ship {
            design: design_idx,
            survivors: d.crew as i32,
            captain,
            brain,
            reactors: vec![Sys::default(); d.reactors],
            batteries: (0..d.batteries)
                .map(|_| Battery { sys: Sys::default(), charge: d.battery_capacity })
                .collect(),
            banks: vec![Bank::default(); d.banks],
            tubes: vec![Tube::default(); d.tubes],
            launchers: vec![Launcher::default(); d.launchers],
            rails: vec![Rail::default(); d.rails],
            drives: vec![Drive::default(); d.drives],
            shields,
            transporters: vec![Sys::default(); d.transporters],
            hull_hits: 0,
            armour: 0.0,
            destruct_countdown: -1.0,
            scanner: Sys::default(),
            cloak_capable: d.can_cloak,
            cloaked: false,
            cloak: Sys::default(),
            boarders: 0,
            boarders_nation: 0,
            hits_this_cycle: 0,
            partner: None,
            docked_to: None,
            docked_ships: Vec::new(),
            repair_priority: None,
            impulse: d.has_impulse.then(Sys::default),
            tractor: d.has_tractor.then(Sys::default),
            tractor_engaged: false,
            tractor_target: None,
            tow_bearing: 0.0,
            group: None,
            max_warp_attainable: d.max_warp,
            torps_left: d.torps_carried as i32,
            probes_left: d.probes_carried as i32,
            rail_rounds_left: d.rail.as_deref()
                .and_then(|r| self.data.rail(r))
                .map(|r| r.rounds as i32 * d.rails.max(1) as i32)
                .unwrap_or(0),
            life_failures: 0,
            life_accum: 0.0,
            fighters_left: d.fighters as i32,
        }
    }

    /// Ship strength estimate (`sub_106F7` ≈26909):
    /// `1 + charged_banks × banks_range/1000 + loaded_tubes × 6`.
    pub fn strength_of(&self, id: ObjId) -> f64 {
        let o = self.obj(id);
        let Some(ship) = o.ship.as_ref() else { return 0.0 };
        let d = &self.data.ships[ship.design];
        let charged = ship.charged_banks(d.banks_charge);
        let loaded = ship.loaded_tubes();
        // near-future: count charged rails like banks
        let rails = ship
            .rails
            .iter()
            .filter(|r| !r.sys.destroyed() && r.charge >= 100.0)
            .count();
        let rail_range = d.rail.as_deref().and_then(|r| self.data.rail(r)).map(|r| r.range).unwrap_or(0.0);
        1.0 + charged as f64 * d.banks_range / STRENGTH_DIVISOR
            + rails as f64 * rail_range / STRENGTH_DIVISOR
            + loaded as f64 * 6.0
    }

    /// Total live strength for a nation side (`sub_107E4` ≈27070).
    pub fn side_strength(&self, nation: usize) -> f64 {
        self.ship_ids()
            .into_iter()
            .filter(|&i| self.obj(i).nation == nation && !self.obj(i).is_hulk())
            .map(|i| self.strength_of(i))
            .sum()
    }

    /// Nations that still field crewed ships.
    pub fn live_sides(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .ship_ids()
            .into_iter()
            .filter(|&i| !self.obj(i).is_hulk() && self.obj(i).det == Det::None)
            .map(|i| self.obj(i).nation)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    pub fn record_start_strengths(&mut self) {
        for n in 0..self.data.nations.len() {
            self.start_strength[n] = self.side_strength(n);
        }
    }
}
