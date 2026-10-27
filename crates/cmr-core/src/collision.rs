//! Reconstruction of the collision/damage response and the stage trigger-volume
//! types (the confidently-recovered G04 pieces).
//!
//! Static scenery collision is produced earlier in Mechanics by
//! `FUN_0043d5d0 -> FUN_0043e640 -> FUN_0043d760`. The later per-car step
//! `FUN_00410a60` delegates the crash path to `FUN_004115d0` once the
//! out-of-control flag `car+0x680` is set. The cleanly-transcribable Q16.16
//! damage/reset arithmetic from that downstream path is reconstructed here:
//!
//! - **Impact strength** (`FUN_0041f630`): the contact impulse is accumulated
//!   into a vec3 (`car+0x3cc..`); its magnitude is thresholded at `0x2624` and
//!   the excess is scaled by `0xf536` and clamped to `1.0`. The same 0..1 impact
//!   drives the pitched impact sound, the force-feedback rumble, and the damage.
//! - **Structural crash counter** (`FUN_004115d0`): an integer `car+0x678`
//!   increments once per contacting tick, clamps at `100`, and latches the
//!   "totalled" flag `car+0x67c` on reaching the cap.
//! - **Per-region mesh damage** (`FUN_0041f880`): each vertex within unit
//!   distance of the contact accrues `w = (1 - dist^2) * impact` into a per-region
//!   slot clamped to `1.0`.
//! - **Panel-shed damage levels** (`FUN_0044a770`): a region advances one damage
//!   level when its accumulated damage crosses `0x3333` (0->1) or `0x6666` (1->2).
//! - **Collision materials** (`FUN_004756f0`): the recovered material ids for the
//!   crash-reaction sound selection.
//! - **Stage trigger volumes** (`FUN_00403a00`): the recovered trigger-type ids,
//!   including the out-of-bounds "kill" volume and the surface switches.
//!
//! See evidence `rea.official-eu.sim-collision`.
//!
//! Observations: the formulas, constants, and type ids above. Inferences: the
//! two coexisting damage representations (coarse scalar + fine per-region); the
//! material/trigger id meanings. The orientation re-righting blend and signed
//! `car+0x75c` countdown are implemented below. The reset *trigger* remains an
//! external input (`ctrl+0x3ec` is read by the recovered crash path); no
//! position-snap, time-penalty, or damage->performance mapping is invented.

use crate::fixed_point::{Fixed16, Vec3};

/// Minimum impulse magnitude that registers as an impact (`0x2624` ~ 0.149).
pub const IMPACT_THRESHOLD: Fixed16 = Fixed16::from_raw(0x2624);

/// Slope mapping the excess impulse to the 0..1 impact strength (`0xf536` ~ 0.958).
pub const IMPACT_SLOPE: Fixed16 = Fixed16::from_raw(0xf536);

/// Structural crash-counter cap; reaching it latches the "totalled" flag.
pub const CRASH_COUNTER_MAX: u32 = 100;

/// Region-damage threshold to advance from damage level 0 to 1 (`0x3333` ~ 0.20).
pub const DAMAGE_LEVEL_1_THRESHOLD: Fixed16 = Fixed16::from_raw(0x3333);

/// Region-damage threshold to advance from damage level 1 to 2 (`0x6666` ~ 0.40).
pub const DAMAGE_LEVEL_2_THRESHOLD: Fixed16 = Fixed16::from_raw(0x6666);

/// Collision-severity threshold below which the crash reaction uses the
/// material-specific / "DAMAGE_1" sound bank (`0x4ccc` ~ 0.30), from
/// `FUN_004756f0`. The higher tier-2/tier-3 boundary was not recovered.
pub const SEVERITY_TIER1_MAX: Fixed16 = Fixed16::from_raw(0x4ccc);

/// Impact strength from the accumulated contact impulse, reproducing
/// `FUN_0041f630`: magnitude below [`IMPACT_THRESHOLD`] is no impact (`None`);
/// otherwise `clamp((mag - threshold) * slope, 0, 1)`.
pub fn collision_impact(impulse: Vec3) -> Option<Fixed16> {
    let magnitude = impulse.length();
    if magnitude < IMPACT_THRESHOLD {
        return None;
    }
    let impact = magnitude.sub(IMPACT_THRESHOLD).mul(IMPACT_SLOPE);
    Some(impact.clamp(Fixed16::ZERO, Fixed16::ONE))
}

/// One structural-damage tick (`FUN_004115d0`): increment the crash counter
/// while a lower contact point is touching, clamp at [`CRASH_COUNTER_MAX`], and
/// report whether the car is now totalled (the `car+0x67c` latch).
pub fn accrue_crash_tick(counter: u32) -> (u32, bool) {
    let next = counter.saturating_add(1).min(CRASH_COUNTER_MAX);
    (next, next >= CRASH_COUNTER_MAX)
}

