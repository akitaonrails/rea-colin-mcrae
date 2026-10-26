//! Reconstruction of the vehicle drivetrain and engine model.
//!
//! The 25 Hz Mechanics update drives a per-car vehicle-dynamics dispatcher
//! (`FUN_00407690`) whose callees compute the engine and gearbox state in the
//! per-car control struct (0x464 stride, base `DAT_005fffc8`). This module
//! reconstructs the confidently-recovered Q16.16 formulas from those callees:
//!
//! - **Gearbox** (`FUN_0041b810` automatic): current gear is a signed byte at
//!   `+0x43a`, `0..=6` forward and `7` reverse; a gear whose ratio is zero is
//!   neutral. Auto-shift compares RPM against per-gear thresholds scaled by the
//!   redline: upshift when `rpm > upshift[gear] * redline`, downshift when
//!   `rpm < downshift[gear] * redline` (the recovered `ratio * redline >> 16`).
//! - **RPM / rev limiter** (`FUN_0040d0c0`): RPM lives at `+0x2e0`, redline at
//!   `+0x15c`. Over-redline RPM is clamped and produces a cut
//!   `min(margin, 1.0) * 0x51eb` with a limiter flag; negative RPM clamps to 0.
//!   The re-enable hysteresis constant is `0xcccd` (~0.8).
//! - **Engine torque** (`FUN_0040ce40`): `torque = throttle * maxTorque -
//!   engBrake * rpm^2` (each a `>>16` Q16.16 multiply); under the limiter the
//!   torque is forced to `-cut`. Drive torque to the wheels is
//!   `torque * ratio[gear] * finalDrive`, then distributed by the recovered
//!   centre-differential split ([drive_torque_split]).
//! - **Actuator smoothing** (`FUN_004088c0`): each actuator slews toward its
//!   demand, snapping when within one unit (`0x10000`) else stepping by a unit,
//!   with a dead-zone that zeroes magnitudes below `0x7ae`.
//! - **Display RPM** (`FUN_00488d60`, feeds the tachometer and engine audio):
//!   a normalised engine-load value maps to `norm * 6500 + 2000`, clamped to
//!   `[2000, 0x2134]` (8500).
//!
//! Values are signed Q16.16 ([`crate::fixed_point::Fixed16`]); `0x10000` = 1.0.
//! See evidence `rea.official-eu.sim-drivetrain`.
//!
//! Observations: the formulas, constants and field offsets above, read from the
//! decompiled callees. Inferences: gear `7` is reverse and a zero ratio is
//! neutral; the display-RPM literals map engine load to a 2000..8500 tach.
//! The per-wheel arrays are now distinguished: raw speeds at `+0x2a8[4]`,
//! smoothed/current speeds at `+0x2b8[4]`, and smoothing gains at `+0x2c8[4]`.
//! Unknowns (not reproduced): the slip-force gate and per-wheel drive
//! application, the remaining per-wheel accumulation, parts of the
//! torque-to-velocity integration, and the loader that copies per-gear ratio
//! tables into the control struct.

use crate::fixed_point::Fixed16;

/// Coasting drag torque (`0x1999` ~ 0.1 in Q16.16) that `FUN_0040d810`
/// substitutes for the engine torque when the car is in gear with neither the
/// throttle nor the brake pressed. The centre split still applies to it.
pub const COAST_DRAG_TORQUE: Fixed16 = Fixed16::from_raw(0x1999);

