//! Damage control (§11; `sub_D72F` 21416, kernel `sub_F6AE` 24949).

use crate::events::ReportKind;
use crate::game::Game;
use crate::object::{ObjId, RepairClass, Sys};

/// Per ship per cycle: efficiency = docked ? host.docked_repair_ratio :
/// survivors / design crew; skip below 0.1. Priority class ×4, others ×0.5.
/// Destroyed (100%) items repairable only while docked, behind a 2%/cycle
/// gate. progress += rand(0, 2 × class_rate × e); integer part heals dmg%.
pub fn repair_step(g: &mut Game) {
    for id in g.ship_ids() {
        repair_ship(g, id);
    }
}

fn class_mult(priority: Option<RepairClass>, class: RepairClass) -> f64 {
    match priority {
        None => 1.0,
        Some(p) if p == class => 4.0,
        Some(_) => 0.5,
    }
}

/// Repair kernel (`sub_F6AE`). Returns true when the item just completed.
fn fix_item(sys: &mut Sys, rate: f64, docked: bool, prog_roll: f64, gate: bool) -> bool {
    if sys.dmg == 0 {
        return false;
    }
    // destroyed items are only repairable while docked, behind a 2% gate
    if sys.dmg >= 100 && !(docked && gate) {
        return false;
    }
    sys.progress += prog_roll * rate;
    let whole = sys.progress as i32;
    if whole <= 0 {
        return false;
    }
    sys.progress -= whole as f64;
    sys.dmg = (sys.dmg - whole).max(0);
    if sys.dmg == 0 {
        sys.progress = 0.0;
        true
    } else {
        false
    }
}

fn repair_ship(g: &mut Game, id: ObjId) {
    let (design_idx, docked_to, survivors) = {
        let s = g.obj(id).ship.as_ref().unwrap();
        (s.design, s.docked_to, s.survivors)
    };
    let d = g.data.ships[design_idx].clone();
    let e = if let Some(host) = docked_to.filter(|&h| g.get(h).is_some()) {
        let host_design = g.obj(host).ship.as_ref().unwrap().design;
        g.data.ships[host_design].docked_repair_ratio
    } else {
        survivors as f64 / d.crew.max(1) as f64
    };
    if e < 0.1 {
        return;
    }
    let docked = docked_to.is_some();
    let priority = g.obj(id).ship.as_ref().unwrap().repair_priority;
    let name = g.obj(id).name.clone();
    let side = g.obj(id).nation;

    // pre-roll enough randomness so the ship borrow below stays clean
    let n_items = {
        let s = g.obj(id).ship.as_ref().unwrap();
        s.reactors.len()
            + s.batteries.len()
            + s.banks.len()
            + s.tubes.len()
            + s.launchers.len()
            + s.rails.len()
            + s.drives.len()
            + s.shields.len()
            + s.transporters.len()
            + 4
    };
    let rolls: Vec<(f64, bool)> =
        (0..n_items).map(|_| (g.rng.range(0.0, 2.0), g.rng.percent(2.0))).collect();
    let mut completed: Vec<String> = Vec::new();

    {
        let s = g.obj_mut(id).ship.as_mut().unwrap();
        let mut i = 0usize;
        let mut next = move || {
            let r = rolls[i % rolls.len()];
            i += 1;
            r
        };
        let run = |sys: &mut Sys, class: RepairClass, rate: f64, label: String,
                       next: &mut dyn FnMut() -> (f64, bool),
                       completed: &mut Vec<String>| {
            let (roll, gate) = next();
            if fix_item(sys, rate * class_mult(priority, class) * e, docked, roll, gate) {
                completed.push(label);
            }
        };
        for k in 0..s.reactors.len() {
            run(&mut s.reactors[k], RepairClass::Reactors, d.reactor_repair,
                format!("reactor {}", k + 1), &mut next, &mut completed);
        }
        for k in 0..s.batteries.len() {
            run(&mut s.batteries[k].sys, RepairClass::Batteries, d.battery_repair,
                format!("battery {}", k + 1), &mut next, &mut completed);
        }
        for k in 0..s.banks.len() {
            run(&mut s.banks[k].sys, RepairClass::Banks, d.banks_repair,
                format!("phaser bank {}", k + 1), &mut next, &mut completed);
        }
        for k in 0..s.tubes.len() {
            run(&mut s.tubes[k].sys, RepairClass::Tubes, d.tube_repair,
                format!("torpedo tube {}", k + 1), &mut next, &mut completed);
        }
        for k in 0..s.launchers.len() {
            run(&mut s.launchers[k].sys, RepairClass::Launchers, d.probe_repair,
                format!("launcher {}", k + 1), &mut next, &mut completed);
        }
        for k in 0..s.rails.len() {
            run(&mut s.rails[k].sys, RepairClass::Banks, d.banks_repair,
                format!("railgun {}", k + 1), &mut next, &mut completed);
        }
        for k in 0..s.drives.len() {
            run(&mut s.drives[k].sys, RepairClass::Drives, d.drive_repair,
                format!("warp drive {}", k + 1), &mut next, &mut completed);
        }
        for k in 0..s.shields.len() {
            run(&mut s.shields[k].sys, RepairClass::Shields, d.shield_repair,
                format!("shield {}", k + 1), &mut next, &mut completed);
        }
        for k in 0..s.transporters.len() {
            run(&mut s.transporters[k], RepairClass::Transporter, d.transporter_repair,
                format!("transporter {}", k + 1), &mut next, &mut completed);
        }
        run(&mut s.scanner, RepairClass::Scanner, d.scanner_repair,
            "sensors".to_string(), &mut next, &mut completed);
        if s.cloak_capable {
            run(&mut s.cloak, RepairClass::Cloak, d.cloak_repair,
                "cloaking device".to_string(), &mut next, &mut completed);
        }
        if let Some(imp) = s.impulse.as_mut() {
            run(imp, RepairClass::Impulse, d.impulse_repair,
                "impulse engine".to_string(), &mut next, &mut completed);
        }
        if let Some(tr) = s.tractor.as_mut() {
            run(tr, RepairClass::Tractor, d.tractor_repair,
                "tractor beam".to_string(), &mut next, &mut completed);
        }
    }

    // "repairs completed" reports from a named engineer (`sub_F781` 25053)
    for what in completed {
        let engineer = {
            let names = &g.data.ships[design_idx].crew_names;
            if names.is_empty() {
                "Engineering".to_string()
            } else {
                names[g.rng.irange(0, names.len() as i32 - 1).clamp(0, names.len() as i32 - 1)
                    as usize]
                    .clone()
            }
        };
        g.say(
            Some(side),
            &engineer,
            format!("Repairs completed on the {name}'s {what}."),
            ReportKind::Crew,
        );
    }
}