/// Deformation weight for a mesh vertex at squared distance `dist_sq` (Q16.16,
/// unit-normalised) from the contact point under a given `impact`, reproducing
/// `FUN_0041f880`: `w = (1 - dist^2) * impact` for vertices within unit distance,
/// zero otherwise.
pub fn deform_weight(dist_sq: Fixed16, impact: Fixed16) -> Fixed16 {
    if dist_sq >= Fixed16::ONE {
        return Fixed16::ZERO;
    }
    Fixed16::ONE.sub(dist_sq).mul(impact)
}

/// Accumulate a deform weight into a per-region damage slot, clamped to `1.0`
/// (the `FUN_0041f880` region accumulator `DAT_0050e0d8`).
pub fn accumulate_region_damage(slot: Fixed16, weight: Fixed16) -> Fixed16 {
    slot.add(weight).min(Fixed16::ONE)
}

/// The damage level a region reaches (`FUN_0044a770`): advances one level when
/// its accumulated damage crosses the matching threshold. Levels are `0..=2`.
pub fn next_damage_level(region_damage: Fixed16, current_level: u8) -> u8 {
    match current_level {
        0 if region_damage > DAMAGE_LEVEL_1_THRESHOLD => 1,
        1 if region_damage > DAMAGE_LEVEL_2_THRESHOLD => 2,
        _ => current_level,
    }
}

/// Collision materials selected by the hit object's material id in the crash
/// reaction (`FUN_004756f0`); only these ids were recovered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CollisionMaterial {
    /// Material id `5`.
    Metal,
    /// Material id `8`.
    HayBale,
    /// Material id `0xd`.
    Plastic,
    /// Material id `0x10`.
    Rubber,
}

impl CollisionMaterial {
    /// Map a recovered material id to its material, or `None` for an id outside
    /// the recovered set.
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            5 => Some(CollisionMaterial::Metal),
            8 => Some(CollisionMaterial::HayBale),
            0xd => Some(CollisionMaterial::Plastic),
            0x10 => Some(CollisionMaterial::Rubber),
            _ => None,
        }
    }
}

/// Stage trigger-volume types dispatched by `FUN_00403a00` on the car's current
/// track node (`car+0x750`). Only the behaviourally-identified types are named;
/// the surface switches are classified by [`surface_switch_index`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriggerType {
    /// Type `0`: a split / time checkpoint (`FUN_00444380`).
    SplitCheckpoint,
    /// Type `1`: the finish line.
    Finish,
    /// Type `2`: a trigger sound (also the default).
    Sound,
    /// Type `6`: a "kill" volume - forces the totalled latch `car+0x67c`.
    Kill,
    /// Type `0x15`: out-of-bounds - arms the `car+0x75c` countdown (game mode 8).
    OutOfBounds,
}

impl TriggerType {
    /// Classify a recovered trigger-type id, or `None` for one not behaviourally
    /// identified (including the surface switches `7..=0xB`, see
    /// [`surface_switch_index`]).
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(TriggerType::SplitCheckpoint),
            1 => Some(TriggerType::Finish),
            2 => Some(TriggerType::Sound),
            6 => Some(TriggerType::Kill),
            0x15 => Some(TriggerType::OutOfBounds),
            _ => None,
        }
    }
}

/// First trigger-type id that selects a surface/region switch (`FUN_00403e70(0)`).
pub const SURFACE_SWITCH_FIRST: u8 = 7;

/// Last trigger-type id that selects a surface/region switch (`FUN_00403e70(4)`).
pub const SURFACE_SWITCH_LAST: u8 = 0xB;

/// The surface-switch argument `0..=4` for trigger-type ids `7..=0xB`
/// (`FUN_00403a00` -> `FUN_00403e70(id - 7)`), or `None` for other ids.
pub fn surface_switch_index(trigger_type: u8) -> Option<u8> {
    if (SURFACE_SWITCH_FIRST..=SURFACE_SWITCH_LAST).contains(&trigger_type) {
        Some(trigger_type - SURFACE_SWITCH_FIRST)
    } else {
        None
    }
}

/// Result of one signed `car+0x75c` out-of-bounds countdown tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutOfBoundsTick {
    pub timer: i16,
    pub timeout_sound: bool,
    pub totalled: bool,
    pub finish: bool,
}

fn high_i16(product: i64) -> i16 {
    (product >> 32) as i16
}

fn q16_mul(left: i32, right: i32) -> i32 {
    Fixed16::from_raw(left).mul(Fixed16::from_raw(right)).raw()
}

