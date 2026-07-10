//! The begin2 AI (Part III of AI_AND_COMBAT.md). Preserved exactly —
//! Daniel spent years tuning it.

pub mod behavior;
pub mod context;
pub mod missions;
pub mod targeting;

pub use context::Ctx;

use crate::data::Nation;
use crate::game::Game;
use crate::math::Rng;
use crate::object::ObjId;
use serde::{Deserialize, Serialize};

/// Mission codes (§12.8, `sub_1F3B8` jump table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Mission {
    Escort { ship: ObjId, range: f64 },     // 1
    Attack { ship: ObjId },                 // 2
    Course { course: f64 },                 // 3
    Phaser { ship: ObjId },                 // 4
    Torpedo { ship: ObjId },                // 5
    Probe { ship: ObjId },                  // 6
    Standoff,                               // 7
    Transport { count: i32, ship: ObjId },  // 8
    Dock { base: ObjId },                   // 9
    Undock,                                 // 10
    Tow { ship: ObjId, dest: ObjId },       // 11
    Release,                                // 12
    Recover { ship: ObjId },                // 14
    Eject { ship: ObjId },                  // 15
    Approach { ship: ObjId, range: f64 },   // 16
    Tractor { ship: ObjId },                // 17
    Stop,                                   // 18
    Defend { ship: ObjId },                 // 22
    HoldFire,                               // (order: hold fire)
}

/// Stance (brain+1Ah).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stance {
    Normal,      // 0
    Retreat,     // 2
    DestructRam, // 0x15
}

/// The per-ship AI brain (0xA0 bytes at body+22h; §12.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brain {
    pub target: Option<ObjId>,      // +0
    pub target_ordered: bool,       // +4
    pub mission: Option<Mission>,   // +6/+8/+0Ch/+10h
    pub last_mission_announced: u8, // +18h dedup
    pub stance: Stance,             // +1Ah
    pub steering_cooldown: i32,     // +1Ch
    pub weave_base: f64,            // +1Eh
    pub weave_amp: f64,             // +2Eh
    pub weave_side: f64,            // +36h (±1)
    pub weave_mode: u8,             // +38h (1 toward, 2 away)
    pub last_prox: f64,             // +3Ah (-1 = unset)
    pub remote_det_gate: bool,      // +42h
    pub overheat_latch: bool,       // +46h
    pub retreat_announced: bool,    // +48h
    pub strength_phaser: f64,       // +4Ah
    pub strength_torp: f64,         // +52h
    pub strength: f64,              // +5Ah
    pub strength_max: f64,          // +62h (at spawn)
    pub aggression: f64,            // +6Ah
    pub bravery: f64,               // +72h
    pub loyalty: f64,               // +7Ah
    pub fanaticism: f64,            // +82h
    pub torp_pressure: i32,         // +8Ah incoming torpedo pressure (0-90)
    pub last_lead_offset: f64,      // +8Ch
    pub battery_frac: f64,          // +94h
    pub shields_starved: bool,      // +9Ch
    pub hold_fire: bool,            // (order)
}

impl Brain {
    /// Personality roll at spawn / capture (`sub_1D4BD` 55844):
    /// clamp01(nation_value ± uniform(deviation)) per attribute.
    pub fn roll(nation: &Nation, rng: &mut Rng) -> Brain {
        let dev = nation.deviation;
        let aggression = (nation.aggression + rng.range(-dev, dev)).clamp(0.0, 1.0);
        let bravery = (nation.bravery + rng.range(-dev, dev)).clamp(0.0, 1.0);
        let loyalty = (nation.loyalty + rng.range(-dev, dev)).clamp(0.0, 1.0);
        let fanaticism = (nation.fanaticism + rng.range(-dev, dev)).clamp(0.0, 1.0);
        Brain {
            target: None,
            target_ordered: false,
            mission: None,
            last_mission_announced: 0,
            stance: Stance::Normal,
            steering_cooldown: 2,
            weave_base: 0.0,
            weave_amp: 0.0,
            weave_side: if rng.unit() < 0.5 { -1.0 } else { 1.0 },
            weave_mode: 1,
            last_prox: -1.0,
            remote_det_gate: false,
            overheat_latch: false,
            retreat_announced: false,
            strength_phaser: 0.0,
            strength_torp: 0.0,
            strength: 0.0,
            strength_max: 0.0,
            aggression,
            bravery,
            loyalty,
            fanaticism,
            torp_pressure: 0,
            last_lead_offset: 0.0,
            battery_frac: 1.0,
            shields_starved: false,
            hold_fire: false,
        }
    }
}

