//! High-fidelity ephemeris via `anise` + JPL SPK kernels (de440.bsp).
//! Enabled with the `spice` feature; the kernel path comes from the
//! `BEGIN_SPICE_KERNEL` environment variable (or ./de440.bsp). Bodies not
//! covered by the loaded kernel fall back to kepler.rs transparently.

use crate::math::Vec3;
use anise::almanac::Almanac;
use anise::constants::frames::SSB_J2000;
use anise::prelude::{Epoch, Frame};
use std::sync::OnceLock;

static ALMANAC: OnceLock<Option<Almanac>> = OnceLock::new();

fn almanac() -> Option<&'static Almanac> {
    ALMANAC
        .get_or_init(|| {
            let path = std::env::var("BEGIN_SPICE_KERNEL").unwrap_or_else(|_| "de440.bsp".into());
            match Almanac::default().load(&path) {
                Ok(a) => Some(a),
                Err(e) => {
                    eprintln!("spice: could not load {path}: {e}; using built-in ephemeris");
                    None
                }
            }
        })
        .as_ref()
}

fn naif_id(name: &str) -> Option<i32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "sun" => 10,
        "mercury" => 199,
        "venus" => 299,
        "earth" => 399,
        "luna" | "moon" => 301,
        "mars" => 499,
        "phobos" => 401,
        "deimos" => 402,
        "jupiter" => 599,
        "io" => 501,
        "europa" => 502,
        "ganymede" => 503,
        "callisto" => 504,
        "saturn" => 699,
        "enceladus" => 602,
        "titan" => 606,
        "uranus" => 799,
        "neptune" => 899,
        "triton" => 801,
        "ceres" => 2000001,
        _ => return None,
    })
}

/// Heliocentric position in km at a Julian date, from the loaded kernels.
pub fn body_pos(name: &str, jd: f64) -> Option<Vec3> {
    let alm = almanac()?;
    let id = naif_id(name)?;
    let epoch = Epoch::from_jde_utc(jd);
    let frame = Frame::from_ephem_j2000(id);
    let sun = Frame::from_ephem_j2000(10);
    let state = alm.translate(frame, sun, epoch, None).ok()?;
    let _ = SSB_J2000;
    Some(Vec3::new(state.radius_km.x, state.radius_km.y, state.radius_km.z))
}
