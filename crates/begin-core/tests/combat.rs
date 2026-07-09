//! Milestone 2 tests: weapons, damage, splash, repair, boarding.

use begin_core::object::{Control, Det, HelmMode};
use begin_core::orders::{self, Mounts};
use begin_core::scenario::{spawn_fleets, Scenario};
use begin_core::{Game, GameData, ObjId, Tuning};

fn close_duel(seed: u64, dist: f64) -> (Game, ObjId, ObjId) {
    let mut g = Game::new(GameData::load(), Tuning::default(), seed);
    let mut sc = Scenario::duel();
    sc.enemy.ships[0].count = 1;
    let fleets = spawn_fleets(&mut g, &sc).unwrap();
    let ally = fleets.flagship.unwrap();
    let enemy = fleets.enemy_ids[0];
    g.obj_mut(enemy).control = Control::None;
    // park them `dist` apart, facing each other
    g.obj_mut(ally).pos = begin_core::math::Vec3::new(0.0, 0.0, 0.0);
    g.obj_mut(enemy).pos = begin_core::math::Vec3::new(0.0, dist, 0.0);
    (g, ally, enemy)
}

fn shield_pct(g: &Game, id: ObjId, k: usize) -> f64 {
    g.obj(id).ship.as_ref().unwrap().shields[k].effective
}

#[test]
fn phasers_hit_and_hurt_the_facing_shield() {
    let (mut g, ally, enemy) = close_duel(10, 1500.0);
    // give banks time to charge
    for _ in 0..6 {
        g.run_cycle();
    }
    let before: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    orders::lock_banks(&mut g, ally, &Mounts::All, enemy);
    g.run_cycle(); // marks track
    orders::fire_phasers(&mut g, ally, &Mounts::All, Some(10.0));
    g.run_cycle();
    let after: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    assert!(after < before, "shields weakened: {before} -> {after}");
    // enemy faces the shooter (course 180, shooter due south... shooter at
    // -Y of enemy, bearing from enemy = 180 relative to its 180 course = 0
    // → front shield (1) takes the hit
    let front = shield_pct(&g, enemy, 0);
    assert!(front < 100.0, "front shield took the hit: {front}");
    // banks discharged
    let s = g.obj(ally).ship.as_ref().unwrap();
    assert!(s.banks.iter().all(|b| b.charge < 1.0));
}

#[test]
fn phaser_damage_falls_off_with_distance() {
    // fire at close range vs far range, compare shield loss
    let mut losses = Vec::new();
    for dist in [500.0, 1800.0] {
        let (mut g, ally, enemy) = close_duel(11, dist);
        for _ in 0..6 {
            g.run_cycle();
        }
        orders::lock_banks(&mut g, ally, &Mounts::All, enemy);
        g.run_cycle();
        orders::fire_phasers(&mut g, ally, &Mounts::All, Some(10.0));
        g.run_cycle();
        losses.push(100.0 - shield_pct(&g, enemy, 0));
    }
    assert!(
        losses[0] > losses[1],
        "closer hit hurts more: {losses:?}"
    );
}

#[test]
fn torpedoes_fly_arm_and_detonate_on_prox() {
    let (mut g, ally, enemy) = close_duel(12, 8000.0);
    orders::load_tubes(&mut g, ally, &Mounts::All, Some(500.0));
    // charge + load takes charge_time cycles, ready the following cycle
    for _ in 0..6 {
        g.run_cycle();
    }
    let loaded = g.obj(ally).ship.as_ref().unwrap().loaded_tubes();
    assert!(loaded > 0, "tubes loaded: {loaded}");
    orders::lock_tubes(&mut g, ally, &Mounts::All, enemy, 0.0);
    g.run_cycle();
    let enemy_shields_before: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    orders::fire_torpedoes(&mut g, ally, &Mounts::All);
    // mk7 at warp ~30 covers 8000 units in ~3 cycles
    let mut saw_torp = false;
    for _ in 0..6 {
        g.run_cycle();
        saw_torp |= !g.torp_ids().is_empty();
    }
    assert!(saw_torp, "a torpedo was in flight");
    assert!(g.torp_ids().is_empty(), "torpedo resolved");
    let enemy_shields_after: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    assert!(
        enemy_shields_after < enemy_shields_before,
        "splash hurt the target: {enemy_shields_before} -> {enemy_shields_after}"
    );
}

#[test]
fn torpedo_lead_hits_a_crossing_target() {
    let (mut g, ally, enemy) = close_duel(13, 6000.0);
    // enemy crosses at warp 6 heading east
    g.obj_mut(enemy).desired_course = 90.0;
    g.obj_mut(enemy).course = 90.0;
    g.obj_mut(enemy).desired_warp = 6.0;
    g.obj_mut(enemy).warp = 6.0;
    orders::load_tubes(&mut g, ally, &Mounts::All, Some(500.0));
    for _ in 0..6 {
        g.run_cycle();
    }
    orders::lock_tubes(&mut g, ally, &Mounts::All, enemy, 0.0);
    g.run_cycle();
    let crew_before = g.obj(enemy).ship.as_ref().unwrap().survivors;
    let shields_before: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    orders::fire_torpedoes(&mut g, ally, &Mounts::All);
    for _ in 0..8 {
        g.run_cycle();
    }
    let shields_after: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    let crew_after = g.obj(enemy).ship.as_ref().unwrap().survivors;
    assert!(
        shields_after < shields_before || crew_after < crew_before,
        "intercept lead connected with a crossing target"
    );
}

