//! Built-in ephemeris fallback: JPL "Approximate Positions of the Planets"
//! (E.M. Standish, Table 1, valid 1800 AD - 2050 AD) Keplerian elements for
//! the eight planets, plus Ceres and the major moons on simplified circular
//! orbits. Good to arcminutes — far more than a tactical simulator needs.
//!
//! Units: kilometers (1 game unit = 1 km), heliocentric ecliptic J2000.

use crate::math::Vec3;

pub const AU: f64 = 1.495_978_707e8; // km

/// a(AU), e, I(deg), L(deg), long.peri(deg), long.node(deg) + per-century rates
pub struct Elements {
    pub name: &'static str,
    pub radius_km: f64,
    pub el: [f64; 6],
    pub rate: [f64; 6],
}

pub const PLANETS: [Elements; 9] = [
    Elements {
        name: "Mercury",
        radius_km: 2439.7,
        el: [0.38709927, 0.20563593, 7.00497902, 252.25032350, 77.45779628, 48.33076593],
        rate: [0.00000037, 0.00001906, -0.00594749, 149472.67411175, 0.16047689, -0.12534081],
    },
    Elements {
        name: "Venus",
        radius_km: 6051.8,
        el: [0.72333566, 0.00677672, 3.39467605, 181.97909950, 131.60246718, 76.67984255],
        rate: [0.00000390, -0.00004107, -0.00078890, 58517.81538729, 0.00268329, -0.27769418],
    },
    Elements {
        name: "Earth",
        radius_km: 6371.0,
        el: [1.00000261, 0.01671123, -0.00001531, 100.46457166, 102.93768193, 0.0],
        rate: [0.00000562, -0.00004392, -0.01294668, 35999.37244981, 0.32327364, 0.0],
    },
    Elements {
        name: "Mars",
        radius_km: 3389.5,
        el: [1.52371034, 0.09339410, 1.84969142, -4.55343205, -23.94362959, 49.55953891],
        rate: [0.00001847, 0.00007882, -0.00813131, 19140.30268499, 0.44441088, -0.29257343],
    },
    Elements {
        name: "Jupiter",
        radius_km: 69911.0,
        el: [5.20288700, 0.04838624, 1.30439695, 34.39644051, 14.72847983, 100.47390909],
        rate: [-0.00011607, -0.00013253, -0.00183714, 3034.74612775, 0.21252668, 0.20469106],
    },
    Elements {
        name: "Saturn",
        radius_km: 58232.0,
        el: [9.53667594, 0.05386179, 2.48599187, 49.95424423, 92.59887831, 113.66242448],
        rate: [-0.00125060, -0.00050991, 0.00193609, 1222.49362201, -0.41897216, -0.28867794],
    },
    Elements {
        name: "Uranus",
        radius_km: 25362.0,
        el: [19.18916464, 0.04725744, 0.77263783, 313.23810451, 170.95427630, 74.01692503],
        rate: [-0.00196176, -0.00004397, -0.00242939, 428.48202785, 0.40805281, 0.04240589],
    },
    Elements {
        name: "Neptune",
        radius_km: 24622.0,
        el: [30.06992276, 0.00859048, 1.77004347, -55.12002969, 44.96476227, 131.78422574],
        rate: [0.00026291, 0.00005105, 0.00035372, 218.45945325, -0.32241464, -0.00508664],
    },
    // Ceres: mean elements (J2000-ish), rates: mean motion only
    Elements {
        name: "Ceres",
        radius_km: 469.7,
        el: [2.7675, 0.0758, 10.594, 95.989, 73.597, 80.393],
        rate: [0.0, 0.0, 0.0, 78193.30, 0.0, 0.0],
    },
];

/// Simplified circular-orbit moons: (name, parent, radius_km, orbit_km,
/// period_days, phase0_deg).
pub const MOONS: [(&str, &str, f64, f64, f64, f64); 10] = [
    ("Luna", "Earth", 1737.4, 384400.0, 27.321661, 135.0),
    ("Phobos", "Mars", 11.3, 9376.0, 0.31891, 0.0),
    ("Deimos", "Mars", 6.2, 23463.0, 1.26244, 90.0),
    ("Io", "Jupiter", 1821.6, 421800.0, 1.769138, 20.0),
    ("Europa", "Jupiter", 1560.8, 671100.0, 3.551181, 130.0),
    ("Ganymede", "Jupiter", 2634.1, 1070400.0, 7.154553, 250.0),
    ("Callisto", "Jupiter", 2410.3, 1882700.0, 16.689017, 310.0),
    ("Titan", "Saturn", 2574.7, 1221870.0, 15.945421, 70.0),
    ("Enceladus", "Saturn", 252.1, 238040.0, 1.370218, 180.0),
    ("Triton", "Neptune", 1353.4, 354759.0, -5.876854, 45.0),
];