/// The recovered centre-differential drive split of `FUN_0040d810`.
///
/// The engine torque is scaled by the per-car-type front/rear factors
/// (`DAT_0050e1a4`/`DAT_0050e1a8[type * 0x334]`, runtime game data) and then
/// distributed by the centre split `+0x170` (`0` = all rear, `0x10000` = all
/// front):
///
/// ```text
/// front = ((engine_torque * front_factor) >> 16) * center_split      >> 16
/// rear  = ((engine_torque * rear_factor)  >> 16) * (1 - center_split) >> 16
/// ```
///
/// A handbrake (`+0x30c`) adds its recovered extra torque (`+0x2f4`) to the rear
/// pair only. The per-wheel application loop (sign from the wheel-speed
/// direction plus the per-car-type wheel constant `DAT_0050e18c`) is the next
/// stage and is not reproduced here. The split is used as its raw signed Q16.16
/// value; the shipped arithmetic does not clamp it to `[0, 1]`.
#[must_use]
pub fn drive_torque_split(
    engine_torque: Fixed16,
    center_split: Fixed16,
    front_factor: Fixed16,
    rear_factor: Fixed16,
    handbrake_torque: Option<Fixed16>,
) -> (Fixed16, Fixed16) {
    let remainder = Fixed16::ONE.sub(center_split);
    let front = engine_torque.mul(front_factor).mul(center_split);
    let mut rear = engine_torque.mul(rear_factor).mul(remainder);
    if let Some(extra) = handbrake_torque {
        rear = rear.add(extra);
    }
    (front, rear)
}

/// Wheel-speed pair divergence (`0x1999` ~ 0.1 Q16.16) above which the
/// end-of-pass equalizer `FUN_00410230` averages an axle's left/right speeds.
pub const WHEEL_PAIR_EQUALIZE_THRESHOLD: i32 = 0x1999;

/// The recovered axle wheel-speed equalizer `FUN_00410230`.
///
/// The shipped force pass ends by comparing each front/rear wheel-speed pair
/// (`+0x2a8`/`+0x2ac` and `+0x2b0`/`+0x2b4`); when the pair differs by more
/// than [`WHEEL_PAIR_EQUALIZE_THRESHOLD`] both wheels are set to
/// `(left + right) * 0x8000 >> 16` (the truncated average). Differences at or
/// below the threshold are left untouched.
#[must_use]
pub fn equalize_wheel_speeds(left: Fixed16, right: Fixed16) -> (Fixed16, Fixed16) {
    let difference = left.raw().wrapping_sub(right.raw()).wrapping_abs();
    if difference <= WHEEL_PAIR_EQUALIZE_THRESHOLD {
        return (left, right);
    }
    let sum = Fixed16::from_raw(left.raw().wrapping_add(right.raw()));
    let average = sum.mul(Fixed16::from_raw(0x8000));
    (average, average)
}

/// The coasting split: [`COAST_DRAG_TORQUE`] distributed directly by the centre
/// split (the recovered in-gear, no-throttle/no-brake branch of
/// `FUN_0040d810`). This branch bypasses the per-car front/rear factors used by
/// the driven path.
#[must_use]
pub fn coast_torque_split(center_split: Fixed16) -> (Fixed16, Fixed16) {
    drive_torque_split(
        COAST_DRAG_TORQUE,
        center_split,
        Fixed16::ONE,
        Fixed16::ONE,
        None,
    )
}

/// Highest forward gear index (`0..=6` forward).
pub const MAX_FORWARD_GEAR: i8 = 6;

/// Gear byte value that selects reverse (`'\a'`).
pub const REVERSE_GEAR: i8 = 7;

/// Rev-limiter re-enable hysteresis fraction (`0xcccd` ~ 0.8).
pub const REV_LIMITER_HYSTERESIS: Fixed16 = Fixed16::from_raw(0xcccd);

/// Rev-limiter cut coefficient (`0x51eb` ~ 0.32), applied to the over-redline
/// margin.
pub const REV_LIMITER_CUT_COEFF: Fixed16 = Fixed16::from_raw(0x51eb);

/// Actuator dead-zone magnitude (`0x7ae`): smaller magnitudes snap to zero.
pub const ACTUATOR_DEADZONE: i32 = 0x7ae;

/// Automatic-gearbox power threshold (`0xb333` ~ 0.70): above it, high-power
/// cars skip second gear on launch.
pub const GEAR_SKIP_POWER_THRESHOLD: Fixed16 = Fixed16::from_raw(0xb333);

