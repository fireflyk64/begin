//! Solar-system environment: planetary bodies from JPL data, stations
//! attached to bodies, procedurally generated rings/asteroids. Designed-in
//! from the start but deliberately small and self-contained (the ephemeris
//! is scenery, spawn anchoring and hazard — combat accelerations dwarf
//! gravity; "in high orbit it plays exactly like begin").
//!
//! Units: 1 game unit = 1 km. Coordinates are relative to the scenario
//! origin (the spawn anchor body's position at epoch) so numbers stay small
//! near the action. Each cycle advances 1 second of ephemeris time.
//!
//! With the `spice` feature and a configured kernel, positions come from
//! `anise` + de440.bsp; the built-in Keplerian fallback (kepler.rs) covers
//! planets, Ceres and major moons otherwise.

pub mod kepler;
#[cfg(feature = "spice")]
pub mod spice;

use crate::events::ReportKind;
use crate::game::Game;
use crate::math::Vec3;
use crate::object::Det;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyState {
    pub name: String,
    pub pos: Vec3,   // game units relative to scenario origin
    pub radius: f64, // km
}

/// A ring / asteroid-field phenomenon: a band around a host body in which
/// rocks are procedurally generated (fixed seed) when a ship is nearby.
#[derive(Debug, Clone)]
pub struct Ring {
    pub host: String,
    pub r_in: f64,
    pub r_out: f64,
    pub density: f64, // rocks per 1000-km cell, roughly
}

/// A station bound to a body at low or geosynchronous orbit.
#[derive(Debug, Clone)]
pub struct Station {
    pub ship: crate::object::ObjId,
    pub body: String,
    pub altitude: f64,   // km above surface
    pub period_s: f64,   // orbital period
    pub phase0: f64,     // radians at epoch
}

#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// Julian date at cycle 0; 0.0 = no environment (classic empty space).
    pub epoch_jd: f64,
    /// Anchor body whose epoch position is the coordinate origin.
    pub origin_body: Option<String>,
    origin_offset: Vec3,
    pub bodies: Vec<BodyState>,
    pub rings: Vec<Ring>,
    pub stations: Vec<Station>,
    pub seed: u64,
    /// Procedural rocks near ships this cycle (scenery + hazard).
    pub rocks: Vec<BodyState>,
    /// Spawn anchor: (body, altitude km) that fleets spawn near.
    pub spawn_anchor: Option<(String, f64)>,
}

/// Parse a `YYYY-MM-DD` date into a Julian date (0.0 = no environment).
pub fn parse_epoch(s: &str) -> f64 {
    let parts: Vec<i64> = s.split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return 0.0;
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);
    let a = (14 - m) / 12;
    let y2 = y + 4800 - a;
    let m2 = m + 12 * a - 3;
    let jdn = d + (153 * m2 + 2) / 5 + 365 * y2 + y2 / 4 - y2 / 100 + y2 / 400 - 32045;
    jdn as f64 - 0.5
}

/// Heliocentric position of a body by name — spice kernels when configured,
/// Keplerian fallback otherwise.
fn helio_pos(name: &str, jd: f64) -> Option<Vec3> {
    #[cfg(feature = "spice")]
    if let Some(p) = spice::body_pos(name, jd) {
        return Some(p);
    }
    kepler::body_pos(name, jd)
}

