//! Boarding combat & transporters (§12.1-12.2; `sub_C010` 18795,
//! `sub_16897` 41286, `sub_16A70` 41544).

use crate::ai::Brain;
use crate::events::ReportKind;
use crate::game::Game;
use crate::object::*;

/// §12.1 round-by-round boarding combat, per ship carrying boarders.
pub fn boarding_step(g: &mut Game) {
    for id in g.ship_ids() {
        let (boarders, boarders_nation, defenders, name) = {
            let o = g.obj(id);
            let s = o.ship.as_ref().unwrap();
            (s.boarders, s.boarders_nation, s.survivors, o.name.clone())
        };
        if boarders <= 0 {
            continue;
        }
        // 35% chance combat pauses this cycle while defenders hold out
        if defenders >= 6 && g.rng.percent(35.0) {
            continue;
        }
        let att_level = g.data.nations[boarders_nation].boarding_level;
        let def_level = g.data.nations[g.obj(id).nation].boarding_level;
        let rounds = (g.rng.unit() * boarders as f64) as i32;
        let mut att = boarders;
        let mut def = defenders;
        for _ in 0..rounds {
            if att <= 0 || def <= 0 {
                break;
            }
            let a = g.rng.range(0.0, att_level);
            let d = g.rng.range(0.0, def_level);
            if a > d + 0.1 {
                def -= 1;
            } else if d > a + 0.1 {
                att -= 1;
            }
        }
        {
            let s = g.obj_mut(id).ship.as_mut().unwrap();
            s.boarders = att.max(0);
            s.survivors = def.max(0);
        }
        if att <= 0 {
            g.say(
                None,
                "",
                format!("The {name}'s crew has repelled the boarders!"),
                ReportKind::Alert,
            );
            continue;
        }
        if def < 6 {
            if att < 6 {
                // mutual annihilation: a drifting derelict
                let s = g.obj_mut(id).ship.as_mut().unwrap();
                s.boarders = 0;
                s.survivors = 0;
                g.say(
                    None,
                    "",
                    format!("Fighting aboard the {name} has left no one alive."),
                    ReportKind::Alert,
                );
            } else {
                capture(g, id, boarders_nation, att);
            }
        }
    }
}

/// Capture: the ship changes nation, gets AI control and a fresh brain
/// (`sub_1D4BD`); a running self-destruct may be defused.
fn capture(g: &mut Game, id: ObjId, new_nation: usize, crew: i32) {
    let name = g.obj(id).name.clone();
    let brain = Brain::roll(&g.data.nations[new_nation].clone(), &mut g.rng);
    // the original gives boarders a 10%/cycle chance to defuse a running
    // self-destruct; we approximate with a one-shot 50% (≈ 10%/cycle over
    // the 5-cycle countdown)
    let defused = g.rng.percent(50.0);
    {
        let o = g.obj_mut(id);
        o.nation = new_nation;
        o.control = Control::Ai;
        let s = o.ship.as_mut().unwrap();
        s.survivors = crew;
        s.boarders = 0;
        s.brain = brain;
        if defused && s.destruct_countdown >= 0.0 {
            s.destruct_countdown = -1.0;
        }
        s.cloaked = false;
        s.tractor_engaged = false;
        s.tractor_target = None;
    }
    let nation_name = g.data.nations[new_nation].adjective.clone();
    g.say(
        None,
        "",
        format!("The {name} has been captured by {nation_name} boarders!"),
        ReportKind::Alert,
    );
}

/// §12.2 transporter capacity: min(Σ beam_cap of working transporters,
/// from.survivors − 6, room aboard the target).
pub fn beam_capacity(g: &Game, from: ObjId, to: ObjId) -> i32 {
    let s = g.obj(from).ship.as_ref().unwrap();
    let d = &g.data.ships[s.design];
    let cap: i32 = s
        .transporters
        .iter()
        .filter(|t| !t.destroyed())
        .map(|_| d.beam_cap as i32)
        .sum();
    let spare = s.survivors - 6;
    let t = g.obj(to).ship.as_ref().unwrap();
    let td = &g.data.ships[t.design];
    let room = td.life_support as i32 - t.survivors - t.boarders;
    cap.min(spare).min(room).max(0)
}

/// §12.2 validity + execution. Beaming onto an enemy ship is boarding.
pub fn transport(g: &mut Game, from: ObjId, to: ObjId, count: i32) -> Result<i32, String> {
    if g.get(to).is_none() {
        return Err("no such ship".into());
    }
    let (from_nation, from_pos, side) = {
        let o = g.obj(from);
        (o.nation, o.pos, o.nation)
    };
    let (to_nation, to_pos) = {
        let o = g.obj(to);
        (o.nation, o.pos)
    };
    let d = {
        let s = g.obj(from).ship.as_ref().unwrap();
        g.data.ships[s.design].clone()
    };
    let dist = (to_pos - from_pos).len();
    if dist > d.beam_range {
        return Err("target is out of transporter range".into());
    }
    if g.fog && !g.obj(to).contact(side).visible {
        return Err("no sensor contact with the target".into());
    }
    // target's shields must be down (`sub_FFCC`)
    let shields_up = g
        .obj(to)
        .ship
        .as_ref()
        .map(|s| s.shields.iter().any(|sh| sh.effective > 0.0))
        .unwrap_or(false);
    if shields_up {
        return Err("the target's shields are up".into());
    }
    let hostile = to_nation != from_nation;
    if hostile && g.obj(to).ship.as_ref().unwrap().docked() {
        return Err("the target is docked".into());
    }
    // existing boarders must be ours
    {
        let t = g.obj(to).ship.as_ref().unwrap();
        if t.boarders > 0 && t.boarders_nation != from_nation {
            return Err("another boarding party is already aboard".into());
        }
    }
    let n = count.min(beam_capacity(g, from, to));
    if n <= 0 {
        return Err("transporters cannot move anyone right now".into());
    }
    {
        let s = g.obj_mut(from).ship.as_mut().unwrap();
        s.survivors -= n;
    }
    {
        let t = g.obj_mut(to).ship.as_mut().unwrap();
        if hostile {
            t.boarders += n;
            t.boarders_nation = from_nation;
        } else {
            t.survivors += n;
        }
    }
    Ok(n)
}
