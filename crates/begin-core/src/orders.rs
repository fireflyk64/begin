//! Order executors: the flag-setting layer used by both the player command
//! parser and the AI (§7; `sub_AD74` 16059, `sub_BAD0` 17954, `sub_BC4D`
//! 18183, `sub_BD34` 18331, `sub_BB62` 18037, `sub_BBD3` 18107...).
//! Commands only set flags; the pipeline resolves them (fidelity rule 3).

use crate::constants::*;
use crate::game::Game;
use crate::math::norm360;
use crate::object::*;

/// Which mounts an order addresses: all, or a 1-based list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mounts {
    All,
    List(Vec<usize>),
}

impl Mounts {
    pub fn contains(&self, idx0: usize) -> bool {
        match self {
            Mounts::All => true,
            Mounts::List(v) => v.contains(&(idx0 + 1)),
        }
    }
}

// ---- phasers ----

/// `fire phasers <list> spread <s>` (`sub_AD74`).
pub fn fire_phasers(g: &mut Game, id: ObjId, which: &Mounts, spread: Option<f64>) -> usize {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    let mut n = 0;
    for (k, b) in s.banks.iter_mut().enumerate() {
        if !which.contains(k) || b.sys.destroyed() || !b.enabled || b.charge <= 0.0 {
            continue;
        }
        b.fire = true;
        if let Some(sp) = spread {
            b.spread = sp.clamp(SPREAD_MIN, SPREAD_DEFAULT);
        }
        n += 1;
    }
    n
}

/// AI helper: fire the first `n` fully-charged banks at `spread` (`sub_BAD0`).
pub fn fire_n_charged_banks(g: &mut Game, id: ObjId, n: usize, spread: f64) -> usize {
    let charge_full = {
        let s = g.obj(id).ship.as_ref().unwrap();
        g.data.ships[s.design].banks_charge
    };
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    let mut fired = 0;
    for b in s.banks.iter_mut() {
        if fired >= n {
            break;
        }
        if b.sys.destroyed() || !b.enabled || b.charge < charge_full {
            continue;
        }
        b.fire = true;
        b.spread = spread.clamp(SPREAD_MIN, SPREAD_DEFAULT);
        fired += 1;
    }
    fired
}

/// `lock banks <list> on <ship>` (`sub_BC4D`). Self-lock is forbidden.
pub fn lock_banks(g: &mut Game, id: ObjId, which: &Mounts, target: ObjId) -> usize {
    if target == id {
        return 0;
    }
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    let mut n = 0;
    for (k, b) in s.banks.iter_mut().enumerate() {
        if which.contains(k) && !b.sys.destroyed() {
            b.lock = Some(target);
            n += 1;
        }
    }
    n
}

/// `turn banks <list> <mark>` (`sub_AF38`): manual mark, drops the lock.
pub fn turn_banks(g: &mut Game, id: ObjId, which: &Mounts, mark: f64) {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    for (k, b) in s.banks.iter_mut().enumerate() {
        if which.contains(k) && !b.sys.destroyed() {
            b.lock = None;
            b.mark = norm360(mark);
        }
    }
}

/// `enable/disable banks <list>`.
pub fn enable_banks(g: &mut Game, id: ObjId, which: &Mounts, enabled: bool) {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    for (k, b) in s.banks.iter_mut().enumerate() {
        if which.contains(k) {
            b.enabled = enabled;
            if !enabled {
                b.fire = false;
            }
        }
    }
}

// ---- torpedo tubes ----

/// `fire torpedoes <list>` (`sub_BB62`).
pub fn fire_torpedoes(g: &mut Game, id: ObjId, which: &Mounts) -> usize {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    let mut n = 0;
    for (k, t) in s.tubes.iter_mut().enumerate() {
        if !which.contains(k) || t.sys.destroyed() || t.loaded.is_none() || t.charge < 100.0 {
            continue;
        }
        t.fire = true;
        n += 1;
    }
    n
}

/// `lock tubes <list> on <ship>` with an optional AI lead offset
/// (`sub_BCBA`/`sub_1F2C6`).
pub fn lock_tubes(g: &mut Game, id: ObjId, which: &Mounts, target: ObjId, lead: f64) -> usize {
    if target == id {
        return 0;
    }
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    let mut n = 0;
    for (k, t) in s.tubes.iter_mut().enumerate() {
        if which.contains(k) && !t.sys.destroyed() {
            t.lock = Some(target);
            t.lead_offset = lead;
            n += 1;
        }
    }
    n
}

/// `turn tubes <list> <mark>`: manual mark, drops the lock.
pub fn turn_tubes(g: &mut Game, id: ObjId, which: &Mounts, mark: f64) {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    for (k, t) in s.tubes.iter_mut().enumerate() {
        if which.contains(k) && !t.sys.destroyed() {
            t.lock = None;
            t.mark = norm360(mark);
        }
    }
}

/// `load tubes <list> prox <p>` (`sub_BD34`): enables loading and sets the
/// proximity for torpedoes loaded from now on.
pub fn load_tubes(g: &mut Game, id: ObjId, which: &Mounts, prox: Option<f64>) {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    for (k, t) in s.tubes.iter_mut().enumerate() {
        if which.contains(k) && !t.sys.destroyed() {
            t.loading_enabled = true;
            if let Some(p) = prox {
                t.prox = p;
                // a change of prox reloads the tube (AI housekeeping §12.5)
                if let Some(loaded) = t.loaded.as_mut() {
                    loaded.prox = p;
                }
            }
        }
    }
}

