//! Solar-system environment: planetary bodies, stations, procedurally
//! generated rings/asteroid fields. Designed-in from the start but
//! deliberately small and self-contained (prompt requirement); combat
//! accelerations dwarf gravity, so bodies are scenery + spawn anchors.

use crate::math::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyState {
    pub name: String,
    pub pos: Vec3,    // game units relative to scenario origin
    pub radius: f64,  // game units
}

#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// Epoch date (Julian date at cycle 0); each cycle advances 1 second.
    pub epoch_jd: f64,
    pub bodies: Vec<BodyState>,
}

/// Parse a `YYYY-MM-DD` date into a Julian date (0.0 = no environment).
pub fn parse_epoch(s: &str) -> f64 {
    let parts: Vec<i64> = s.split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return 0.0;
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);
    // standard Fliegel-Van Flandern Gregorian → JDN
    let a = (14 - m) / 12;
    let y2 = y + 4800 - a;
    let m2 = m + 12 * a - 3;
    let jdn = d + (153 * m2 + 2) / 5 + 365 * y2 + y2 / 4 - y2 / 100 + y2 / 400 - 32045;
    jdn as f64 - 0.5
}

/// Configure the environment for a scenario (real ephemeris arrives with the
/// environment milestone; without an epoch the play area is empty space,
/// exactly like classic begin).
pub fn setup(g: &mut crate::game::Game, epoch_jd: f64, spawn_body: Option<&str>, seed: u64) {
    g.env.epoch_jd = epoch_jd;
    let _ = (spawn_body, seed);
}
