//! Scenario setup: fleets, spawn placement, names.

use crate::game::Game;
use crate::math::{dir, Vec3};
use crate::object::{Control, ObjId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetEntry {
    pub class: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideConfig {
    pub nation: String,
    pub ships: Vec<FleetEntry>,
    /// Class of the ship the (first) human commands; None = all AI.
    pub flagship: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub ally: SideConfig,
    pub enemy: SideConfig,
    /// `random` setup: spread ships out of sensor range, fog of war on.
    pub random_placement: bool,
    pub seed: u64,
    /// Julian date epoch for the environment (0 = no environment).
    pub epoch_jd: f64,
    /// Optional spawn anchor: named body + orbit ("low"/"high"), e.g.
    /// vessels spawn near Mars in high orbit.
    pub spawn_body: Option<String>,
}

impl Scenario {
    pub fn duel() -> Scenario {
        Scenario {
            ally: SideConfig {
                nation: "Federation".into(),
                ships: vec![FleetEntry { class: "Heavy Cruiser".into(), count: 1 }],
                flagship: Some("Heavy Cruiser".into()),
            },
            enemy: SideConfig {
                nation: "Klingon".into(),
                ships: vec![FleetEntry { class: "Battle Cruiser".into(), count: 2 }],
                flagship: None,
            },
            random_placement: false,
            seed: 0,
            epoch_jd: 0.0,
            spawn_body: None,
        }
    }
}

/// Max ships per side per the v2 setup ("Up to 60 ships,bases,etc.").
pub const MAX_FLEET: usize = 60;

pub struct SpawnedFleets {
    /// The local player's flagship (if any human ship was requested).
    pub flagship: Option<ObjId>,
    pub ally_ids: Vec<ObjId>,
    pub enemy_ids: Vec<ObjId>,
}

/// Spawn both fleets. Classic placement: fleets face each other ~40000
/// units apart, ships in loose line abreast (out-of-sensor "random"
/// placement scatters them instead). All ships spawn coplanar (mark 0).
pub fn spawn_fleets(g: &mut Game, sc: &Scenario) -> Result<SpawnedFleets, String> {
    let mut flagship = None;
    let mut ally_ids = Vec::new();
    let mut enemy_ids = Vec::new();

    for (side_idx, side) in [&sc.ally, &sc.enemy].into_iter().enumerate() {
        let enemy = side_idx == 1;
        let nation_idx = g
            .data
            .nation_idx(&side.nation)
            .ok_or_else(|| format!("unknown nation {}", side.nation))?;
        let _ = nation_idx;
        // fleet center & facing
        let (center, facing) = if enemy {
            (Vec3::new(0.0, 20000.0, 0.0), 180.0)
        } else {
            (Vec3::new(0.0, -20000.0, 0.0), 0.0)
        };
        let mut used_names: Vec<String> = Vec::new();
        let mut slot = 0usize;
        for entry in &side.ships {
            let design_idx = g
                .data
                .ships
                .iter()
                .position(|d| {
                    d.nation.eq_ignore_ascii_case(&side.nation)
                        && d.name.eq_ignore_ascii_case(&entry.class)
                })
                .or_else(|| {
                    let found = g.data.find_class(&side.nation, &entry.class)?;
                    let name = found.name.clone();
                    g.data.ships.iter().position(|d| d.name == name && d.nation == found.nation)
                })
                .ok_or_else(|| format!("unknown {} class {}", side.nation, entry.class))?;
            for k in 0..entry.count {
                let d = &g.data.ships[design_idx];
                // pick an unused ship name for the class
                let name = d
                    .ship_names
                    .iter()
                    .find(|n| !used_names.contains(n))
                    .cloned()
                    .unwrap_or_else(|| format!("{}-{}", d.abbrev, slot + 1));
                used_names.push(name.clone());
                let pos = if sc.random_placement {
                    center
                        + Vec3::new(
                            g.rng.range(-80000.0, 80000.0),
                            g.rng.range(-80000.0, 80000.0),
                            0.0,
                        )
                } else {
                    // line abreast, 3000 apart, perpendicular to facing
                    let across = dir(crate::math::norm360(facing + 90.0), 0.0);
                    center + across * ((slot as f64 - 1.5) * 3000.0)
                };
                // the first ship of the flagship class on the ally side is
                // the local player's ship
                let is_flag = !enemy
                    && flagship.is_none()
                    && side
                        .flagship
                        .as_deref()
                        .map(|f| {
                            g.data.ships[design_idx].name.eq_ignore_ascii_case(f)
                                || g.data
                                    .find_class(&side.nation, f)
                                    .map(|fd| fd.name == g.data.ships[design_idx].name)
                                    .unwrap_or(false)
                        })
                        .unwrap_or(k == 0 && slot == 0);
                let control = if is_flag { Control::Local } else { Control::Ai };
                let id = g
                    .spawn_ship(design_idx, name, pos, facing, control)
                    .ok_or("object arena full")?;
                if is_flag {
                    flagship = Some(id);
                }
                if enemy {
                    enemy_ids.push(id);
                } else {
                    ally_ids.push(id);
                }
                slot += 1;
            }
        }
    }
    g.fog = sc.random_placement;
    g.record_start_strengths();
    Ok(SpawnedFleets { flagship, ally_ids, enemy_ids })
}
