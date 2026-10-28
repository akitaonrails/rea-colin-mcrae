//! The pedal half of the per-car control processor `FUN_0041b5b0`.
//!
//! The mechanics driver `FUN_00407690` calls the processor once per car per
//! tick, after the low-speed steer cut and before every integrator. Its pedal
//! half turns the raw control words written by the in-race mapper
//! (`+0x304` throttle, `+0x308` brake, `+0x30c` handbrake) into the response
//! words the drivetrain reads: `+0x2d8` (throttle response, read by the engine
//! force), `+0x2ec` (brake response, read by the second wheel loop) and
//! `+0x2f4` (handbrake torque).
//!
//! Observations (instruction level, `0x41b6bf..0x41b7cc`, `FUN_0041c9b0`,
//! `FUN_0041cb40`, `FUN_0041cc20`):
//!
//! - Analog throttle (`+0x414 != 0`) sets `+0x2d8 = qmul(+0x148, +0x304)`, or
//!   zero when the pedal is released. Digital throttle runs a ramp `+0x2dc`
//!   that rises by `+0x150` while pressed (cap `0x10000`, where `+0x2d8 =
//!   +0x148` exactly) and falls by `qmul(+0x150, 0x30000)` when released (below
//!   zero both words clear). "Pressed" also counts the brake when `+0x408` is
//!   set.
//! - Analog brake (`+0x418 != 0`): released clears `+0x2ec` only; pressed with
//!   `+0x408` clear sets `+0x2ec = qmul(+0x16c, +0x308)`; pressed with `+0x408`
//!   set drives the throttle response instead, `+0x2d8 = qmul(+0x148, +0x308)`,
//!   and clears `+0x2ec`. Digital brake ramps `+0x2f0` by `+0x174` toward
//!   `+0x16c` while pressed with `+0x408` clear and otherwise clears both
//!   words; afterwards, with analog throttle and `+0x408` set, `+0x2d8 =
//!   qmul(+0x148, +0x308)`.
//! - The handbrake always ramps `+0x2f8` by `+0x17c` toward `+0x184` into
//!   `+0x2f4` while `+0x30c` is set, and clears both otherwise.
//! - Every unsaturated ramp output is `qmul(max, table[ftol(qmul(ramp,
//!   0x5a0000) * (1/5760)) & 0xfff])` against the engine trig table
//!   `DAT_0082d380`: a quarter-sine ease. The `.rdata` double at `0x4e62f8` is
//!   slightly above `1/5760`, so the truncating `__ftol` index equals the
//!   integer quotient toward zero for every signed 32-bit product; under 24-bit
//!   x87 precision it would differ only for products far outside the clamped
//!   ramp range.
//!
//! Inferences: `+0x408` swaps the pedal roles (it looks like a reverse mode),
//! and the shape is an ease-in. Neither is established. The gearbox, steering
//! and flag parts of the processor are separate and not reconstructed here.

use crate::fixed_point::Fixed16;

/// `qmul(ramp, 0x5a0000)`: a ramp of `1.0` maps to 90 (degrees, in Q16).
const RAMP_DEGREES: Fixed16 = Fixed16::from_raw(0x5a_0000);
/// The released digital throttle falls three times faster than it rises.
const THROTTLE_RELEASE_RATE: Fixed16 = Fixed16::from_raw(0x3_0000);
/// Degrees-in-Q16 per trig-table step: `360 * 65536 / 4096`.
const DEGREES_Q16_PER_TABLE_STEP: i32 = 5760;
/// Ramps saturate at `1.0`.
const RAMP_FULL: i32 = 0x1_0000;

/// Per-car response scalars from the control record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PedalResponseParams {
    /// `+0x148`: full throttle response.
    pub throttle_max_148: i32,
    /// `+0x150`: digital throttle ramp rate per tick.
    pub throttle_rate_150: i32,
    /// `+0x16c`: full brake response.
    pub brake_max_16c: i32,
    /// `+0x174`: digital brake ramp rate per tick.
    pub brake_rate_174: i32,
    /// `+0x17c`: handbrake ramp rate per tick.
    pub handbrake_rate_17c: i32,
    /// `+0x184`: full handbrake torque.
    pub handbrake_max_184: i32,
}

/// The raw pedal words the in-race mapper writes each tick (`FUN_00449580`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RawPedals {
    pub throttle_304: i32,
    pub brake_308: i32,
    pub throttle_analog_414: bool,
    pub brake_analog_418: bool,
}

impl PedalInputs {
    /// Combine the mapper's pedal words with the handbrake and the per-car
    /// `+0x408` mode.
    #[must_use]
    pub fn from_raw(raw: RawPedals, handbrake_30c: bool, mode_408: i32) -> Self {
        Self {
            throttle_304: raw.throttle_304,
            brake_308: raw.brake_308,
            handbrake_30c,
            mode_408,
            throttle_analog_414: raw.throttle_analog_414,
            brake_analog_418: raw.brake_analog_418,
        }
    }
}

