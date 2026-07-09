//! Damage application and splash resolution (§9 `dealDamageToHull` 25192,
//! §10 `splashDamage` 22271 / `splash_starter` 21127).

use crate::constants::*;
use crate::events::ReportKind;
use crate::game::Game;
use crate::math::bearing_of;
use crate::object::*;

/// Warhead / damage types (§9: 4 = phaser, 1 = SHIELD_BORE, 2 = plasma).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageType {
    Phaser,     // 4
    Antimatter, // 0 (also railgun slugs & kinetic rounds)
    ShieldBore, // 1 (Orion Auger: x4 vs shields, never penetrates)
    Plasma,     // 2
}

impl DamageType {
    pub fn from_warhead(w: u8) -> DamageType {
        match w {
            1 => DamageType::ShieldBore,
            2 => DamageType::Plasma,
            _ => DamageType::Antimatter,
        }
    }
}

/// §9 `dealDamageToHull(target, face_angle, damage, type)`.
/// `face_angle` is the relative bearing the hit arrives from.
pub fn deal_damage(g: &mut Game, target: ObjId, face_angle: f64, damage: f64, dtype: DamageType) {
    if g.get(target).is_none() || damage <= 0.0 {
        return;
    }
    let damage = (damage.round()).min(DAMAGE_CAP);
    match g.obj(target).kind {
        Kind::Ship => deal_damage_ship(g, target, face_angle, damage, dtype),
        Kind::Torp => {
            // only decay-type (plasma) torpedoes are damageable — phaser
            // point defense burns them down; strength doubles as the plasma
            // remaining-strength field
            let is_plasma =
                crate::systems::torpedoes::is_decay_type(g, g.obj(target).torp.as_ref().unwrap().design);
            if is_plasma {
                let o = g.obj_mut(target);
                let t = o.torp.as_mut().unwrap();
                t.strength -= damage;
                if t.strength <= 0.0 {
                    o.det = Det::Expire; // fizzle, no blast
                }
            }
        }
        Kind::Probe => {
            let o = g.obj_mut(target);
            let p = o.probe.as_mut().unwrap();
            p.hp -= damage;
            if p.hp <= 0.0 {
                o.det = Det::Expire; // destroyed before detonation
            }
        }
    }
}

fn deal_damage_ship(g: &mut Game, target: ObjId, face_angle: f64, damage: f64, dtype: DamageType) {
    let design_idx = g.obj(target).ship.as_ref().unwrap().design;
    let (shield_strength_eu, shield_absorption) = {
        let d = &g.data.ships[design_idx];
        (d.shield_strength, d.shield_absorption)
    };

    // 1. cloak reveal + torp pressure decay (brain+8Ah -= 5)
    {
        let s = g.obj_mut(target).ship.as_mut().unwrap();
        s.hits_this_cycle += 1;
        s.brain.torp_pressure = (s.brain.torp_pressure - 5).max(0);
    }

    // 2-3. facing shield absorbs
    let mut damage = damage;
    let shield_idx = {
        let s = g.obj(target).ship.as_ref().unwrap();
        crate::systems::shields::facing_shield(s, face_angle)
    };
    if let Some(si) = shield_idx {
        let s = g.obj_mut(target).ship.as_mut().unwrap();
        let sh = &mut s.shields[si];
        let capacity = shield_strength_eu * sh.effective / 100.0 * SHIELD_ABSORB_QUARTER;
        let incoming = if dtype == DamageType::ShieldBore { damage * 4.0 } else { damage };
        let absorbed = incoming.min(capacity);
        let mut loss = 1.0 + absorbed * (1.0 - shield_absorption * SHIELD_ABSORB_QUARTER);
        if sh.state == ShieldState::Reinforced {
            loss *= 0.5;
        }
        sh.strength = (sh.strength - loss).max(0.0);
        sh.effective = sh.effective.min(sh.strength);
        sh.hits += 1;
        if dtype == DamageType::ShieldBore {
            return; // the Auger never penetrates
        }
        damage -= absorbed;
        if damage <= 0.0 {
            return;
        }
    }

    // 4. hull: armour first, then integrity check, crew casualties
    {
        let hull_integrity = g.obj(target).hull_integrity;
        let s = g.obj_mut(target).ship.as_mut().unwrap();
        if s.armour > 0.0 {
            let soaked = s.armour.min(damage);
            s.armour -= soaked;
            damage -= soaked;
            if damage <= 0.0 {
                return;
            }
        }
        s.hull_hits += 1;
        if damage >= hull_integrity {
            g.obj_mut(target).det = Det::Destroyed;
            return;
        }
    }
    let dmg_i = damage as i32;
    let cas = g.rng.irange(0, dmg_i) + g.rng.irange(0, dmg_i);
    let boarder_cas = g.rng.irange(0, dmg_i);
    let name = g.obj(target).name.clone();
    let survivors = {
        let s = g.obj_mut(target).ship.as_mut().unwrap();
        s.survivors = (s.survivors - cas).max(0);
        s.boarders = (s.boarders - boarder_cas).max(0);
        s.survivors
    };
    if survivors > 0 && survivors < 6 {
        let s = g.obj_mut(target).ship.as_mut().unwrap();
        s.survivors = 0;
        g.say(None, "", format!("All hands aboard the {name} have been lost."), ReportKind::Alert);
    }

    // docked ships recursively take the hit if dmg >= 10
    if damage >= 10.0 {
        let docked: Vec<ObjId> = g.obj(target).ship.as_ref().unwrap().docked_ships.clone();
        for d in docked {
            deal_damage(g, d, face_angle, damage, dtype);
        }
    }

    // 5. system damage: n = dmg/10 rolls, independent per-class chances
    let n = (damage / 10.0) as i32;
    system_damage(g, target, n);
}

