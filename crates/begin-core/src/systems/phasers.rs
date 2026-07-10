//! Phaser charging & resolution (§7.1; `sub_B1C1` 16697, `phaserDamage`
//! 23256) plus the railgun variant (§7.4).

use crate::constants::*;
use crate::events::ReportKind;
use crate::game::Game;
use crate::math::*;
use crate::object::*;
use crate::systems::damage::{deal_damage, DamageType};
use crate::systems::helm::relative_bearing;

/// `sub_CA97` — charge all banks, then resolve fire-flagged ones.
pub fn phaser_step(g: &mut Game) {
    for id in g.ship_ids() {
        charge_banks(g, id);
        charge_rails(g, id);
        track_locked_banks(g, id);
    }
    for id in g.ship_ids() {
        resolve_banks(g, id);
        resolve_rails(g, id);
    }
}

/// `sub_B1C1`: per enabled, undamaged, non-firing bank:
/// charge += min(needed, banks_charge/banks_rate + 0.001, pool); pool pays.
fn charge_banks(g: &mut Game, id: ObjId) {
    let design_idx = g.obj(id).ship.as_ref().unwrap().design;
    let (banks_charge, banks_rate) = {
        let d = &g.data.ships[design_idx];
        (d.banks_charge, d.banks_rate)
    };
    let o = g.obj_mut(id);
    let mut pool = o.pool;
    let s = o.ship.as_mut().unwrap();
    for b in s.banks.iter_mut() {
        if b.sys.destroyed() || !b.enabled || b.fire {
            continue;
        }
        let needed = (banks_charge - b.charge).max(0.0);
        let step = (banks_charge / banks_rate + 0.001).min(needed).min(pool.max(0.0));
        b.charge += step;
        pool -= step;
    }
    o.pool = pool;
}

/// Railguns charge mechanically (no pool draw): 100/charge_time % per cycle.
fn charge_rails(g: &mut Game, id: ObjId) {
    let design_idx = g.obj(id).ship.as_ref().unwrap().design;
    let charge_time = g.data.ships[design_idx]
        .rail
        .as_deref()
        .and_then(|r| g.data.rail(r))
        .map(|r| r.charge_time)
        .unwrap_or(1.0)
        .max(0.01);
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    let has_rounds = s.rail_rounds_left > 0;
    for r in s.rails.iter_mut() {
        if r.sys.destroyed() || r.fire || !has_rounds {
            continue;
        }
        r.charge = (r.charge + 100.0 / charge_time).min(100.0);
    }
}

/// Bank marks auto-track their lock targets (`sub_B875`).
fn track_locked_banks(g: &mut Game, id: ObjId) {
    let side = g.obj(id).nation;
    let n = g.obj(id).ship.as_ref().unwrap().banks.len();
    for k in 0..n {
        let lock = g.obj(id).ship.as_ref().unwrap().banks[k].lock;
        if let Some(t) = lock.filter(|&t| g.get(t).is_some()) {
            let mark = relative_bearing(g, id, t, side);
            g.obj_mut(id).ship.as_mut().unwrap().banks[k].mark = mark;
        }
    }
    let n = g.obj(id).ship.as_ref().unwrap().rails.len();
    for k in 0..n {
        let lock = g.obj(id).ship.as_ref().unwrap().rails[k].lock;
        if let Some(t) = lock.filter(|&t| g.get(t).is_some()) {
            let mark = relative_bearing(g, id, t, side);
            g.obj_mut(id).ship.as_mut().unwrap().rails[k].mark = mark;
        }
    }
}

