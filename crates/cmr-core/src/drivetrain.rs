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

/// Whether the per-wheel ratio candidate is inside the unit gate used by the
/// second force-pass loop: `|dot * 4 - speed| < 0x10000` (pseudo-absolute).
#[must_use]
pub fn wheel_ratio_within_unit(ratio: i32) -> bool {
    ratio.wrapping_abs() < 0x10000
}

/// The sign-aware residual saturation of the second per-wheel loop.
///
/// When the residual and the force term share a sign (both positive or both
/// negative), the residual is reduced by the force term only when the force's
/// magnitude is smaller; otherwise it is zeroed. Opposite signs (or either
/// value being zero) leave the residual unchanged. Magnitudes use the native
/// pseudo-absolute rule, so `i32::MIN` keeps its sign under wrapping.
#[must_use]
pub fn residual_with_saturation(residual: i32, force: i32) -> i32 {
    let same_sign = (residual > 0 && force > 0) || (residual < 0 && force < 0);
    if !same_sign {
        return residual;
    }
    if force.wrapping_abs() < residual.wrapping_abs() {
        residual.wrapping_sub(force)
    } else {
        0
    }
}

/// The gear-ratio residual gate of the second per-wheel loop: when
/// `ctl+0x2ec` is zero and the gear ratio is zero or has the opposite sign to
/// the residual, the residual is zeroed; otherwise it passes through.
#[must_use]
pub fn gate_residual_by_gear_ratio(residual: i32, gear_ratio: i32, config_2ec: i32) -> i32 {
    if config_2ec != 0 {
        return residual;
    }
    if (gear_ratio > 0 && residual < 0) || (gear_ratio < 0 && residual > 0) || gear_ratio == 0 {
        return 0;
    }
    residual
}

/// The second residual of the second per-wheel loop:
/// `-qmul(qmul(ratio, 0x4000), DAT_0050c5c4)`, then `qmul(..., 0x147a)` when
/// the per-wheel flag is set. Negation and multiplication wrap.
#[must_use]
pub fn second_loop_residual(ratio: Fixed16, scale: Fixed16, flagged: bool) -> Fixed16 {
    let residual = Fixed16::from_raw(
        ratio
            .mul(Fixed16::from_raw(0x4000))
            .mul(scale)
            .raw()
            .wrapping_neg(),
    );
    if flagged {
        residual.mul(Fixed16::from_raw(0x147a))
    } else {
        residual
    }
}

/// The third residual of the second per-wheel loop: `-qmul(dot, DAT_0050c5c4)`
/// with wrapping negation.
#[must_use]
pub fn second_loop_axis_residual(dot: Fixed16, scale: Fixed16) -> Fixed16 {
    Fixed16::from_raw(dot.mul(scale).raw().wrapping_neg())
}

/// Whether the residual exceeds the unit gate used to select the second-loop
/// normalization stage: `|residual| > 0x10000` (pseudo-absolute).
#[must_use]
pub fn second_loop_ratio_is_large(residual: i32) -> bool {
    residual.wrapping_abs() > 0x10000
}

fn q16_mul(left: i32, right: i32) -> i32 {
    ((i64::from(left) * i64::from(right)) >> 16) as i32
}

/// The large-ratio normalization result of the second per-wheel loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecondLoopNormalization {
    /// The normalized summed direction (zero when its square sum is zero).
    pub direction: [i32; 3],
    /// `qmul(square_sum, normalizer)`.
    pub length: i32,
    /// The selected normalizer: `|local_30|` or `|local_2c|` by the branch rule.
    pub normalizer: i32,
    /// `2^32 / normalizer`, or `-1` when the normalizer is zero.
    pub reciprocal: i32,
}

