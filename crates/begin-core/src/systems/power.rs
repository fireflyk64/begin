//! Power generation, warp drives, life support, cloak/tractor upkeep
//! (§5.3-5.4, §6). Decoded exactly from `sub_C4F7` 19337, `sub_EED2` 24006,
//! `sub_F07A` 24210, `sub_EFB2` 24112, `sub_F259` 24428, `sub_F101` 24276.
//!
//! The true begin2 power model (resolves AI_AND_COMBAT.md Part IV item 1):
//! - WARP POWER (node+54h) = Σ drive_health × design.warp_power — warp is
//!   paid by the *drives*, not the reactors (`sub_F07A` iterates drives).
//! - OTHER POWER (node+5Ch) = Σ reactor_health × reactor_output
//!   + Σ charge of *undamaged* batteries (`sub_EED2`; a damaged battery is
//!   offline until repaired to 0%).
//! - Warp costs desired_warp × warp_power_use from WARP POWER; **half** the
//!   unused warp budget spills into the pool (`fmul flt_32CBE` = 0.5).
//! - Drains from the pool, in order: life support (design crew / 10),
//!   shields, cloak, tractor. Weapon charging draws later in the pipeline.
//! - End of cycle (`sub_F101`): undamaged batteries refill to capacity from
//!   the pool; RESIDUAL POWER = pool_before_refill − old battery total
//!   (negative = net battery drain); brain battery fraction = new/capacity.

use crate::constants::*;
use crate::events::ReportKind;
use crate::game::Game;
use crate::object::*;

/// `sub_F07A` — WARP POWER: Σ over drives of health × design.warp_power.
/// (Also the destruct-yield basis in §10.)
pub fn gross_warp_power(g: &Game, id: ObjId) -> f64 {
    let o = g.obj(id);
    let Some(s) = o.ship.as_ref() else { return 0.0 };
    let d = &g.data.ships[s.design];
    s.drives.iter().map(|dr| dr.sys.health() * d.warp_power).sum()
}

/// `sub_EED2` — OTHER POWER pool: reactors × health + undamaged batteries.
pub fn gross_pool(g: &Game, id: ObjId) -> f64 {
    let o = g.obj(id);
    let Some(s) = o.ship.as_ref() else { return 0.0 };
    let d = &g.data.ships[s.design];
    let reactors: f64 = s.reactors.iter().map(|r| d.reactor_output * r.health()).sum();
    let batteries: f64 = s.batteries.iter().filter(|b| b.sys.dmg == 0).map(|b| b.charge).sum();
    reactors + batteries
}

/// Max attainable warp (§5.3, `sub_EFB2`): warp-1 impulse fallback; uses the
/// pre-cost warp budget.
pub fn max_attainable_warp(g: &Game, id: ObjId) -> f64 {
    let o = g.obj(id);
    let Some(s) = o.ship.as_ref() else { return f64::MAX };
    let d = &g.data.ships[s.design];
    let fallback = match &s.impulse {
        Some(imp) if !imp.destroyed() => 1.0,
        _ => 0.0,
    };
    if o.warp_budget <= 0.0 || d.warp_power_use <= 0.0 {
        return fallback;
    }
    let w = (o.warp_budget / d.warp_power_use).min(d.max_warp);
    if w <= 1.0 {
        fallback
    } else {
        w
    }
}