/// `phaserDamage` 23256: cone hitscan, hits **everything** in the cone —
/// friend or foe. damage = charge × (45/spread) × sqrt(1 − d/r) × 0.5.
fn resolve_banks(g: &mut Game, id: ObjId) {
    let Some(o) = g.get(id) else { return };
    let Some(ship) = o.ship.as_ref() else { return };
    let firing: Vec<usize> = ship
        .banks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.fire && !b.sys.destroyed() && b.charge > 0.0)
        .map(|(k, _)| k)
        .collect();
    if firing.is_empty() {
        return;
    }
    let design_idx = ship.design;
    let range = g.data.ships[design_idx].banks_range;
    let (my_pos, my_course, my_mark, my_name) = (o.pos, o.course, o.mark, o.name.clone());
    let count = firing.len();

    for k in firing {
        let (mark, spread, charge) = {
            let b = &g.obj(id).ship.as_ref().unwrap().banks[k];
            (b.mark, b.spread.clamp(SPREAD_MIN, SPREAD_DEFAULT), b.charge)
        };
        let facing = norm360(my_course + mark);
        let axis = dir(facing, my_mark);
        g.flash(crate::events::Flash::Beam { from: my_pos, to: my_pos + axis * range });
        for other in g.ids() {
            if other == id {
                continue;
            }
            let oo = g.obj(other);
            if let Some(s2) = oo.ship.as_ref() {
                if s2.cloaked {
                    continue; // can't hit what the sensors can't see
                }
            }
            let delta = oo.pos - my_pos;
            let dist = delta.len();
            if dist >= range || dist <= 1e-9 {
                continue;
            }
            if axis.angle_to(delta) > spread / 2.0 {
                continue;
            }
            let dmg = charge
                * (g.tuning.phaser_dam_mult / spread)
                * (1.0 - dist / range).sqrt()
                * PHASER_HALF;
            let face = norm360(bearing_of(-delta.x, -delta.y) - oo.course);
            deal_damage(g, other, face, dmg, DamageType::Phaser);
        }
        let b = &mut g.obj_mut(id).ship.as_mut().unwrap().banks[k];
        b.charge = 0.0;
        b.fire = false;
    }
    let plural = if count == 1 { "phaser" } else { "phasers" };
    g.say(None, "", format!("{my_name} firing {count} {plural}!"), ReportKind::Info);
}

/// §7.4 railguns: same cone pipeline, flat damage (no falloff inside range),
/// slugs at 0.01-0.1% c resolve same-cycle. No splash; normal shield
/// interaction (antimatter type).
fn resolve_rails(g: &mut Game, id: ObjId) {
    let Some(o) = g.get(id) else { return };
    let Some(ship) = o.ship.as_ref() else { return };
    let firing: Vec<usize> = ship
        .rails
        .iter()
        .enumerate()
        .filter(|(_, r)| r.fire && !r.sys.destroyed() && r.charge >= 100.0)
        .map(|(k, _)| k)
        .collect();
    if firing.is_empty() {
        return;
    }
    let design_idx = ship.design;
    let Some(rd) = g.data.ships[design_idx].rail.as_deref().and_then(|r| g.data.rail(r)).cloned()
    else {
        return;
    };
    let (my_pos, my_course, my_mark, my_name) = (o.pos, o.course, o.mark, o.name.clone());
    let count = firing.len();

    for k in firing {
        if g.obj(id).ship.as_ref().unwrap().rail_rounds_left <= 0 {
            break;
        }
        let mark = g.obj(id).ship.as_ref().unwrap().rails[k].mark;
        let facing = norm360(my_course + mark);
        let axis = dir(facing, my_mark);
        g.flash(crate::events::Flash::Beam { from: my_pos, to: my_pos + axis * rd.range });
        for other in g.ids() {
            if other == id {
                continue;
            }
            let oo = g.obj(other);
            if let Some(s2) = oo.ship.as_ref() {
                if s2.cloaked {
                    continue;
                }
            }
            let delta = oo.pos - my_pos;
            let dist = delta.len();
            if dist >= rd.range || dist <= 1e-9 {
                continue;
            }
            if axis.angle_to(delta) > rd.spread / 2.0 {
                continue;
            }
            let face = norm360(bearing_of(-delta.x, -delta.y) - oo.course);
            deal_damage(g, other, face, rd.damage, DamageType::Antimatter);
        }
        let s = g.obj_mut(id).ship.as_mut().unwrap();
        s.rail_rounds_left -= 1;
        let r = &mut s.rails[k];
        r.charge = 0.0;
        r.fire = false;
    }
    g.say(
        None,
        "",
        format!("{my_name} firing {count} railgun{}!", if count == 1 { "" } else { "s" }),
        ReportKind::Info,
    );
}

/// A bank can bear on a target if |bank facing − bearing| < 22.5°
/// (`sub_1DCBC` 56795). Used by the AI and the UI.
pub fn bank_bears(g: &Game, id: ObjId, bank: usize, target: ObjId) -> bool {
    let o = g.obj(id);
    let s = o.ship.as_ref().unwrap();
    let facing = norm360(o.course + s.banks[bank].mark);
    let (tb, _) = crate::systems::helm::target_bearing_mark(g, id, target, o.nation);
    ang_dist(facing, tb) < BANK_BEARS_CONE
}