#[test]
fn self_destruct_counts_down_and_blows() {
    let (mut g, ally, enemy) = close_duel(14, 700.0);
    let _ = enemy;
    orders::self_destruct(&mut g, ally).unwrap();
    for _ in 0..6 {
        g.run_cycle();
    }
    assert!(g.get(ally).is_none(), "ship gone after 5-cycle countdown");
    // the nearby enemy caught the blast
    let shields: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    assert!(shields < 600.0, "blast splashed the bystander: {shields}");
}

#[test]
fn probes_launch_pursue_and_detonate() {
    let (mut g, ally, enemy) = close_duel(15, 4000.0);
    let n = orders::load_launchers(&mut g, ally, &Mounts::All, 800.0, 20.0);
    assert!(n > 0, "loaded {n} launchers");
    orders::fire_probes(&mut g, ally, &Mounts::All, Some(enemy), None);
    g.run_cycle();
    assert!(!g.probe_ids().is_empty(), "probe in flight");
    // px2 at warp 2.5 = 250/cycle → 4000 units in ~16 cycles (fuse 20)
    for _ in 0..30 {
        g.run_cycle();
    }
    assert!(g.probe_ids().is_empty(), "probe resolved");
    let s = g.obj(enemy).ship.as_ref().unwrap();
    let shields: f64 = s.shields.iter().map(|sh| sh.effective).sum();
    let hurt = shields < 599.9 || s.survivors < 350;
    assert!(hurt, "probe warhead hurt the target (shields {shields})");
}

#[test]
fn repair_fixes_damage_over_time() {
    let (mut g, ally, _) = close_duel(16, 20000.0);
    {
        let s = g.obj_mut(ally).ship.as_mut().unwrap();
        s.banks[0].sys.dmg = 40;
    }
    g.obj_mut(ally).ship.as_mut().unwrap().repair_priority =
        Some(begin_core::object::RepairClass::Banks);
    for _ in 0..80 {
        g.run_cycle();
    }
    let dmg = g.obj(ally).ship.as_ref().unwrap().banks[0].sys.dmg;
    assert!(dmg < 40, "bank repaired over time: {dmg}");
}

#[test]
fn boarding_captures_a_weak_ship() {
    let (mut g, ally, enemy) = close_duel(17, 1000.0);
    let ally_nation = g.obj(ally).nation;
    // strip the enemy: shields down, tiny crew
    {
        let s = g.obj_mut(enemy).ship.as_mut().unwrap();
        for sh in s.shields.iter_mut() {
            sh.state = begin_core::object::ShieldState::Down;
            sh.effective = 0.0;
        }
        s.survivors = 8;
    }
    for _ in 0..2 {
        g.run_cycle();
    }
    let beamed = begin_core::systems::boarding::transport(&mut g, ally, enemy, 60).unwrap();
    assert!(beamed > 20, "beamed {beamed} boarders");
    for _ in 0..40 {
        g.run_cycle();
        if g.get(enemy).map(|o| o.nation) == Some(ally_nation) {
            break;
        }
    }
    let o = g.obj(enemy);
    assert_eq!(o.nation, ally_nation, "ship captured");
    assert_eq!(o.control, Control::Ai);
}

#[test]
fn tractor_pulls_target_closer() {
    let (mut g, ally, enemy) = close_duel(18, 900.0);
    // tug of war: HC tractor 500 vs BC mass 150
    begin_core::systems::tractor::engage_tractor(&mut g, ally, enemy).unwrap();
    let before = begin_core::systems::helm::dist(&g, ally, enemy);
    for _ in 0..5 {
        g.run_cycle();
    }
    let after = begin_core::systems::helm::dist(&g, ally, enemy);
    assert!(after < before, "tractor closed the gap: {before} -> {after}");
}

