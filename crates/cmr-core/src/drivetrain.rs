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
//! Unknowns (not reproduced): the element order within the 4-wide actuator
//! vector (`+0x2b8`), the final torque->velocity integrators
//! (`FUN_0040a4a0`/`FUN_00409f90`/`FUN_0040d740`), and the loader that copies
//! the per-gear ratio tables into the control struct.

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
/// stage and is not reproduced here.
#[must_use]
pub fn drive_torque_split(
    engine_torque: Fixed16,
    center_split: Fixed16,
    front_factor: Fixed16,
    rear_factor: Fixed16,
    handbrake_torque: Option<Fixed16>,
) -> (Fixed16, Fixed16) {
    let split = center_split.raw().clamp(0, Fixed16::ONE.raw());
    let split = Fixed16::from_raw(split);
    let remainder = Fixed16::ONE.sub(split);
    let front = engine_torque.mul(front_factor).mul(split);
    let mut rear = engine_torque.mul(rear_factor).mul(remainder);
    if let Some(extra) = handbrake_torque {
        rear = rear.add(extra);
    }
    (front, rear)
}

/// The coasting split: [`COAST_DRAG_TORQUE`] distributed by the centre split
/// (the recovered in-gear, no-throttle/no-brake branch of `FUN_0040d810`).
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

        // The split clamps to 0..=1 rather than extrapolating.
        let (front, rear) = drive_torque_split(
            torque,
            Fixed16::from_int(3),
            front_factor,
            rear_factor,
            None,
        );
        assert_eq!(front, Fixed16::from_int(1));
        assert_eq!(rear, Fixed16::ZERO);
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
}