/// §6 per-ship power step (`sub_C4F7`).
pub fn power_step(g: &mut Game, id: ObjId) {
    let Some(o) = g.get(id) else { return };
    if o.ship.is_none() {
        return;
    }
    let side = g.obj(id).nation;
    let name = g.obj(id).name.clone();

    // sub_EED2: initialize WARP POWER and the pool
    let warp_power = gross_warp_power(g, id);
    let pool0 = gross_pool(g, id);
    {
        let o = g.obj_mut(id);
        o.warp_budget = warp_power;
        o.pool = pool0;
    }

    // sub_EFB2: max attainable warp, cached (body+6A8h); clamp desired warp
    let maxw = max_attainable_warp(g, id);
    {
        let o = g.obj_mut(id);
        o.ship.as_mut().unwrap().max_warp_attainable = maxw;
        if o.desired_warp > maxw {
            o.desired_warp = maxw;
        }
        if o.desired_warp < MIN_WARP {
            o.desired_warp = MIN_WARP;
        }
    }

    // warp cost from the warp budget; warp_ratio for drive temperature
    let (design_crew, warp_power_use, docked) = {
        let o = g.obj(id);
        let s = o.ship.as_ref().unwrap();
        let d = &g.data.ships[s.design];
        (d.crew, d.warp_power_use, s.docked())
    };
    let warp_ratio = {
        let o = g.obj_mut(id);
        let ratio = if o.desired_warp < 1.0 && maxw == 0.0 {
            0.0
        } else if maxw > 0.0 {
            o.desired_warp.abs() / maxw
        } else {
            0.0
        };
        o.warp_budget -= o.desired_warp.abs() * warp_power_use;
        ratio
    };

    // sub_F259: drive temperature
    drive_temperature(g, id, warp_ratio);

    // half the unused warp budget spills into the pool (flt_32CBE = 0.5)
    {
        let o = g.obj_mut(id);
        if o.warp_budget > 0.0 {
            o.pool += o.warp_budget * PHASER_HALF;
        }
    }

    // life support: design crew / 10 per cycle; 3 consecutive failures kill
    // the crew (`sub_F64C`); docked ships and dead hulks exempt
    let survivors = g.obj(id).ship.as_ref().unwrap().survivors;
    if survivors > 0 && !docked {
        let need = design_crew as f64 / 10.0;
        let o = g.obj_mut(id);
        if o.pool >= need {
            o.pool -= need;
            o.ship.as_mut().unwrap().life_failures = 0;
        } else {
            let s = o.ship.as_mut().unwrap();
            s.life_failures += 1;
            let failures = s.life_failures;
            if failures >= LIFE_SUPPORT_FAILURES_FATAL {
                let s = g.obj_mut(id).ship.as_mut().unwrap();
                s.survivors = 0;
                s.boarders = 0;
                g.say(
                    None,
                    "",
                    format!("{name}'s life support has failed.  All hands lost."),
                    ReportKind::Alert,
                );
            } else {
                g.officer_say(side, "Life support is failing!".into(), ReportKind::Alert);
            }
        }
    }

    // shields power + regen (`sub_C883`)
    super::shields::shields_step(g, id);

    // cloak upkeep
    let (cloaked, cloak_cost) = {
        let o = g.obj(id);
        let s = o.ship.as_ref().unwrap();
        (s.cloak_capable && s.cloaked, g.data.ships[s.design].cloak_energy)
    };
    if cloaked {
        let o = g.obj_mut(id);
        if o.pool >= cloak_cost {
            o.pool -= cloak_cost;
        } else {
            o.ship.as_mut().unwrap().cloaked = false;
            g.officer_say(side, "Insufficient power to maintain the cloak.".into(), ReportKind::Crew);
        }
    }

    // tractor upkeep
    let (engaged, tractor_cost) = {
        let o = g.obj(id);
        let s = o.ship.as_ref().unwrap();
        (s.tractor_engaged, g.data.ships[s.design].tractor_energy)
    };
    if engaged {
        let o = g.obj_mut(id);
        if o.pool >= tractor_cost {
            o.pool -= tractor_cost;
        } else {
            let s = o.ship.as_mut().unwrap();
            s.tractor_engaged = false;
            s.tractor_target = None;
            g.officer_say(side, "Tractor beam released: insufficient power.".into(), ReportKind::Crew);
        }
    }
}

/// §5.4 warp temperature (`sub_F259`), per drive.
fn drive_temperature(g: &mut Game, id: ObjId, warp_ratio: f64) {
    let design_idx = g.obj(id).ship.as_ref().unwrap().design;
    let warp_eff = g.data.ships[design_idx].warp_efficiency;
    let side = g.obj(id).nation;
    let name = g.obj(id).name.clone();
    let mut destroyed_report = false;
    {
        let s = g.obj_mut(id).ship.as_mut().unwrap();
        for dr in s.drives.iter_mut() {
            if dr.sys.destroyed() {
                let t = (dr.temp - warp_eff * TEMP_RATE).max(0.0);
                dr.temp_delta = t - dr.temp;
                dr.temp = t;
                continue;
            }
            let delta = (warp_ratio - dr.sys.health() * warp_eff) * TEMP_RATE;
            let t = (dr.temp + delta).max(TEMP_FLOOR);
            dr.temp_delta = t - dr.temp;
            dr.temp = t;
            if dr.temp > TEMP_LIMIT {
                dr.sys.dmg = 100;
                destroyed_report = true;
            }
        }
    }
    if destroyed_report {
        g.say(
            Some(side),
            "",
            format!("{name}'s warp drive has overheated and been destroyed!"),
            ReportKind::Alert,
        );
    }
}

/// End-of-cycle battery recharge + residual power + tow position lock
/// (`sub_DE7C` 22201, `sub_F101` 24276).
pub fn battery_recharge(g: &mut Game) {
    for id in g.ship_ids() {
        let design_idx = g.obj(id).ship.as_ref().unwrap().design;
        let capacity = g.data.ships[design_idx].battery_capacity;
        let o = g.obj_mut(id);
        let mut pool = o.pool;
        let pool_before = pool;
        let s = o.ship.as_mut().unwrap();
        let mut old_total = 0.0;
        let mut new_total = 0.0;
        let mut cap_total = 0.0;
        for b in s.batteries.iter_mut() {
            if b.sys.dmg != 0 {
                continue; // damaged batteries are offline (`sub_F101` skips)
            }
            cap_total += capacity;
            old_total += b.charge;
            let fill = pool.clamp(0.0, capacity);
            new_total += fill;
            b.charge = fill;
            pool -= fill;
        }
        s.brain.battery_frac = if cap_total > 0.0 { new_total / cap_total } else { 0.0 };
        // RESIDUAL POWER (node+64h): generation minus drains this cycle
        o.residual = pool_before - old_total;
        o.pool = 0.0;

        // towed / docked objects glued to partner (`sub_DE7C+4E`)
        if let Some(partner) = o.ship.as_ref().unwrap().partner {
            let ppos = g.get(partner).map(|p| p.pos);
            if let Some(ppos) = ppos {
                let o = g.obj_mut(id);
                o.pos = ppos;
                o.vel = crate::math::Vec3::ZERO;
            }
        }
    }
}