/// Minimum and maximum display (tachometer) RPM.
pub const DISPLAY_RPM_MIN: i32 = 2000;
pub const DISPLAY_RPM_MAX: i32 = 0x2134; // 8500
/// Span added on top of the minimum by a full engine-load value.
pub const DISPLAY_RPM_SPAN: i32 = 6500;

/// Whether a gear is neutral, i.e. its ratio is zero (`ctl[0x1d0 + gear*4] == 0`).
pub fn is_neutral(gear_ratio: Fixed16) -> bool {
    gear_ratio == Fixed16::ZERO
}

/// Whether a gear index selects reverse.
pub fn is_reverse(gear: i8) -> bool {
    gear == REVERSE_GEAR
}

/// The RPM threshold for a shift: `threshold_ratio * redline` (the recovered
/// `ctl[..+gear*4] * ctl+0x15c >> 16`).
pub fn shift_threshold_rpm(threshold_ratio: Fixed16, redline: Fixed16) -> Fixed16 {
    threshold_ratio.mul(redline)
}

/// Automatic upshift test: RPM above the per-gear upshift threshold.
pub fn should_upshift(rpm: Fixed16, upshift_ratio: Fixed16, redline: Fixed16) -> bool {
    rpm > shift_threshold_rpm(upshift_ratio, redline)
}

/// Automatic downshift test: RPM below the per-gear downshift threshold.
pub fn should_downshift(rpm: Fixed16, downshift_ratio: Fixed16, redline: Fixed16) -> bool {
    rpm < shift_threshold_rpm(downshift_ratio, redline)
}

/// Result of the rev limiter for a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevLimit {
    /// RPM after clamping to `[0, redline]`.
    pub rpm: Fixed16,
    /// Whether the limiter engaged this step.
    pub active: bool,
    /// The torque cut applied while limiting (`0` when inactive).
    pub cut: Fixed16,
}

/// Apply the recovered rev limiter (`FUN_0040d0c0`): clamp RPM into
/// `[0, redline]`, and when over the redline report the cut
/// `min(margin, 1.0) * 0x51eb`.
pub fn apply_rev_limiter(rpm: Fixed16, redline: Fixed16) -> RevLimit {
    if rpm.raw() < 0 {
        return RevLimit {
            rpm: Fixed16::ZERO,
            active: false,
            cut: Fixed16::ZERO,
        };
    }
    if rpm > redline {
        let margin = rpm.sub(redline);
        let capped = if margin > Fixed16::ONE {
            Fixed16::ONE
        } else {
            margin
        };
        return RevLimit {
            rpm: redline,
            active: true,
            cut: capped.mul(REV_LIMITER_CUT_COEFF),
        };
    }
    RevLimit {
        rpm,
        active: false,
        cut: Fixed16::ZERO,
    }
}

/// Recovered engine torque (`FUN_0040ce40`):
/// `throttle * max_torque - eng_brake * rpm^2`.
pub fn engine_torque(
    throttle_demand: Fixed16,
    max_torque: Fixed16,
    eng_brake: Fixed16,
    rpm: Fixed16,
) -> Fixed16 {
    let drive = throttle_demand.mul(max_torque);
    let braking = eng_brake.mul(rpm.mul(rpm));
    drive.sub(braking)
}

/// Drive torque delivered to the wheels: `engine_torque * ratio * final_drive`
/// (the recovered two-stage `>>16` multiply, before the four-wheel split).
pub fn drive_torque(engine_torque: Fixed16, gear_ratio: Fixed16, final_drive: Fixed16) -> Fixed16 {
    engine_torque.mul(gear_ratio).mul(final_drive)
}

/// Apply the recovered actuator dead-zone: magnitudes below [`ACTUATOR_DEADZONE`]
/// snap to zero.
pub fn apply_deadzone(value: Fixed16) -> Fixed16 {
    if value.raw().abs() < ACTUATOR_DEADZONE {
        Fixed16::ZERO
    } else {
        value
    }
}