/// Reconstructs the normalization branch of the second per-wheel loop.
///
/// The normalizer is `|local_30|` when `|local_2c| < 0x10001` or
/// `|local_2c| < |local_30|` (the large-ratio branch), and `|local_2c|`
/// otherwise (the small-ratio branch). Both branches then scale the two
/// direction sources by `qmul(local_2c, reciprocal)` and
/// `qmul(local_30, reciprocal)`, sum them componentwise, and normalize the sum
/// with the legacy generated-table rule: a zero square sum yields a zero
/// direction, otherwise each component is multiplied by `2^32/root`. `length`
/// is `qmul(square_sum, normalizer)`.
#[must_use]
pub fn normalize_second_loop_direction(
    local_68: [i32; 3],
    local_98: [i32; 3],
    local_2c: i32,
    local_30: i32,
) -> SecondLoopNormalization {
    let abs_2c = local_2c.wrapping_abs();
    let abs_30 = local_30.wrapping_abs();
    let normalizer = if abs_2c < 0x10001 || abs_2c < abs_30 {
        abs_30
    } else {
        abs_2c
    };
    let reciprocal = if normalizer == 0 {
        -1
    } else {
        (0x1_0000_0000_i64 / i64::from(normalizer)) as i32
    };
    let scale_2c = q16_mul(local_2c, reciprocal);
    let scale_30 = q16_mul(local_30, reciprocal);
    let summed = std::array::from_fn(|index| {
        q16_mul(local_68[index], scale_2c).wrapping_add(q16_mul(local_98[index], scale_30))
    });
    let square_sum = summed.into_iter().fold(0_i32, |sum, component| {
        sum.wrapping_add(q16_mul(component, component))
    });
    SecondLoopNormalization {
        direction: crate::compiled_car_initializer::normalize_compiled_lane_vector(summed),
        length: q16_mul(square_sum, normalizer),
        normalizer,
        reciprocal,
    }
}

/// Reconstructs the arctan index fold used after the second-loop normalization.
///
/// `product` is the 64-bit `local_2c * local_34` product and `denominator` is
/// `local_30`. A zero product yields `0`; a zero denominator with a nonzero
/// product yields `0x400`. Otherwise the ratio is folded into the first octant
/// (`min/max`, so it never exceeds `1.0`), the table is indexed by
/// `(numerator << 16) / denominator >> 7`, the index is complemented
/// (`0x400 - index`) when the original ratio exceeded `1.0`, and negated when
/// the product and denominator have opposite signs. The table is supplied by
/// the caller because it is runtime-built, like the cosine table.
#[must_use]
pub fn arctan_ratio_index(product: i64, denominator: i32, table: &[i16]) -> i16 {
    if product == 0 {
        return 0;
    }
    if denominator == 0 {
        return 0x400;
    }
    let negative = (product < 0) ^ (denominator < 0);
    let product_abs = product.abs();
    let denominator_abs = i64::from(denominator).abs();
    let (numerator, divisor, complement) = if denominator_abs < product_abs {
        (denominator_abs, product_abs, true)
    } else {
        (product_abs, denominator_abs, false)
    };
    let ratio = (numerator << 16) / divisor;
    let mut index = table[(ratio >> 7) as usize];
    if complement {
        index = 0x400 - index;
    }
    if negative {
        index = -index;
    }
    index
}

/// The `ctl+0x420`-gated accumulation factor of the second per-wheel loop.
///
/// The factor is produced only when the speed delta and the gear ratio have
/// the same nonzero sign: `qmul(qmul(delta, 0x4000), qmul(DAT_0050c5c4,
/// 0x1999))`. When the per-wheel limit at `ctl+0x35c` is below the result's
/// magnitude, the result is replaced by the signed limit (`-limit` when the
/// result is below `1`, otherwise `limit`). Returns `None` on a sign mismatch
/// or a zero delta/ratio.
#[must_use]
pub fn second_loop_accumulation_factor(
    speed_delta: i32,
    gear_ratio: i32,
    scale_c5c4: Fixed16,
    limit: i32,
) -> Option<i32> {
    let sign_match = (speed_delta > 0 && gear_ratio > 0) || (speed_delta < 0 && gear_ratio < 0);
    if !sign_match {
        return None;
    }
    let factor = scale_c5c4.mul(Fixed16::from_raw(0x1999));
    let scaled = q16_mul(speed_delta, 0x4000);
    let mut value = q16_mul(scaled, factor.raw());
    if limit < value.wrapping_abs() {
        value = if value < 1 { -limit } else { limit };
    }
    Some(value)
}

/// Accumulates one direction's force into the three accumulators:
/// `accumulator += qmul(component, factor)` with wrapping addition.
pub fn accumulate_direction_force(accumulators: &mut [i32; 3], direction: [i32; 3], factor: i32) {
    for (accumulator, component) in accumulators.iter_mut().zip(direction) {
        *accumulator = accumulator.wrapping_add(q16_mul(component, factor));
    }
}

/// The final table-driven accumulation factor of the second per-wheel loop.
///
/// A zero dot produces no factor. Otherwise the dot is shifted right by two
/// (`dot >> 2`), scaled by `qmul(DAT_0050c5c4, table_factor)` when the table
/// entry is present, or by `qmul(DAT_0050c5c4, 0x8000)` otherwise, and the
/// result is negated with wrapping.
#[must_use]
pub fn final_loop_accumulation_factor(
    dot: i32,
    scale_c5c4: Fixed16,
    table_factor: Option<i32>,
) -> Option<i32> {
    if dot == 0 {
        return None;
    }
    let scaled = dot >> 2;
    let factor = match table_factor {
        Some(table) => scale_c5c4.mul(Fixed16::from_raw(table)),
        None => scale_c5c4.mul(Fixed16::from_raw(0x8000)),
    };
    Some(q16_mul(scaled, factor.raw()).wrapping_neg())
}

