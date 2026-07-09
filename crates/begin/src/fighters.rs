//! Battlestar fighter operations (§7.4: fighters are tiny ships launched
//! and recovered like docked ships from a carrier hull).

use begin_core::ai::Mission;
use begin_core::math::{dir, norm360};
use begin_core::object::{Control, ObjId};
use begin_core::Game;

/// Launch up to `n` fighters; they spawn around the carrier under AI
/// control with a standing order to defend it.
pub fn launch_fighters(g: &mut Game, carrier: ObjId, n: usize) -> Result<usize, String> {
    let (design_idx, remaining, pos, course, nation) = {
        let o = g.obj(carrier);
        let s = o.ship.as_ref().unwrap();
        let d = &g.data.ships[s.design];
        let Some(fc) = d.fighter_class.clone() else {
            return Err("This ship carries no fighters.".into());
        };
        let fi = g
            .data
            .ships
            .iter()
            .position(|x| x.name == fc && x.nation == d.nation)
            .ok_or("Unknown fighter class.")?;
        (fi, s.fighters_left, o.pos, o.course, o.nation)
    };
    let _ = nation;
    if remaining <= 0 {
        return Err("No fighters left in the bays.".into());
    }
    let count = n.min(remaining as usize).min(8); // one squadron per order
    let mut launched = 0;
    for k in 0..count {
        let cname = {
            let d = &g.data.ships[design_idx];
            let base = d
                .ship_names
                .get(k % d.ship_names.len().max(1))
                .cloned()
                .unwrap_or_else(|| "Viper".into());
            format!("{}-{}", base, g.cycle % 100 * 10 + k as u64)
        };
        let spread = norm360(course + (k as f64 - count as f64 / 2.0) * 30.0);
        let offset = dir(spread, 0.0) * 300.0;
        let Some(id) = g.spawn_ship(design_idx, cname, pos + offset, course, Control::Ai) else {
            break;
        };
        g.obj_mut(id).desired_warp = 6.0;
        begin_core::ai::missions::receive_order(g, id, Mission::Defend { ship: carrier });
        launched += 1;
    }
    g.obj_mut(carrier).ship.as_mut().unwrap().fighters_left -= launched as i32;
    Ok(launched)
}

/// Order every fighter of our carrier's class home.
pub fn recall_fighters(g: &mut Game, carrier: ObjId) -> usize {
    let side = g.obj(carrier).nation;
    let fighter_class = {
        let s = g.obj(carrier).ship.as_ref().unwrap();
        g.data.ships[s.design].fighter_class.clone()
    };
    let Some(fc) = fighter_class else { return 0 };
    let mut n = 0;
    for id in g.ship_ids() {
        if id == carrier || g.obj(id).nation != side {
            continue;
        }
        let is_fighter = {
            let s = g.obj(id).ship.as_ref().unwrap();
            g.data.ships[s.design].name == fc
        };
        if is_fighter {
            begin_core::ai::missions::receive_order(g, id, Mission::Dock { base: carrier });
            n += 1;
        }
    }
    n
}

/// Fighters that dock with their carrier are struck below and restored to
/// the bays. Call once per cycle.
pub fn absorb_docked_fighters(g: &mut Game) {
    for carrier in g.ship_ids() {
        let fighter_class = {
            let Some(o) = g.get(carrier) else { continue };
            let s = o.ship.as_ref().unwrap();
            g.data.ships[s.design].fighter_class.clone()
        };
        let Some(fc) = fighter_class else { continue };
        let docked: Vec<ObjId> = g.obj(carrier).ship.as_ref().unwrap().docked_ships.clone();
        for f in docked {
            let is_fighter = g
                .get(f)
                .and_then(|o| o.ship.as_ref())
                .map(|s| g.data.ships[s.design].name == fc)
                .unwrap_or(false);
            if is_fighter {
                begin_core::systems::tractor::undock(g, f);
                g.remove(f);
                g.obj_mut(carrier).ship.as_mut().unwrap().fighters_left += 1;
            }
        }
    }
}