/// Raw per-tick pedal words.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PedalInputs {
    pub throttle_304: i32,
    pub brake_308: i32,
    pub handbrake_30c: bool,
    /// `+0x408`: swaps which pedal drives the throttle response.
    pub mode_408: i32,
    /// `+0x414`: throttle came from the analog path.
    pub throttle_analog_414: bool,
    /// `+0x418`: brake came from the analog path.
    pub brake_analog_418: bool,
}

/// The persistent ramp and response words the processor owns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PedalResponseState {
    pub response_2d8: i32,
    pub throttle_ramp_2dc: i32,
    pub brake_response_2ec: i32,
    pub brake_ramp_2f0: i32,
    pub handbrake_torque_2f4: i32,
    pub handbrake_ramp_2f8: i32,
}

impl PedalResponseState {
    /// Every ramp and response at zero, the loader-cleared state.
    pub const ZERO: Self = Self {
        response_2d8: 0,
        throttle_ramp_2dc: 0,
        brake_response_2ec: 0,
        brake_ramp_2f0: 0,
        handbrake_torque_2f4: 0,
        handbrake_ramp_2f8: 0,
    };
}

fn qmul(left: i32, right: i32) -> i32 {
    Fixed16::from_raw(left).mul(Fixed16::from_raw(right)).raw()
}

/// The quarter-sine ease shared by the three ramps: `qmul(max,
/// table[(qmul(ramp, 0x5a0000) / 5760) & 0xfff])`, the quotient truncating
/// toward zero like `__ftol`.
///
/// Returns `None` if `trig` is shorter than the 4096-entry engine table.
#[must_use]
pub fn eased_response(max: i32, ramp: i32, trig: &[i32]) -> Option<i32> {
    let degrees = qmul(ramp, RAMP_DEGREES.raw());
    let index = (degrees / DEGREES_Q16_PER_TABLE_STEP) & 0xfff;
    trig.get(index as usize).map(|&sine| qmul(max, sine))
}

/// One rising ramp step: add `rate`, saturate at `1.0` with the exact `max`,
/// else ease. Returns `(ramp, response)`.
fn rise(ramp: i32, rate: i32, max: i32, trig: &[i32]) -> Option<(i32, i32)> {
    let ramp = ramp.wrapping_add(rate);
    if ramp > RAMP_FULL {
        return Some((RAMP_FULL, max));
    }
    Some((ramp, eased_response(max, ramp, trig)?))
}

/// The digital throttle ramp `FUN_0041c9b0`.
fn digital_throttle(
    state: &mut PedalResponseState,
    input: PedalInputs,
    params: PedalResponseParams,
    trig: &[i32],
) -> Option<()> {
    let pressed = input.throttle_304 != 0 || (input.mode_408 != 0 && input.brake_308 != 0);
    if pressed {
        (state.throttle_ramp_2dc, state.response_2d8) = rise(
            state.throttle_ramp_2dc,
            params.throttle_rate_150,
            params.throttle_max_148,
            trig,
        )?;
        return Some(());
    }
    let fall = qmul(params.throttle_rate_150, THROTTLE_RELEASE_RATE.raw());
    let ramp = state.throttle_ramp_2dc.wrapping_sub(fall);
    if ramp < 0 {
        state.throttle_ramp_2dc = 0;
        state.response_2d8 = 0;
    } else {
        state.throttle_ramp_2dc = ramp;
        state.response_2d8 = eased_response(params.throttle_max_148, ramp, trig)?;
    }
    Some(())
}

