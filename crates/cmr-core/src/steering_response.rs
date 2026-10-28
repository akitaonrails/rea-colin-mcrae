//! The steering half of the per-car control processor `FUN_0041b5b0`.
//!
//! The processor picks the analog routine `FUN_0041c290` when control `+0x410`
//! is set and the digital routine `FUN_0041c3f0` otherwise. Both turn the raw
//! steer magnitudes (`+0x2fc` left, `+0x300` right, after the mechanics
//! driver's low-speed cut) into the wheel steer angle `+0x3e0` (16-bit BAM)
//! and its target `+0x3e2`. The mechanics driver adds `+0x3e0` to the per-car
//! base angle `DAT_0050e4fc[type]` and rotates the lane source by the sum,
//! which is how steering reaches the wheel forces.
//!
//! Observations (instruction level, `0x41c290..0x41c3de`, `0x41c3f0..0x41c6bc`,
//! `FUN_0041c6c0`, `FUN_0041c830`, `FUN_0041c8c0`):
//!
//! - Maximum angle `DAT_0050d738` = `(i16)+0x3e4 * 0x1680`. With flag
//!   `+0x440 & 1` the factor `0x10000 - min(qmul(|dot(row0, velocity)|,
//!   +0x450), +0x454)` multiplies `+0x3e4` first.
//! - Digital: the held-direction word `+0x114` steps by `+-0x10000` toward the
//!   held side (reset to zero when it points the other way, saturating at
//!   `+-0x10000`, zero when neither is held). The rate `DAT_0050d740` is
//!   `qmul(qmul(+0x44c, dt), |+0x114|)`, optionally scaled by `(|+0x318| -
//!   1)^2` (`+0x440 & 2`), positive for left and negative for right; the
//!   centering rate `DAT_0050d748` is `+0x188` opposing the side the position
//!   currently turns toward. Both are scaled by `k = qmul(0x10000 -
//!   qmul(surface_a + surface_b, 0x8000), car_factor)`. The position `+0x318`
//!   then either centers (zeroed when the combined step would cross zero) or
//!   integrates toward `+-0x10000` with an immediate minimum deflection of
//!   `qmul(+0x45c, k)`. The target is
//!   `+0x3e2 = ftol(qmul(DAT_0050d738, +0x318) * -(1/5760))`, and `+0x3e0`
//!   approaches it by `qmul(0x22, 0x10000 - k) + 0x22` per tick, snapping when
//!   closer than that.
//! - Analog: `v = qmul(qmul(right - left, +0x458), car_factor)` clamped to
//!   `+-0x10000`; the angle is `ftol(qmul(DAT_0050d738, v) * (1/5760))`, and
//!   both `+0x3e0` and `+0x3e2` move toward it with one wrap fold (`d > 0x800`
//!   becomes `0x1000 - d`) and slew limits `0x5a` and `0xb4`, all in 16-bit
//!   wrapping arithmetic.
//! - The `.rdata` doubles at `0x4e62f8` and `0x4e6318` are `+-(1/5760)`, each
//!   slightly larger in magnitude than the exact ratio, so the truncating
//!   `__ftol` result equals the integer quotient toward zero. That is exact
//!   under 53- and 64-bit x87 precision for every signed 32-bit product; under
//!   24-bit precision it holds while the quotient stays below about 2^12, which
//!   covers the recovered `+0x3e4 = 0x2aa`.
//!
//! Inputs not recovered here are explicit: the two front-wheel values of the
//! static table `DAT_004f74ac` (indexed by `+0x42a/+0x42c`), the per-car-type
//! factor `DAT_0050e188`, and the control scalars.

use crate::fixed_point::{Fixed16, Vec3};
use crate::vehicle_dynamics::RawSteer;

/// `DAT_0050d738` scale: the maximum angle word times 5760 (`0x1680`).
const MAX_ANGLE_SCALE: i32 = 0x1680;
/// Degrees-in-Q16 per trig-table step, `360 * 65536 / 4096`.
const ANGLE_DIVISOR: i32 = 5760;
const ONE: i32 = 0x1_0000;