/// Advance the signed out-of-bounds timer in `FUN_00407690`. Positive values
/// count down and wrap to `-1`; negative values count toward the terminal
/// threshold. The timeout sound is requested only at the exact positive threshold
/// in game state 5, matching the `FUN_0041d270` call gate.
#[must_use]
pub fn advance_out_of_bounds(
    timer: i16,
    slow_motion: bool,
    state_is_five: bool,
) -> OutOfBoundsTick {
    if timer > 0 {
        let scale = if slow_motion { 0x320000 } else { 0x190000 };
        let timeout = high_i16(scale * 0xc0000).wrapping_sub(high_i16(scale * 0x20000));
        let next = timer.wrapping_sub(1);
        return OutOfBoundsTick {
            timer: if next == 0 { -1 } else { next },
            timeout_sound: timer == timeout && state_is_five,
            totalled: false,
            finish: false,
        };
    }
    if timer == 0 {
        return OutOfBoundsTick {
            timer,
            timeout_sound: false,
            totalled: false,
            finish: false,
        };
    }
    let scale = if slow_motion {
        0x320000_i64
    } else {
        0x190000_i64
    };
    let terminal = high_i16(scale * 0x30000).wrapping_add(high_i16(scale * 0x60000));
    let timer = timer.wrapping_sub(1);
    let totalled = timer < terminal.wrapping_neg();
    OutOfBoundsTick {
        timer,
        timeout_sound: false,
        totalled,
        finish: totalled,
    }
}

/// Mutable control/basis state consumed by the crash reset path of
/// `FUN_004115d0`. Row names preserve their native offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrashResetInput {
    pub alternate: bool,
    pub countdown: u8,
    pub row_258: [i32; 3],
    pub row_264: [i32; 3],
    pub row_270: [i32; 3],
    pub normal_324: [i32; 3],
}

/// Recovered reorientation and reset-completion effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrashResetOutcome {
    pub row_258: [i32; 3],
    pub row_264: [i32; 3],
    pub row_270: [i32; 3],
    pub countdown: u8,
    pub active: bool,
    pub totalled: bool,
}

