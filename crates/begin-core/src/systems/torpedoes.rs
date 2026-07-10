//! Torpedo tubes: charging, loading, firing solutions, spawning, prox and
//! time fuses (§7.2; `sub_CB5F` 20043, `sub_B926` 17715, `sub_E9B5` 23463,
//! `sub_D2D2` 20900, `sub_D130` 20709, salvo merge `sub_EC1D` ≈23680).

use crate::constants::*;
use crate::events::ReportKind;
use crate::game::Game;
use crate::math::*;
use crate::object::*;
use crate::systems::helm::{angle_off, relative_bearing, target_bearing_mark};

pub fn is_decay_type(g: &Game, design: usize) -> bool {
    g.data.torps[design].warhead_type == 2
}

/// `sub_CB5F`: charge tubes, auto-load, then fire flagged tubes.
pub fn tube_step(g: &mut Game) {
    for id in g.ship_ids() {
        charge_and_load(g, id);
    }
    for id in g.ship_ids() {
        fire_tubes(g, id);
    }
}

/// Tube charging (`sub_B351`, same layout as bank charging) and auto-reload.
/// A tube charges over the torpedo's charge_time cycles (manual: the -, =, ≡
/// symbols are the last three cycles), paying damage/charge_time energy per
/// cycle, and loads at 100% — ready the *following* cycle.
fn charge_and_load(g: &mut Game, id: ObjId) {
    let design = g.obj(id).ship.as_ref().unwrap().design;
    let Some(torp_name) = g.data.ships[design].torp.clone() else { return };
    let Some(torp_idx) = g.data.torps.iter().position(|t| t.name == torp_name) else { return };
    let td = g.data.torps[torp_idx].clone();

    let o = g.obj_mut(id);
    let mut pool = o.pool;
    let s = o.ship.as_mut().unwrap();
    for tube in s.tubes.iter_mut() {
        if tube.sys.destroyed() || tube.fire || !tube.loading_enabled {
            continue;
        }
        if tube.loaded.is_some() {
            continue;
        }
        if s.torps_left <= 0 {
            continue;
        }
        // charge toward 100%
        let step_pct = 100.0 / td.charge_time.max(0.01);
        let cost = td.damage / td.charge_time.max(0.01);
        if pool < cost {
            continue;
        }
        pool -= cost;
        tube.charge = (tube.charge + step_pct).min(100.0);
        if tube.charge >= 100.0 {
            // load a torpedo with the ordered prox (clamped to design limits)
            let prox = if tube.prox > 0.0 {
                tube.prox.clamp(td.min_prox, td.max_prox)
            } else {
                td.max_prox
            };
            tube.loaded = Some(TorpState {
                damage: td.damage,
                strength: td.max_time_fuse,
                prox,
                arm: td.arm_time,
                salvo: 1,
                design: torp_idx,
            });
            s.torps_left -= 1;
        }
    }
    o.pool = pool;
}

/// Firing solution (`sub_B926` 17715): homing → mark = relative bearing;
/// else true intercept lead:
/// mark = rel_bearing + asin(sin(angle_off) × target_warp / torp_velocity) + lead.
fn firing_solution(g: &Game, id: ObjId, target: ObjId, torp_velocity: f64, homing: bool, lead_offset: f64) -> f64 {
    let side = g.obj(id).nation;
    let rel = relative_bearing(g, id, target, side);
    if homing {
        return rel;
    }
    let off = angle_off(g, id, target, side);
    let target_warp = g.obj(target).warp;
    let ratio = (off.to_radians().sin() * target_warp / torp_velocity.max(0.001)).clamp(-1.0, 1.0);
    norm360(rel + ratio.asin().to_degrees() + lead_offset)
}

