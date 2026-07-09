//! Shield power, regeneration and face selection (§6; `sub_C883` 19700,
//! `sub_EE0A` 23888).

use crate::constants::*;
use crate::game::Game;
use crate::math::norm360;
use crate::object::*;

/// Per-cycle shield power & regen (`sub_C883`).
pub fn shields_step(g: &mut Game, id: ObjId) {
    let design_idx = g.obj(id).ship.as_ref().unwrap().design;
    let shield_energy = g.data.ships[design_idx].shield_energy;
    let recharge = g.data.ships[design_idx].shield_recharge;

    let o = g.obj_mut(id);
    let pool = o.pool;
    let s = o.ship.as_mut().unwrap();

    let cost: f64 = s
        .shields
        .iter()
        .filter(|sh| !sh.sys.destroyed() && sh.state != ShieldState::Down)
        .map(|sh| {
            if sh.state == ShieldState::Reinforced {
                shield_energy * SHIELD_REINFORCE_COST
            } else {
                shield_energy
            }
        })
        .sum();

    if pool < cost {
        // power starvation: all shields drop this cycle (§6) and the AI's
        // shields-starved flag is raised (brain+9Ch)
        for sh in s.shields.iter_mut() {
            sh.effective = 0.0;
        }
        s.brain.shields_starved = true;
        return;
    }
    o.pool -= cost;
    s.brain.shields_starved = false;

    for sh in s.shields.iter_mut() {
        if sh.sys.destroyed() {
            sh.strength = 0.0;
            sh.effective = 0.0;
            continue;
        }
        let ceiling = (100 - sh.sys.dmg) as f64;
        if sh.state != ShieldState::Down && sh.strength < ceiling {
            let gain = (sh.strength / 100.0 * recharge).max(SHIELD_REGEN_MIN);
            sh.strength = (sh.strength + gain).min(ceiling);
        }
        sh.strength = sh.strength.min(ceiling);
        sh.effective = if sh.state == ShieldState::Down { 0.0 } else { sh.strength };
    }
}

/// Face bitmask for a hit arriving at `face_angle` relative bearing
/// (§6 `sub_EE0A`): ±30° front=1, 30-90=2, 90-150=8, 150-210=0x20,
/// 210-270=0x10, 270-330=4.
pub fn face_mask(face_angle: f64) -> u16 {
    let a = norm360(face_angle);
    if a < 30.0 || a >= 330.0 {
        0x01
    } else if a < 90.0 {
        0x02
    } else if a < 150.0 {
        0x08
    } else if a < 210.0 {
        0x20
    } else if a < 270.0 {
        0x10
    } else {
        0x04
    }
}

/// First non-destroyed, non-down shield covering the face (§6).
pub fn facing_shield(ship: &Ship, face_angle: f64) -> Option<usize> {
    let mask = face_mask(face_angle);
    ship.shields.iter().position(|sh| {
        !sh.sys.destroyed() && sh.state != ShieldState::Down && (sh.coverage & mask) != 0
    })
}

/// Effective absorb points of a shield (manual "Effective" column):
/// design strength × strength% (per-eu display used by AI shield-break math).
pub fn shield_eu(g: &Game, id: ObjId, shield_idx: usize) -> f64 {
    let s = g.obj(id).ship.as_ref().unwrap();
    let d = &g.data.ships[s.design];
    d.shield_strength * s.shields[shield_idx].effective / 100.0
}