/// Apply the reset-triggered orientation blend and countdown. This does not
/// invent a trigger producer: `ctrl+0x3ec` remains an external recovered input.
#[must_use]
pub fn apply_crash_reset(input: CrashResetInput) -> CrashResetOutcome {
    use crate::compiled_car_initializer::normalize_compiled_lane_vector;
    use crate::drivetrain::{cross_q16, project_and_normalize};

    let (target, other, normal) = if input.alternate {
        (
            input.row_264,
            input.row_270,
            input.normal_324.map(i32::wrapping_neg),
        )
    } else {
        (input.row_270, input.row_258, input.normal_324)
    };
    let delta = std::array::from_fn(|index| normal[index].wrapping_sub(target[index]));
    let distance = crate::legacy_math::sqrt_q16(delta.iter().fold(0_i32, |sum, value| {
        sum.wrapping_add(q16_mul(*value, *value))
    }) as u32) as i32;
    let target = if distance < 0x28f {
        normal
    } else {
        let direction = normalize_compiled_lane_vector(delta);
        normalize_compiled_lane_vector(std::array::from_fn(|index| {
            target[index].wrapping_add(q16_mul(direction[index], 0x28f))
        }))
    };
    let other = project_and_normalize(other, target);
    let cross = normalize_compiled_lane_vector(cross_q16(target, other));
    let (row_258, row_264, row_270) = if input.alternate {
        (cross, target, other)
    } else {
        (other, cross, target)
    };
    let countdown = input.countdown.wrapping_sub(1);
    let active = countdown != 0;
    CrashResetOutcome {
        row_258,
        row_264,
        row_270,
        countdown,
        active,
        totalled: !active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_ignores_below_threshold_and_clamps_above() {
        // Magnitude below the threshold: no impact.
        let small = Vec3::new(Fixed16::from_raw(0x1000), Fixed16::ZERO, Fixed16::ZERO);
        assert_eq!(collision_impact(small), None);
        // A unit impulse: a mid-range impact, below full scale.
        let unit = Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO);
        let impact = collision_impact(unit).expect("unit impulse registers");
        assert!(impact > Fixed16::ZERO && impact < Fixed16::ONE);
        // A large impulse clamps to full scale.
        let big = Vec3::new(Fixed16::from_int(10), Fixed16::ZERO, Fixed16::ZERO);
        assert_eq!(collision_impact(big), Some(Fixed16::ONE));
        // Just above the threshold (0x3000 round-trips exactly through mul+sqrt):
        // a small but positive impact.
        let just_over = Vec3::new(Fixed16::from_raw(0x3000), Fixed16::ZERO, Fixed16::ZERO);
        let low = collision_impact(just_over).expect("above threshold registers");
        assert!(low > Fixed16::ZERO && low < impact);
    }

    #[test]
    fn crash_counter_clamps_and_latches_totalled() {
        assert_eq!(accrue_crash_tick(0), (1, false));
        assert_eq!(accrue_crash_tick(98), (99, false));
        assert_eq!(accrue_crash_tick(99), (100, true));
        // Already at the cap stays capped and totalled.
        assert_eq!(accrue_crash_tick(100), (100, true));
    }

    #[test]
    fn deform_weight_falls_off_and_accumulates_clamped() {
        let impact = Fixed16::ONE;
        // At the contact point (dist^2 = 0): full weight = impact.
        assert_eq!(deform_weight(Fixed16::ZERO, impact), Fixed16::ONE);
        // Halfway (dist^2 = 0.5): half weight.
        assert_eq!(
            deform_weight(Fixed16::from_raw(0x8000), impact),
            Fixed16::from_raw(0x8000)
        );
        // At or beyond unit distance: no weight.
        assert_eq!(deform_weight(Fixed16::ONE, impact), Fixed16::ZERO);
        // Region damage accumulates and clamps at 1.0.
        let slot = accumulate_region_damage(Fixed16::from_raw(0xc000), Fixed16::from_raw(0x8000));
        assert_eq!(slot, Fixed16::ONE);
    }

    #[test]
    fn damage_level_advances_one_step_per_threshold() {
        // Below the first threshold stays at level 0.
        assert_eq!(next_damage_level(Fixed16::from_raw(0x2000), 0), 0);
        // Past the first threshold: level 1 (and only one step).
        assert_eq!(next_damage_level(Fixed16::from_raw(0x4000), 0), 1);
        // From level 1, past the second threshold: level 2.
        assert_eq!(next_damage_level(Fixed16::from_raw(0x7000), 1), 2);
        // From level 1, still below the second threshold: stays at 1.
        assert_eq!(next_damage_level(Fixed16::from_raw(0x4000), 1), 1);
        // Level 2 is the cap.
        assert_eq!(next_damage_level(Fixed16::ONE, 2), 2);
    }

    #[test]
    fn out_of_bounds_countdown_preserves_signed_thresholds() {
        assert_eq!(
            advance_out_of_bounds(250, false, true),
            OutOfBoundsTick {
                timer: 249,
                timeout_sound: true,
                totalled: false,
                finish: false,
            }
        );
        assert_eq!(advance_out_of_bounds(1, false, false).timer, -1);
        assert_eq!(advance_out_of_bounds(0, false, false).timer, 0);
        assert!(!advance_out_of_bounds(-224, false, false).totalled);
        let expired = advance_out_of_bounds(-225, false, false);
        assert!(expired.totalled && expired.finish);
        assert!(advance_out_of_bounds(-450, true, false).totalled);
    }

    #[test]
    fn crash_reset_blends_rows_and_latches_only_when_countdown_expires() {
        let input = CrashResetInput {
            alternate: false,
            countdown: 2,
            row_258: [0x10000, 0, 0],
            row_264: [0, 0, 0x10000],
            row_270: [0, 0x10000, 0],
            normal_324: [0, 0x10000, 0],
        };
        let outcome = apply_crash_reset(input);
        assert_eq!(outcome.row_258, [65522, 0, 0]);
        assert_eq!(outcome.row_264, [0, 0, -65535]);
        assert_eq!(outcome.row_270, input.row_270);
        assert_eq!(outcome.countdown, 1);
        assert!(outcome.active && !outcome.totalled);

        let outcome = apply_crash_reset(CrashResetInput {
            alternate: true,
            countdown: 1,
            normal_324: [0, 0, -0x10000],
            ..input
        });
        assert_eq!(outcome.row_258, [-65535, 0, 0]);
        assert_eq!(outcome.row_264, input.row_264);
        assert_eq!(outcome.row_270, [0, 65522, 0]);
        assert!(!outcome.active && outcome.totalled);
    }

    #[test]
    fn material_and_trigger_ids_map_to_the_recovered_set() {
        assert_eq!(
            CollisionMaterial::from_id(5),
            Some(CollisionMaterial::Metal)
        );
        assert_eq!(
            CollisionMaterial::from_id(0x10),
            Some(CollisionMaterial::Rubber)
        );
        assert_eq!(CollisionMaterial::from_id(0), None);

        assert_eq!(TriggerType::from_id(0), Some(TriggerType::SplitCheckpoint));
        assert_eq!(TriggerType::from_id(6), Some(TriggerType::Kill));
        assert_eq!(TriggerType::from_id(0x15), Some(TriggerType::OutOfBounds));
        // Surface-switch ids are not named trigger types.
        assert_eq!(TriggerType::from_id(7), None);
    }

    #[test]
    fn surface_switch_index_covers_the_recovered_range() {
        assert_eq!(surface_switch_index(7), Some(0));
        assert_eq!(surface_switch_index(0xB), Some(4));
        assert_eq!(surface_switch_index(6), None);
        assert_eq!(surface_switch_index(0xC), None);
    }
}