/// `unload tubes <list>`: removes the torpedo, allows a fresh load.
pub fn unload_tubes(g: &mut Game, id: ObjId, which: &Mounts) {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    for (k, t) in s.tubes.iter_mut().enumerate() {
        if which.contains(k) && t.loaded.take().is_some() {
            t.charge = 0.0;
            s.torps_left += 1;
        }
    }
}

// ---- probe launchers ----

/// `fire probes <list> at <ship>` / `... course <c>` (`sub_BBD3`).
pub fn fire_probes(
    g: &mut Game,
    id: ObjId,
    which: &Mounts,
    at: Option<ObjId>,
    course: Option<f64>,
) -> usize {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    let mut n = 0;
    for (k, l) in s.launchers.iter_mut().enumerate() {
        if !which.contains(k) || l.sys.destroyed() || l.loaded.is_none() {
            continue;
        }
        l.fire = true;
        l.at_target = at;
        l.course = course.map(norm360).unwrap_or(l.course);
        n += 1;
    }
    n
}

/// `load launchers <list> prox <p> time <t>`.
pub fn load_launchers(g: &mut Game, id: ObjId, which: &Mounts, prox: f64, time: f64) -> usize {
    let count = g.obj(id).ship.as_ref().unwrap().launchers.len();
    let mut n = 0;
    for k in 0..count {
        if which.contains(k) && crate::systems::probes::load_launcher(g, id, k, prox, time) {
            n += 1;
        }
    }
    n
}

/// `unload launchers <list>`.
pub fn unload_launchers(g: &mut Game, id: ObjId, which: &Mounts) {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    for (k, l) in s.launchers.iter_mut().enumerate() {
        if which.contains(k) && l.loaded.take().is_some() {
            s.probes_left += 1;
        }
    }
}

// ---- railguns (near-future) ----

pub fn fire_rails(g: &mut Game, id: ObjId, which: &Mounts) -> usize {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    let mut n = 0;
    for (k, r) in s.rails.iter_mut().enumerate() {
        if which.contains(k) && !r.sys.destroyed() && r.charge >= 100.0 {
            r.fire = true;
            n += 1;
        }
    }
    n
}

pub fn lock_rails(g: &mut Game, id: ObjId, which: &Mounts, target: ObjId) -> usize {
    if target == id {
        return 0;
    }
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    let mut n = 0;
    for (k, r) in s.rails.iter_mut().enumerate() {
        if which.contains(k) && !r.sys.destroyed() {
            r.lock = Some(target);
            n += 1;
        }
    }
    n
}

// ---- shields ----

/// `raise/lower shields <list>`.
pub fn set_shields(g: &mut Game, id: ObjId, which: &Mounts, up: bool) {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    for (k, sh) in s.shields.iter_mut().enumerate() {
        if which.contains(k) && !sh.sys.destroyed() {
            sh.state = if up { ShieldState::Up } else { ShieldState::Down };
            if !up {
                sh.effective = 0.0;
            }
        }
    }
}

/// `reenforce <#>` — one shield at quadruple power cost; drops any other
/// reinforcement (the AI reinforces the weakest damaged shield, §12.3).
pub fn reinforce_shield(g: &mut Game, id: ObjId, which: Option<usize>) {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    for (k, sh) in s.shields.iter_mut().enumerate() {
        if sh.sys.destroyed() {
            continue;
        }
        if Some(k) == which {
            sh.state = ShieldState::Reinforced;
        } else if sh.state == ShieldState::Reinforced {
            sh.state = ShieldState::Up;
        }
    }
}

// ---- misc ----

/// `cloak on/off`.
pub fn set_cloak(g: &mut Game, id: ObjId, on: bool) -> Result<(), String> {
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    if !s.cloak_capable {
        return Err("this ship has no cloaking device".into());
    }
    if s.cloak.destroyed() {
        return Err("the cloaking device is damaged".into());
    }
    s.cloaked = on;
    Ok(())
}

/// `destruct` — 5-cycle countdown (§10.4); costs destruct_energy.
pub fn self_destruct(g: &mut Game, id: ObjId) -> Result<(), String> {
    let cost = {
        let s = g.obj(id).ship.as_ref().unwrap();
        g.data.ships[s.design].destruct_energy
    };
    let o = g.obj_mut(id);
    if o.pool < cost {
        // the original charges the pool at order time
    }
    o.pool = (o.pool - cost).max(0.0);
    o.ship.as_mut().unwrap().destruct_countdown = DESTRUCT_COUNTDOWN;
    Ok(())
}

/// `abort destruct`.
pub fn abort_destruct(g: &mut Game, id: ObjId) {
    g.obj_mut(id).ship.as_mut().unwrap().destruct_countdown = -1.0;
}

/// `helm course <c> warp <w>` (+ 3D mark via `320^22` syntax).
pub fn helm(g: &mut Game, id: ObjId, course: Option<f64>, mark: Option<f64>, warp: Option<f64>) {
    let o = g.obj_mut(id);
    o.helm = HelmMode::Course;
    o.pursue = None;
    if let Some(c) = course {
        o.desired_course = norm360(c);
    }
    if let Some(m) = mark {
        o.desired_mark = m.clamp(-90.0, 90.0);
    }
    if let Some(w) = warp {
        o.desired_warp = w.clamp(MIN_WARP, 20.0);
    }
}

/// `pursue <ship> [warp]` / `elude <ship> [warp]`.
pub fn pursue(g: &mut Game, id: ObjId, target: ObjId, warp: Option<f64>, elude: bool) {
    let o = g.obj_mut(id);
    o.helm = if elude { HelmMode::Elude } else { HelmMode::Pursue };
    o.pursue = Some(target);
    if let Some(w) = warp {
        o.desired_warp = w.clamp(MIN_WARP, 20.0);
    }
}
