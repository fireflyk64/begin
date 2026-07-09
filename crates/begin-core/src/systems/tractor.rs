//! Tractor beams & docking (§12.3; `sub_100B2` 26135, `sub_102A7` 26336,
//! docking permission `sub_1623D` 40317).

use crate::constants::*;
use crate::events::ReportKind;
use crate::game::Game;
use crate::object::*;

/// Per-cycle tractor physics + auto-release (`sub_102A7`, `sub_100B2`).
pub fn tractor_step(g: &mut Game) {
    for id in g.ship_ids() {
        let (engaged, target) = {
            let s = g.obj(id).ship.as_ref().unwrap();
            (s.tractor_engaged, s.tractor_target)
        };
        if !engaged {
            continue;
        }
        let Some(tid) = target.filter(|&t| g.get(t).is_some()) else {
            release(g, id, false);
            continue;
        };
        // auto-release on detonating or docked targets
        let bad = {
            let t = g.obj(tid);
            t.det != Det::None
                || t.ship.as_ref().map(|s| s.docked()).unwrap_or(false)
        };
        if bad {
            release(g, id, true);
            continue;
        }
        // pull physics: ratio = tractor_strength / target mass, weakened by
        // distance (÷ max(1, dist/1000)); below 0.1 the beam snaps
        let (my_pos, strength) = {
            let o = g.obj(id);
            let s = o.ship.as_ref().unwrap();
            (o.pos, g.data.ships[s.design].tractor_strength)
        };
        let (t_pos, t_mass) = {
            let t = g.obj(tid);
            let mass = t
                .ship
                .as_ref()
                .map(|s| g.data.ships[s.design].mass)
                .unwrap_or(1.0);
            (t.pos, mass.max(1.0))
        };
        let delta = my_pos - t_pos;
        let dist = delta.len();
        let ratio = (strength / t_mass) / (dist / TRACTOR_DIST_SCALE).max(1.0);
        if ratio < 0.1 {
            release(g, id, true);
            continue;
        }
        // pull distance per cycle toward the ship (ratio × 5.0 per sub-step
        // in the original = ratio × 100 units/cycle — tow speed "warp=ratio").
        // The helm recomputes ship velocity every cycle, so we apply the pull
        // to the position directly; it sums with the target's own movement.
        let pull = (ratio * UNITS_PER_WARP_PER_CYCLE).min(dist);
        if dist > 1e-9 {
            let step = delta * (pull / dist);
            let t = g.obj_mut(tid);
            t.pos += step;
        }
        // tow bearing for the display
        {
            let bearing = crate::math::bearing_of(-delta.x, -delta.y);
            let s = g.obj_mut(id).ship.as_mut().unwrap();
            s.tow_bearing = bearing;
        }
    }
}

fn release(g: &mut Game, id: ObjId, notice: bool) {
    let side = g.obj(id).nation;
    {
        let s = g.obj_mut(id).ship.as_mut().unwrap();
        s.tractor_engaged = false;
        s.tractor_target = None;
    }
    if notice {
        g.officer_say(side, "Tractor beam released.".into(), ReportKind::Crew);
    }
}

/// Engage the tractor on a target within range (player/AI `tractor <ship>`).
pub fn engage_tractor(g: &mut Game, id: ObjId, target: ObjId) -> Result<(), String> {
    let s = g.obj(id).ship.as_ref().unwrap();
    if s.tractor.as_ref().map(|t| t.destroyed()).unwrap_or(true) {
        return Err("we have no working tractor beam".into());
    }
    let dist = crate::systems::helm::dist(g, id, target);
    if dist > TRACTOR_RANGE {
        return Err("the target is beyond tractor range".into());
    }
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    s.tractor_engaged = true;
    s.tractor_target = Some(target);
    Ok(())
}

/// Docking permission (`sub_1623D`): same nation, both alive, host has
/// capacity, within 1000, total docked mass fits.
pub fn can_dock(g: &Game, ship: ObjId, base: ObjId) -> Result<(), String> {
    let s = g.obj(ship);
    let b = g.obj(base);
    if s.nation != b.nation {
        return Err("they will not let us dock".into());
    }
    let bd = &g.data.ships[b.ship.as_ref().unwrap().design];
    if bd.mass_capacity <= 0.0 {
        return Err("that ship has no docking bay".into());
    }
    if crate::systems::helm::dist(g, ship, base) > DOCK_RANGE {
        return Err("we are too far away to dock".into());
    }
    let sd = &g.data.ships[s.ship.as_ref().unwrap().design];
    let docked_mass: f64 = b
        .ship
        .as_ref()
        .unwrap()
        .docked_ships
        .iter()
        .filter_map(|&d| g.get(d))
        .filter_map(|o| o.ship.as_ref())
        .map(|sh| g.data.ships[sh.design].mass)
        .sum();
    if docked_mass + sd.mass > bd.mass_capacity {
        return Err("their docking bay is full".into());
    }
    Ok(())
}

pub fn dock(g: &mut Game, ship: ObjId, base: ObjId) -> Result<(), String> {
    can_dock(g, ship, base)?;
    {
        let s = g.obj_mut(ship);
        s.desired_warp = 0.0;
        s.warp = 0.0;
        let sh = s.ship.as_mut().unwrap();
        sh.docked_to = Some(base);
        sh.partner = Some(base);
    }
    g.obj_mut(base).ship.as_mut().unwrap().docked_ships.push(ship);
    let name = g.obj(ship).name.clone();
    let bname = g.obj(base).name.clone();
    g.say(None, "", format!("The {name} has docked with the {bname}."), ReportKind::Info);
    Ok(())
}

pub fn undock(g: &mut Game, ship: ObjId) {
    let base = g.obj(ship).ship.as_ref().unwrap().docked_to;
    {
        let sh = g.obj_mut(ship).ship.as_mut().unwrap();
        sh.docked_to = None;
        sh.partner = None;
    }
    if let Some(b) = base.filter(|&b| g.get(b).is_some()) {
        g.obj_mut(b).ship.as_mut().unwrap().docked_ships.retain(|&d| d != ship);
    }
}
