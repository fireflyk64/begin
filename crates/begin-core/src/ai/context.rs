//! Per-cycle AI context (`sub_1E08A` 57274 + target cache `sub_1E42D` 57627).
//! The original cached these in globals; we build a struct per AI ship.

use crate::game::Game;
use crate::object::*;
use crate::systems::helm::{apparent_dist, target_bearing_mark};

pub struct Ctx {
    pub id: ObjId,
    pub side: usize,
    pub max_warp: f64,      // dbl_38692
    pub can_move: bool,     // word_38690
    pub op_banks: usize,    // word_3860C
    pub charged_banks: usize, // word_3860A
    pub op_tubes: usize,    // word_38608
    pub loaded_tubes: usize, // word_38606
    pub op_launchers: usize, // word_38604
    pub loaded_launchers: usize, // word_38602
    pub bank_range: f64,    // dbl_38626
    pub bank_charge_full: f64,
    pub torp_max_prox: f64, // dbl_385FA
    pub probe_velocity: f64, // dbl_385EA
    pub probe_max_range: f64, // dbl_3860E = (arm+max_time) × velocity × 100
    /// nearest enemy heading at me within ±10° (dword_38646 + dbl_3863E)
    pub nearest_incoming: Option<(ObjId, f64)>,
    // target cache (sub_1E42D)
    pub target: Option<TargetCtx>,
}

pub struct TargetCtx {
    pub id: ObjId,             // dword_3867C
    pub dist: f64,             // dbl_38674
    pub bearing: f64,          // dbl_3866C
    pub mark: f64,             // (3D)
    pub pressure: i32,         // word_3865A target's incoming-torp pressure
    pub max_warp: f64,         // dbl_38664
    pub banks: usize,          // word_3865E
    pub tubes: usize,          // word_38662
    pub torp_min_range: f64,   // dbl_3861E
    pub torp_max_range: f64,   // dbl_38616
    pub phaser_dominance: f64, // dbl_38636
    pub torp_dominance: f64,   // dbl_3862E
}

impl Ctx {
    pub fn build(g: &mut Game, id: ObjId) -> Ctx {
        let side = g.obj(id).nation;
        let (design_idx, brain_target) = {
            let s = g.obj(id).ship.as_ref().unwrap();
            (s.design, s.brain.target)
        };
        let d = g.data.ships[design_idx].clone();
        let torp_design = d.torp.as_deref().and_then(|n| g.data.torp(n)).cloned();
        let probe_design = d.probe.as_deref().and_then(|n| g.data.probe(n)).cloned();

        // strength components cached in the brain (sub_106F7)
        {
            let strength = g.strength_of(id);
            let s = g.obj(id).ship.as_ref().unwrap();
            let phaser_part = s.charged_banks(d.banks_charge) as f64 * d.banks_range / 1000.0;
            let torp_part = s.loaded_tubes() as f64 * 6.0;
            let b = &mut g.obj_mut(id).ship.as_mut().unwrap().brain;
            b.strength_phaser = phaser_part;
            b.strength_torp = torp_part;
            b.strength = strength;
            if b.strength_max < strength {
                b.strength_max = strength;
            }
        }

        let s = g.obj(id).ship.as_ref().unwrap();
        let max_warp = s.max_warp_attainable;
        let ctx_counts = (
            s.operational_banks(),
            s.charged_banks(d.banks_charge),
            s.operational_tubes(),
            s.loaded_tubes(),
            s.operational_launchers(),
            s.loaded_launchers(),
        );

        // nearest enemy pointed at me within ±10° (sub_1E08A tail)
        let my_pos = g.obj(id).pos;
        let mut nearest: Option<(ObjId, f64)> = None;
        for e in g.ship_ids() {
            let o = g.obj(e);
            if o.nation == side || o.is_hulk() {
                continue;
            }
            if g.fog && !o.contact(side).ever {
                continue;
            }
            let (b_to_me, _) = target_bearing_mark(g, e, id, side);
            if crate::math::ang_dist(b_to_me, o.course) > crate::constants::AI_APPROACH_CONE {
                continue;
            }
            let dist = (o.pos - my_pos).len();
            if nearest.map(|(_, nd)| dist < nd).unwrap_or(true) {
                nearest = Some((e, dist));
            }
        }

        let mut ctx = Ctx {
            id,
            side,
            max_warp,
            can_move: max_warp > 0.0,
            op_banks: ctx_counts.0,
            charged_banks: ctx_counts.1,
            op_tubes: ctx_counts.2,
            loaded_tubes: ctx_counts.3,
            op_launchers: ctx_counts.4,
            loaded_launchers: ctx_counts.5,
            bank_range: d.banks_range,
            bank_charge_full: d.banks_charge,
            torp_max_prox: torp_design.as_ref().map(|t| t.max_prox).unwrap_or(0.0),
            probe_velocity: probe_design.as_ref().map(|p| p.velocity).unwrap_or(0.0),
            probe_max_range: probe_design
                .as_ref()
                .map(|p| (p.arm_time + p.max_time_fuse) * p.velocity * 100.0)
                .unwrap_or(0.0),
            nearest_incoming: nearest,
            target: None,
        };
        if let Some(t) = brain_target.filter(|&t| g.get(t).is_some()) {
            ctx.cache_target(g, t);
        }
        ctx
    }

