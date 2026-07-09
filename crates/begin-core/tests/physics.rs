//! Milestone 1 test: two ships fly (helm, power, drives).

use begin_core::object::Control;
use begin_core::scenario::{spawn_fleets, Scenario};
use begin_core::{Game, GameData, Tuning};

fn duel_game(seed: u64) -> (Game, begin_core::ObjId, begin_core::ObjId) {
    let mut g = Game::new(GameData::load(), Tuning::default(), seed);
    let mut sc = Scenario::duel();
    sc.enemy.ships[0].count = 1;
    let fleets = spawn_fleets(&mut g, &sc).unwrap();
    let ally = fleets.flagship.unwrap();
    let enemy = fleets.enemy_ids[0];
    // keep the enemy inert for physics tests
    g.obj_mut(enemy).control = Control::None;
    (g, ally, enemy)
}

#[test]
fn ships_accelerate_and_move() {
    let (mut g, ally, _) = duel_game(1);
    let start = g.obj(ally).pos;
    g.obj_mut(ally).desired_warp = 6.0;
    g.obj_mut(ally).desired_course = 0.0;
    for _ in 0..80 {
        g.run_cycle();
    }
    let o = g.obj(ally);
    assert!((o.warp - 6.0).abs() < 0.01, "reached warp 6, got {}", o.warp);
    // 0° = +Y: ship moved north
    assert!(o.pos.y > start.y + 5000.0, "moved: {:?} -> {:?}", start, o.pos);
    assert!((o.pos.x - start.x).abs() < 1.0);
}

#[test]
fn instant_helm_below_warp_one() {
    let (mut g, ally, _) = duel_game(2);
    g.obj_mut(ally).desired_warp = 1.0;
    g.obj_mut(ally).desired_course = 135.0;
    g.run_cycle();
    let o = g.obj(ally);
    assert_eq!(o.warp, 1.0);
    assert_eq!(o.course, 135.0);
}

#[test]
fn turn_rate_scales_with_warp() {
    let (mut g, ally, _) = duel_game(3);
    // warp 6 is sustainable for an HC (ratio < warp_efficiency: no overheat)
    g.obj_mut(ally).desired_warp = 6.0;
    for _ in 0..80 {
        g.run_cycle();
    }
    assert!((g.obj(ally).warp - 6.0).abs() < 0.01, "warp {}", g.obj(ally).warp);
    // now order a 180° turn: at warp 6 a Heavy Cruiser needs several cycles
    g.obj_mut(ally).desired_course = 180.0;
    g.run_cycle();
    let after_one = g.obj(ally).course;
    assert!(after_one > 0.0 && after_one < 180.0, "partial turn, got {after_one}");
    for _ in 0..60 {
        g.run_cycle();
    }
    assert!((g.obj(ally).course - 180.0).abs() < 0.01);
}

#[test]
fn speed_is_100_units_per_warp_per_cycle() {
    let (mut g, ally, _) = duel_game(4);
    g.obj_mut(ally).desired_warp = 1.0;
    g.run_cycle(); // instant to warp 1
    let before = g.obj(ally).pos;
    g.run_cycle();
    let after = g.obj(ally).pos;
    let moved = (after - before).len();
    assert!((moved - 100.0).abs() < 1e-6, "warp 1 = 100 units/cycle, got {moved}");
}

#[test]
fn pursue_tracks_out_of_plane() {
    let (mut g, ally, enemy) = duel_game(5);
    // enemy climbs out of plane
    g.obj_mut(enemy).desired_warp = 4.0;
    g.obj_mut(enemy).desired_mark = 45.0;
    g.obj_mut(enemy).desired_course = 0.0;
    // ally pursues
    g.obj_mut(ally).helm = begin_core::object::HelmMode::Pursue;
    g.obj_mut(ally).pursue = Some(enemy);
    g.obj_mut(ally).desired_warp = 6.0;
    for _ in 0..50 {
        g.run_cycle();
    }
    let e = g.obj(enemy);
    assert!(e.pos.z > 1000.0, "enemy climbed, z={}", e.pos.z);
    let a = g.obj(ally);
    assert!(a.mark > 5.0, "pursuer pitched up, mark={}", a.mark);
    // and is actually closing or tracking upward
    assert!(a.pos.z > 0.0, "pursuer left the plane, z={}", a.pos.z);
}

#[test]
fn planar_lock_confines_to_plane() {
    let mut g = Game::new(GameData::load(), Tuning { planar_lock: true, ..Tuning::default() }, 6);
    let sc = Scenario::duel();
    let fleets = spawn_fleets(&mut g, &sc).unwrap();
    let ally = fleets.flagship.unwrap();
    g.obj_mut(ally).desired_warp = 5.0;
    g.obj_mut(ally).desired_mark = 60.0;
    for _ in 0..20 {
        g.run_cycle();
    }
    let o = g.obj(ally);
    assert_eq!(o.pos.z, 0.0);
    assert_eq!(o.mark, 0.0);
}

#[test]
fn flat_out_warp_overheats_and_destroys_drives() {
    let (mut g, ally, _) = duel_game(7);
    g.obj_mut(ally).desired_warp = 20.0; // clamped to max attainable
    let mut peak: f64 = 0.0;
    for _ in 0..400 {
        g.run_cycle();
        let s = g.obj(ally).ship.as_ref().unwrap();
        peak = peak.max(s.max_drive_temp());
    }
    // running flat out heats the drives past the floor...
    assert!(peak > 12.0, "peak temp {peak}");
    // ...and eventually past the 40m limit, destroying them (manual: the
    // player must slow down to cool; the AI has a temperature panic for this)
    let s = g.obj(ally).ship.as_ref().unwrap();
    assert!(
        s.drives.iter().all(|d| d.sys.destroyed()),
        "drives destroyed after sustained max warp: temps {:?}",
        s.drives.iter().map(|d| d.temp).collect::<Vec<_>>()
    );
    // impulse fallback caps warp at 1
    g.obj_mut(ally).desired_warp = 9.0;
    g.run_cycle();
    assert!(g.obj(ally).desired_warp <= 1.0);
}

#[test]
fn max_warp_impulse_fallback() {
    let (mut g, ally, _) = duel_game(8);
    {
        let s = g.obj_mut(ally).ship.as_mut().unwrap();
        for d in s.drives.iter_mut() {
            d.sys.dmg = 100;
        }
    }
    g.obj_mut(ally).desired_warp = 9.0;
    g.run_cycle();
    let o = g.obj(ally);
    assert!(o.desired_warp <= 1.0, "impulse fallback caps at warp 1, got {}", o.desired_warp);
}
