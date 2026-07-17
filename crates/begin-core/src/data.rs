//! Static game data: ship/torpedo/probe designs and nation dogma, extracted
//! from begin2.exe by `tools/extract_stats.py` (v2.00 stats), plus the
//! near-future extension set (battlestars, fighters, railguns) loaded from
//! `data/nearfuture.json`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipDesign {
    pub name: String,
    pub abbrev: String,
    pub nation: String,
    pub crew: u32,
    pub mass: f64,
    pub max_warp: f64,
    pub w1accel: f64,
    pub decel: f64,
    pub warp_power_use: f64,
    pub w1turn: f64,
    pub destruct: f64,
    pub scanner_reflect: f64,
    pub reactors: usize,
    pub reactor_output: f64,
    pub reactor_repair: f64,
    pub batteries: usize,
    pub battery_capacity: f64,
    pub battery_repair: f64,
    pub banks: usize,
    pub banks_rate: f64,
    pub banks_charge: f64,
    pub banks_range: f64,
    pub banks_repair: f64,
    pub tubes: usize,
    pub torp: Option<String>,
    pub tube_repair: f64,
    pub launchers: usize,
    pub probe: Option<String>,
    pub probe_repair: f64,
    pub drives: usize,
    pub warp_power: f64,
    pub warp_efficiency: f64,
    pub drive_repair: f64,
    pub shields: usize,
    pub shield_strength: f64,
    pub shield_absorption: f64,
    pub shield_recharge: f64,
    pub shield_energy: f64,
    pub shield_repair: f64,
    pub transporters: usize,
    pub beam_cap: u32,
    pub beam_range: f64,
    pub transporter_repair: f64,
    pub scanner_range: f64,
    pub scanner_repair: f64,
    pub can_cloak: bool,
    pub cloak_energy: f64,
    pub cloak_repair: f64,
    pub ship_names: Vec<String>,
    pub captain_names: Vec<String>,
    pub mass_capacity: f64,
    pub docked_repair_ratio: f64,
    pub life_support: u32,
    pub has_impulse: bool,
    pub impulse_repair: f64,
    pub has_tractor: bool,
    pub tractor_strength: f64,
    pub tractor_repair: f64,
    pub tractor_energy: f64,
    pub destruct_energy: f64,
    pub probes_carried: u32,
    pub torps_carried: u32,
    pub crew_names: Vec<String>,
    /// Near-future extensions (absent in classic data)
    #[serde(default)]
    pub rails: usize,
    #[serde(default)]
    pub rail: Option<String>,
    #[serde(default)]
    pub fighters: usize,
    #[serde(default)]
    pub fighter_class: Option<String>,
}