/// Heliocentric ecliptic position of a planet at Julian date `jd`, km.
pub fn planet_pos(el: &Elements, jd: f64) -> Vec3 {
    let t = (jd - 2451545.0) / 36525.0; // centuries past J2000
    let a = (el.el[0] + el.rate[0] * t) * AU;
    let e = el.el[1] + el.rate[1] * t;
    let i = (el.el[2] + el.rate[2] * t).to_radians();
    let l = el.el[3] + el.rate[3] * t;
    let lp = el.el[4] + el.rate[4] * t;
    let ln = el.el[5] + el.rate[5] * t;
    let omega = (lp - ln).to_radians(); // argument of perihelion
    let node = ln.to_radians();
    let m = (l - lp).rem_euclid(360.0).to_radians(); // mean anomaly

    // solve Kepler's equation
    let mut ea = if e < 0.8 { m } else { std::f64::consts::PI };
    for _ in 0..12 {
        let d = ea - e * ea.sin() - m;
        ea -= d / (1.0 - e * ea.cos());
    }
    // heliocentric coords in orbital plane
    let xv = a * (ea.cos() - e);
    let yv = a * (1.0 - e * e).sqrt() * ea.sin();
    // rotate: argument of perihelion, inclination, node
    let (cw, sw) = (omega.cos(), omega.sin());
    let (ci, si) = (i.cos(), i.sin());
    let (cn, sn) = (node.cos(), node.sin());
    let x = (cw * cn - sw * sn * ci) * xv + (-sw * cn - cw * sn * ci) * yv;
    let y = (cw * sn + sw * cn * ci) * xv + (-sw * sn + cw * cn * ci) * yv;
    let z = (sw * si) * xv + (cw * si) * yv;
    Vec3::new(x, y, z)
}

/// Position of a named body (planet, moon, Ceres, or the Sun) at `jd`,
/// heliocentric km. Moons ride their parent on a circular orbit.
pub fn body_pos(name: &str, jd: f64) -> Option<Vec3> {
    if name.eq_ignore_ascii_case("Sun") {
        return Some(Vec3::ZERO);
    }
    if let Some(p) = PLANETS.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
        return Some(planet_pos(p, jd));
    }
    for &(mname, parent, _r, orbit, period, phase0) in MOONS.iter() {
        if mname.eq_ignore_ascii_case(name) {
            let pp = body_pos(parent, jd)?;
            let ang = (phase0 + 360.0 * (jd - 2451545.0) / period).to_radians();
            return Some(pp + Vec3::new(orbit * ang.cos(), orbit * ang.sin(), 0.0));
        }
    }
    None
}

pub fn body_radius(name: &str) -> Option<f64> {
    if name.eq_ignore_ascii_case("Sun") {
        return Some(696000.0);
    }
    if let Some(p) = PLANETS.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
        return Some(p.radius_km);
    }
    MOONS
        .iter()
        .find(|(m, ..)| m.eq_ignore_ascii_case(name))
        .map(|&(_, _, r, ..)| r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earth_at_one_au() {
        let jd = 2451545.0; // J2000
        let p = body_pos("Earth", jd).unwrap();
        let r = p.len() / AU;
        assert!((r - 1.0).abs() < 0.02, "Earth at ~1 AU, got {r}");
    }

    #[test]
    fn mars_distance_sane() {
        let jd = 2460000.5; // 2023-02-25
        let e = body_pos("Earth", jd).unwrap();
        let m = body_pos("Mars", jd).unwrap();
        let d = (m - e).len() / AU;
        assert!((0.3..2.7).contains(&d), "Earth-Mars {d} AU");
    }

    #[test]
    fn luna_orbits_earth() {
        let jd = 2451545.0;
        let e = body_pos("Earth", jd).unwrap();
        let l = body_pos("Luna", jd).unwrap();
        let d = (l - e).len();
        assert!((d - 384400.0).abs() < 1.0, "Luna at {d} km");
    }
}