/// `sub_E9B5` — spawn (or salvo-merge) torpedoes for fire-flagged tubes.
fn fire_tubes(g: &mut Game, id: ObjId) {
    let Some(o) = g.get(id) else { return };
    let Some(ship) = o.ship.as_ref() else { return };
    let firing: Vec<usize> = ship
        .tubes
        .iter()
        .enumerate()
        .filter(|(_, t)| t.fire && !t.sys.destroyed() && t.loaded.is_some() && t.charge >= 100.0)
        .map(|(k, _)| k)
        .collect();
    if firing.is_empty() {
        return;
    }
    let (my_pos, my_course, my_mark, my_nation, my_name) =
        (o.pos, o.course, o.mark, o.nation, o.name.clone());
    let n_nations = g.data.nations.len();
    let mut fired = 0usize;
    // (course, mark, design, target) of torps spawned this volley — salvo merge
    let mut spawned: Vec<(f64, f64, usize, Option<ObjId>, ObjId)> = Vec::new();

    for k in firing {
        let (state, lock, tube_mark, lead) = {
            let t = &g.obj(id).ship.as_ref().unwrap().tubes[k];
            (t.loaded.clone().unwrap(), t.lock, t.mark, t.lead_offset)
        };
        let td = g.data.torps[state.design].clone();
        // solution
        let (course, mark) = if let Some(tgt) = lock.filter(|&t| g.get(t).is_some()) {
            let m = firing_solution(g, id, tgt, td.velocity, td.homing, lead);
            // 3D: aim at the target's elevation
            let (_, elev) = target_bearing_mark(g, id, tgt, my_nation);
            (norm360(my_course + m), elev)
        } else {
            (norm360(my_course + tube_mark), my_mark)
        };
        // clear the tube
        {
            let t = &mut g.obj_mut(id).ship.as_mut().unwrap().tubes[k];
            t.loaded = None;
            t.charge = 0.0;
            t.fire = false;
        }
        // salvo merge (`sub_EC1D`): same course/design/target this cycle
        if let Some(&(_, _, _, _, prev)) = spawned
            .iter()
            .find(|&&(c, m, d, tg, _)| {
                (c - course).abs() < 1e-9 && (m - mark).abs() < 1e-9 && d == state.design && tg == lock
            })
        {
            if let Some(p) = g.get_mut(prev) {
                if let Some(ts) = p.torp.as_mut() {
                    ts.salvo += 1;
                    fired += 1;
                    continue;
                }
            }
        }
        // velocity with variance
        let velocity = td.velocity * (1.0 + g.rng.range(-td.speed_variance, td.speed_variance));
        let torp = Object {
            kind: Kind::Torp,
            name: td.name.clone(),
            nation: my_nation,
            ballistic: !td.homing,
            warp: velocity,
            desired_warp: velocity,
            course,
            desired_course: course,
            mark,
            desired_mark: mark,
            pos: my_pos,
            vel: dir(course, mark) * (velocity * SUBSTEP_SCALE),
            warp_budget: 0.0,
            pool: 0.0,
            residual: 0.0,
            det: Det::None,
            helm: if td.homing { HelmMode::Pursue } else { HelmMode::Course },
            pursue: if td.homing { lock } else { None },
            owner: Some(id),
            ship: None,
            torp: Some(state),
            probe: None,
            control: Control::None,
            contacts: vec![Contact::default(); n_nations],
            hull_integrity: 1.0,
        };
        if let Some(tid) = g.insert(torp) {
            spawned.push((course, mark, g.obj(tid).torp.as_ref().unwrap().design, lock, tid));
            fired += 1;
            // incoming-torpedo pressure on the target's brain (cap 90)
            if let Some(tgt) = lock {
                if let Some(t) = g.get_mut(tgt) {
                    if let Some(s) = t.ship.as_mut() {
                        s.brain.torp_pressure = (s.brain.torp_pressure + 1).min(AI_TORP_PRESSURE_CAP);
                    }
                }
            }
        }
    }
    if fired > 0 {
        let plural = if fired == 1 { "torpedo" } else { "torpedos" };
        g.say(None, "", format!("{my_name} firing {fired} {plural}!"), ReportKind::Info);
    }
}