/// Per-car steering scalars from the control record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SteeringParams {
    /// `+0x188`: digital centering rate.
    pub centering_188: i32,
    /// `+0x3e4`: maximum steer angle (16-bit).
    pub max_angle_3e4: i16,
    /// `+0x440`: bit 0 enables speed reduction, bit 1 position-dependent rate.
    pub flags_440: u8,
    /// `+0x44c`: digital steer rate.
    pub rate_44c: i32,
    /// `+0x450/+0x454`: speed-reduction scale and cap.
    pub speed_scale_450: i32,
    pub speed_cap_454: i32,
    /// `+0x458`: analog gain.
    pub analog_gain_458: i32,
    /// `+0x45c`: digital minimum deflection.
    pub min_deflection_45c: i32,
    /// `DAT_004eac6c`, the solver time step.
    pub time_step: i32,
}

/// The steering words the processor owns across ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SteeringState {
    /// `+0x318`: digital steer position in `[-0x10000, 0x10000]`.
    pub position_318: i32,
    /// `+0x114`: digital held-direction word.
    pub direction_114: i32,
    /// `+0x3e0`: wheel steer angle (BAM).
    pub angle_3e0: i16,
    /// `+0x3e2`: steer target angle (BAM).
    pub target_3e2: i16,
}

impl SteeringState {
    /// Every word at zero, the loader-cleared state.
    pub const ZERO: Self = Self {
        position_318: 0,
        direction_114: 0,
        angle_3e0: 0,
        target_3e2: 0,
    };
}

/// Per-tick inputs besides the control scalars.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SteeringInputs {
    /// The steer magnitudes after the driver's low-speed cut.
    pub steer: RawSteer,
    /// `+0x410`: steering came from the analog path.
    pub analog_410: bool,
    /// `DAT_0050e188[car +0x738]`, the per-car-type steering factor.
    pub car_factor_e188: i32,
    /// `DAT_004f74ac[+0x42c]` and `DAT_004f74ac[+0x42a]`.
    pub front_surface_factors: [i32; 2],
    /// Car basis row 0 (`+0x258`) and velocity (`+0xd4`).
    pub forward_axis: Vec3,
    pub velocity: Vec3,
}

fn qmul(left: i32, right: i32) -> i32 {
    Fixed16::from_raw(left).mul(Fixed16::from_raw(right)).raw()
}

/// `FUN_0041c6c0`: the maximum angle `DAT_0050d738`.
#[must_use]
pub fn max_steer_angle(params: SteeringParams, inputs: SteeringInputs) -> i32 {
    let max = i32::from(params.max_angle_3e4);
    if params.flags_440 & 1 == 0 {
        return max.wrapping_mul(MAX_ANGLE_SCALE);
    }
    let forward_speed = inputs
        .forward_axis
        .dot(inputs.velocity)
        .raw()
        .wrapping_abs();
    let reduction = qmul(forward_speed, params.speed_scale_450).min(params.speed_cap_454);
    qmul(ONE.wrapping_sub(reduction), max).wrapping_mul(MAX_ANGLE_SCALE)
}

/// `FUN_0041c830`: advance the held-direction word `+0x114`.
fn advance_direction(direction: i32, steer: RawSteer) -> i32 {
    if steer.left_2fc != 0 {
        let base = if direction < 0 { 0 } else { direction };
        return base.wrapping_add(ONE).min(ONE);
    }
    if steer.right_300 != 0 {
        let base = if direction > 0 { 0 } else { direction };
        return base.wrapping_sub(ONE).max(-ONE);
    }
    0
}

/// `FUN_0041c8c0`: the signed digital rate `DAT_0050d740`, after advancing
/// `+0x114`.
fn digital_rate(state: &mut SteeringState, params: SteeringParams, steer: RawSteer) -> i32 {
    state.direction_114 = advance_direction(state.direction_114, steer);
    if steer.left_2fc == 0 && steer.right_300 == 0 {
        return 0;
    }
    let mut rate = qmul(params.rate_44c, params.time_step);
    if params.flags_440 & 2 != 0 {
        let from_full = state.position_318.wrapping_abs().wrapping_sub(ONE);
        rate = qmul(rate, qmul(from_full, from_full));
    }
    let step = qmul(rate, state.direction_114.wrapping_abs());
    if steer.right_300 != 0 {
        step.wrapping_neg()
    } else {
        step
    }
}

/// Convert a scaled position to a BAM angle: `ftol(qmul(max, value) * sign /
/// 5760)` as the integer quotient toward zero, low 16 bits.
fn position_angle(max_angle: i32, value: i32, negate: bool) -> i16 {
    let quotient = qmul(max_angle, value) / ANGLE_DIVISOR;
    (if negate {
        quotient.wrapping_neg()
    } else {
        quotient
    }) as i16
}