/// Configure the environment for a scenario. `spawn_body` accepts
/// "Mars", "Mars:low", "Mars:high", "Saturn:rings".
pub fn setup(g: &mut Game, epoch_jd: f64, spawn_body: Option<&str>, seed: u64) {
    g.env.epoch_jd = epoch_jd;
    g.env.seed = seed;
    if epoch_jd <= 0.0 {
        return; // classic empty space
    }
    // parse the anchor
    let (anchor_name, orbit) = match spawn_body {
        Some(s) => {
            let (b, o) = s.split_once(':').unwrap_or((s, "high"));
            (Some(b.to_string()), o.to_ascii_lowercase())
        }
        None => (None, "high".into()),
    };
    let origin_name = anchor_name.clone().unwrap_or_else(|| "Earth".to_string());
    g.env.origin_body = Some(origin_name.clone());
    g.env.origin_offset = helio_pos(&origin_name, epoch_jd).unwrap_or(Vec3::ZERO);

    // populate the body table: Sun, planets, Ceres, moons
    let mut bodies = vec![BodyState {
        name: "Sun".into(),
        pos: Vec3::ZERO,
        radius: 696000.0,
    }];
    for p in kepler::PLANETS.iter() {
        bodies.push(BodyState { name: p.name.into(), pos: Vec3::ZERO, radius: p.radius_km });
    }
    for &(m, _, r, ..) in kepler::MOONS.iter() {
        bodies.push(BodyState { name: m.into(), pos: Vec3::ZERO, radius: r });
    }
    g.env.bodies = bodies;

    // ring phenomena: Saturn's rings and the main asteroid belt
    g.env.rings = vec![
        Ring { host: "Saturn".into(), r_in: 74500.0, r_out: 140220.0, density: 3.0 },
        Ring {
            host: "Sun".into(),
            r_in: 2.1 * kepler::AU,
            r_out: 3.3 * kepler::AU,
            density: 0.02,
        },
    ];

    // spawn anchor altitude
    if let Some(body) = anchor_name {
        let radius = kepler::body_radius(&body).unwrap_or(6000.0);
        let altitude = match orbit.as_str() {
            "low" => radius * 0.25 + 300.0,
            "geo" | "geosync" => radius * 5.6,
            "rings" => radius * 1.4,
            _ => radius * 12.0, // "high" — begin-style free space
        };
        g.env.spawn_anchor = Some((body, altitude));
    }

    update_positions(g);
}

/// Where fleets spawn: near the anchor body (offset by its radius+altitude)
/// or free space at the origin.
pub fn spawn_center(g: &Game) -> Vec3 {
    match &g.env.spawn_anchor {
        Some((body, altitude)) => {
            let pos = g
                .env
                .bodies
                .iter()
                .find(|b| b.name.eq_ignore_ascii_case(body))
                .map(|b| b.pos)
                .unwrap_or(Vec3::ZERO);
            let r = kepler::body_radius(body).unwrap_or(6000.0) + altitude;
            pos + Vec3::new(r, 0.0, 0.0)
        }
        None => Vec3::ZERO,
    }
}

/// Attach a station ship to a body at low or geosynchronous orbit.
pub fn attach_station(g: &mut Game, ship: crate::object::ObjId, body: &str, orbit: &str) -> Result<(), String> {
    let radius = kepler::body_radius(body).ok_or_else(|| format!("unknown body {body}"))?;
    let (altitude, period_s) = match orbit {
        "low" => (radius * 0.25 + 300.0, 5400.0),
        _ => (radius * 5.6, 86164.0), // geosynchronous
    };
    let phase0 = (g.env.stations.len() as f64) * 1.1;
    g.env.stations.push(Station {
        ship,
        body: body.to_string(),
        altitude,
        period_s,
        phase0,
    });
    place_station(g, g.env.stations.len() - 1);
    Ok(())
}

fn place_station(g: &mut Game, idx: usize) {
    let st = g.env.stations[idx].clone();
    let Some(body) = g.env.bodies.iter().find(|b| b.name.eq_ignore_ascii_case(&st.body)) else {
        return;
    };
    let r = body.radius + st.altitude;
    let t = g.cycle as f64;
    let ang = st.phase0 + t * std::f64::consts::TAU / st.period_s;
    let pos = body.pos + Vec3::new(r * ang.cos(), r * ang.sin(), 0.0);
    if let Some(o) = g.get_mut(st.ship) {
        o.pos = pos;
        o.vel = Vec3::ZERO;
        o.warp = 0.0;
        o.desired_warp = 0.0;
    }
}

/// Per-cycle environment update, called from the cycle pipeline.
pub fn update(g: &mut Game) {
    if g.env.epoch_jd <= 0.0 {
        return;
    }
    update_positions(g);
    for i in 0..g.env.stations.len() {
        if g.get(g.env.stations[i].ship).is_some() {
            place_station(g, i);
        }
    }
    generate_rocks(g);
    hazards(g);
}

fn update_positions(g: &mut Game) {
    let jd = g.env.epoch_jd + g.cycle as f64 / 86400.0;
    let origin = g.env.origin_offset;
    for b in g.env.bodies.iter_mut() {
        if let Some(p) = helio_pos(&b.name, jd) {
            b.pos = p - origin;
        }
    }
}

fn hash2(seed: u64, a: i64, b: i64) -> u64 {
    let mut x = seed ^ (a as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ (b as u64).wrapping_mul(0xC2B2AE3D27D4EB4F);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51AFD7ED558CCD);
    x ^= x >> 33;
    x
}

