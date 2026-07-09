//! Warp acceleration (§5.1, `sub_CCE2` 20260) and turning/guidance
//! (§5.2, `sub_CEB8` 20451), extended to 3D with the mark angle.

use crate::constants::*;
use crate::game::Game;
use crate::math::*;
use crate::object::*;

/// §5.1 — warp change for every object (`sub_CCE2`).
pub fn warp_accel(g: &mut Game) {
    for id in g.ids() {
        let o = g.obj(id);
        let (w1accel, decel) = match o.ship.as_ref() {
            Some(s) => {
                let d = &g.data.ships[s.design];
                (d.w1accel, d.decel)
            }
            // Ordnance reaches design velocity instantly at spawn and keeps it.
            None => (f64::MAX, f64::MAX),
        };
        let o = g.obj_mut(id);
        let desired = o.desired_warp.max(MIN_WARP);
        let diff = desired - o.warp;
        if diff.abs() < WARP_SNAP {
            o.warp = desired;
        } else if desired <= 1.0 && o.warp <= 1.0 {
            o.warp = desired; // instant below warp 1
        } else if diff > 0.0 {
            let rate = if o.warp >= 1.0 { w1accel / o.warp } else { w1accel };
            o.warp += rate.min(diff);
        } else {
            o.warp -= decel.min(-diff);
        }
    }
}

/// §5.2 — pursue/elude re-aim, turning, and velocity vectors (`sub_CEB8`).
pub fn turn_and_guide(g: &mut Game) {
    let planar = g.tuning.planar_lock;
    for id in g.ids() {
        let o = g.obj(id);
        if o.ballistic {
            continue; // +0Ah: unguided ordnance never steers
        }
        // Pursue/elude: desired course/mark from (possibly last-known) target
        // position — sub_1340E/sub_13535 honor the ghost (§8).
        let helm = o.helm;
        let target = o.pursue;
        if helm != HelmMode::Course {
            if let Some(tid) = target.filter(|&t| g.get(t).is_some()) {
                let viewer_side = o.nation;
                let (tb, tm) = target_bearing_mark(g, id, tid, viewer_side);
                let o = g.obj_mut(id);
                match helm {
                    HelmMode::Pursue => {
                        o.desired_course = tb;
                        o.desired_mark = tm;
                    }
                    HelmMode::Elude => {
                        o.desired_course = norm360(tb + 180.0);
                        o.desired_mark = -tm;
                    }
                    HelmMode::Course => unreachable!(),
                }
            }
        }

        let w1turn = match g.obj(id).ship.as_ref() {
            Some(s) => g.data.ships[s.design].w1turn,
            None => f64::MAX, // homing ordnance snaps to its desired course
        };
        let o = g.obj_mut(id);
        if planar {
            o.desired_mark = 0.0;
            o.mark = 0.0;
            o.pos.z = 0.0;
        }
        let dc = ang_delta(o.course, o.desired_course);
        let dm = o.desired_mark - o.mark;
        // Not a ship, or slow, or tiny diff → snap (ships turn instantly ≤ warp 1)
        if o.kind != Kind::Ship || o.warp <= 1.0 || (dc.abs() < 1e-9 && dm.abs() < 1e-9) {
            o.course = o.desired_course;
            o.mark = o.desired_mark;
        } else {
            let rate = w1turn / o.warp;
            o.course = norm360(o.course + dc.signum() * dc.abs().min(rate));
            o.mark += dm.signum() * dm.abs().min(rate);
        }
        o.mark = o.mark.clamp(-90.0, 90.0);
        // velocity per sub-step (§1)
        o.vel = dir(o.course, o.mark) * (o.warp * SUBSTEP_SCALE);
    }
}

/// Bearing and mark from `from` to `to`, honoring last-known position when
/// the viewer side has lost contact (`sub_1340E` 33782 / §8).
pub fn target_bearing_mark(g: &Game, from: ObjId, to: ObjId, side: usize) -> (f64, f64) {
    let p = g.obj(from).pos;
    let t = apparent_pos(g, to, side);
    let d = t - p;
    (bearing_of(d.x, d.y), mark_of(d))
}

/// Position of `id` as seen by `side` (ghost when contact lost).
pub fn apparent_pos(g: &Game, id: ObjId, side: usize) -> Vec3 {
    let o = g.obj(id);
    if !g.fog || o.nation == side {
        return o.pos;
    }
    let c = o.contact(side);
    if c.visible {
        o.pos
    } else {
        c.last_pos
    }
}

/// Distance honoring last-known position (`sub_13108` 33452).
pub fn apparent_dist(g: &Game, from: ObjId, to: ObjId, side: usize) -> f64 {
    (apparent_pos(g, to, side) - g.obj(from).pos).len()
}

/// True distance (`distToTarget` 33592).
pub fn dist(g: &Game, a: ObjId, b: ObjId) -> f64 {
    (g.obj(b).pos - g.obj(a).pos).len()
}

/// Relative bearing "MARK column" (`sub_1375C` 34152):
/// normalize(bearing(a→b) − a.course).
pub fn relative_bearing(g: &Game, a: ObjId, b: ObjId, side: usize) -> f64 {
    let (tb, _) = target_bearing_mark(g, a, b, side);
    norm360(tb - g.obj(a).course)
}

/// Target's course angle off the line of sight (`sub_1372B` 34118) — the
/// angle used by the torpedo lead solution.
pub fn angle_off(g: &Game, shooter: ObjId, target: ObjId, side: usize) -> f64 {
    let (bearing_back, _) = target_bearing_mark(g, target, shooter, side);
    norm360(bearing_back - g.obj(target).course)
}

/// Integrate one movement sub-step for every object (`sub_D0DC` 20667).
pub fn integrate_substep(g: &mut Game) {
    let planar = g.tuning.planar_lock;
    for id in g.ids() {
        let o = g.obj_mut(id);
        o.pos += o.vel;
        if planar {
            o.pos.z = 0.0;
        }
    }
}