/// Reconstructs the projection-normalization used twice in the force-pass
/// head: `rejected = state - normal * dot(state, normal)` with wrapping Q16.16
/// arithmetic, then the legacy generated-table normalization (zero square sum
/// yields a zero vector). This is the Gram-Schmidt rejection of the AK11
/// normal at `+0x324/+0x328/+0x32c` out of the state vector, producing the two
/// per-wheel direction sources.
#[must_use]
pub fn project_and_normalize(state: [i32; 3], normal: [i32; 3]) -> [i32; 3] {
    let dot = (0..3).fold(0_i32, |sum, index| {
        sum.wrapping_add(q16_mul(state[index], normal[index]))
    });
    let rejected =
        std::array::from_fn(|index| state[index].wrapping_sub(q16_mul(normal[index], dot)));
    crate::compiled_car_initializer::normalize_compiled_lane_vector(rejected)
}

/// The Q16.16 cross product used by the force-pass head: `v x u` in exact
/// right-handed order with wrapping products and subtractions.
#[must_use]
pub fn cross_q16(v: [i32; 3], u: [i32; 3]) -> [i32; 3] {
    crate::compiled_car_initializer::compiled_post_lane_cross_v_by_u(v, u)
}

/// Reconstructs the cross-then-project-normalize sequence of the force-pass
/// head: the cross `v x u` is projected against the AK11 normal and normalized
/// with the legacy generated-table rule.
#[must_use]
pub fn cross_and_project_normalize(v: [i32; 3], u: [i32; 3], normal: [i32; 3]) -> [i32; 3] {
    project_and_normalize(cross_q16(v, u), normal)
}

/// The three global drive factors computed near the end of the force-pass
/// head: `DAT_0050c5c4 = qmul(state+0x52c, DAT_004eac70)`,
/// `DAT_0050c600 = qmul(DAT_004eac70, 0xf000)`, and
/// `DAT_0050c5c0 = qmul(DAT_004eac6c, 0x11113)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriveFactors {
    pub scale_c5c4: i32,
    pub scale_c600: i32,
    pub factor_c5c0: i32,
}

/// Reconstructs the three global drive factors from the runtime state word at
/// `+0x52c` and the data constants `DAT_004eac70`/`DAT_004eac6c` (whose values
/// are game data, not recovered constants).
#[must_use]
pub fn compute_drive_factors(state_52c: i32) -> DriveFactors {
    DriveFactors {
        scale_c5c4: q16_mul(state_52c, 0x4eac70),
        scale_c600: q16_mul(0x4eac70, 0xf000),
        factor_c5c0: q16_mul(0x4eac6c, 0x11113),
    }
}

/// Reconstructs the two-vector orthogonalization used by the force-pass head's
/// alternate branch.
///
/// With `scaled_dot = qmul(qdot(v, w), 0x20000)`, it first forms
/// `rejected = w - v * scaled_dot`, then `result = v - rejected * qdot(v,
/// rejected)`, and normalizes the result with the legacy generated-table rule
/// (zero square sum yields a zero vector).
#[must_use]
pub fn orthonormalize_pair(v: [i32; 3], w: [i32; 3]) -> [i32; 3] {
    let dot = (0..3).fold(0_i32, |sum, index| {
        sum.wrapping_add(q16_mul(v[index], w[index]))
    });
    let scaled_dot = q16_mul(dot, 0x20000);
    let rejected: [i32; 3] =
        std::array::from_fn(|index| w[index].wrapping_sub(q16_mul(v[index], scaled_dot)));
    let second_dot = (0..3).fold(0_i32, |sum, index| {
        sum.wrapping_add(q16_mul(v[index], rejected[index]))
    });
    let result: [i32; 3] =
        std::array::from_fn(|index| v[index].wrapping_sub(q16_mul(rejected[index], second_dot)));
    crate::compiled_car_initializer::normalize_compiled_lane_vector(result)
}

/// The control-struct store offsets of the four three-dword basis rows
/// produced by the force-pass head: the initial direction sources, the first
/// projection results, the second direction sources, and the second
/// projection results.
pub const HEAD_BASIS_STORE_OFFSETS: [[u16; 3]; 4] = [
    [0x380, 0x384, 0x388],
    [0x38c, 0x390, 0x394],
    [0x398, 0x39c, 0x3a0],
    [0x3a4, 0x3a8, 0x3ac],
];

