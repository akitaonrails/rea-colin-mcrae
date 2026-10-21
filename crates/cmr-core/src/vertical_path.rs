//! The start -> drive -> finish -> save-replay vertical path (G10).
//!
//! This composes the recovered subsystems end to end over the deterministic
//! race state ([`crate::race_state`]):
//!
//! 1. **start** - a stage run begins with the clock at zero and the car at rest;
//! 2. **drive** - a scripted control sequence is stepped at the fixed 40 ms tick,
//!    crossing checkpoints (de-duplicated with [`crate::stage_timing`]'s per-car
//!    passed-bitmask) and recording each tick's drive actuator into the replay
//!    input trace using the recovered quantize + XOR-delta codec
//!    ([`crate::replay`]);
//! 3. **finish** - crossing the finish line freezes the elapsed time in
//!    centiseconds (the `stage_timing` readout);
//! 4. **save-replay** - the recorded inputs, re-run from the same start, reproduce
//!    the *identical* finish time and final car state - the defining property of
//!    the input-based re-simulation replay (G08).
//!
//! Faithful here: the composition order, the checkpoint de-dup, the timing
//! readout, the replay codec primitives, and the re-sim determinism. A harness
//! simplification (documented, not claimed as original behaviour): the stage is a
//! 1-D gate line along +x, and the serialized actuator byte uses the recovered
//! `quantize_actuator`/`delta_encode` over the forward-force channel only - the
//! exact per-byte record bit layout is a G08 unknown.
//!
//! See evidence `impl.sim-vertical-path`.

use crate::fixed_point::Fixed16;
use crate::race_state::{CarState, Controls, RaceState, SimParams};
use crate::replay::{delta_decode, delta_encode, quantize_actuator};
use crate::stage_timing::{checkpoint_passed, mark_checkpoint_passed};

/// A checkpoint gate on the +x stage line.
#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    /// The x coordinate the car must reach to trigger the gate.
    pub x: Fixed16,
    /// The pacenote/checkpoint id (carried, not interpreted here).
    pub id: u16,
}

/// A minimal stage layout for the vertical path: ordered checkpoints and a
/// finish line, all as +x gates.
#[derive(Clone, Debug)]
pub struct StageConfig {
    /// Checkpoints in ascending x order.
    pub checkpoints: Vec<Checkpoint>,
    /// The finish-line x coordinate.
    pub finish_x: Fixed16,
}

/// The outcome of driving a stage run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageOutcome {
    /// Whether the finish line was crossed within the scripted ticks.
    pub finished: bool,
    /// The finish time in centiseconds, if the car finished.
    pub finish_centiseconds: Option<i32>,
    /// The number of checkpoints crossed (de-duplicated).
    pub checkpoints_passed: u32,
    /// The car state at the end of the run (or at finish).
    pub final_car: CarState,
    /// The number of ticks simulated.
    pub ticks: usize,
    /// The per-tick forward-actuator bytes, XOR-delta coded (the save trace).
    pub trace: Vec<u8>,
}

/// Drive a scripted control sequence over a stage from a given start, composing
/// the recovered subsystems. Stops at the finish line or when the script ends.
pub fn drive_stage(
    start: CarState,
    controls: &[Controls],
    config: &StageConfig,
    params: &SimParams,
) -> StageOutcome {
    let mut state = RaceState::new(start);
    // One passed-bitmask per checkpoint (car index 0 here).
    let mut checkpoint_bits = vec![0u32; config.checkpoints.len()];
    let mut passed = 0u32;
    let mut finished = false;
    let mut finish_centiseconds = None;
    let mut ticks = 0usize;
    let mut trace = Vec::with_capacity(controls.len());
    let mut previous_byte = 0u8;

    for control in controls {
        let sample = state.step(control, params);
        ticks += 1;

        // Save the forward actuator into the replay trace (quantize + XOR-delta).
        let quantized = quantize_actuator(control.forward_force.raw());
        let encoded = delta_encode(previous_byte, quantized);
        trace.push(encoded);
        previous_byte = quantized;

        // Checkpoint crossings (de-duplicated per car via the passed-bitmask).
        for (index, checkpoint) in config.checkpoints.iter().enumerate() {
            if sample.car.position.x >= checkpoint.x
                && !checkpoint_passed(checkpoint_bits[index], 0)
            {
                checkpoint_bits[index] = mark_checkpoint_passed(checkpoint_bits[index], 0);
                passed += 1;
            }
        }

        // Finish line: freeze the time and stop.
        if sample.car.position.x >= config.finish_x {
            finished = true;
            finish_centiseconds = Some(sample.centiseconds);
            return StageOutcome {
                finished,
                finish_centiseconds,
                checkpoints_passed: passed,
                final_car: sample.car,
                ticks,
                trace,
            };
        }
    }

    StageOutcome {
        finished,
        finish_centiseconds,
        checkpoints_passed: passed,
        final_car: state.car(),
        ticks,
        trace,
    }
}