/// AI dispatcher (`sub_20D55` 62774) — the priority-ordered behavior tree
/// (§12.2). Reflexes and weapon actions block the rest of the tree for the
/// cycle; helm orders persist between cycles.
pub fn ai_think(g: &mut Game, id: ObjId) {
    if g.get(id).is_none() || g.obj(id).is_hulk() {
        return;
    }
    let mut ctx = Ctx::build(g, id);

    // reflex block (`sub_20C86`)
    if behavior::reflexes(g, &ctx) {
        return;
    }

    // targeting (`sub_1E30D` + `sub_207F9`), then cache target context
    targeting::select_target(g, &mut ctx);

    // remote-detonate chasing ordnance (`sub_20AF8`)
    if behavior::remote_detonate(g, &ctx) {
        return;
    }

    // weapons block (`sub_20D07`): phasers → torpedoes → probes → boarding
    if behavior::weapons(g, &mut ctx) {
        housekeeping(g, &ctx); // keep tubes loading even on weapon cycles
        return;
    }

    // Romulans cloak when idle (`sub_20BF2`)
    if behavior::cloak_when_idle(g, &ctx) {
        return;
    }

    let (has_mission, stance) = {
        let b = &g.obj(id).ship.as_ref().unwrap().brain;
        (b.mission.is_some(), b.stance)
    };
    if has_mission {
        missions::execute(g, &mut ctx);
    } else if stance != Stance::Normal {
        behavior::stance_helm(g, &ctx);
    } else {
        behavior::morale(g, &ctx);
        let stance_now = g.obj(id).ship.as_ref().unwrap().brain.stance;
        if stance_now != Stance::Normal {
            behavior::stance_helm(g, &ctx);
        } else {
            behavior::default_maneuver(g, &ctx);
        }
    }

    housekeeping(g, &ctx);
}

/// Lock/load housekeeping (`sub_1F09F` 59195): keep tubes loading with the
/// right prox, keep mounts locked on the target.
fn housekeeping(g: &mut Game, ctx: &Ctx) {
    use crate::orders::{self, Mounts};
    let Some(t) = ctx.target.as_ref() else {
        // no target: still keep the tubes loading at default prox
        orders::load_tubes(g, ctx.id, &Mounts::All, None);
        return;
    };
    let tid = t.id;
    // prox = 100 for slow targets else target_max_warp × 50, capped by design
    let prox = if t.max_warp <= 2.0 {
        100.0
    } else {
        (t.max_warp * 50.0).min(ctx.torp_max_prox)
    };
    let last = g.obj(ctx.id).ship.as_ref().unwrap().brain.last_prox;
    if (last - prox).abs() > 1.0 {
        orders::load_tubes(g, ctx.id, &Mounts::All, Some(prox));
        g.obj_mut(ctx.id).ship.as_mut().unwrap().brain.last_prox = prox;
    } else {
        orders::load_tubes(g, ctx.id, &Mounts::All, None);
    }
    // lock everything on the target
    let needs_tube_lock = {
        let s = g.obj(ctx.id).ship.as_ref().unwrap();
        s.tubes.iter().any(|tb| !tb.sys.destroyed() && tb.lock != Some(tid))
    };
    if needs_tube_lock {
        let lead = g.obj(ctx.id).ship.as_ref().unwrap().brain.last_lead_offset;
        orders::lock_tubes(g, ctx.id, &Mounts::All, tid, lead, 0.0);
    }
    let needs_bank_lock = {
        let s = g.obj(ctx.id).ship.as_ref().unwrap();
        s.banks.iter().any(|b| !b.sys.destroyed() && b.lock != Some(tid))
    };
    if needs_bank_lock {
        orders::lock_banks(g, ctx.id, &Mounts::All, tid);
    }
    // near-future mounts join the party
    let has_rails = !g.obj(ctx.id).ship.as_ref().unwrap().rails.is_empty();
    if has_rails {
        orders::lock_rails(g, ctx.id, &Mounts::All, tid);
        orders::fire_rails(g, ctx.id, &Mounts::All);
    }
}
