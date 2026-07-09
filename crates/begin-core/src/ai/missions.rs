//! Player orders — the 22 ally missions (§12.8; `sub_1F3B8` 59564).

use crate::ai::behavior::{combat_speed, phaser_range_maneuver, torp_range_maneuver, weave_to};
use crate::ai::{Ctx, Mission};
use crate::constants::*;
use crate::events::ReportKind;
use crate::game::Game;
use crate::math::*;
use crate::object::*;
use crate::orders;
use crate::systems::helm::{apparent_dist, target_bearing_mark};

/// Loyalty gate at order time (`sub_200BD` 61163): allies disobey with
/// probability rand ≥ loyalty×2, answering with an insult.
/// Returns false when the order is refused.
pub fn receive_order(g: &mut Game, id: ObjId, mission: Mission) -> bool {
    let (loyalty, docked, captain) = {
        let s = g.obj(id).ship.as_ref().unwrap();
        (s.brain.loyalty, s.docked(), s.captain.clone())
    };
    let side = g.obj(id).nation;
    // docked ships refuse everything except undock
    if docked && !matches!(mission, Mission::Undock | Mission::Release) {
        g.say(
            Some(side),
            &captain,
            "We are docked.  We can't do that.".to_string(),
            ReportKind::Ally,
        );
        return false;
    }
    if g.rng.unit() >= loyalty * 2.0 {
        let lines = &g.data.messages.insult;
        let line = lines
            [g.rng.irange(0, lines.len() as i32 - 1).clamp(0, lines.len() as i32 - 1) as usize]
            .clone();
        g.say(Some(side), &captain, line, ReportKind::Ally);
        return false;
    }
    // orders that resolve immediately
    match mission {
        Mission::Release => {
            let s = g.obj_mut(id).ship.as_mut().unwrap();
            s.tractor_engaged = false;
            s.tractor_target = None;
            g.say(Some(side), &captain, "Tractor beam released.".into(), ReportKind::Ally);
            return true;
        }
        Mission::Undock => {
            crate::systems::tractor::undock(g, id);
            g.say(Some(side), &captain, "Undocking.".into(), ReportKind::Ally);
            return true;
        }
        Mission::Attack { ship } => {
            let b = &mut g.obj_mut(id).ship.as_mut().unwrap().brain;
            b.target = Some(ship);
            b.target_ordered = true;
            b.mission = Some(Mission::Attack { ship });
        }
        Mission::HoldFire => {
            g.obj_mut(id).ship.as_mut().unwrap().brain.hold_fire = true;
            g.say(Some(side), &captain, "Holding fire.".into(), ReportKind::Ally);
            return true;
        }
        m => {
            g.obj_mut(id).ship.as_mut().unwrap().brain.mission = Some(m);
        }
    }
    g.say(Some(side), &captain, "Acknowledged.".into(), ReportKind::Ally);
    true
}

/// Cancel any mission (order `cancel` / `disengage`).
pub fn cancel_order(g: &mut Game, id: ObjId) {
    let b = &mut g.obj_mut(id).ship.as_mut().unwrap().brain;
    b.mission = None;
    b.target_ordered = false;
    b.hold_fire = false;
}