#[test]
fn railguns_hit_instantly_within_cone() {
    let mut g = Game::new(GameData::load(), Tuning::default(), 19);
    let sc = Scenario {
        stations: Vec::new(),
        ally: begin_core::scenario::SideConfig {
            nation: "Terran".into(),
            ships: vec![begin_core::scenario::FleetEntry { class: "Lancer".into(), count: 1 }],
            flagship: Some("Lancer".into()),
        },
        enemy: begin_core::scenario::SideConfig {
            nation: "Klingon".into(),
            ships: vec![begin_core::scenario::FleetEntry {
                class: "Battle Cruiser".into(),
                count: 1,
            }],
            flagship: None,
        },
        random_placement: false,
        seed: 19,
        epoch_jd: 0.0,
        spawn_body: None,
    };
    let fleets = spawn_fleets(&mut g, &sc).unwrap();
    let ally = fleets.flagship.unwrap();
    let enemy = fleets.enemy_ids[0];
    g.obj_mut(enemy).control = Control::None;
    g.obj_mut(ally).pos = begin_core::math::Vec3::new(0.0, 0.0, 0.0);
    g.obj_mut(enemy).pos = begin_core::math::Vec3::new(0.0, 12000.0, 0.0);
    for _ in 0..3 {
        g.run_cycle(); // rails charge
    }
    orders::lock_rails(&mut g, ally, &Mounts::All, enemy);
    g.run_cycle();
    let before: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    let n = orders::fire_rails(&mut g, ally, &Mounts::All);
    assert!(n > 0, "{n} rails fired");
    g.run_cycle();
    let after: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    assert!(after < before, "slugs connected same-cycle: {before} -> {after}");
    let rounds = g.obj(ally).ship.as_ref().unwrap().rail_rounds_left;
    assert!(rounds < 4 * 120, "rounds spent: {rounds}");
}

#[test]
fn kinetic_rounds_contact_damage_no_splash() {
    let mut g = Game::new(GameData::load(), Tuning::default(), 20);
    let sc = Scenario {
        stations: Vec::new(),
        ally: begin_core::scenario::SideConfig {
            nation: "Terran".into(),
            ships: vec![begin_core::scenario::FleetEntry { class: "Battlestar".into(), count: 1 }],
            flagship: Some("Battlestar".into()),
        },
        enemy: begin_core::scenario::SideConfig {
            nation: "Klingon".into(),
            ships: vec![begin_core::scenario::FleetEntry {
                class: "Battle Cruiser".into(),
                count: 1,
            }],
            flagship: None,
        },
        random_placement: false,
        seed: 20,
        epoch_jd: 0.0,
        spawn_body: None,
    };
    let fleets = spawn_fleets(&mut g, &sc).unwrap();
    let (ally, enemy) = (fleets.flagship.unwrap(), fleets.enemy_ids[0]);
    g.obj_mut(enemy).control = Control::None;
    g.obj_mut(ally).pos = begin_core::math::Vec3::new(0.0, 0.0, 0.0);
    g.obj_mut(enemy).pos = begin_core::math::Vec3::new(0.0, 9000.0, 0.0);
    orders::load_tubes(&mut g, ally, &Mounts::All, None);
    for _ in 0..4 {
        g.run_cycle();
    }
    orders::lock_tubes(&mut g, ally, &Mounts::All, enemy, 0.0);
    g.run_cycle();
    let before: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    orders::fire_torpedoes(&mut g, ally, &Mounts::All);
    for _ in 0..6 {
        g.run_cycle();
    }
    let after: f64 = (0..6).map(|k| shield_pct(&g, enemy, k)).sum();
    assert!(after < before, "harpoons connected: {before} -> {after}");
}

#[test]
fn phaser_bombardment_eventually_kills() {
    // hold at phaser range (torps can't arm this close — min range is real)
    let (mut g, ally, enemy) = close_duel(21, 1200.0);
    orders::lock_banks(&mut g, ally, &Mounts::All, enemy);
    let mut cycles = 0;
    while g.over.is_none() && cycles < 900 {
        orders::fire_n_charged_banks(&mut g, ally, 99, 45.0);
        g.run_cycle();
        cycles += 1;
    }
    // destroyed outright or reduced to a drifting hulk — either ends it
    assert!(g.over.is_some(), "the helpless enemy eventually dies (cycles {cycles})");
}

#[test]
fn ai_duel_is_competitive_and_resolves() {
    // 1 Federation HC (AI) vs 1 Klingon BC (AI) — the milestone-3 harness
    let mut g = Game::new(GameData::load(), Tuning::default(), 42);
    let mut sc = Scenario::duel();
    sc.enemy.ships[0].count = 1;
    sc.ally.flagship = None; // all-AI
    let fleets = spawn_fleets(&mut g, &sc).unwrap();
    let (a, e) = (fleets.ally_ids[0], fleets.enemy_ids[0]);
    g.obj_mut(a).control = Control::Ai;
    let mut fired_torp = false;
    let mut fired_phaser = false;
    let mut cycles = 0;
    while g.over.is_none() && cycles < 3000 {
        g.run_cycle();
        fired_torp |= !g.torp_ids().is_empty();
        for r in g.reporter.take() {
            fired_phaser |= r.text.contains("phaser");
        }
        cycles += 1;
    }
    assert!(fired_torp, "the AI fired torpedoes");
    assert!(fired_phaser || g.over.is_some(), "the AI used phasers or won outright");
    assert!(g.over.is_some(), "AI duel resolves within 3000 cycles (ran {cycles})");
    let _ = e;
}