/// How the digital routine moves `+0x318` this tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DigitalAction {
    Center,
    Left,
    Right,
}

/// The digital routine `FUN_0041c3f0`.
fn digital_steering(
    mut state: SteeringState,
    params: SteeringParams,
    inputs: SteeringInputs,
) -> SteeringState {
    let max_angle = max_steer_angle(params, inputs);
    let rate = digital_rate(&mut state, params, inputs.steer);
    let mut centering = if state.position_318 == 0 {
        0
    } else if qmul(max_angle, state.position_318) > 0 {
        params.centering_188.wrapping_neg()
    } else {
        params.centering_188
    };
    let surface = inputs.front_surface_factors[0].wrapping_add(inputs.front_surface_factors[1]);
    let scale = qmul(
        ONE.wrapping_sub(qmul(surface, 0x8000)),
        inputs.car_factor_e188,
    );
    let rate = qmul(rate, scale);
    centering = qmul(centering, scale);

    let left = inputs.steer.left_2fc != 0;
    let right = inputs.steer.right_300 != 0;
    // Branch order at `0x41c4e6..0x41c512`.
    let action = match (left, right) {
        (false, false) => DigitalAction::Center,
        (true, false) if centering > 0 => DigitalAction::Center,
        (true, false) => DigitalAction::Left,
        (false, true) if centering < 0 => DigitalAction::Center,
        (false, true) => DigitalAction::Right,
        (true, true) if centering == 0 => DigitalAction::Right,
        (true, true) => DigitalAction::Center,
    };
    let minimum = qmul(params.min_deflection_45c, scale);
    let position = state.position_318;
    state.position_318 = match action {
        DigitalAction::Center => {
            let combined = rate.wrapping_add(centering);
            if position.wrapping_abs() <= combined.wrapping_abs() {
                0
            } else {
                position.wrapping_add(combined)
            }
        }
        DigitalAction::Right => {
            let moved = position.wrapping_add(rate);
            if moved < -ONE {
                -ONE
            } else {
                moved.min(minimum.wrapping_neg())
            }
        }
        DigitalAction::Left => {
            let moved = position.wrapping_add(rate);
            if moved > ONE { ONE } else { moved.max(minimum) }
        }
    };

    state.target_3e2 = position_angle(max_angle, state.position_318, true);
    let step = (qmul(0x22, ONE.wrapping_sub(scale)).wrapping_add(0x22)) as i16;
    let delta = state.target_3e2.wrapping_sub(state.angle_3e0);
    state.angle_3e0 = if i32::from(delta).abs() < i32::from(step) {
        state.target_3e2
    } else if delta > 0 {
        state.angle_3e0.wrapping_add(step)
    } else {
        state.angle_3e0.wrapping_sub(step)
    };
    state
}

/// One analog slew: fold a difference above `0x800`, clamp to `+-limit`, and
/// subtract it, in 16-bit wrapping arithmetic.
fn slew_toward(current: i16, angle: i16, limit: i16) -> i16 {
    let mut delta = current.wrapping_sub(angle);
    if delta > 0x800 {
        delta = 0x1000_i16.wrapping_sub(delta);
    }
    current.wrapping_sub(delta.clamp(-limit, limit))
}

/// The analog routine `FUN_0041c290`.
fn analog_steering(
    mut state: SteeringState,
    params: SteeringParams,
    inputs: SteeringInputs,
) -> SteeringState {
    let demand = inputs.steer.right_300.wrapping_sub(inputs.steer.left_2fc);
    let value = qmul(qmul(demand, params.analog_gain_458), inputs.car_factor_e188).clamp(-ONE, ONE);
    let angle = position_angle(max_steer_angle(params, inputs), value, false);
    state.angle_3e0 = slew_toward(state.angle_3e0, angle, 0x5a);
    state.target_3e2 = slew_toward(state.target_3e2, angle, 0xb4);
    state
}