/// One actuator slew step toward `target` (`FUN_004088c0`): snap when within one
/// unit (`1.0`), otherwise step by a unit toward the target.
pub fn slew_step(current: Fixed16, target: Fixed16) -> Fixed16 {
    let delta = target.sub(current);
    if delta.raw().abs() < Fixed16::ONE.raw() {
        target
    } else if delta.raw() > 0 {
        current.add(Fixed16::ONE)
    } else {
        current.sub(Fixed16::ONE)
    }
}

/// Map a normalised engine-load value to the display (tachometer) RPM:
/// `clamp(norm * 6500 + 2000, 2000, 8500)` (`FUN_00488d60`).
pub fn display_rpm(normalized_load: Fixed16) -> i32 {
    let scaled = normalized_load
        .mul(Fixed16::from_int(DISPLAY_RPM_SPAN))
        .to_int_floor();
    (scaled + DISPLAY_RPM_MIN).clamp(DISPLAY_RPM_MIN, DISPLAY_RPM_MAX)
}

/// The slip candidate computed per wheel by `FUN_0040d810`:
/// `dot * 4 - wheel_speed` with wrapping 32-bit arithmetic. `dot` is the Q16.16
/// projection of the precomputed wheel vector onto the per-wheel axis.
#[must_use]
pub fn wheel_slip_candidate(dot: i32, wheel_speed: i32) -> i32 {
    dot.wrapping_mul(4).wrapping_sub(wheel_speed)
}

/// The sign-disagreement flag between the wheel dot product and the slip
/// candidate: set when they have opposite signs. Zero never disagrees.
#[must_use]
pub fn slip_sign_disagreement(dot: i32, slip: i32) -> bool {
    (dot > 0 && slip < 0) || (dot < 0 && slip > 0)
}

/// The slip force before the downshift: `qmul(slip, DAT_0050c600) * 4`
/// clamped symmetrically to the per-wheel limit at `ctl+0x378`.
///
/// The clamp is the exact native two-condition form: `limit < value` selects
/// `limit`, otherwise `value < -limit` selects `-limit`; values at or inside
/// the bounds pass through unchanged (including the negative-limit edge case).
#[must_use]
pub fn clamp_wheel_slip_force(slip: Fixed16, scale: Fixed16, limit: i32) -> i32 {
    let value = slip.mul(scale).raw().wrapping_mul(4);
    if limit < value {
        limit
    } else if value < -limit {
        -limit
    } else {
        value
    }
}

/// The downshift applied to the clamped slip force: `5` when the per-wheel
/// flag is set, otherwise `2` when both config dwords are zero, else `3`.
#[must_use]
pub const fn wheel_slip_shift(flag: i32, config_304: i32, config_308: i32) -> u32 {
    if flag != 0 {
        5
    } else if config_304 == 0 && config_308 == 0 {
        2
    } else {
        3
    }
}

/// Whether the slip force is applied this iteration: always for front wheels,
/// otherwise only when the handbrake (`ctl+0x30c`) is off or the slip sign
/// disagrees. When it is not applied, the wheel force is zeroed and the drive
/// split is reapplied instead.
#[must_use]
pub fn apply_wheel_slip(is_front_wheel: bool, handbrake: i32, sign_disagreement: bool) -> bool {
    is_front_wheel || handbrake == 0 || sign_disagreement
}

/// The per-wheel drive application of `FUN_0040d810`: a negative wheel speed
/// adds the axle split and the per-car-type wheel constant, a zero wheel speed
/// leaves the force unchanged, and a positive wheel speed subtracts them.
#[must_use]
pub fn apply_wheel_drive(force: i32, split: i32, wheel_constant: i32, wheel_speed: i32) -> i32 {
    if wheel_speed < 0 {
        force.wrapping_add(split).wrapping_add(wheel_constant)
    } else if wheel_speed >= 1 {
        force.wrapping_sub(split).wrapping_sub(wheel_constant)
    } else {
        force
    }
}

/// Wheel-speed integration: `speed + qmul(force, DAT_0050c5c0)` with wrapping
/// addition. The factor is the global value computed earlier in the pass.
#[must_use]
pub fn integrate_wheel_speed(speed: i32, force: Fixed16, factor: Fixed16) -> i32 {
    speed.wrapping_add(force.mul(factor).raw())
}