/// The config-gated vector-selection branch of the force-pass head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadVectorSelection {
    /// The alternate pair-orthogonalization branch (`LAB_0040e18d`).
    Orthogonalize,
    /// The projection results replace the initial direction sources when
    /// `DAT_00d3ca80+0x25 & 0x10` is set.
    SwapProjection,
    /// The initial direction sources are kept.
    KeepInitial,
}

/// Reconstructs the config-gated vector selection of the force-pass head.
///
/// The alternate orthogonalization branch runs when `DAT_00d3ca80+0x26 & 0x80`
/// is set, or when `DAT_00d3ca88 == 0` and `FUN_0042a840()` returns nonzero.
/// Otherwise the projection results replace the initial sources only when
/// `DAT_00d3ca80+0x25 & 0x10` is set. `FUN_0042a840` is not called when
/// `DAT_00d3ca88 != 0`.
#[must_use]
pub fn select_head_vector_path(
    config_26: u8,
    special_stage: i32,
    helper_42a840: i32,
    config_25: u8,
) -> HeadVectorSelection {
    if config_26 & 0x80 != 0 {
        return HeadVectorSelection::Orthogonalize;
    }
    if special_stage == 0 && helper_42a840 != 0 {
        return HeadVectorSelection::Orthogonalize;
    }
    if config_25 & 0x10 != 0 {
        return HeadVectorSelection::SwapProjection;
    }
    HeadVectorSelection::KeepInitial
}

/// Inputs for one first-loop per-wheel iteration of the force pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstWheelInputs {
    /// The force accumulator at `+0x540` before the iteration.
    pub force: i32,
    /// The wheel speed at `+0x2b4` before integration.
    pub wheel_speed: i32,
    /// The axle split for this wheel (front or rear).
    pub split: i32,
    /// The per-car-type wheel constant `DAT_0050e18c`.
    pub wheel_constant: i32,
    /// The precomputed wheel vector.
    pub wheel_vector: [i32; 3],
    /// The per-wheel state vector.
    pub state_vector: [i32; 3],
    /// The slip scale `DAT_0050c600`.
    pub scale_c600: i32,
    /// The slip limit at `ctl+0x378`.
    pub limit: i32,
    /// The downshift from [`wheel_slip_shift`].
    pub shift: u32,
    /// The handbrake value at `ctl+0x30c`.
    pub handbrake: i32,
    /// The wheel-speed integration factor `DAT_0050c5c0`.
    pub factor_c5c0: i32,
    /// The gear byte at `ctl+0x43a`.
    pub gear: i32,
    /// The config dword at `ctl+0x40c`.
    pub config_40c: i32,
    /// The config dword at `ctl+0x158`.
    pub config_158: i32,
    /// The gear ratio at `ctl+0x1d0[gear*4]`.
    pub gear_ratio: i32,
    /// Whether this is a front wheel (the slip gate applies always).
    pub is_front_wheel: bool,
}

/// Effects of one first-loop per-wheel iteration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstWheelEffects {
    /// The final force stored at `+0x2d4`.
    pub force_at_2d4: i32,
    /// The force accumulator at `+0x540` after the iteration.
    pub force_at_540: i32,
    /// The integrated and gated wheel speed at `+0x2b4`.
    pub wheel_speed: i32,
    /// Whether the slip contribution was applied (vs zeroed and redriven).
    pub slip_applied: bool,
}