/// Decode a saved forward-actuator trace back to the quantized values, the
/// inverse of the XOR-delta save in [`drive_stage`] (a self-consistency check on
/// the replay codec; the quantization itself is lossy).
pub fn decode_trace(trace: &[u8]) -> Vec<u8> {
    let mut previous = 0u8;
    let mut out = Vec::with_capacity(trace.len());
    for &encoded in trace {
        let value = delta_decode(previous, encoded);
        out.push(value);
        previous = value;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> SimParams {
        SimParams {
            inverse_mass: Fixed16::from_raw(0x8000),
            gravity: crate::fixed_point::Vec3::ZERO,
        }
    }

    fn full_throttle(ticks: usize) -> Vec<Controls> {
        vec![
            Controls {
                steer_rate: 0,
                forward_force: Fixed16::ONE,
                handbrake: false,
            };
            ticks
        ]
    }

    fn stage() -> StageConfig {
        StageConfig {
            checkpoints: vec![
                Checkpoint {
                    x: Fixed16::from_int(1),
                    id: 10,
                },
                Checkpoint {
                    x: Fixed16::from_int(4),
                    id: 11,
                },
            ],
            finish_x: Fixed16::from_int(9),
        }
    }

    #[test]
    fn a_full_run_finishes_passing_both_checkpoints() {
        let outcome = drive_stage(
            CarState::at_rest(),
            &full_throttle(400),
            &stage(),
            &params(),
        );
        assert!(outcome.finished);
        assert_eq!(outcome.checkpoints_passed, 2);
        let cs = outcome.finish_centiseconds.expect("finished");
        assert!(cs > 0);
        // The finish is at or past the finish line.
        assert!(outcome.final_car.position.x >= Fixed16::from_int(9));
        // The run stopped at the finish tick, not the full script.
        assert!(outcome.ticks < 400);
    }

    #[test]
    fn replaying_the_recorded_inputs_reproduces_the_outcome() {
        // Record a run.
        let script = full_throttle(400);
        let first = drive_stage(CarState::at_rest(), &script, &stage(), &params());
        // Re-run the SAME inputs from the SAME start: identical finish and state
        // (the input-based re-simulation guarantee).
        let replay = drive_stage(CarState::at_rest(), &script, &stage(), &params());
        assert_eq!(first, replay);
    }

    #[test]
    fn an_unfinished_run_reports_progress_without_a_time() {
        // Too few ticks to reach the finish line.
        let outcome = drive_stage(CarState::at_rest(), &full_throttle(5), &stage(), &params());
        assert!(!outcome.finished);
        assert_eq!(outcome.finish_centiseconds, None);
        assert_eq!(outcome.ticks, 5);
    }

    #[test]
    fn the_saved_trace_round_trips_through_the_codec() {
        let outcome = drive_stage(CarState::at_rest(), &full_throttle(32), &stage(), &params());
        // The XOR-delta save decodes back to a constant quantized forward value
        // (constant full throttle -> zero deltas after the first).
        let decoded = decode_trace(&outcome.trace);
        let expected = quantize_actuator(Fixed16::ONE.raw());
        assert!(decoded.iter().all(|&v| v == expected));
        // Constant input -> every delta after the first is zero.
        assert!(outcome.trace[1..].iter().all(|&d| d == 0));
    }
}