/// Procedural ring rocks near ships (fixed seed → same rocks every visit).
fn generate_rocks(g: &mut Game) {
    const CELL: f64 = 2000.0; // km
    const NEAR: f64 = 30000.0; // generation radius around a ship
    let mut rocks = Vec::new();
    let rings = g.env.rings.clone();
    let ship_pos: Vec<Vec3> = g.ship_ids().iter().map(|&i| g.obj(i).pos).collect();
    for ring in &rings {
        let Some(host) = g.env.bodies.iter().find(|b| b.name.eq_ignore_ascii_case(&ring.host))
        else {
            continue;
        };
        for sp in &ship_pos {
            let rel = *sp - host.pos;
            let dist = (rel.x * rel.x + rel.y * rel.y).sqrt();
            if dist < ring.r_in - NEAR || dist > ring.r_out + NEAR {
                continue;
            }
            // walk the cell grid around the ship
            let c0x = ((sp.x - NEAR) / CELL).floor() as i64;
            let c1x = ((sp.x + NEAR) / CELL).floor() as i64;
            let c0y = ((sp.y - NEAR) / CELL).floor() as i64;
            let c1y = ((sp.y + NEAR) / CELL).floor() as i64;
            for cx in c0x..=c1x {
                for cy in c0y..=c1y {
                    let h = hash2(g.env.seed, cx, cy);
                    let count = ((h % 1000) as f64 / 1000.0 * ring.density) as u64
                        + u64::from((h % 1000) as f64 / 1000.0 < ring.density.fract());
                    for k in 0..count.min(4) {
                        let h2 = hash2(h, k as i64, 17);
                        let px = cx as f64 * CELL + (h2 % 1000) as f64 / 1000.0 * CELL;
                        let py = cy as f64 * CELL + ((h2 >> 10) % 1000) as f64 / 1000.0 * CELL;
                        let rrel = Vec3::new(px - host.pos.x, py - host.pos.y, 0.0);
                        let rd = (rrel.x * rrel.x + rrel.y * rrel.y).sqrt();
                        if rd < ring.r_in || rd > ring.r_out {
                            continue;
                        }
                        let radius = 1.0 + ((h2 >> 20) % 30) as f64;
                        rocks.push(BodyState {
                            name: "rock".into(),
                            pos: Vec3::new(px, py, ((h2 >> 25) % 400) as f64 - 200.0),
                            radius,
                        });
                    }
                }
            }
        }
    }
    rocks.truncate(600); // solar-system-sized cap
    g.env.rocks = rocks;
}

/// Planet impact + ring hazard.
fn hazards(g: &mut Game) {
    let bodies = g.env.bodies.clone();
    let rings = g.env.rings.clone();
    for id in g.ship_ids() {
        let (pos, warp) = {
            let o = g.obj(id);
            (o.pos, o.warp.abs())
        };
        // lithobraking
        for b in &bodies {
            if (pos - b.pos).len() < b.radius {
                let name = g.obj(id).name.clone();
                g.obj_mut(id).det = Det::Destroyed;
                g.say(
                    None,
                    "",
                    format!("The {name} has flown into {}!", b.name),
                    ReportKind::Alert,
                );
            }
        }
        // tearing through a ring at speed risks rock strikes
        if warp > 2.0 {
            for ring in &rings {
                let Some(host) = bodies.iter().find(|b| b.name.eq_ignore_ascii_case(&ring.host))
                else {
                    continue;
                };
                let rel = pos - host.pos;
                let dist = (rel.x * rel.x + rel.y * rel.y).sqrt();
                if dist > ring.r_in && dist < ring.r_out && rel.z.abs() < 500.0 {
                    let chance = (warp - 2.0) * 1.5 * ring.density.min(3.0);
                    if g.rng.percent(chance) {
                        let dmg = g.rng.range(2.0, warp * 3.0);
                        let face = g.rng.range(0.0, 360.0);
                        let name = g.obj(id).name.clone();
                        g.say(
                            None,
                            "",
                            format!("{name} has struck ring debris!"),
                            ReportKind::Alert,
                        );
                        crate::systems::damage::deal_damage(
                            g,
                            id,
                            face,
                            dmg,
                            crate::systems::damage::DamageType::Antimatter,
                        );
                    }
                }
            }
        }
    }
}
