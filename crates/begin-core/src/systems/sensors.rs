//! Sensors & fog of war (§8; `sub_E673` 23114). Generalized from the
//! original's single player-side tracking to per-nation sides so every
//! human and AI ship honors the same stale-contact rules.

use crate::constants::CLOAK_REFLECT_SCALE;
use crate::events::ReportKind;
use crate::game::Game;
use crate::object::*;

pub fn sensor_update(g: &mut Game) {
    if !g.fog {
        // fog disabled: everything visible to everyone, contacts fresh
        for id in g.ids() {
            let cycle = g.cycle;
            let (pos, warp, course, mark) = {
                let o = g.obj(id);
                (o.pos, o.warp, o.course, o.mark)
            };
            let o = g.obj_mut(id);
            for c in o.contacts.iter_mut() {
                c.visible = true;
                c.ever = true;
                c.last_pos = pos;
                c.last_warp = warp;
                c.last_course = course;
                c.last_cycle = cycle;
                c.last_mark = mark;
            }
        }
        return;
    }

    let sides: Vec<usize> = (0..g.data.nations.len()).collect();
    let ids = g.ids();

    // collect each side's sensor platforms: (pos, range) of ships with a
    // working scanner and of data probes owned by the side
    let mut platforms: Vec<Vec<(crate::math::Vec3, f64)>> = vec![Vec::new(); sides.len()];
    for &id in &ids {
        let o = g.obj(id);
        match o.kind {
            Kind::Ship => {
                let s = o.ship.as_ref().unwrap();
                if s.scanner_works() {
                    let d = &g.data.ships[s.design];
                    platforms[o.nation].push((o.pos, d.scanner_range));
                }
            }
            Kind::Probe => {
                let p = o.probe.as_ref().unwrap();
                let d = &g.data.probes[p.design];
                if d.scan_range > 0.0 {
                    platforms[o.nation].push((o.pos, d.scan_range));
                }
            }
            Kind::Torp => {}
        }
    }

    for &id in &ids {
        let (own_nation, reflect, name, kind) = {
            let o = g.obj(id);
            let reflect = match o.kind {
                Kind::Ship => {
                    let s = o.ship.as_ref().unwrap();
                    let base = g.data.ships[s.design].scanner_reflect;
                    // cloaked & not hit this cycle: nearly invisible (×0.005)
                    if s.cloaked && s.hits_this_cycle == 0 {
                        base * CLOAK_REFLECT_SCALE
                    } else {
                        base
                    }
                }
                _ => 0.5, // probes (and torps) reflect 0.5
            };
            (o.nation, reflect, o.name.clone(), o.kind)
        };
        for &side in &sides {
            let visible = if side == own_nation {
                true
            } else {
                // per-axis box test, not a circle (original quirk)
                let pos = g.obj(id).pos;
                platforms[side].iter().any(|&(p, range)| {
                    let r = range * reflect;
                    (pos.x - p.x).abs() < r && (pos.y - p.y).abs() < r && (pos.z - p.z).abs() < r
                })
            };
            let cycle = g.cycle;
            let (pos, warp, course, mark) = {
                let o = g.obj(id);
                (o.pos, o.warp, o.course, o.mark)
            };
            let o = g.obj_mut(id);
            let c = &mut o.contacts[side];
            let was = c.visible;
            c.visible = visible;
            if visible {
                c.ever = true;
                c.last_pos = pos;
                c.last_warp = warp;
                c.last_course = course;
                c.last_mark = mark;
                c.last_cycle = cycle;
            }
            // transition reports, only about ships and only to that side
            if kind == Kind::Ship && side != own_nation {
                if visible && !was {
                    g.officer_say(side, format!("We have contact with the {name}."), ReportKind::Info);
                } else if !visible && was {
                    g.officer_say(side, format!("We have lost contact with the {name}."), ReportKind::Info);
                }
            }
        }
    }
}