/// Prox fuses, run every movement sub-step (`sub_D2D2` 20900).
/// Torpedoes trigger on **any** ship (friendly fire is real); probes only on
/// other-nation ships or their deliberate target. Kinetic rounds deal
/// contact damage directly instead of detonating.
pub fn prox_fuses(g: &mut Game) {
    let ships = g.ship_ids();
    for id in g.ids() {
        let o = g.obj(id);
        match o.kind {
            Kind::Torp => {
                let t = o.torp.as_ref().unwrap();
                if o.det != Det::None || t.arm > 0.0 {
                    continue;
                }
                let (prox, pos, kinetic) =
                    (t.prox, o.pos, g.data.torps[t.design].kinetic);
                let hit = ships.iter().copied().find(|&s| {
                    s != id
                        && g.obj(s).det == Det::None
                        && !g.obj(s).ship.as_ref().map(|sh| sh.cloaked).unwrap_or(false)
                        && (g.obj(s).pos - pos).len() < prox.max(1.0)
                });
                if let Some(victim) = hit {
                    if kinetic {
                        // contact penetrator: direct hull damage, no blast
                        let (dmg, salvo) = {
                            let t = g.obj(id).torp.as_ref().unwrap();
                            (t.damage, t.salvo.max(1))
                        };
                        let vpos = g.obj(victim).pos;
                        let back = pos - vpos;
                        let face =
                            norm360(bearing_of(back.x, back.y) - g.obj(victim).course);
                        for _ in 0..salvo {
                            crate::systems::damage::deal_damage(
                                g,
                                victim,
                                face,
                                dmg,
                                crate::systems::damage::DamageType::Antimatter,
                            );
                        }
                        g.obj_mut(id).det = Det::Expire;
                    } else {
                        g.obj_mut(id).det = Det::Detonate;
                    }
                }
            }
            Kind::Probe => {
                let p = o.probe.as_ref().unwrap();
                if o.det != Det::None || p.arm > 0.0 {
                    continue;
                }
                let (prox, pos, nation, deliberate) =
                    (p.prox, o.pos, o.nation, p.deliberate_target);
                let hit = ships.iter().copied().any(|s| {
                    s != id
                        && g.obj(s).det == Det::None
                        && !g.obj(s).ship.as_ref().map(|sh| sh.cloaked).unwrap_or(false)
                        && (g.obj(s).nation != nation || Some(s) == deliberate)
                        && (g.obj(s).pos - pos).len() < prox.max(1.0)
                });
                if hit {
                    g.obj_mut(id).det = Det::Detonate;
                }
            }
            Kind::Ship => {}
        }
    }
}

/// Fuse bookkeeping, once per cycle (`sub_D130` 20709): arm counters, time
/// fuses (torp expiry fizzles, probe expiry detonates), ship self-destruct
/// countdowns.
pub fn fuse_step(g: &mut Game) {
    let mut countdowns: Vec<(String, i32)> = Vec::new();
    for id in g.ids() {
        let o = g.obj_mut(id);
        match o.kind {
            Kind::Torp => {
                let t = o.torp.as_mut().unwrap();
                if t.arm > 0.0 {
                    t.arm -= 1.0;
                } else {
                    t.strength -= 1.0;
                    if t.strength <= 0.0 && o.det == Det::None {
                        o.det = Det::Expire; // quiet fizzle
                    }
                }
            }
            Kind::Probe => {
                let p = o.probe.as_mut().unwrap();
                if p.arm > 0.0 {
                    p.arm -= 1.0;
                } else {
                    p.time -= 1.0;
                    if p.time <= 0.0 && o.det == Det::None {
                        o.det = Det::Detonate; // probes blow on expiry
                    }
                }
            }
            Kind::Ship => {
                let name = o.name.clone();
                let s = o.ship.as_mut().unwrap();
                if s.destruct_countdown >= 0.0 {
                    s.destruct_countdown -= 1.0;
                    if s.destruct_countdown <= 0.0 && o.det == Det::None {
                        o.det = Det::Detonate;
                    } else if s.destruct_countdown > 0.0 {
                        countdowns.push((name, s.destruct_countdown as i32));
                    }
                }
            }
        }
    }
    for (name, k) in countdowns {
        let plural = if k == 1 { "" } else { "s" };
        g.say(
            None,
            "",
            format!("The {name} will self destruct in {k} cycle{plural}!"),
            crate::events::ReportKind::Alert,
        );
    }
}