/// The wheel-speed zeroing gate after integration.
///
/// When `gear == 0`, `ctl+0x40c != 0`, or `ctl+0x158` equals the per-wheel
/// constant (`0x10000` for rear wheels, `0` for front wheels), a sign flip
/// from positive to negative or negative to positive zeroes the new speed.
/// Otherwise the new speed is zeroed when its sign differs from the gear
/// ratio's sign (the XOR sign-bit test). A speed of zero never counts as a
/// flip.
#[must_use]
pub fn gate_wheel_speed(
    old_speed: i32,
    new_speed: i32,
    gear: i32,
    config_40c: i32,
    config_158: i32,
    gear_ratio: i32,
    is_rear_wheel: bool,
) -> i32 {
    let flip = (old_speed > 0 && new_speed < 0) || (old_speed < 0 && new_speed > 0);
    let gate =
        gear == 0 || config_40c != 0 || config_158 == if is_rear_wheel { 0x10000 } else { 0 };
    if gate {
        if flip {
            return 0;
        }
        return new_speed;
    }
    if (gear_ratio ^ new_speed) & 0x8000_0000u32 as i32 != 0 {
        return 0;
    }
    new_speed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_split_distributes_drive_torque_front_and_rear() {
        let torque = Fixed16::from_int(2);
        let front_factor = Fixed16::from_raw(0x8000); // 0.5
        let rear_factor = Fixed16::from_raw(0x8000);

        // All front: 2.0 * 0.5 = 1.0 front, nothing rear.
        let (front, rear) =
            drive_torque_split(torque, Fixed16::ONE, front_factor, rear_factor, None);
        assert_eq!(front, Fixed16::from_int(1));
        assert_eq!(rear, Fixed16::ZERO);

        // All rear.
        let (front, rear) =
            drive_torque_split(torque, Fixed16::ZERO, front_factor, rear_factor, None);
        assert_eq!(front, Fixed16::ZERO);
        assert_eq!(rear, Fixed16::from_int(1));

        // Half/half.
        let (front, rear) = drive_torque_split(
            torque,
            Fixed16::from_raw(0x8000),
            front_factor,
            rear_factor,
            None,
        );
        assert_eq!(front, Fixed16::from_raw(0x8000));
        assert_eq!(rear, Fixed16::from_raw(0x8000));
    }

    #[test]
    fn centre_split_preserves_raw_out_of_range_values() {
        let (front, rear) = drive_torque_split(
            Fixed16::from_int(2),
            Fixed16::from_int(2),
            Fixed16::from_raw(0x8000),
            Fixed16::from_raw(0x8000),
            None,
        );
        assert_eq!(front, Fixed16::from_int(2));
        assert_eq!(rear, Fixed16::from_int(-1));

        let (front, rear) = drive_torque_split(
            Fixed16::from_int(2),
            Fixed16::from_int(-1),
            Fixed16::from_raw(0x8000),
            Fixed16::from_raw(0x8000),
            None,
        );
        assert_eq!(front, Fixed16::from_int(-1));
        assert_eq!(rear, Fixed16::from_int(2));
    }

    #[test]
    fn handbrake_torque_lands_on_the_rear_axle_only() {
        let (front, rear) = drive_torque_split(
            Fixed16::from_int(2),
            Fixed16::from_raw(0x8000),
            Fixed16::from_int(1),
            Fixed16::from_int(1),
            Some(Fixed16::from_raw(0x4000)),
        );
        assert_eq!(front, Fixed16::from_int(1));
        assert_eq!(rear, Fixed16::from_raw(0x1_4000));
    }

    #[test]
    fn coasting_uses_the_recovered_drag_torque() {
        let (front, rear) = coast_torque_split(Fixed16::from_raw(0x8000));
        // 0.1 split half/half -> 0.05 each.
        assert_eq!(front, Fixed16::from_raw(0x0ccc));
        assert_eq!(rear, Fixed16::from_raw(0x0ccc));
        assert_eq!(
            coast_torque_split(Fixed16::ZERO),
            (Fixed16::ZERO, Fixed16::from_raw(0x1999))
        );
    }

    #[test]
    fn wheel_pair_equalizer_averages_only_beyond_the_threshold() {
        // Differences at or below 0.1 leave the pair untouched.
        assert_eq!(
            equalize_wheel_speeds(fx(0x10000), fx(0x10000 + 0x1999)),
            (fx(0x10000), fx(0x11999))
        );
        // Beyond it both wheels take the truncated average.
        assert_eq!(
            equalize_wheel_speeds(fx(0x10000), fx(0x14000)),
            (fx(0x12000), fx(0x12000))
        );
        // Opposite signs average to zero.
        assert_eq!(
            equalize_wheel_speeds(fx(-0x10000), fx(0x10000)),
            (Fixed16::ZERO, Fixed16::ZERO)
        );
        // Odd sums floor, matching the recovered `*0x8000 >> 16`.
        assert_eq!(
            equalize_wheel_speeds(fx(0x10000), fx(0x18001)),
            (fx(0x14000), fx(0x14000))
        );
        // The first raw value beyond the threshold takes the averaging path.
        assert_eq!(
            equalize_wheel_speeds(Fixed16::ZERO, fx(0x199a)),
            (fx(0x0ccd), fx(0x0ccd))
        );
        // The 32-bit add wraps before the Q16.16 multiply by one half.
        assert_eq!(
            equalize_wheel_speeds(fx(i32::MAX), Fixed16::ONE),
            (fx(0xc000_7fff_u32 as i32), fx(0xc000_7fff_u32 as i32))
        );
        // `abs(INT_MIN)` remains INT_MIN under the shipped wrapping arithmetic,
        // so this exceptional difference follows the unchanged path.
        assert_eq!(
            equalize_wheel_speeds(fx(i32::MIN), Fixed16::ZERO),
            (fx(i32::MIN), Fixed16::ZERO)
        );
    }

    fn fx(raw: i32) -> Fixed16 {
        Fixed16::from_raw(raw)
    }

    #[test]
    fn gear_classification_matches_recovery() {
        assert!(is_reverse(REVERSE_GEAR));
        assert!(!is_reverse(1));
        assert!(is_neutral(Fixed16::ZERO));
        assert!(!is_neutral(Fixed16::ONE));
        assert_eq!(MAX_FORWARD_GEAR, 6);
    }

    #[test]
    fn shift_thresholds_scale_by_redline() {
        let redline = Fixed16::from_int(7000);
        // 0.9 * redline upshift point.
        let up = fx(0xe666); // ~0.9
        assert_eq!(shift_threshold_rpm(up, redline), up.mul(redline));
        assert!(should_upshift(Fixed16::from_int(6500), up, redline));
        assert!(!should_upshift(Fixed16::from_int(6000), up, redline));
        let down = fx(0x4000); // 0.25
        assert!(should_downshift(Fixed16::from_int(1000), down, redline));
        assert!(!should_downshift(Fixed16::from_int(3000), down, redline));
    }

    #[test]
    fn rev_limiter_clamps_and_cuts() {
        let redline = Fixed16::from_int(7000);
        // Under redline: pass through, no cut.
        let under = apply_rev_limiter(Fixed16::from_int(5000), redline);
        assert_eq!(under.rpm, Fixed16::from_int(5000));
        assert!(!under.active);
        assert_eq!(under.cut, Fixed16::ZERO);
        // Over redline: clamp to redline, active, cut = min(margin,1.0)*coeff.
        let over = apply_rev_limiter(Fixed16::from_int(7100), redline);
        assert_eq!(over.rpm, redline);
        assert!(over.active);
        assert_eq!(over.cut, Fixed16::ONE.mul(REV_LIMITER_CUT_COEFF)); // margin>1 capped
        // Negative RPM clamps to zero.
        let neg = apply_rev_limiter(fx(-1), redline);
        assert_eq!(neg.rpm, Fixed16::ZERO);
        assert!(!neg.active);
    }

    #[test]
    fn engine_and_drive_torque_follow_the_formula() {
        let throttle = Fixed16::ONE; // full
        let max_torque = Fixed16::from_int(400);
        let eng_brake = Fixed16::ZERO; // ignore braking term
        let rpm = Fixed16::from_int(5000);
        assert_eq!(
            engine_torque(throttle, max_torque, eng_brake, rpm),
            max_torque
        );
        // With engine braking, torque drops by eng_brake * rpm^2.
        let eb = fx(0x10); // tiny
        let t = engine_torque(throttle, max_torque, eb, Fixed16::from_int(2));
        assert_eq!(
            t,
            max_torque.sub(eb.mul(Fixed16::from_int(2).mul(Fixed16::from_int(2))))
        );
        // Drive torque is torque * ratio * final_drive.
        let ratio = Fixed16::from_int(3);
        let final_drive = Fixed16::from_int(4);
        assert_eq!(
            drive_torque(max_torque, ratio, final_drive),
            max_torque.mul(ratio).mul(final_drive)
        );
    }

    #[test]
    fn deadzone_and_slew_match_recovery() {
        // Below the dead-zone snaps to zero; at/above it passes.
        assert_eq!(apply_deadzone(fx(0x7ad)), Fixed16::ZERO);
        assert_eq!(apply_deadzone(fx(-0x7ad)), Fixed16::ZERO);
        assert_eq!(apply_deadzone(fx(0x7ae)), fx(0x7ae));
        // Slew snaps within one unit, else steps by a unit.
        assert_eq!(slew_step(Fixed16::ZERO, fx(0x8000)), fx(0x8000)); // |delta|<1.0 snaps
        assert_eq!(slew_step(Fixed16::ZERO, Fixed16::from_int(5)), Fixed16::ONE);
        assert_eq!(
            slew_step(Fixed16::from_int(5), Fixed16::ZERO),
            Fixed16::from_int(5).sub(Fixed16::ONE)
        );
    }

    #[test]
    fn display_rpm_maps_and_clamps() {
        assert_eq!(display_rpm(Fixed16::ZERO), 2000);
        assert_eq!(display_rpm(Fixed16::ONE), 8500); // 6500 + 2000
        assert_eq!(display_rpm(fx(0x8000)), 2000 + 3250); // half load
        // Over-unit load clamps to the max.
        assert_eq!(display_rpm(Fixed16::from_int(2)), DISPLAY_RPM_MAX);
    }

    #[test]
    fn slip_candidate_is_dot_times_four_minus_wheel_speed_with_wrapping() {
        assert_eq!(wheel_slip_candidate(100, 20), 380);
        assert_eq!(wheel_slip_candidate(-100, 20), -420);
        assert_eq!(wheel_slip_candidate(100, -20), 420);
        // dot * 4 wraps before the subtraction.
        assert_eq!(wheel_slip_candidate(i32::MAX, 0), -4);
        assert_eq!(wheel_slip_candidate(i32::MAX, 1), -5);
    }

    #[test]
    fn slip_sign_disagreement_requires_opposite_nonzero_signs() {
        assert!(slip_sign_disagreement(1, -1));
        assert!(slip_sign_disagreement(-1, 1));
        assert!(!slip_sign_disagreement(1, 1));
        assert!(!slip_sign_disagreement(-1, -1));
        assert!(!slip_sign_disagreement(0, -1));
        assert!(!slip_sign_disagreement(1, 0));
        assert!(!slip_sign_disagreement(0, 0));
    }

    #[test]
    fn slip_force_clamps_symmetrically_to_the_wheel_limit() {
        // qmul(slip, scale) * 4: within the limit it passes through.
        assert_eq!(clamp_wheel_slip_force(fx(1), Fixed16::ONE, 100), 4);
        assert_eq!(
            clamp_wheel_slip_force(Fixed16::ONE, Fixed16::ONE, 0x40000),
            0x40000
        );
        // Above and below the bounds it clamps.
        assert_eq!(
            clamp_wheel_slip_force(Fixed16::from_int(100), Fixed16::ONE, 50),
            50
        );
        assert_eq!(
            clamp_wheel_slip_force(Fixed16::from_int(-100), Fixed16::ONE, 50),
            -50
        );
        // Exact boundaries pass through (the comparisons are strict).
        assert_eq!(clamp_wheel_slip_force(fx(12), Fixed16::ONE, 48), 48);
        assert_eq!(clamp_wheel_slip_force(fx(-12), Fixed16::ONE, 48), -48);
        // The native negative-limit edge case is preserved.
        assert_eq!(clamp_wheel_slip_force(Fixed16::ONE, Fixed16::ONE, -50), -50);
        // The multiply-by-four wraps before the clamp.
        assert_eq!(clamp_wheel_slip_force(fx(i32::MAX), Fixed16::ONE, 100), -4);
    }

    #[test]
    fn slip_shift_selects_five_two_or_three_by_flag_and_config() {
        assert_eq!(wheel_slip_shift(1, 0, 0), 5);
        assert_eq!(wheel_slip_shift(0, 0, 0), 2);
        assert_eq!(wheel_slip_shift(0, 1, 0), 3);
        assert_eq!(wheel_slip_shift(0, 0, 1), 3);
        assert_eq!(wheel_slip_shift(-1, 1, 1), 5);
    }

    #[test]
    fn wheel_drive_applies_split_and_constant_by_speed_sign() {
        assert_eq!(apply_wheel_drive(10, 100, 5, -1), 115);
        assert_eq!(apply_wheel_drive(10, 100, 5, 0), 10);
        assert_eq!(apply_wheel_drive(10, 100, 5, 1), -95);
        assert_eq!(apply_wheel_drive(i32::MAX, 1, 1, -1), i32::MIN + 1);
        assert_eq!(apply_wheel_drive(i32::MIN, 1, 1, 1), i32::MAX - 1);
    }

    #[test]
    fn wheel_speed_integration_adds_the_scaled_force_with_wrapping() {
        assert_eq!(
            integrate_wheel_speed(10, fx(0x20000), Fixed16::from_raw(0x8000)),
            10 + 0x10000
        );
        assert_eq!(integrate_wheel_speed(7, Fixed16::ZERO, Fixed16::ONE), 7);
        assert_eq!(
            integrate_wheel_speed(i32::MAX, Fixed16::ONE, Fixed16::ONE),
            i32::MIN + 0xffff
        );
    }

    #[test]
    fn wheel_speed_gate_zeroes_sign_flips_and_ratio_mismatches() {
        // Gear zero takes the flip gate; no flip leaves the speed alone.
        assert_eq!(gate_wheel_speed(1, -1, 0, 0, 0, 0x10000, false), 0);
        assert_eq!(gate_wheel_speed(1, 2, 0, 0, 0, 0x10000, false), 2);
        // Front wheel with config_158 == 0 also takes the flip gate.
        assert_eq!(gate_wheel_speed(-1, 1, 1, 0, 0, 0x10000, false), 0);
        // Rear wheel needs config_158 == 0x10000 for that gate.
        assert_eq!(gate_wheel_speed(1, 2, 1, 0, 0x10000, 0x10000, true), 2);
        // With the gate untaken (config_158 differs from the rear constant),
        // the gear-ratio sign decides instead.
        assert_eq!(gate_wheel_speed(1, -2, 1, 0, 0, 0x10000, true), 0);
        assert_eq!(gate_wheel_speed(1, -2, 1, 0, 0, -0x10000, true), -2);
        // A nonzero +0x40c takes the flip gate too.
        assert_eq!(gate_wheel_speed(1, -1, 1, 1, 0x10000, 0x10000, true), 0);
    }

    #[test]
    fn slip_gate_applies_front_always_and_rear_only_without_handbrake_or_disagreement() {
        assert!(apply_wheel_slip(true, 1, false));
        assert!(apply_wheel_slip(false, 0, false));
        assert!(apply_wheel_slip(false, 1, true));
        assert!(!apply_wheel_slip(false, 1, false));
    }
}