#[derive(Clone, Copy)]
enum Class {
    Shields,
    Tubes,
    Banks,
    Rails,
    Drives,
    Reactors,
    Launchers,
    Batteries,
    Transporters,
    Scanner,
    Cloak,
    Impulse,
    Tractor,
}

/// §9.5 system damage rolls (`sub_F820` 25155): shields 75%, tubes 50%,
/// banks 45%, drives 40%, reactors 60% (25% outright, else double),
/// launchers 20%, batteries 15%, transporters 10% (double), scanner 10%,
/// cloak 10% (+decloak), impulse 5%, tractor 5% (+release).
fn system_damage(g: &mut Game, target: ObjId, n: i32) {
    if n <= 0 {
        return;
    }
    const CLASSES: [(Class, f64); 13] = [
        (Class::Shields, 75.0),
        (Class::Tubes, 50.0),
        (Class::Banks, 45.0),
        (Class::Rails, 45.0),
        (Class::Drives, 40.0),
        (Class::Reactors, 60.0),
        (Class::Launchers, 20.0),
        (Class::Batteries, 15.0),
        (Class::Transporters, 10.0),
        (Class::Scanner, 10.0),
        (Class::Cloak, 10.0),
        (Class::Impulse, 5.0),
        (Class::Tractor, 5.0),
    ];
    let side = g.obj(target).nation;
    let name = g.obj(target).name.clone();
    let mut reports: Vec<String> = Vec::new();
    for _ in 0..n {
        for &(class, pct) in CLASSES.iter() {
            if !g.rng.percent(pct) {
                continue;
            }
            let amount = 5 + g.rng.irange(0, 25);
            let count = {
                let s = g.obj(target).ship.as_ref().unwrap();
                match class {
                    Class::Shields => s.shields.len(),
                    Class::Tubes => s.tubes.len(),
                    Class::Banks => s.banks.len(),
                    Class::Rails => s.rails.len(),
                    Class::Drives => s.drives.len(),
                    Class::Reactors => s.reactors.len(),
                    Class::Launchers => s.launchers.len(),
                    Class::Batteries => s.batteries.len(),
                    Class::Transporters => s.transporters.len(),
                    Class::Scanner => 1,
                    Class::Cloak => usize::from(s.cloak_capable),
                    Class::Impulse => usize::from(s.impulse.is_some()),
                    Class::Tractor => usize::from(s.tractor.is_some()),
                }
            };
            if count == 0 {
                continue;
            }
            let k = g.rng.irange(0, count as i32 - 1).clamp(0, count as i32 - 1) as usize;
            let outright = g.rng.percent(25.0); // used by reactors only
            let s = g.obj_mut(target).ship.as_mut().unwrap();
            match class {
                Class::Shields => {
                    let sh = &mut s.shields[k];
                    let was = sh.sys.destroyed();
                    sh.sys.dmg = (sh.sys.dmg + amount).min(100);
                    sh.strength = sh.strength.min((100 - sh.sys.dmg) as f64);
                    sh.effective = sh.effective.min(sh.strength);
                    if sh.sys.destroyed() && !was {
                        reports.push(format!("{name}'s shield {} has been destroyed.", k + 1));
                    }
                }
                Class::Tubes => {
                    let t = &mut s.tubes[k];
                    let was = t.sys.destroyed();
                    t.sys.dmg = (t.sys.dmg + amount).min(100);
                    // a damaged tube loses charge, lock, and its torpedo
                    t.charge = 0.0;
                    t.lock = None;
                    t.loaded = None;
                    t.fire = false;
                    if t.sys.destroyed() && !was {
                        reports.push(format!("{name}'s torpedo tube {} has been destroyed.", k + 1));
                    }
                }
                Class::Banks => {
                    let b = &mut s.banks[k];
                    let was = b.sys.destroyed();
                    b.sys.dmg = (b.sys.dmg + amount).min(100);
                    b.charge = 0.0;
                    b.lock = None;
                    b.fire = false;
                    if b.sys.destroyed() && !was {
                        reports.push(format!("{name}'s phaser bank {} has been destroyed.", k + 1));
                    }
                }
                Class::Rails => {
                    let r = &mut s.rails[k];
                    r.sys.dmg = (r.sys.dmg + amount).min(100);
                    r.charge = 0.0;
                    r.fire = false;
                }
                Class::Drives => {
                    let dr = &mut s.drives[k];
                    let was = dr.sys.destroyed();
                    dr.sys.dmg = (dr.sys.dmg + amount).min(100);
                    if dr.sys.destroyed() && !was {
                        reports.push(format!("{name}'s warp drive has been destroyed!"));
                    }
                }
                Class::Reactors => {
                    let r = &mut s.reactors[k];
                    let was = r.destroyed();
                    if outright {
                        r.dmg = 100;
                    } else {
                        r.dmg = (r.dmg + amount * 2).min(100);
                    }
                    if r.destroyed() && !was {
                        reports.push(format!("A reactor aboard the {name} has been destroyed!"));
                    }
                }
                Class::Launchers => {
                    let l = &mut s.launchers[k];
                    l.sys.dmg = (l.sys.dmg + amount).min(100);
                    if l.sys.destroyed() {
                        l.loaded = None;
                        l.fire = false;
                    }
                }
                Class::Batteries => {
                    s.batteries[k].sys.dmg = (s.batteries[k].sys.dmg + amount).min(100);
                }
                Class::Transporters => {
                    s.transporters[k].dmg = (s.transporters[k].dmg + amount * 2).min(100);
                }
                Class::Scanner => {
                    s.scanner.dmg = (s.scanner.dmg + amount).min(100);
                    if s.scanner.destroyed() {
                        reports.push(format!("{name}'s sensors are out!"));
                    }
                }
                Class::Cloak => {
                    s.cloak.dmg = (s.cloak.dmg + amount).min(100);
                    s.cloaked = false;
                }
                Class::Impulse => {
                    if let Some(imp) = s.impulse.as_mut() {
                        imp.dmg = (imp.dmg + amount).min(100);
                    }
                }
                Class::Tractor => {
                    if let Some(tr) = s.tractor.as_mut() {
                        tr.dmg = (tr.dmg + amount).min(100);
                        s.tractor_engaged = false;
                        s.tractor_target = None;
                    }
                }
            }
        }
    }
    for text in reports {
        g.say(Some(side), "", text, ReportKind::Alert);
    }
}