/// Composes one first-loop per-wheel iteration of the force pass from the
/// recovered rules, in native order: drive application, slip candidate and
/// sign-disagreement, the slip gate and clamped/downshifted slip force (or a
/// zeroed redrive), wheel-speed integration, and the zeroing gate.
#[must_use]
pub fn execute_first_wheel_iteration(input: FirstWheelInputs) -> FirstWheelEffects {
    let mut force = apply_wheel_drive(
        input.force,
        input.split,
        input.wheel_constant,
        input.wheel_speed,
    );
    let dot = (0..3).fold(0_i32, |sum, index| {
        sum.wrapping_add(q16_mul(
            input.wheel_vector[index],
            input.state_vector[index],
        ))
    });
    let slip = wheel_slip_candidate(dot, input.wheel_speed);
    let disagreement = slip_sign_disagreement(dot, slip);
    let slip_applied = apply_wheel_slip(input.is_front_wheel, input.handbrake, disagreement);
    if slip_applied {
        let slip_force = clamp_wheel_slip_force(
            Fixed16::from_raw(slip),
            Fixed16::from_raw(input.scale_c600),
            input.limit,
        ) >> input.shift;
        force = force.wrapping_add(slip_force);
    } else {
        force = 0;
        force = apply_wheel_drive(force, input.split, input.wheel_constant, input.wheel_speed);
    }
    let integrated = integrate_wheel_speed(
        input.wheel_speed,
        Fixed16::from_raw(force),
        Fixed16::from_raw(input.factor_c5c0),
    );
    FirstWheelEffects {
        force_at_2d4: force,
        force_at_540: force,
        wheel_speed: gate_wheel_speed(
            input.wheel_speed,
            integrated,
            input.gear,
            input.config_40c,
            input.config_158,
            input.gear_ratio,
            !input.is_front_wheel,
        ),
        slip_applied,
    }
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

    #[test]
    fn wheel_ratio_gate_uses_pseudo_absolute_unit_bound() {
        assert!(wheel_ratio_within_unit(0x10000 - 1));
        assert!(!wheel_ratio_within_unit(0x10000));
        assert!(wheel_ratio_within_unit(-(0x10000 - 1)));
        assert!(!wheel_ratio_within_unit(-0x10000));
        // Pseudo-absolute of INT_MIN stays INT_MIN, which passes the signed test.
        assert!(wheel_ratio_within_unit(i32::MIN));
    }

    #[test]
    fn residual_saturation_shares_sign_and_compares_magnitudes() {
        assert_eq!(residual_with_saturation(100, 40), 60);
        assert_eq!(residual_with_saturation(100, 100), 0);
        assert_eq!(residual_with_saturation(100, 200), 0);
        assert_eq!(residual_with_saturation(-100, -40), -60);
        assert_eq!(residual_with_saturation(-100, -100), 0);
        assert_eq!(residual_with_saturation(100, -40), 100);
        assert_eq!(residual_with_saturation(0, 40), 0);
        assert_eq!(residual_with_saturation(100, 0), 100);
    }

    #[test]
    fn residual_gear_ratio_gate_zeroes_zero_and_opposite_sign_ratios() {
        assert_eq!(gate_residual_by_gear_ratio(100, 0, 1), 100);
        assert_eq!(gate_residual_by_gear_ratio(100, 0, 0), 0);
        assert_eq!(gate_residual_by_gear_ratio(-100, 1, 0), 0);
        assert_eq!(gate_residual_by_gear_ratio(100, -1, 0), 0);
        assert_eq!(gate_residual_by_gear_ratio(100, 1, 0), 100);
        assert_eq!(gate_residual_by_gear_ratio(-100, -1, 0), -100);
    }

    #[test]
    fn second_loop_residual_scales_negates_and_optionally_applies_147a() {
        assert_eq!(
            second_loop_residual(fx(0x20000), Fixed16::ONE, false),
            fx(-0x8000)
        );
        assert_eq!(
            second_loop_residual(fx(0x20000), Fixed16::ONE, true),
            fx(-2621)
        );
        assert_eq!(
            second_loop_residual(fx(i32::MAX), Fixed16::ONE, false),
            fx(-536_870_911)
        );
    }

    #[test]
    fn second_loop_axis_residual_negates_the_scaled_dot() {
        assert_eq!(
            second_loop_axis_residual(fx(0x10000), Fixed16::ONE),
            fx(-0x10000)
        );
        // The wrapping negation of INT_MIN stays INT_MIN.
        assert_eq!(
            second_loop_axis_residual(fx(i32::MIN), Fixed16::ONE),
            fx(i32::MIN)
        );
    }

    #[test]
    fn second_loop_ratio_gate_uses_pseudo_absolute_bound() {
        assert!(!second_loop_ratio_is_large(0x10000));
        assert!(second_loop_ratio_is_large(0x10001));
        assert!(second_loop_ratio_is_large(-0x10001));
        // Pseudo-absolute of INT_MIN stays INT_MIN, which is not large.
        assert!(!second_loop_ratio_is_large(i32::MIN));
    }

    #[test]
    fn second_loop_normalization_normalizes_the_summed_direction() {
        // local_30 = 2.0: normalizer 0x20000, reciprocal 0x8000. The summed
        // direction is the third source alone, a unit Z vector.
        let result = normalize_second_loop_direction([0; 3], [0, 0, 0x10000], 0, 0x20000);
        assert_eq!(result.direction, [0, 0, 65_522]);
        assert_eq!(result.length, 0x20000);
        assert_eq!(result.normalizer, 0x20000);
        assert_eq!(result.reciprocal, 0x8000);
    }

    #[test]
    fn second_loop_normalization_zero_square_sum_yields_zero_direction() {
        let result = normalize_second_loop_direction([0; 3], [0; 3], 0x10000, 0x20000);
        assert_eq!(result.direction, [0; 3]);
        assert_eq!(result.length, 0);
        assert_eq!(result.reciprocal, 0x8000);
    }

    #[test]
    fn second_loop_normalization_zero_normalizer_uses_minus_one_reciprocal() {
        let result = normalize_second_loop_direction([0x1000000, 0, 0], [0; 3], 0x10000, 0);
        assert_eq!(result.normalizer, 0);
        assert_eq!(result.reciprocal, -1);
        assert_eq!(result.direction, [-0x10000, 0, 0]);
        assert_eq!(result.length, 0);
    }

    #[test]
    fn second_loop_normalization_selects_the_small_ratio_normalizer() {
        // |local_2c| = 4.0 exceeds |local_30| = 2.0 and the unit gate, so the
        // small-ratio branch normalizes with |local_2c|.
        let result =
            normalize_second_loop_direction([0x10000, 0, 0], [0, 0x10000, 0], 0x40000, 0x20000);
        assert_eq!(result.normalizer, 0x40000);
        assert_eq!(result.reciprocal, 0x4000);
        assert_eq!(result.length, 0x50000);
        assert_eq!(
            result.direction,
            crate::compiled_car_initializer::normalize_compiled_lane_vector([0x10000, 0x8000, 0])
        );
    }

    #[test]
    fn second_loop_normalization_branch_prefers_the_larger_residual_magnitude() {
        // |local_2c| = 2.0 is above the unit gate but below |local_30| = 4.0,
        // so the large-ratio branch still uses |local_30|.
        let result = normalize_second_loop_direction([0; 3], [0; 3], 0x20000, 0x40000);
        assert_eq!(result.normalizer, 0x40000);
        // |local_2c| = 1.0 is below the unit gate: |local_30| is used even
        // though it is smaller.
        let result = normalize_second_loop_direction([0; 3], [0; 3], 0x10000, 0x8000);
        assert_eq!(result.normalizer, 0x8000);
        assert_eq!(result.reciprocal, 0x20000);
    }

    #[test]
    fn arctan_index_handles_zero_and_folds_ratios_into_the_first_octant() {
        let table: Vec<i16> = (0..=0x1000).collect();
        // Zero product and zero denominator special cases.
        assert_eq!(arctan_ratio_index(0, 0x20000, &table), 0);
        assert_eq!(arctan_ratio_index(0x10000, 0, &table), 0x400);
        // ratio = 0.5: index = table[(0x10000 << 16) / 0x20000 >> 7] = table[0x100].
        assert_eq!(arctan_ratio_index(0x10000, 0x20000, &table), 0x100);
        // ratio = 4: reciprocal fold, then complement.
        assert_eq!(arctan_ratio_index(0x40000, 0x10000, &table), 0x400 - 0x80);
    }

    #[test]
    fn arctan_index_negates_on_opposite_signs_and_complements() {
        let table: Vec<i16> = (0..=0x1000).collect();
        assert_eq!(arctan_ratio_index(-0x10000, 0x20000, &table), -0x100);
        assert_eq!(arctan_ratio_index(0x10000, -0x20000, &table), -0x100);
        assert_eq!(arctan_ratio_index(-0x10000, -0x20000, &table), 0x100);
        // A complemented index is negated after the complement.
        assert_eq!(
            arctan_ratio_index(-0x40000, 0x10000, &table),
            -(0x400 - 0x80)
        );
    }

    #[test]
    fn second_loop_accumulation_factor_gates_scales_and_clamps() {
        // Sign mismatches and zero delta/ratio produce no factor.
        assert_eq!(
            second_loop_accumulation_factor(0x10000, -1, Fixed16::ONE, 0x1000),
            None
        );
        assert_eq!(
            second_loop_accumulation_factor(0, 0x10000, Fixed16::ONE, 0x1000),
            None
        );
        assert_eq!(
            second_loop_accumulation_factor(0x10000, 0, Fixed16::ONE, 0x1000),
            None
        );
        // qmul(qmul(delta, 0x4000), qmul(scale, 0x1999)) within the limit.
        assert_eq!(
            second_loop_accumulation_factor(0x10000, 0x10000, Fixed16::ONE, 0x10000),
            Some(0x666)
        );
        // Above the limit it is replaced by the signed limit.
        assert_eq!(
            second_loop_accumulation_factor(0x100000, 1, Fixed16::ONE, 0x1000),
            Some(0x1000)
        );
        assert_eq!(
            second_loop_accumulation_factor(-0x100000, -1, Fixed16::ONE, 0x1000),
            Some(-0x1000)
        );
    }

    #[test]
    fn accumulate_direction_force_adds_scaled_components_with_wrapping() {
        let mut accumulators = [1, 2, 3];
        accumulate_direction_force(&mut accumulators, [0x10000; 3], 0x8000);
        assert_eq!(accumulators, [0x8001, 0x8002, 0x8003]);
        let mut accumulators = [i32::MAX; 3];
        accumulate_direction_force(&mut accumulators, [0x10000; 3], 0x10000);
        assert_eq!(accumulators, [i32::MIN + 0xffff; 3]);
    }

    #[test]
    fn final_loop_accumulation_factor_shifts_scales_and_negates() {
        // A zero dot produces no factor.
        assert_eq!(
            final_loop_accumulation_factor(0, Fixed16::ONE, Some(0x20000)),
            None
        );
        // dot >> 2 then scaled by the table factor.
        assert_eq!(
            final_loop_accumulation_factor(0x10000, Fixed16::ONE, Some(0x20000)),
            Some(-0x8000)
        );
        // Without a table entry the 0x8000 constant is used.
        assert_eq!(
            final_loop_accumulation_factor(0x10000, Fixed16::ONE, None),
            Some(-0x2000)
        );
        // Negative dots shift arithmetically and the result negates back.
        assert_eq!(
            final_loop_accumulation_factor(-0x10000, Fixed16::ONE, None),
            Some(0x2000)
        );
    }

    #[test]
    fn projection_normalization_rejects_the_normal_component() {
        // Parallel state and normal: the rejection is zero.
        assert_eq!(
            project_and_normalize([0x10000, 0, 0], [0x10000, 0, 0]),
            [0; 3]
        );
        // Orthogonal state: it is unchanged before normalization.
        assert_eq!(
            project_and_normalize([0, 0x10000, 0], [0x10000, 0, 0]),
            [0, 65_522, 0]
        );
        // Nonzero projection is removed exactly.
        assert_eq!(
            project_and_normalize([0x8000, 0x10000, 0], [0x10000, 0, 0]),
            [0, 65_522, 0]
        );
        // The other axis is rejected out of a mixed state.
        assert_eq!(
            project_and_normalize([0x10000, 0x10000, 0], [0, 0x10000, 0]),
            [65_522, 0, 0]
        );
    }

    #[test]
    fn projection_normalization_handles_zero_normal_and_zero_state() {
        // A zero normal leaves the state unchanged before normalization.
        assert_eq!(
            project_and_normalize([0, 0, 0x10000], [0; 3]),
            [0, 0, 65_522]
        );
        // A zero state stays zero even with a nonzero normal.
        assert_eq!(project_and_normalize([0; 3], [0x10000, 0, 0]), [0; 3]);
    }

    #[test]
    fn cross_q16_is_right_handed_with_wrapping_products() {
        assert_eq!(cross_q16([0x10000, 0, 0], [0, 0x10000, 0]), [0, 0, 0x10000]);
        assert_eq!(
            cross_q16([0, 0x10000, 0], [0x10000, 0, 0]),
            [0, 0, -0x10000]
        );
        assert_eq!(cross_q16([0x10000, 0, 0], [0x10000, 0, 0]), [0; 3]);
    }

    #[test]
    fn cross_and_project_normalize_projects_the_cross_against_the_normal() {
        // Parallel cross and normal: the rejection is zero.
        assert_eq!(
            cross_and_project_normalize([0x10000, 0, 0], [0, 0x10000, 0], [0, 0, 0x10000]),
            [0; 3]
        );
        // Orthogonal normal: the cross is normalized directly.
        assert_eq!(
            cross_and_project_normalize([0x10000, 0, 0], [0, 0x10000, 0], [0x10000, 0, 0]),
            [0, 0, 65_522]
        );
    }

    #[test]
    fn drive_factors_use_the_state_word_and_data_constants() {
        let factors = compute_drive_factors(0x20000);
        assert_eq!(factors.scale_c5c4, q16_mul(0x20000, 0x4eac70));
        assert_eq!(factors.scale_c600, q16_mul(0x4eac70, 0xf000));
        assert_eq!(factors.factor_c5c0, q16_mul(0x4eac6c, 0x11113));
        // The state word scales only the first factor.
        let zero = compute_drive_factors(0);
        assert_eq!(zero.scale_c5c4, 0);
        assert_eq!(zero.scale_c600, factors.scale_c600);
        assert_eq!(zero.factor_c5c0, factors.factor_c5c0);
    }

    #[test]
    fn pair_orthogonalization_normalizes_parallel_and_orthogonal_pairs() {
        // Orthogonal input: dot = 0, so the result is just the normalized v.
        assert_eq!(
            orthonormalize_pair([0x10000, 0, 0], [0, 0x10000, 0]),
            [65_522, 0, 0]
        );
        // Parallel input: the rejection collapses to zero.
        assert_eq!(
            orthonormalize_pair([0x10000, 0, 0], [0x10000, 0, 0]),
            [0; 3]
        );
        // Swapped orthogonal input: the other axis is returned normalized.
        assert_eq!(
            orthonormalize_pair([0, 0x10000, 0], [0x10000, 0, 0]),
            [0, 65_522, 0]
        );
    }

    #[test]
    fn pair_orthogonalization_handles_zero_inputs() {
        assert_eq!(orthonormalize_pair([0; 3], [0x10000, 0, 0]), [0; 3]);
        assert_eq!(orthonormalize_pair([0x10000, 0, 0], [0; 3]), [65_522, 0, 0]);
    }

    #[test]
    fn head_vector_selection_follows_the_config_gates() {
        // The mirror bit forces the orthogonalization branch.
        assert_eq!(
            select_head_vector_path(0x80, 1, 1, 0),
            HeadVectorSelection::Orthogonalize
        );
        // A nonzero helper also selects it (only when the special stage is zero).
        assert_eq!(
            select_head_vector_path(0, 0, 1, 0),
            HeadVectorSelection::Orthogonalize
        );
        // With a special stage the helper is not called, so the projection flag decides.
        assert_eq!(
            select_head_vector_path(0, 1, 1, 0x10),
            HeadVectorSelection::SwapProjection
        );
        assert_eq!(
            select_head_vector_path(0, 1, 1, 0),
            HeadVectorSelection::KeepInitial
        );
        // The projection flag alone also swaps.
        assert_eq!(
            select_head_vector_path(0, 0, 0, 0x10),
            HeadVectorSelection::SwapProjection
        );
        assert_eq!(
            select_head_vector_path(0, 0, 0, 0),
            HeadVectorSelection::KeepInitial
        );
    }

    #[test]
    fn head_basis_store_offsets_are_four_disjoint_three_dword_rows() {
        assert_eq!(
            HEAD_BASIS_STORE_OFFSETS,
            [
                [0x380, 0x384, 0x388],
                [0x38c, 0x390, 0x394],
                [0x398, 0x39c, 0x3a0],
                [0x3a4, 0x3a8, 0x3ac],
            ]
        );
        for pair in HEAD_BASIS_STORE_OFFSETS.windows(2) {
            assert!(pair[0][2] + 4 <= pair[1][0]);
        }
    }

    fn first_wheel_input() -> FirstWheelInputs {
        FirstWheelInputs {
            force: 10,
            wheel_speed: 1,
            split: 100,
            wheel_constant: 5,
            wheel_vector: [0x10000, 0, 0],
            state_vector: [0x10000, 0, 0],
            scale_c600: 0x10000,
            limit: 0x10000,
            shift: 2,
            handbrake: 0,
            factor_c5c0: 0x10000,
            gear: 0,
            config_40c: 0,
            config_158: 0,
            gear_ratio: 0x10000,
            is_front_wheel: true,
        }
    }

    #[test]
    fn first_wheel_iteration_applies_drive_slip_integration_and_gate() {
        let effects = execute_first_wheel_iteration(first_wheel_input());
        // Drive: 10 - 100 - 5 = -95. Slip clamp: 0x10000 >> 2 = 0x4000, so the
        // final force is 16289 and the integrated speed is 16290 (no flip).
        assert_eq!(effects.force_at_2d4, 16289);
        assert_eq!(effects.force_at_540, 16289);
        assert_eq!(effects.wheel_speed, 16290);
        assert!(effects.slip_applied);
    }

    #[test]
    fn first_wheel_iteration_redrives_when_the_slip_gate_closes() {
        let mut input = first_wheel_input();
        input.is_front_wheel = false;
        input.handbrake = 1;
        let effects = execute_first_wheel_iteration(input);
        // The slip gate closes (rear + handbrake + no disagreement), so the
        // force is zeroed and redriven: 0 - 100 - 5 = -105. The speed flips to zero.
        assert_eq!(effects.force_at_2d4, -105);
        assert_eq!(effects.wheel_speed, 0);
        assert!(!effects.slip_applied);
    }

    #[test]
    fn first_wheel_iteration_applies_slip_on_rear_without_handbrake() {
        let mut input = first_wheel_input();
        input.is_front_wheel = false;
        let effects = execute_first_wheel_iteration(input);
        // Handbrake off means the rear slip gate opens too.
        assert!(effects.slip_applied);
        assert_eq!(effects.force_at_2d4, 16289);
        assert_eq!(effects.wheel_speed, 16290);
    }
}