    /// `sub_1E42D` — target context cache.
    pub fn cache_target(&mut self, g: &Game, tid: ObjId) {
        let (dist, bearing, mark) = {
            let d = apparent_dist(g, self.id, tid, self.side);
            let (b, m) = target_bearing_mark(g, self.id, tid, self.side);
            (d, b, m)
        };
        let t = g.obj(tid);
        let ts = t.ship.as_ref();
        let td_design = ts.map(|s| &g.data.ships[s.design]);
        let (pressure, tmax_warp, tbanks, ttubes) = match (ts, td_design) {
            (Some(s), Some(_d)) => (
                s.brain.torp_pressure,
                s.max_warp_attainable,
                s.operational_banks(),
                s.operational_tubes(),
            ),
            _ => (0, t.warp, 0, 0),
        };
        // torp range vs this target (sub_1D7A3): closing speed based
        let my = g.obj(self.id);
        let my_design = &g.data.ships[my.ship.as_ref().unwrap().design];
        let (tmin, tmax) = if let Some(torp) = my_design.torp.as_deref().and_then(|n| g.data.torp(n))
        {
            let rel = crate::systems::helm::angle_off(g, self.id, tid, self.side);
            let closing = torp.velocity + t.warp * rel.to_radians().cos();
            let closing = closing.max(0.1);
            (
                torp.arm_time * closing * 100.0,
                (torp.arm_time + torp.max_time_fuse) * closing * 100.0,
            )
        } else {
            (0.0, 0.0)
        };
        // weapon dominance: my strength component / theirs (1000 if none)
        let my_brain = &my.ship.as_ref().unwrap().brain;
        let (their_phaser, their_torp) = ts
            .map(|s| (s.brain.strength_phaser, s.brain.strength_torp))
            .unwrap_or((0.0, 0.0));
        let phaser_dom =
            if their_phaser > 0.0 { my_brain.strength_phaser / their_phaser } else { 1000.0 };
        let torp_dom = if their_torp > 0.0 { my_brain.strength_torp / their_torp } else { 1000.0 };
        self.target = Some(TargetCtx {
            id: tid,
            dist,
            bearing,
            mark,
            pressure,
            max_warp: tmax_warp,
            banks: tbanks,
            tubes: ttubes,
            torp_min_range: tmin,
            torp_max_range: tmax,
            phaser_dominance: phaser_dom,
            torp_dominance: torp_dom,
        });
    }
}
