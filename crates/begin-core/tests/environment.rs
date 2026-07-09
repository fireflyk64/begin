//! Environment milestone tests: ephemeris, stations, procgen rings, hazards.

use begin_core::scenario::{spawn_fleets, Scenario, StationSpec};
use begin_core::{Game, GameData, Tuning};

#[test]
fn station_orbits_its_body() {
    let mut g = Game::new(GameData::load(), Tuning::default(), 5);
    let mut sc = Scenario::duel();
    sc.epoch_jd = begin_core::env::parse_epoch("2026-07-09");
    sc.spawn_body = Some("Earth:geo".into());
    sc.stations = vec![StationSpec {
        class: "Starbase".into(),
        body: "Earth".into(),
        orbit: "geo".into(),
        ally: true,
    }];
    begin_core::env::setup(&mut g, sc.epoch_jd, sc.spawn_body.as_deref(), 5);
    let fleets = spawn_fleets(&mut g, &sc).unwrap();
    let station = *fleets.ally_ids.last().unwrap();
    let earth = g.env.bodies.iter().find(|b| b.name == "Earth").unwrap().pos;
    let r0 = (g.obj(station).pos - earth).len();
    // geosync ≈ 5.6 body radii above the surface + radius
    assert!(r0 > 30000.0 && r0 < 60000.0, "station at {r0} km from Earth center");
    let p0 = g.obj(station).pos;
    for _ in 0..600 {
        g.run_cycle();
    }
    let earth = g.env.bodies.iter().find(|b| b.name == "Earth").unwrap().pos;
    let r1 = (g.obj(station).pos - earth).len();
    assert!((r1 - r0).abs() < 100.0, "orbit radius held: {r0} vs {r1}");
    assert!((g.obj(station).pos - p0).len() > 10.0, "station actually moves along the orbit");
}

#[test]
fn ring_rocks_appear_near_saturn() {
    let mut g = Game::new(GameData::load(), Tuning::default(), 6);
    let mut sc = Scenario::duel();
    sc.epoch_jd = begin_core::env::parse_epoch("2026-07-09");
    sc.spawn_body = Some("Saturn:rings".into());
    begin_core::env::setup(&mut g, sc.epoch_jd, sc.spawn_body.as_deref(), 6);
    let _ = spawn_fleets(&mut g, &sc).unwrap();
    g.run_cycle();
    assert!(
        !g.env.rocks.is_empty(),
        "procedural rocks generated near the rings (got {})",
        g.env.rocks.len()
    );
    // determinism: same seed, same rocks
    let snapshot: Vec<(i64, i64)> =
        g.env.rocks.iter().map(|r| (r.pos.x as i64, r.pos.y as i64)).collect();
    let mut g2 = Game::new(GameData::load(), Tuning::default(), 6);
    begin_core::env::setup(&mut g2, sc.epoch_jd, sc.spawn_body.as_deref(), 6);
    let _ = spawn_fleets(&mut g2, &sc).unwrap();
    g2.run_cycle();
    let snapshot2: Vec<(i64, i64)> =
        g2.env.rocks.iter().map(|r| (r.pos.x as i64, r.pos.y as i64)).collect();
    assert_eq!(snapshot, snapshot2, "fixed seed → same phenomenon");
}

#[test]
fn flying_into_a_planet_is_fatal() {
    let mut g = Game::new(GameData::load(), Tuning::default(), 7);
    let mut sc = Scenario::duel();
    sc.enemy.ships[0].count = 1;
    sc.epoch_jd = begin_core::env::parse_epoch("2026-07-09");
    sc.spawn_body = Some("Luna:low".into());
    begin_core::env::setup(&mut g, sc.epoch_jd, sc.spawn_body.as_deref(), 7);
    let fleets = spawn_fleets(&mut g, &sc).unwrap();
    let me = fleets.flagship.unwrap();
    for &e in &fleets.enemy_ids {
        g.obj_mut(e).control = begin_core::object::Control::None; // bystanders
    }
    g.obj_mut(me).control = begin_core::object::Control::Local;
    // steer for Luna's center, correcting every cycle
    for _ in 0..400 {
        if g.get(me).is_none() {
            break;
        }
        let luna = g.env.bodies.iter().find(|b| b.name == "Luna").unwrap().pos;
        let d = luna - g.obj(me).pos;
        let course = begin_core::math::bearing_of(d.x, d.y);
        begin_core::orders::helm(&mut g, me, Some(course), Some(0.0), Some(4.0));
        g.run_cycle();
    }
    assert!(g.get(me).is_none(), "lithobraking is not survivable");
}
