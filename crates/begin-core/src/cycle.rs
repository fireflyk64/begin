//! The simulation cycle (§3 `XXSimulate` 5374) and resolution pipeline
//! (§4 `detonation` 26446). Order is load-bearing: the AI's decisions
//! depend on it (counts computed before firing, pressure decay on hit).

use crate::constants::SUBSTEPS;
use crate::game::{EndState, Game};
use crate::object::*;

impl Game {
    /// Run one full cycle. Player/remote commands must already have been
    /// applied (they only set flags; resolution happens here).
    pub fn run_cycle(&mut self) {
        if self.over.is_some() {
            return;
        }
        self.cycle += 1;

        // environment: ephemeris positions, stations, ring rocks, hazards
        crate::env::update(self);

        // sensor contact update (§8) — before AI thinks
        crate::systems::sensors::sensor_update(self);

        // AI control dispatch (`sub_20D55`); humans already queued commands
        for id in self.ship_ids() {
            if self.get(id).map(|o| o.control == Control::Ai && !o.is_hulk()).unwrap_or(false) {
                crate::ai::ai_think(self, id);
            }
        }

        self.detonation_pipeline();

        // end-condition check (`sub_1044E` 26592)
        self.check_end();
    }

    /// §4 — the `detonation` pipeline. Every step in original order.
    fn detonation_pipeline(&mut self) {
        // 1. clear locks & pursue targets aimed at dying objects (`sub_C3B9`)
        self.clear_dead_locks();

        // 2. power, drives, life support, shields, cloak, tractor (`sub_C4F7`)
        for id in self.ship_ids() {
            crate::systems::power::power_step(self, id);
        }

        // 3. phaser charging + resolution (`sub_CA97` → `phaserDamage`)
        crate::systems::phasers::phaser_step(self);

        // 4. resolve detonation chain (`splash_starter`)
        crate::systems::damage::splash_starter(self);

        // 5. tube charging + fire → spawn torpedoes (`sub_CB5F` → `sub_E9B5`)
        crate::systems::torpedoes::tube_step(self);

        // 6. fire launchers → spawn probes (`sub_CC27` → `sub_ECDC`)
        crate::systems::probes::launcher_step(self);

        // 7. warp acceleration for every object (`sub_CCE2`)
        crate::systems::helm::warp_accel(self);

        // 8. turning + homing guidance + velocity vectors (`sub_CEB8`)
        crate::systems::helm::turn_and_guide(self);

        // 9. movement sub-steps interleaved with prox fuses; the original ran
        //    19×5 with ordnance and 20×5 without — we run 20 uniformly (§1)
        if self.any_ordnance() {
            for _ in 0..SUBSTEPS {
                crate::systems::helm::integrate_substep(self);
                crate::systems::torpedoes::prox_fuses(self);
                crate::systems::damage::splash_starter(self);
            }
        } else {
            for _ in 0..SUBSTEPS {
                crate::systems::helm::integrate_substep(self);
            }
        }

        // 10. fuse bookkeeping (`sub_D130`): arm counters, time fuses,
        //     self-destruct countdowns
        crate::systems::torpedoes::fuse_step(self);

        // 11. splash again; then damage control / repair (`sub_D72F`)
        crate::systems::damage::splash_starter(self);
        crate::systems::repair::repair_step(self);

        // 12. boarding combat (`sub_C010`)
        crate::systems::boarding::boarding_step(self);

        // 13. tractor auto-release + pull physics; battery recharge +
        //     residual power + tow position lock (`sub_102A7`, `sub_DE7C`)
        crate::systems::tractor::tractor_step(self);
        crate::systems::power::battery_recharge(self);

        // reset per-cycle hit counters (cloak reveal window)
        for id in self.ship_ids() {
            if let Some(o) = self.get_mut(id) {
                if let Some(s) = o.ship.as_mut() {
                    s.hits_this_cycle = 0;
                }
            }
        }
    }

    /// `sub_C3B9` 19191 — drop locks/pursues pointing at dying objects.
    fn clear_dead_locks(&mut self) {
        let dying: Vec<ObjId> = self
            .ids()
            .into_iter()
            .filter(|&i| self.obj(i).det != Det::None)
            .collect();
        if dying.is_empty() {
            return;
        }
        let is_dying = |id: Option<ObjId>| id.map(|i| dying.contains(&i)).unwrap_or(false);
        for id in self.ids() {
            let o = self.obj_mut(id);
            if is_dying(o.pursue) {
                o.pursue = None;
                if o.helm != HelmMode::Course {
                    o.helm = HelmMode::Course;
                    o.desired_course = o.course;
                }
            }
            if let Some(s) = o.ship.as_mut() {
                for b in s.banks.iter_mut() {
                    if is_dying(b.lock) {
                        b.lock = None;
                    }
                }
                for t in s.tubes.iter_mut() {
                    if is_dying(t.lock) {
                        t.lock = None;
                    }
                }
                for r in s.rails.iter_mut() {
                    if is_dying(r.lock) {
                        r.lock = None;
                    }
                }
                if is_dying(s.tractor_target) {
                    s.tractor_target = None;
                    s.tractor_engaged = false;
                }
                if is_dying(s.brain.target) {
                    s.brain.target = None;
                }
            }
        }
    }

    /// `sub_1044E` — the game ends when a side is eliminated (or on quit,
    /// handled by the front-end).
    fn check_end(&mut self) {
        let live = self.live_sides();
        if live.len() <= 1 {
            self.over = Some(EndState::Over { winner: live.first().copied() });
        }
    }

    /// Endgame rating (`sub_104BB` ≈26660): 7 tiers from the score.
    /// Returns (tier 0..6, message) for the given side.
    pub fn evaluation(&self, side: usize) -> (usize, String) {
        let enemy_start: f64 = (0..self.data.nations.len())
            .filter(|&n| n != side)
            .map(|n| self.start_strength[n])
            .sum();
        let ally_start = self.start_strength[side].max(1.0);
        let enemy_now: f64 = (0..self.data.nations.len())
            .filter(|&n| n != side)
            .map(|n| self.side_strength(n))
            .sum();
        let ally_now = self.side_strength(side);
        let ally_loss = ((ally_start - ally_now) / ally_start).clamp(0.0, 1.0);
        let enemy_loss = if enemy_start > 0.0 {
            ((enemy_start - enemy_now) / enemy_start).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let odds = (enemy_start / ally_start).max(0.05);
        // §13: (enemy_losses × odds − ally_losses / odds) scaled
        let score = (enemy_loss * odds - ally_loss / odds) * 10.0;
        let tier = crate::constants::ENDGAME_THRESHOLDS
            .iter()
            .position(|&t| score > t)
            .unwrap_or(crate::constants::ENDGAME_THRESHOLDS.len());
        let tier = tier.min(6);
        let n = &self.data.nations[side];
        let msg = n
            .endgame
            .get(tier)
            .cloned()
            .unwrap_or_else(|| "Simulation complete.".into());
        (tier, format!("{}{}", n.endgame_intro, msg))
    }
}
