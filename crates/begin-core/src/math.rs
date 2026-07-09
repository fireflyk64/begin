//! Angles, bearings, vectors, RNG.
//!
//! Conventions (AI_AND_COMBAT.md §1):
//! - Bearings are compass degrees: 0° = +Y, 90° = +X, computed as
//!   `atan2(dx, dy).to_degrees()` normalized to [0, 360).
//! - Mark (3D elevation) is degrees in [-90, +90] relative to the reference
//!   plane; helm syntax `320^22`.
//! - Speed: warp factor × 100 distance units per cycle, integrated in 20
//!   sub-steps of warp × 5.0 (uniform 20 sub-steps; the original binary's
//!   19×5 ordnance path is documented as a quirk and intentionally fixed).

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Mul, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
    pub fn len(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    pub fn len_sq(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn norm(self) -> Vec3 {
        let l = self.len();
        if l <= 1e-12 {
            Vec3::ZERO
        } else {
            self * (1.0 / l)
        }
    }
    /// Angle between two vectors in degrees [0, 180]. Used for 3D weapon cones.
    pub fn angle_to(self, o: Vec3) -> f64 {
        let d = self.len() * o.len();
        if d <= 1e-12 {
            return 0.0;
        }
        (self.dot(o) / d).clamp(-1.0, 1.0).acos().to_degrees()
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

/// Normalize an angle to [0, 360). (`mysteryDamFunc`, asm 34297)
pub fn norm360(a: f64) -> f64 {
    let r = a % 360.0;
    if r < 0.0 {
        r + 360.0
    } else {
        r
    }
}

/// Angular distance in [0, 180]. (`sub_1378D`, asm 34186)
pub fn ang_dist(a: f64, b: f64) -> f64 {
    let d = norm360(a - b);
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

/// Signed shortest rotation from `from` to `to`, in (-180, 180].
pub fn ang_delta(from: f64, to: f64) -> f64 {
    let mut d = norm360(to - from);
    if d > 180.0 {
        d -= 360.0;
    }
    d
}

/// Compass bearing (degrees) of the in-plane direction dx, dy.
/// 0° = +Y, 90° = +X. (`whichShieldFace` 33683, `sub_1340E` 33782)
pub fn bearing_of(dx: f64, dy: f64) -> f64 {
    if dx == 0.0 && dy == 0.0 {
        return 0.0;
    }
    norm360(dx.atan2(dy).to_degrees())
}

/// Elevation ("mark") angle of a 3D displacement, degrees in [-90, +90].
pub fn mark_of(d: Vec3) -> f64 {
    let planar = (d.x * d.x + d.y * d.y).sqrt();
    if planar == 0.0 && d.z == 0.0 {
        return 0.0;
    }
    d.z.atan2(planar).to_degrees()
}

/// Unit direction vector for a (course, mark) pair.
/// Velocity per sub-step = dir(course, mark) × warp × 5.0 (§1).
pub fn dir(course: f64, mark: f64) -> Vec3 {
    let (sc, cc) = course.to_radians().sin_cos();
    let (sm, cm) = mark.to_radians().sin_cos();
    Vec3::new(sc * cm, cc * cm, sm)
}

/// Deterministic RNG reproducing the original's `rand()/32767` shapes
/// (`randRangeHelper` 33390). 15-bit output like Borland C's rand().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng { state: seed.wrapping_mul(0x9E3779B97F4A7C15) | 1 }
    }
    fn next15(&mut self) -> u32 {
        // xorshift64*, top 15 bits — same range/shape as DOS rand()
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        ((x.wrapping_mul(0x2545F4914F6CDD1D)) >> 49) as u32
    }
    /// Uniform double in [0, 1]. (`rand()/32767`)
    pub fn unit(&mut self) -> f64 {
        self.next15() as f64 / 32767.0
    }
    /// Uniform double in [lo, hi]. (`sub_1305E`, 33326)
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    /// Uniform integer in [lo, hi] (floor of range). (`randRange`, 33352)
    pub fn irange(&mut self, lo: i32, hi: i32) -> i32 {
        (self.range(lo as f64, hi as f64)).floor() as i32
    }
    /// True with n% probability. (`percentRand`, 33413)
    pub fn percent(&mut self, n: f64) -> bool {
        self.unit() * 100.0 < n
    }
    /// ±1 with equal probability (AI weave side).
    pub fn side(&mut self) -> f64 {
        if self.unit() < 0.5 {
            -1.0
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearing_compass_convention() {
        assert!((bearing_of(0.0, 1.0) - 0.0).abs() < 1e-9); // +Y = 0°
        assert!((bearing_of(1.0, 0.0) - 90.0).abs() < 1e-9); // +X = 90°
        assert!((bearing_of(0.0, -1.0) - 180.0).abs() < 1e-9);
        assert!((bearing_of(-1.0, 0.0) - 270.0).abs() < 1e-9);
    }

    #[test]
    fn dir_matches_bearing() {
        for c in [0.0, 45.0, 137.0, 250.0, 359.0] {
            let v = dir(c, 0.0);
            assert!((bearing_of(v.x, v.y) - c).abs() < 1e-9, "course {c}");
        }
        let up = dir(0.0, 90.0);
        assert!(up.z > 0.9999);
    }

    #[test]
    fn angles() {
        assert_eq!(norm360(-30.0), 330.0);
        assert_eq!(ang_dist(350.0, 10.0), 20.0);
        assert_eq!(ang_delta(350.0, 10.0), 20.0);
        assert_eq!(ang_delta(10.0, 350.0), -20.0);
    }

    #[test]
    fn rng_shapes() {
        let mut r = Rng::new(42);
        let mut acc = 0.0;
        for _ in 0..10000 {
            let u = r.unit();
            assert!((0.0..=1.0).contains(&u));
            acc += u;
        }
        assert!((acc / 10000.0 - 0.5).abs() < 0.02);
    }
}