/// Apply the steering half of `FUN_0041b5b0`.
#[must_use]
pub fn apply_steering(
    state: SteeringState,
    params: SteeringParams,
    inputs: SteeringInputs,
) -> SteeringState {
    if inputs.analog_410 {
        analog_steering(state, params, inputs)
    } else {
        digital_steering(state, params, inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(
            Fixed16::from_raw(x),
            Fixed16::from_raw(y),
            Fixed16::from_raw(z),
        )
    }

    fn params() -> SteeringParams {
        SteeringParams {
            centering_188: 0x3333,
            max_angle_3e4: 0x2aa,
            flags_440: 0,
            rate_44c: 0x4000,
            speed_scale_450: 0x1000,
            speed_cap_454: 0x8000,
            analog_gain_458: 0x1_0000,
            min_deflection_45c: 0x2000,
            time_step: 0x1_0000,
        }
    }

    fn inputs(left: i32, right: i32) -> SteeringInputs {
        SteeringInputs {
            steer: RawSteer {
                left_2fc: left,
                right_300: right,
                analog_410: false,
            },
            analog_410: false,
            car_factor_e188: 0x1_0000,
            front_surface_factors: [0, 0],
            forward_axis: v(0x1_0000, 0, 0),
            velocity: Vec3::ZERO,
        }
    }

    #[test]
    fn max_angle_scales_by_5760_and_optionally_reduces_with_forward_speed() {
        assert_eq!(max_steer_angle(params(), inputs(0, 0)), 0x2aa * 0x1680);
        let reduced = SteeringParams {
            flags_440: 1,
            ..params()
        };
        let moving = SteeringInputs {
            velocity: v(-0x4_0000, 0, 0),
            ..inputs(0, 0)
        };
        // |dot| = 4.0, times 1/16 = 0x4000, below the 0x8000 cap.
        assert_eq!(
            max_steer_angle(reduced, moving),
            qmul(0xc000, 0x2aa) * 0x1680
        );
        let fast = SteeringInputs {
            velocity: v(0x40_0000, 0, 0),
            ..inputs(0, 0)
        };
        assert_eq!(max_steer_angle(reduced, fast), qmul(0x8000, 0x2aa) * 0x1680);
    }

    #[test]
    fn held_direction_saturates_resets_on_reversal_and_clears_when_released() {
        let left = RawSteer {
            left_2fc: 1,
            right_300: 0,
            analog_410: false,
        };
        let right = RawSteer {
            left_2fc: 0,
            right_300: 1,
            analog_410: false,
        };
        assert_eq!(advance_direction(0, left), 0x1_0000);
        assert_eq!(advance_direction(0x1_0000, left), 0x1_0000);
        assert_eq!(advance_direction(-0x8000, left), 0x1_0000);
        assert_eq!(advance_direction(0x8000, right), -0x1_0000);
        assert_eq!(advance_direction(-0x1_0000, right), -0x1_0000);
        assert_eq!(advance_direction(0x1_0000, RawSteer::default()), 0);
        // Both held: left is tested first.
        let both = RawSteer {
            left_2fc: 1,
            right_300: 1,
            analog_410: false,
        };
        assert_eq!(advance_direction(-0x1_0000, both), 0x1_0000);
    }

    #[test]
    fn digital_left_jumps_to_the_minimum_deflection_then_integrates_and_saturates() {
        let mut state = SteeringState::ZERO;
        state = apply_steering(state, params(), inputs(1, 0));
        // Rate 0x4000 lands above the 0x2000 minimum.
        assert_eq!(state.position_318, 0x4000);
        assert_eq!(state.direction_114, 0x1_0000);
        assert_eq!(
            state.target_3e2,
            (-(qmul(0x2aa * 0x1680, 0x4000) / 5760)) as i16
        );
        // Left yields a negative target; the angle slews by 0x22 per tick at k = 1.
        assert!(state.target_3e2 < 0);
        assert_eq!(state.angle_3e0, -0x22);
        for _ in 0..10 {
            state = apply_steering(state, params(), inputs(1, 0));
        }
        assert_eq!(state.position_318, 0x1_0000);
        assert_eq!(state.target_3e2, -0x2aa);

        let slow = SteeringParams {
            rate_44c: 0x800,
            ..params()
        };
        let first = apply_steering(SteeringState::ZERO, slow, inputs(1, 0));
        assert_eq!(first.position_318, 0x2000, "minimum deflection");
    }

    #[test]
    fn digital_right_mirrors_left_with_a_negative_position() {
        let state = apply_steering(SteeringState::ZERO, params(), inputs(0, 1));
        assert_eq!(state.position_318, -0x4000);
        assert_eq!(state.direction_114, -0x1_0000);
        assert!(state.target_3e2 > 0);
        let full = SteeringState {
            position_318: -0xf000,
            direction_114: -0x1_0000,
            ..SteeringState::ZERO
        };
        assert_eq!(
            apply_steering(full, params(), inputs(0, 1)).position_318,
            -0x1_0000
        );
    }

    #[test]
    fn released_digital_steering_centers_and_zeroes_instead_of_crossing() {
        let turned = SteeringState {
            position_318: 0x8000,
            ..SteeringState::ZERO
        };
        let next = apply_steering(turned, params(), inputs(0, 0));
        assert_eq!(next.position_318, 0x8000 - 0x3333);
        assert_eq!(next.direction_114, 0);
        let near = SteeringState {
            position_318: 0x3000,
            ..SteeringState::ZERO
        };
        assert_eq!(apply_steering(near, params(), inputs(0, 0)).position_318, 0);
        let negative = SteeringState {
            position_318: -0x8000,
            ..SteeringState::ZERO
        };
        assert_eq!(
            apply_steering(negative, params(), inputs(0, 0)).position_318,
            -0x8000 + 0x3333
        );
    }

    #[test]
    fn steering_against_the_current_side_centers_with_both_rates() {
        // Turned right (negative), pressing left: centering is positive, so the
        // left branch hands over to centering with rate + centering.
        let turned_right = SteeringState {
            position_318: -0x8000,
            ..SteeringState::ZERO
        };
        let next = apply_steering(turned_right, params(), inputs(1, 0));
        assert_eq!(next.position_318, -0x8000 + 0x4000 + 0x3333);
        // Both held at rest: zero centering lets right win.
        let both = apply_steering(SteeringState::ZERO, params(), inputs(1, 1));
        assert_eq!(both.position_318, -0x4000);
    }

    #[test]
    fn surface_and_car_factors_scale_rates_and_the_slew_step() {
        let rough = SteeringInputs {
            front_surface_factors: [0x4000, 0x4000],
            ..inputs(1, 0)
        };
        // k = 1 - 0.25 = 0xc000.
        let state = apply_steering(SteeringState::ZERO, params(), rough);
        assert_eq!(state.position_318, qmul(0x4000, 0xc000));
        let step = (qmul(0x22, 0x4000) + 0x22) as i16;
        assert_eq!(state.angle_3e0, -step);
    }

    #[test]
    fn position_dependent_rate_slows_near_full_lock() {
        let shaped = SteeringParams {
            flags_440: 2,
            ..params()
        };
        let near_full = SteeringState {
            position_318: 0xc000,
            direction_114: 0x1_0000,
            ..SteeringState::ZERO
        };
        let next = apply_steering(near_full, shaped, inputs(1, 0));
        let from_full = 0xc000 - 0x1_0000;
        assert_eq!(
            next.position_318,
            0xc000 + qmul(0x4000, qmul(from_full, from_full))
        );
    }

    #[test]
    fn analog_steering_clamps_the_demand_and_slews_both_angles() {
        let analog = SteeringInputs {
            analog_410: true,
            ..inputs(0, 0x8000)
        };
        let angle = (qmul(0x2aa * 0x1680, 0x8000) / 5760) as i16;
        assert_eq!(angle, 0x155);
        let state = apply_steering(SteeringState::ZERO, params(), analog);
        assert_eq!(state.angle_3e0, 0x5a);
        assert_eq!(state.target_3e2, 0xb4);
        // A huge demand clamps to full lock.
        let hard = SteeringInputs {
            analog_410: true,
            ..inputs(0, 0x7fff_0000)
        };
        let close = SteeringState {
            angle_3e0: 0x2a0,
            target_3e2: 0x2a0,
            ..SteeringState::ZERO
        };
        let next = apply_steering(close, params(), hard);
        assert_eq!((next.angle_3e0, next.target_3e2), (0x2aa, 0x2aa));
        // The digital words are untouched by the analog routine.
        assert_eq!(next.position_318, 0);
    }

    #[test]
    fn analog_slew_folds_differences_above_0x800() {
        // d = 0x900 folds to 0x700, then clamps to 0x5a and is subtracted.
        assert_eq!(slew_toward(0x900, 0, 0x5a), 0x900 - 0x5a);
        assert_eq!(slew_toward(0x800, 0, 0x5a), 0x800 - 0x5a);
        assert_eq!(slew_toward(0, 0x20, 0x5a), 0x20);
        assert_eq!(slew_toward(0, -0x100, 0x5a), -0x5a);
    }
}