/// Per-cycle mission executor (`sub_1F3B8`).
pub fn execute(g: &mut Game, ctx: &mut Ctx) {
    let Some(mission) = g.obj(ctx.id).ship.as_ref().unwrap().brain.mission.clone() else {
        return;
    };
    let me = ctx.id;
    let side = ctx.side;
    match mission {
        Mission::Escort { ship, range } => {
            let Some(_) = g.get(ship) else { return clear(g, me) };
            let dist = apparent_dist(g, me, ship, side);
            let (t_course, t_warp) = {
                let t = g.obj(ship);
                (t.course, t.warp)
            };
            if (dist - range).abs() < AI_ESCORT_TOLERANCE {
                let o = g.obj_mut(me);
                o.helm = HelmMode::Course;
                o.desired_course = t_course;
                o.desired_warp = t_warp;
            } else {
                let warp = if dist > range { (t_warp * 1.5).max(2.0) } else { (t_warp * 0.5).max(0.5) };
                orders::pursue(g, me, ship, Some(warp), false);
            }
        }
        Mission::Attack { ship } => {
            if g.get(ship).is_none() || g.obj(ship).is_hulk() {
                return clear(g, me);
            }
            {
                let b = &mut g.obj_mut(me).ship.as_mut().unwrap().brain;
                b.target = Some(ship);
                b.target_ordered = true;
            }
            if ctx.target.as_ref().map(|t| t.id) != Some(ship) {
                ctx.cache_target(g, ship);
            }
            weave_to(g, ctx, 1);
        }
        Mission::Course { course } => {
            let o = g.obj_mut(me);
            o.helm = HelmMode::Course;
            o.desired_course = norm360(course);
        }
        Mission::Phaser { ship } => {
            if g.get(ship).is_none() || g.obj(ship).is_hulk() {
                return clear(g, me);
            }
            {
                let b = &mut g.obj_mut(me).ship.as_mut().unwrap().brain;
                b.target = Some(ship);
                b.target_ordered = true;
            }
            ctx.cache_target(g, ship);
            phaser_range_maneuver(g, ctx);
        }
        Mission::Torpedo { ship } => {
            if g.get(ship).is_none() || g.obj(ship).is_hulk() {
                return clear(g, me);
            }
            {
                let b = &mut g.obj_mut(me).ship.as_mut().unwrap().brain;
                b.target = Some(ship);
                b.target_ordered = true;
            }
            ctx.cache_target(g, ship);
            torp_range_maneuver(g, ctx);
        }
        Mission::Probe { ship } => {
            if g.get(ship).is_none() {
                return clear(g, me);
            }
            {
                let b = &mut g.obj_mut(me).ship.as_mut().unwrap().brain;
                b.target = Some(ship);
                b.target_ordered = true;
            }
            ctx.cache_target(g, ship);
            // steer to bearing; evade once probes are chasing (§12.8 #6)
            let chasing = g.probe_ids().into_iter().any(|p| g.obj(p).pursue == Some(ship));
            if chasing {
                weave_to(g, ctx, 2);
            } else {
                let (b, m) = target_bearing_mark(g, me, ship, side);
                let o = g.obj_mut(me);
                o.helm = HelmMode::Course;
                o.desired_course = b;
                o.desired_mark = m;
            }
        }
        Mission::Standoff => {
            let Some(t) = ctx.target.as_ref() else { return };
            let aggression = g.obj(me).ship.as_ref().unwrap().brain.aggression;
            let keep = (1.0 - aggression) * AI_STANDOFF_AGGR + AI_STANDOFF_BASE;
            if t.dist < keep - AI_STANDOFF_JITTER {
                weave_to(g, ctx, 2);
            } else if t.dist > keep + AI_STANDOFF_JITTER {
                weave_to(g, ctx, 1);
            } else if g.rng.percent(33.0) {
                let course = g.rng.range(0.0, 360.0);
                let warp = combat_speed(g, ctx);
                let o = g.obj_mut(me);
                o.helm = HelmMode::Course;
                o.desired_course = norm360(course);
                o.desired_warp = warp;
            }
        }
        Mission::Transport { count, ship } => {
            if g.get(ship).is_none() {
                return clear(g, me);
            }
            match crate::systems::boarding::transport(g, me, ship, count) {
                Ok(n) => {
                    let remaining = count - n;
                    let captain = g.obj(me).ship.as_ref().unwrap().captain.clone();
                    g.say(
                        Some(side),
                        &captain,
                        format!("Transported {n} crew."),
                        ReportKind::Ally,
                    );
                    if remaining > 0 {
                        g.obj_mut(me).ship.as_mut().unwrap().brain.mission =
                            Some(Mission::Transport { count: remaining, ship });
                    } else {
                        clear(g, me);
                    }
                }
                Err(_) => {
                    // close in until the beam works
                    orders::pursue(g, me, ship, Some(3.0), false);
                }
            }
        }
        Mission::Dock { base } => {
            if g.get(base).is_none() {
                return clear(g, me);
            }
            if g.obj(me).ship.as_ref().unwrap().docked() {
                return clear(g, me);
            }
            if crate::systems::tractor::dock(g, me, base).is_ok() {
                clear(g, me);
            } else {
                let dist = apparent_dist(g, me, base, side);
                let warp = if dist > 5000.0 { combat_speed(g, ctx) } else { 1.0 };
                orders::pursue(g, me, base, Some(warp), false);
            }
        }
        Mission::Undock | Mission::Release | Mission::HoldFire => clear(g, me),
        Mission::Tow { ship, dest } => {
            if g.get(ship).is_none() || g.get(dest).is_none() {
                return clear(g, me);
            }
            let engaged = g.obj(me).ship.as_ref().unwrap().tractor_engaged;
            if !engaged {
                // strength must dominate the tow's mass (ratio ≥ 1)
                let ratio = {
                    let d = &g.data.ships[g.obj(me).ship.as_ref().unwrap().design];
                    let m = &g.data.ships[g.obj(ship).ship.as_ref().unwrap().design];
                    d.tractor_strength / m.mass.max(1.0)
                };
                if ratio < 1.0 {
                    let captain = g.obj(me).ship.as_ref().unwrap().captain.clone();
                    g.say(
                        Some(side),
                        &captain,
                        "She's too heavy for our tractor beam.".into(),
                        ReportKind::Ally,
                    );
                    return clear(g, me);
                }
                if crate::systems::tractor::engage_tractor(g, me, ship).is_err() {
                    orders::pursue(g, me, ship, Some(2.0), false);
                    return;
                }
            }
            // drag toward the destination at warp = ratio
            let ratio = {
                let d = &g.data.ships[g.obj(me).ship.as_ref().unwrap().design];
                let m = &g.data.ships[g.obj(ship).ship.as_ref().unwrap().design];
                (d.tractor_strength / m.mass.max(1.0)).min(ctx.max_warp)
            };
            let dist = apparent_dist(g, me, dest, side);
            if dist < TRACTOR_RANGE {
                let s = g.obj_mut(me).ship.as_mut().unwrap();
                s.tractor_engaged = false;
                s.tractor_target = None;
                let captain = g.obj(me).ship.as_ref().unwrap().captain.clone();
                g.say(Some(side), &captain, "Tow complete.".into(), ReportKind::Ally);
                clear(g, me);
            } else {
                let (b, m) = target_bearing_mark(g, me, dest, side);
                let o = g.obj_mut(me);
                o.helm = HelmMode::Course;
                o.desired_course = b;
                o.desired_mark = m;
                o.desired_warp = ratio.max(0.5);
            }
        }
        Mission::Recover { ship } => {
            if g.get(ship).is_none() {
                return clear(g, me);
            }
            if crate::systems::tractor::dock(g, ship, me).is_ok() {
                clear(g, me);
            }
        }
        Mission::Eject { ship } => {
            if g.get(ship).is_some() {
                crate::systems::tractor::undock(g, ship);
            }
            clear(g, me);
        }
        Mission::Approach { ship, range } => {
            let Some(_) = g.get(ship) else { return clear(g, me) };
            // station-keeping (`sub_1D342` 55638)
            let dist = apparent_dist(g, me, ship, side);
            let (t_course, t_warp) = {
                let t = g.obj(ship);
                (t.course, t.warp)
            };
            if dist < range {
                let o = g.obj_mut(me);
                o.helm = HelmMode::Course;
                o.desired_course = t_course;
                o.desired_warp = t_warp;
            } else if dist > range * AI_STATION_KEEP_OUTER {
                orders::pursue(g, me, ship, Some(t_warp + 2.0), false);
            } else {
                let (b, m) = target_bearing_mark(g, me, ship, side);
                let o = g.obj_mut(me);
                o.helm = HelmMode::Course;
                o.desired_course = b;
                o.desired_mark = m;
            }
        }
        Mission::Tractor { ship } => {
            if g.get(ship).is_none() {
                return clear(g, me);
            }
            if crate::systems::tractor::engage_tractor(g, me, ship).is_ok() {
                clear(g, me);
            } else {
                orders::pursue(g, me, ship, Some(2.0), false);
            }
        }
        Mission::Stop => {
            g.obj_mut(me).desired_warp = 0.0;
        }
        Mission::Defend { ship } => {
            let Some(_) = g.get(ship) else { return clear(g, me) };
            let dist = apparent_dist(g, me, ship, side);
            if dist > AI_DEFEND_RADIUS {
                let warp = combat_speed(g, ctx);
                orders::pursue(g, me, ship, Some(warp), false);
            } else if ctx.target.is_some() {
                crate::ai::behavior::default_maneuver(g, ctx);
            }
        }
    }
}

fn clear(g: &mut Game, id: ObjId) {
    g.obj_mut(id).ship.as_mut().unwrap().brain.mission = None;
}
