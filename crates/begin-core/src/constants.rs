//! Every tuned constant from the original engine, with its asm location in
//! `begin2_annotated.asm` (AI_AND_COMBAT.md §12.9). Daniel's binary patches
//! are exposed through [`Tuning`] and default to his tuned values.

// --- movement (§1) ---
pub const UNITS_PER_WARP_PER_CYCLE: f64 = 100.0; // flt_35F11 101691
pub const SUBSTEPS: usize = 20; // dbl_332AE=20.0; uniform 20×5.0 (documented fix of 19×5 quirk)
pub const SUBSTEP_SCALE: f64 = 5.0; // dbl_32DBA

// --- helm (§5) ---
pub const WARP_SNAP: f64 = 0.01; // dbl_32DAA
pub const MIN_WARP: f64 = -1.0; // dbl_32DA2 (reverse)

// --- power / drives (§5.4, §6) ---
pub const TEMP_FLOOR: f64 = 12.0; // flt_32FCF 99291
pub const TEMP_LIMIT: f64 = 40.0; // flt_32FD3 99293
pub const TEMP_RATE: f64 = 4.0; // flt_32D40 99201
pub const SHIELD_REGEN_MIN: f64 = 0.1; // dbl_32B48 99180
pub const SHIELD_REINFORCE_COST: f64 = 4.0; // ×4 when reinforced (§6)
pub const LIFE_SUPPORT_FAILURES_FATAL: i32 = 3; // sub_F64C 24917

// --- weapons (§7, §9, §10) ---
pub const PHASER_HALF: f64 = 0.5; // flt_32CBE 99196
pub const BANK_BEARS_CONE: f64 = 22.5; // flt_35FA5 101705
pub const SPREAD_DEFAULT: f64 = 45.0; // flt_35FA9 101706
pub const SPREAD_MIN: f64 = 10.0;
pub const DAMAGE_CAP: f64 = 8000.0; // flt_3327A 99325
pub const SHIELD_ABSORB_QUARTER: f64 = 0.25; // flt_3327E 99326
pub const SPLASH_RADIUS_PER_DAMAGE: f64 = 10.0; // flt_32EEA 99277
pub const CLOAK_REFLECT_SCALE: f64 = 0.005; // dbl_32EEE 99278
pub const DESTRUCT_COUNTDOWN: f64 = 5.0; // flt_35F6E 101702
pub const BLAST_RADIUS_SCALE: f64 = 50.0; // flt_35F19 101693
pub const TRACTOR_RANGE: f64 = 1000.0; // flt_35FF4 / flt_347C0
pub const DOCK_RANGE: f64 = 1000.0; // flt_347C0 100261
pub const TRACTOR_DIST_SCALE: f64 = 1000.0; // flt_332AA 99329
pub const BEAM_SHIELDS_MUST_BE_DOWN: bool = true; // sub_FFCC

// --- endgame (§13) ---
pub const ENDGAME_THRESHOLDS: [f64; 5] = [15.0, 5.0, 2.0, -2.0, -5.0]; // flt_3339B..dbl_333AF 99355-59
pub const STRENGTH_DIVISOR: f64 = 1000.0; // flt_333DD 99398

