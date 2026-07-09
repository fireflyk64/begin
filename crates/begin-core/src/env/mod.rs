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
