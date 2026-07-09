//! Target selection (§12.4; scorer `sub_207F9` 62096, chooser `sub_1E30D`
//! 57502).

use crate::ai::{Ctx, Mission};
use crate::constants::*;
use crate::events::ReportKind;
use crate::game::Game;
use crate::object::*;
use crate::systems::helm::apparent_dist;

const HUGE: f64 = 1e30;

/// Score one candidate. Returns 0 to skip.
fn score(g: &mut Game, ctx: &Ctx, cand: ObjId) -> f64 {
    let me = ctx.id;
    let side = ctx.side;
    let c = g.obj(cand);
    if c.kind != Kind::Ship || cand == me {
        return 0.0;
    }
    if c.nation == side {
        return 0.0;
    }
    let (mission, ordered, current, bravery) = {
        let b = &g.obj(me).ship.as_ref().unwrap().brain;
        (b.mission.clone(), b.target_ordered, b.target, b.bravery)
    };
    // never detected → invisible to the brain
    if g.fog && !c.contact(side).ever {
        return 0.0;
    }
    // dead hulks score 0 — unless it's my probe-mission target (keep = HUGE)
    if c.is_hulk() {
        if let Some(Mission::Probe { ship }) = mission {
            if ship == cand {
                return HUGE;
            }
        }
        return 0.0;
    }
    // mission 22: bias toward whoever is closest to the ward
    if let Some(Mission::Defend { ship: ward }) = mission {
        if g.get(ward).is_some() {
            let d = apparent_dist(g, ward, cand, side).max(0.0);
            if d < 1.0 {
                return HUGE;
            }
            if d > AI_DEFEND_SCORE_RADIUS {
                return 0.0;
            }
            return 1.0 / d;
        }
    }
    // ordered target: obey with probability loyalty×2
    if ordered && current == Some(cand) {
        let loyalty = g.obj(me).ship.as_ref().unwrap().brain.loyalty;
        if g.rng.unit() < loyalty * AI_PURSUE_WARP_BONUS {
            return HUGE;
        }
    }

    let base = g
        .obj(cand)
        .ship
        .as_ref()
        .map(|s| s.brain.strength.max(0.1))
        .unwrap_or(0.1);
    let dist = apparent_dist(g, me, cand, side);
    let band = (dist / AI_SCORE_BAND_WIDTH) as usize;
    let mut m = if band < AI_DISTANCE_BANDS.len() {
        AI_DISTANCE_BANDS[band]
    } else {
        AI_FAR_BAND_MULT
    };
    // in my phaser envelope with working banks
    if ctx.op_banks > 0 && dist < ctx.bank_range {
        m *= 2.0;
    }
    // stickiness
    if current == Some(cand) {
        m *= AI_TARGET_STICKINESS;
    }
    // my probes already chase it (sub_1D9B1)
    let my_probes_chasing = g
        .probe_ids()
        .into_iter()
        .filter(|&p| g.obj(p).owner == Some(me) && g.obj(p).pursue == Some(cand))
        .count();
    if my_probes_chasing > 0 {
        m *= AI_SCORE_HALF;
    }
    // pile-on damping (sub_1D86B): halve if the candidate is already
    // outnumbered relative to my (lack of) bravery
    let targeting_sum: f64 = g
        .ship_ids()
        .into_iter()
        .filter(|&s| {
            s != me
                && g.obj(s)
                    .ship
                    .as_ref()
                    .map(|sh| sh.brain.target == Some(cand))
                    .unwrap_or(false)
        })
        .map(|s| g.obj(s).ship.as_ref().map(|sh| sh.brain.strength).unwrap_or(0.0))
        .sum();
    if targeting_sum > 0.0 {
        let share = base / targeting_sum;
        if (1.0 - bravery) * 5.0 > share {
            m *= AI_SCORE_HALF;
        }
    }
    // stale sensor contact
    if g.fog && !g.obj(cand).contact(side).visible {
        m *= AI_STALE_CONTACT_MULT;
    }
    base * m
}

/// `sub_1E30D` — pick the best target; announce changes; cancel weapon-order
/// missions on a change.
pub fn select_target(g: &mut Game, ctx: &mut Ctx) {
    let me = ctx.id;
    let mut best: Option<(ObjId, f64)> = None;
    for cand in g.ship_ids() {
        let s = score(g, ctx, cand);
        if s > 0.0 && best.map(|(_, bs)| s > bs).unwrap_or(true) {
            best = Some((cand, s));
        }
    }
    let old = g.obj(me).ship.as_ref().unwrap().brain.target;
    let new = best.map(|(id, _)| id);
    if new != old {
        {
            let b = &mut g.obj_mut(me).ship.as_mut().unwrap().brain;
            b.target = new;
            if b.target != old {
                // a self-chosen switch drops the ordered flag and cancels
                // phaser/torpedo/probe orders aimed at the old target
                if !matches!(new, Some(n) if b.target_ordered && Some(n) == old) {
                    b.target_ordered = false;
                }
                if matches!(
                    b.mission,
                    Some(Mission::Phaser { .. }) | Some(Mission::Torpedo { .. }) | Some(Mission::Probe { .. })
                ) {
                    b.mission = None;
                }
            }
        }
        if let Some(t) = new {
            let tname = g.obj(t).name.clone();
            let captain = g.obj(me).ship.as_ref().unwrap().captain.clone();
            let my_name = g.obj(me).name.clone();
            let side = ctx.side;
            g.say(
                Some(side),
                &captain,
                format!("{my_name} engaging the {tname}."),
                ReportKind::Ally,
            );
        }
    }
    if let Some(t) = new.filter(|&t| g.get(t).is_some()) {
        ctx.cache_target(g, t);
    } else {
        ctx.target = None;
    }
}