// --- AI (§12.9) ---
pub const AI_STATION_KEEP_OUTER: f64 = 1.5; // flt_35CBC 101660
pub const AI_PURSUE_WARP_BONUS: f64 = 2.0; // flt_35CC0 101661
pub const AI_SCORE_HALF: f64 = 0.5; // flt_35F05 101685
pub const AI_FANATIC_CREW: f64 = 6.0; // flt_35F1D 101693
pub const AI_FANATIC_RAM: f64 = 0.9; // dbl_35F44 101696
pub const AI_RETREAT_RANGE_BASE: f64 = 1.2; // dbl_35F4C
pub const AI_RETREAT_RANGE_SCALE: f64 = 1e5; // flt_35F54
pub const AI_RETREAT_DAMAGE: f64 = 0.75; // flt_35F58 101700
pub const AI_RETREAT_RECOVER: f64 = 0.5; // sub_1DB12 56572
pub const AI_DRIVE_RATIO: f64 = 0.25; // flt_35FAD 101708
pub const AI_BATTERY_CRAWL: f64 = 0.25; // flt_35FAD twin use
pub const AI_TEMP_PANIC_ENTER: f64 = 0.85; // dbl_35FB9
pub const AI_TEMP_PANIC_EXIT: f64 = 0.4; // dbl_35FC1
pub const AI_TEMP_NORM_BASE: f64 = 12.0; // flt_35FB1
pub const AI_TEMP_NORM_RANGE: f64 = 28.0; // flt_35FB5
pub const AI_APPROACH_CONE: f64 = 10.0; // flt_35FC9 101715
pub const AI_OVERHEAT_REFLEX: f64 = 38.0; // flt_35FF8 101722
pub const AI_AGGR_FIRE_BIAS: f64 = 0.1; // dbl_35FFC 101723
pub const AI_STALE_CONTACT_MULT: f64 = 0.1; // dbl_35FFC twin
pub const AI_JINK_LEAD_OFFSET: f64 = 20.0; // flt_36004 101725
pub const AI_HOMING_CORRIDOR: f64 = 1.1; // dbl_36008 101726
pub const AI_WEAVE_AGGR_BASE: f64 = 1.1; // dbl_36008 twin
pub const AI_PROBE_MISSION_DIV: f64 = 3.0; // flt_36010 101728
pub const AI_SIDESTEP: f64 = 90.0; // flt_36053/dbl_36057
pub const AI_DEFEND_RADIUS: f64 = 3000.0; // flt_360C8 101739
pub const AI_STANDOFF_BASE: f64 = 2e4; // flt_3654F
pub const AI_STANDOFF_AGGR: f64 = 1e4; // flt_3654B
pub const AI_STANDOFF_JITTER: f64 = 5e3; // flt_36553
pub const AI_ESCORT_TOLERANCE: f64 = 500.0; // flt_36580 102462
pub const AI_APPROACH_WARP_AGGR: f64 = 4.0; // flt_36640 102469
pub const AI_WEAVE_THREAT_RANGE: f64 = 25000.0; // flt_36648
pub const AI_WEAVE_DIST_DIV: f64 = 833.33; // dbl_3664C
pub const AI_WEAVE_AMP_MAX: f64 = 31.0; // flt_36654
pub const AI_PD_WINDOW: f64 = 2.5; // flt_3665C 102475
pub const AI_DEFEND_SCORE_RADIUS: f64 = 50000.0; // flt_36660 102476
pub const AI_SCORE_BAND_WIDTH: f64 = 2000.0; // flt_36664 102477
pub const AI_TARGET_STICKINESS: f64 = 1.25; // flt_36668 102478
pub const AI_TORP_PRESSURE_CAP: i32 = 90; // brain+8Ah cap
/// Distance-band score multipliers, 2000-unit bands (ds:0xA65C, asm 101584)
pub const AI_DISTANCE_BANDS: [f64; 10] =
    [3.0, 1.9, 1.75, 1.5, 1.4, 1.3, 1.2, 1.15, 1.125, 1.1];
pub const AI_FAR_BAND_MULT: f64 = 0.5; // ≥ 20000

/// Config knobs Daniel patched in the shipped binary (`begin2/notes.txt`).
#[derive(Debug, Clone)]
pub struct Tuning {
    /// Phaser damage multiplier: 45.0 stock begin2, 16.3125 for "begin1 feel".
    /// (`phaserDamMult`, asm 99290)
    pub phaser_dam_mult: f64,
    /// Splash damage multiplier (`SplashDamMult` 5.0, asm 99276); scale by
    /// 0.3625 together with phaser_dam_mult=16.3125 for begin1 feel.
    pub splash_dam_mult: f64,
    /// Ship self-destruct yield divisor (`ooDestructDamage`, asm 99239).
    /// Stock 9.0; Daniel's tuned value 14.0 ("seems to work nicely").
    pub oo_destruct_damage: f64,
    /// Server-side planar lock: force mark/z to 0 for all objects.
    pub planar_lock: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            phaser_dam_mult: 45.0,
            splash_dam_mult: 5.0,
            oo_destruct_damage: 14.0,
            planar_lock: false,
        }
    }
}

impl Tuning {
    /// The "begin1 feel" profile from notes.txt.
    pub fn begin1() -> Self {
        Tuning {
            phaser_dam_mult: 16.3125,
            splash_dam_mult: 5.0 * 0.3625,
            oo_destruct_damage: 14.0,
            planar_lock: false,
        }
    }
}