/// Railgun design (§7.4): cone hitscan like a phaser but flat damage,
/// no charge decay with distance, tiny spread, slug velocity 0.01-0.1% c.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RailDesign {
    pub name: String,
    pub desc: String,
    pub damage: f64,
    pub range: f64,
    pub spread: f64,
    /// units/cycle, for flavor & future light-lag; effectively instant in-cone
    pub velocity: f64,
    pub charge_time: f64,
    pub rounds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorpDesign {
    pub name: String,
    pub desc: String,
    pub velocity: f64,
    pub damage: f64,
    pub arm_time: f64,
    pub max_time_fuse: f64,
    pub max_prox: f64,
    pub homing: bool,
    pub speed_variance: f64,
    /// 0 = antimatter, 1 = shield-bore (Orion Auger), 2 = plasma (decays,
    /// damageable by phaser point-defense)
    pub warhead_type: u8,
    pub charge_time: f64,
    pub min_prox: f64,
    /// Near-future: contact-fuse ballistic kinetic round (no splash)
    #[serde(default)]
    pub kinetic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeDesign {
    pub name: String,
    pub desc: String,
    pub velocity: f64,
    pub damage: f64,
    pub arm_time: f64,
    pub max_time_fuse: f64,
    pub max_prox: f64,
    pub homing: bool,
    pub scan_range: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nation {
    pub name: String,
    pub adjective: String,
    pub command: String,
    pub aggression: f64,
    pub bravery: f64,
    pub loyalty: f64,
    pub fanaticism: f64,
    pub boarding_level: f64,
    pub deviation: f64,
    pub endgame_intro: String,
    pub endgame: Vec<String>,
    pub officers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Messages {
    pub destruct: Vec<String>,
    pub retreat: Vec<String>,
    pub insult: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GameData {
    pub ships: Vec<ShipDesign>,
    pub torps: Vec<TorpDesign>,
    pub probes: Vec<ProbeDesign>,
    pub rails: Vec<RailDesign>,
    pub nations: Vec<Nation>,
    pub messages: Messages,
}

#[derive(Debug, Clone, Deserialize)]
struct NearFuture {
    #[serde(default)]
    ships: Vec<ShipDesign>,
    #[serde(default)]
    torps: Vec<TorpDesign>,
    #[serde(default)]
    probes: Vec<ProbeDesign>,
    #[serde(default)]
    rails: Vec<RailDesign>,
    #[serde(default)]
    nations: Vec<Nation>,
}

impl GameData {
    /// Load the classic begin2 v2.00 data set plus near-future extensions.
    pub fn load() -> GameData {
        let mut d = GameData {
            ships: serde_json::from_str(include_str!("../data/ships.json")).unwrap(),
            torps: serde_json::from_str(include_str!("../data/torps.json")).unwrap(),
            probes: serde_json::from_str(include_str!("../data/probes.json")).unwrap(),
            rails: Vec::new(),
            nations: serde_json::from_str(include_str!("../data/nations.json")).unwrap(),
            messages: serde_json::from_str(include_str!("../data/messages.json")).unwrap(),
        };
        let nf: NearFuture =
            serde_json::from_str(include_str!("../data/nearfuture.json")).unwrap();
        d.ships.extend(nf.ships);
        d.torps.extend(nf.torps);
        d.probes.extend(nf.probes);
        d.rails.extend(nf.rails);
        d.nations.extend(nf.nations);
        d
    }

    pub fn nation(&self, adjective: &str) -> Option<&Nation> {
        self.nation_idx(adjective).map(|i| &self.nations[i])
    }
    /// Exact match on adjective or name, else a unique case-insensitive
    /// prefix of either ("kli" → Klingon).
    pub fn nation_idx(&self, adjective: &str) -> Option<usize> {
        let q = adjective.to_ascii_lowercase();
        if let Some(i) = self.nations.iter().position(|n| {
            n.adjective.eq_ignore_ascii_case(&q) || n.name.eq_ignore_ascii_case(&q)
        }) {
            return Some(i);
        }
        if q.is_empty() {
            return None;
        }
        let hits: Vec<usize> = self
            .nations
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                n.adjective.to_ascii_lowercase().starts_with(&q)
                    || n.name.to_ascii_lowercase().starts_with(&q)
            })
            .map(|(i, _)| i)
            .collect();
        if hits.len() == 1 {
            Some(hits[0])
        } else {
            None
        }
    }
    /// Ship classes belonging to a nation.
    pub fn classes_of(&self, nation_adj: &str) -> Vec<&ShipDesign> {
        self.ships
            .iter()
            .filter(|s| s.nation.eq_ignore_ascii_case(nation_adj))
            .collect()
    }
    /// Find a class by (possibly partial) name within a nation.
    pub fn find_class(&self, nation_adj: &str, name: &str) -> Option<&ShipDesign> {
        let name = name.to_ascii_lowercase();
        let singular = name.strip_suffix('s').unwrap_or(&name);
        self.classes_of(nation_adj).into_iter().find(|s| {
            let n = s.name.to_ascii_lowercase();
            n == name || n == singular || n.starts_with(singular)
                || s.abbrev.eq_ignore_ascii_case(&name)
        })
    }
    pub fn torp(&self, name: &str) -> Option<&TorpDesign> {
        self.torps.iter().find(|t| t.name.eq_ignore_ascii_case(name))
    }
    pub fn probe(&self, name: &str) -> Option<&ProbeDesign> {
        self.probes.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }
    pub fn rail(&self, name: &str) -> Option<&RailDesign> {
        self.rails.iter().find(|r| r.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_classic_data() {
        let d = GameData::load();
        assert!(d.ships.len() >= 20);
        assert_eq!(d.nations.len() >= 4, true);
        let fed = d.nation("Federation").unwrap();
        assert!((fed.aggression - 0.65).abs() < 1e-9);
        assert!((fed.loyalty - 1.2).abs() < 1e-9);
        let hc = d.find_class("Federation", "Heavy Cruiser").unwrap();
        assert_eq!(hc.crew, 450);
        assert_eq!(hc.shields, 6);
        let mk7 = d.torp("mk7").unwrap();
        assert_eq!(mk7.velocity, 30.0);
        assert_eq!(mk7.damage, 10.0);
        // partial names & plurals
        assert!(d.find_class("Klingon", "frigates").is_some());
        assert!(d.find_class("Federation", "dread").is_some());
    }
}