/// Apply the pedal half of `FUN_0041b5b0` in native order: throttle, brake,
/// then handbrake.
///
/// Returns `None`, leaving `state` unchanged, if `trig` is shorter than the
/// 4096-entry engine table.
#[must_use]
pub fn apply_pedal_responses(
    state: PedalResponseState,
    input: PedalInputs,
    params: PedalResponseParams,
    trig: &[i32],
) -> Option<PedalResponseState> {
    let mut next = state;
    let brake_drives_throttle = input.mode_408 != 0;

    if input.throttle_analog_414 {
        next.response_2d8 = qmul(params.throttle_max_148, input.throttle_304);
    } else {
        digital_throttle(&mut next, input, params, trig)?;
    }

    if input.brake_analog_418 {
        if input.brake_308 == 0 {
            next.brake_response_2ec = 0;
        } else if brake_drives_throttle {
            next.response_2d8 = qmul(params.throttle_max_148, input.brake_308);
            next.brake_response_2ec = 0;
        } else {
            next.brake_response_2ec = qmul(params.brake_max_16c, input.brake_308);
        }
    } else {
        if input.brake_308 != 0 && !brake_drives_throttle {
            (next.brake_ramp_2f0, next.brake_response_2ec) = rise(
                next.brake_ramp_2f0,
                params.brake_rate_174,
                params.brake_max_16c,
                trig,
            )?;
        } else {
            next.brake_ramp_2f0 = 0;
            next.brake_response_2ec = 0;
        }
        if input.throttle_analog_414 && brake_drives_throttle {
            next.response_2d8 = qmul(params.throttle_max_148, input.brake_308);
        }
    }

    if input.handbrake_30c {
        (next.handbrake_ramp_2f8, next.handbrake_torque_2f4) = rise(
            next.handbrake_ramp_2f8,
            params.handbrake_rate_17c,
            params.handbrake_max_184,
            trig,
        )?;
    } else {
        next.handbrake_ramp_2f8 = 0;
        next.handbrake_torque_2f4 = 0;
    }
    Some(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::build_sine_table;

    fn params() -> PedalResponseParams {
        PedalResponseParams {
            throttle_max_148: 0x4_0000,
            throttle_rate_150: 0x4000,
            brake_max_16c: 0x2_0000,
            brake_rate_174: 0x8000,
            handbrake_rate_17c: 0x1_0000,
            handbrake_max_184: 0x3_0000,
        }
    }

    fn digital(throttle: i32, brake: i32) -> PedalInputs {
        PedalInputs {
            throttle_304: throttle,
            brake_308: brake,
            ..PedalInputs::default()
        }
    }

    #[test]
    fn eased_response_is_a_quarter_sine_indexed_by_ramp_over_64() {
        let trig = build_sine_table();
        for ramp in [0, 63, 64, 0x4000, 0x8000, 0xffff, 0x1_0000] {
            assert_eq!(
                eased_response(0x4_0000, ramp, &trig),
                Some(qmul(0x4_0000, trig[(ramp >> 6) as usize])),
                "{ramp:#x}"
            );
        }
        assert_eq!(eased_response(0x4_0000, 0x1_0000, &trig), Some(0x4_0000));
        // Negative products truncate toward zero before the 0xfff mask.
        assert_eq!(
            eased_response(0x1_0000, -64, &trig),
            Some(trig[0xfff]) // index -1 masked to 0xfff
        );
        assert_eq!(eased_response(0x1_0000, -63, &trig), Some(trig[0]));
        assert_eq!(eased_response(1, 0x8000, &trig[..0x100]), None);
    }

    #[test]
    fn digital_throttle_eases_up_saturates_exactly_and_falls_three_times_faster() {
        let trig = build_sine_table();
        let mut state = PedalResponseState::default();
        for tick in 1..=4 {
            state = apply_pedal_responses(state, digital(1, 0), params(), &trig).unwrap();
            assert_eq!(state.throttle_ramp_2dc, tick * 0x4000);
            assert_eq!(
                Some(state.response_2d8),
                eased_response(0x4_0000, tick * 0x4000, &trig)
            );
        }
        state = apply_pedal_responses(state, digital(1, 0), params(), &trig).unwrap();
        assert_eq!(state.throttle_ramp_2dc, 0x1_0000);
        assert_eq!(state.response_2d8, 0x4_0000);

        state = apply_pedal_responses(state, digital(0, 0), params(), &trig).unwrap();
        assert_eq!(state.throttle_ramp_2dc, 0x4000);
        assert_eq!(
            Some(state.response_2d8),
            eased_response(0x4_0000, 0x4000, &trig)
        );
        state = apply_pedal_responses(state, digital(0, 0), params(), &trig).unwrap();
        assert_eq!((state.throttle_ramp_2dc, state.response_2d8), (0, 0));
    }

    #[test]
    fn a_ramp_landing_exactly_on_zero_eases_to_zero_instead_of_clearing() {
        let trig = build_sine_table();
        let state = PedalResponseState {
            throttle_ramp_2dc: 0xc000,
            response_2d8: 0x1234,
            ..PedalResponseState::default()
        };
        let next = apply_pedal_responses(state, digital(0, 0), params(), &trig).unwrap();
        assert_eq!(next.throttle_ramp_2dc, 0);
        assert_eq!(next.response_2d8, qmul(0x4_0000, trig[0]));
    }

    #[test]
    fn mode_408_lets_the_brake_drive_the_throttle_and_mutes_the_brake_response() {
        let trig = build_sine_table();
        let swapped = PedalInputs {
            mode_408: 1,
            ..digital(0, 1)
        };
        let state = PedalResponseState {
            brake_ramp_2f0: 0x8000,
            brake_response_2ec: 0x1111,
            ..PedalResponseState::default()
        };
        let next = apply_pedal_responses(state, swapped, params(), &trig).unwrap();
        assert_eq!(next.throttle_ramp_2dc, 0x4000);
        assert_eq!((next.brake_ramp_2f0, next.brake_response_2ec), (0, 0));
    }

    #[test]
    fn digital_brake_ramps_while_held_and_clears_on_release() {
        let trig = build_sine_table();
        let held = apply_pedal_responses(
            PedalResponseState::default(),
            digital(0, 1),
            params(),
            &trig,
        )
        .unwrap();
        assert_eq!(held.brake_ramp_2f0, 0x8000);
        assert_eq!(
            Some(held.brake_response_2ec),
            eased_response(0x2_0000, 0x8000, &trig)
        );
        let full = apply_pedal_responses(
            PedalResponseState {
                brake_ramp_2f0: 0xc000,
                ..held
            },
            digital(0, 1),
            params(),
            &trig,
        )
        .unwrap();
        assert_eq!(
            (full.brake_ramp_2f0, full.brake_response_2ec),
            (0x1_0000, 0x2_0000)
        );
        let released = apply_pedal_responses(full, digital(0, 0), params(), &trig).unwrap();
        assert_eq!(
            (released.brake_ramp_2f0, released.brake_response_2ec),
            (0, 0)
        );
    }

    #[test]
    fn analog_pedals_scale_directly_and_keep_the_digital_brake_ramp() {
        let trig = build_sine_table();
        let analog = PedalInputs {
            throttle_304: 0x8000,
            brake_308: 0xfc00,
            throttle_analog_414: true,
            brake_analog_418: true,
            ..PedalInputs::default()
        };
        let state = PedalResponseState {
            brake_ramp_2f0: 0x4000,
            throttle_ramp_2dc: 0x4000,
            ..PedalResponseState::default()
        };
        let next = apply_pedal_responses(state, analog, params(), &trig).unwrap();
        assert_eq!(next.response_2d8, qmul(0x4_0000, 0x8000));
        assert_eq!(next.brake_response_2ec, qmul(0x2_0000, 0xfc00));
        // Neither analog path touches the digital ramps.
        assert_eq!(
            (next.throttle_ramp_2dc, next.brake_ramp_2f0),
            (0x4000, 0x4000)
        );

        let released = apply_pedal_responses(
            next,
            PedalInputs {
                throttle_304: 0,
                brake_308: 0,
                ..analog
            },
            params(),
            &trig,
        )
        .unwrap();
        assert_eq!((released.response_2d8, released.brake_response_2ec), (0, 0));
        assert_eq!(released.brake_ramp_2f0, 0x4000);

        // With `+0x408` the analog brake drives `+0x2d8` and mutes `+0x2ec`.
        let swapped = apply_pedal_responses(
            next,
            PedalInputs {
                mode_408: 1,
                throttle_304: 0,
                ..analog
            },
            params(),
            &trig,
        )
        .unwrap();
        assert_eq!(swapped.response_2d8, qmul(0x4_0000, 0xfc00));
        assert_eq!(swapped.brake_response_2ec, 0);
    }

    #[test]
    fn digital_brake_with_analog_throttle_in_mode_408_drives_the_throttle_response() {
        let trig = build_sine_table();
        let input = PedalInputs {
            throttle_304: 0x2000,
            brake_308: 0x8000,
            mode_408: 1,
            throttle_analog_414: true,
            ..PedalInputs::default()
        };
        let next =
            apply_pedal_responses(PedalResponseState::default(), input, params(), &trig).unwrap();
        assert_eq!(next.response_2d8, qmul(0x4_0000, 0x8000));
        assert_eq!(next.brake_response_2ec, 0);
    }

    #[test]
    fn handbrake_torque_ramps_while_held() {
        let trig = build_sine_table();
        let held = PedalInputs {
            handbrake_30c: true,
            ..PedalInputs::default()
        };
        let next =
            apply_pedal_responses(PedalResponseState::default(), held, params(), &trig).unwrap();
        assert_eq!(
            (next.handbrake_ramp_2f8, next.handbrake_torque_2f4),
            (0x1_0000, 0x3_0000)
        );
        let released =
            apply_pedal_responses(next, PedalInputs::default(), params(), &trig).unwrap();
        assert_eq!(
            (released.handbrake_ramp_2f8, released.handbrake_torque_2f4),
            (0, 0)
        );
    }

    #[test]
    fn a_short_trig_table_leaves_no_partial_update() {
        let state = PedalResponseState::default();
        assert_eq!(
            apply_pedal_responses(state, digital(1, 0), params(), &[0; 16]),
            None
        );
    }
}
