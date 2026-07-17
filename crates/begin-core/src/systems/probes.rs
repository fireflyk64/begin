//! Probe launchers & in-flight probe control (§7.3; `sub_CC27` 20154,
//! `sub_ECDC` ≈23760). Probes are slow homing weapons, controllable after
//! launch, remotely detonatable; data probes extend the side's sensors.

use crate::constants::*;
use crate::events::ReportKind;
use crate::game::Game;
use crate::math::*;
use crate::object::*;

/// Load a launcher (player `load launchers` / AI). Probes cost no energy;
/// the launcher must be undamaged. Assigns the control code.
pub fn load_launcher(g: &mut Game, id: ObjId, launcher: usize, prox: f64, time: f64) -> bool {
    let design = g.obj(id).ship.as_ref().unwrap().design;
    let Some(probe_name) = g.data.ships[design].probe.clone() else { return false };
    let Some(probe_idx) = g.data.probes.iter().position(|p| p.name == probe_name) else {
        return false;
    };
    let pd = g.data.probes[probe_idx].clone();
    let code = {
        let a = (b'a' + g.rng.irange(0, 25).clamp(0, 25) as u8) as char;
        let b = (b'a' + g.rng.irange(0, 25).clamp(0, 25) as u8) as char;
        format!("{a}{b}{}", 100 + (g.cycle as usize + launcher * 7) % 900)
    };
    let s = g.obj_mut(id).ship.as_mut().unwrap();
    let l = &mut s.launchers[launcher];
    if l.sys.destroyed() || l.loaded.is_some() {
        return false;
    }
    l.loaded = Some(ProbeState {
        damage: pd.damage,
        hp: pd.damage.max(10.0),
        time: if time > 0.0 { time.min(pd.max_time_fuse) } else { pd.max_time_fuse },
        prox: if prox > 0.0 { prox.min(pd.max_prox) } else { pd.max_prox },
        arm: pd.arm_time,
        design: probe_idx,
        code: code.clone(),
        remote_detonate: false,
        deliberate_target: None,
    });
    l.code = code;
    true
}

/// `sub_CC27` — fire flagged launchers → spawn probes.
pub fn launcher_step(g: &mut Game) {
    for id in g.ship_ids() {
        let Some(o) = g.get(id) else { continue };
        let Some(ship) = o.ship.as_ref() else { continue };
        let firing: Vec<usize> = ship
            .launchers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.fire && !l.sys.destroyed() && l.loaded.is_some())
            .map(|(k, _)| k)
            .collect();
        if firing.is_empty() {
            continue;
        }
        let (my_pos, _my_course, my_mark, my_nation, my_name) =
            (o.pos, o.course, o.mark, o.nation, o.name.clone());
        let n_nations = g.data.nations.len();
        let mut launched = 0;
        // (code, target name or course) for the launch report
        let mut manifest: Vec<(String, String)> = Vec::new();
        for k in firing {
            let (mut state, at_target, course_order) = {
                let l = &mut g.obj_mut(id).ship.as_mut().unwrap().launchers[k];
                let st = l.loaded.take().unwrap();
                l.fire = false;
                (st, l.at_target, l.course)
            };
            let pd = g.data.probes[state.design].clone();
            let target = at_target.filter(|&t| g.get(t).is_some());
            state.deliberate_target = target.filter(|&t| g.obj(t).nation == my_nation);
            let (course, mark) = if let Some(t) = target {
                crate::systems::helm::target_bearing_mark(g, id, t, my_nation)
            } else {
                (norm360(course_order), my_mark)
            };
            manifest.push((
                state.code.clone(),
                match target {
                    Some(t) => format!("at the {}", g.obj(t).name),
                    None => format!("course {course:.0}"),
                },
            ));
            let name = pd.name.clone();
            let probe = Object {
                kind: Kind::Probe,
                name,
                nation: my_nation,
                ballistic: false,
                warp: pd.velocity,
                desired_warp: pd.velocity,
                course,
                desired_course: course,
                mark,
                desired_mark: mark,
                pos: my_pos,
                vel: dir(course, mark) * (pd.velocity * SUBSTEP_SCALE),
                warp_budget: 0.0,
                pool: 0.0,
                residual: 0.0,
                det: Det::None,
                helm: if target.is_some() { HelmMode::Pursue } else { HelmMode::Course },
                pursue: target,
                owner: Some(id),
                ship: None,
                torp: None,
                probe: Some(state),
                control: Control::None,
                contacts: vec![Contact::default(); n_nations],
                hull_integrity: 1.0,
            };
            if g.insert(probe).is_some() {
                launched += 1;
            }
        }
        if launched > 0 {
            let plural = if launched == 1 { "probe" } else { "probes" };
            let codes: Vec<&str> = manifest.iter().map(|(c, _)| c.as_str()).collect();
            // all launched the same way this order → one suffix
            let dest = manifest.first().map(|(_, d)| d.clone()).unwrap_or_default();
            g.say(
                None,
                "",
                format!("{my_name} launching {plural} \"{}\" {dest}!", codes.join("\", \"")),
                ReportKind::Info,
            );
        }
    }
}

/// Re-lock a launched probe on a new target (player `lock probe <code>`).
pub fn lock_probe(g: &mut Game, probe: ObjId, target: ObjId) {
    let o = g.obj_mut(probe);
    o.helm = HelmMode::Pursue;
    o.pursue = Some(target);
    let nation = o.nation;
    if let Some(p) = o.probe.as_mut() {
        p.deliberate_target = None;
    }
    let tgt_nation = g.obj(target).nation;
    if tgt_nation == nation {
        if let Some(p) = g.obj_mut(probe).probe.as_mut() {
            p.deliberate_target = Some(target);
        }
    }
}

/// Steer a launched probe to a manual course (player `turn probe <code>`).
pub fn turn_probe(g: &mut Game, probe: ObjId, course: f64, mark: f64) {
    let o = g.obj_mut(probe);
    o.helm = HelmMode::Course;
    o.pursue = None;
    o.desired_course = norm360(course);
    o.desired_mark = mark.clamp(-90.0, 90.0);
}

/// Remote-detonate (player `destruct probe <code>` / AI `sub_20AF8`).
pub fn detonate_probe(g: &mut Game, probe: ObjId) {
    let o = g.obj_mut(probe);
    if let Some(p) = o.probe.as_mut() {
        p.remote_detonate = true;
    }
    o.det = Det::Detonate;
}

/// Remote-detonate every active probe belonging to `owner` (begin2:
/// "Detonating all our active probes!"). Returns how many.
pub fn detonate_all(g: &mut Game, owner: ObjId) -> usize {
    let mine: Vec<ObjId> = g
        .probe_ids()
        .into_iter()
        .filter(|&p| g.obj(p).owner == Some(owner))
        .collect();
    for &p in &mine {
        detonate_probe(g, p);
    }
    mine.len()
}

/// Find a live probe owned by `owner` with the given control code.
pub fn probe_by_code(g: &Game, owner: ObjId, code: &str) -> Option<ObjId> {
    g.probe_ids().into_iter().find(|&p| {
        g.obj(p).owner == Some(owner)
            && g.obj(p)
                .probe
                .as_ref()
                .map(|st| st.code.eq_ignore_ascii_case(code))
                .unwrap_or(false)
    })
}