/// §10 `splash_starter` — resolve the detonation chain until no object is
/// dying, then batch hit reports.
pub fn splash_starter(g: &mut Game) {
    loop {
        let Some(id) = g.ids().into_iter().find(|&i| g.obj(i).det != Det::None) else {
            break;
        };
        let det = g.obj(id).det;
        if det != Det::Expire {
            splash_damage(g, id);
        }
        if g.obj(id).kind == Kind::Ship {
            let name = g.obj(id).name.clone();
            let text = if det == Det::Detonate {
                format!("The {name} has self destructed!")
            } else {
                format!("The {name} has been destroyed!")
            };
            g.say(None, "", text, ReportKind::Alert);
        }
        g.remove(id);
    }
    flush_hit_reports(g);
}

/// §10.2-10.3 `splashDamage` — blast yield and falloff.
fn splash_damage(g: &mut Game, id: ObjId) {
    let (yield_base, wtype, salvo, pos) = {
        let o = g.obj(id);
        match o.kind {
            Kind::Ship => {
                // yield = warp-power basis / ooDestructDamage (+ design
                // destruct rating on deliberate destruct); `splashDamage+CA`
                // calls sub_F07A — the drive-based gross power
                let s = o.ship.as_ref().unwrap();
                let d = &g.data.ships[s.design];
                let gross = crate::systems::power::gross_warp_power(g, id);
                let mut y = gross / g.tuning.oo_destruct_damage;
                if o.det == Det::Detonate {
                    y += d.destruct;
                }
                (y, DamageType::Antimatter, 1, o.pos)
            }
            Kind::Torp => {
                let t = o.torp.as_ref().unwrap();
                let d = &g.data.torps[t.design];
                if d.kinetic {
                    return; // kinetic rounds have no blast (contact damage)
                }
                let mut y = if t.arm <= 0.0 { t.damage } else { 0.0 };
                if d.warhead_type == 2 && d.max_time_fuse > 0.0 {
                    // plasma decays with remaining time fuse
                    y *= (t.strength / d.max_time_fuse).clamp(0.0, 1.0);
                }
                (y, DamageType::from_warhead(d.warhead_type), t.salvo.max(1), o.pos)
            }
            Kind::Probe => {
                let p = o.probe.as_ref().unwrap();
                let armed = p.arm <= 0.0 || p.remote_detonate;
                (if armed { p.damage } else { 0.0 }, DamageType::Antimatter, 1, o.pos)
            }
        }
    };
    if yield_base <= 0.0 {
        return;
    }

    // recursively detonate docked ships
    if g.obj(id).kind == Kind::Ship {
        let docked: Vec<ObjId> = g.obj(id).ship.as_ref().unwrap().docked_ships.clone();
        for d in docked {
            if let Some(o) = g.get_mut(d) {
                if o.det == Det::None {
                    o.det = Det::Destroyed;
                }
            }
        }
    }

    let total = yield_base * g.tuning.splash_dam_mult;
    let radius = total * SPLASH_RADIUS_PER_DAMAGE;
    for other in g.ids() {
        if other == id {
            continue;
        }
        let o = g.obj(other);
        if o.det != Det::None {
            continue;
        }
        if let Some(s) = o.ship.as_ref() {
            if s.cloaked {
                continue; // cloaked ships are untouched by splash
            }
        }
        let delta = o.pos - pos;
        let dist = delta.len();
        if dist >= radius {
            continue;
        }
        let dmg = total * (1.0 - dist / radius).sqrt();
        // face = bearing from the target toward the blast
        let back = pos - o.pos;
        let face = crate::math::norm360(bearing_of(back.x, back.y) - o.course);
        for _ in 0..salvo {
            deal_damage(g, other, face, dmg, wtype);
        }
    }
}

/// Batched "shield N hit" / hull reports after each resolution wave.
fn flush_hit_reports(g: &mut Game) {
    for id in g.ship_ids() {
        let name = g.obj(id).name.clone();
        let mut lines: Vec<String> = Vec::new();
        {
            let s = g.obj_mut(id).ship.as_mut().unwrap();
            for (k, sh) in s.shields.iter_mut().enumerate() {
                if sh.hits > 0 {
                    lines.push(format!("{name}'s shield {} hit.", k + 1));
                    sh.hits = 0;
                }
            }
            if s.hull_hits > 0 {
                lines.push(format!("{name} has sustained hull damage!"));
                s.hull_hits = 0;
            }
        }
        for text in lines {
            g.say(None, "", text, ReportKind::Alert);
        }
    }
}
