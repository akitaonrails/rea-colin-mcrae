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
//! 1-D gate line along +x, and only the forward-force channel is mapped into the
//! recovered four-byte native control record as analog throttle.
//!
//! See evidence `impl.sim-vertical-path`.

use crate::fixed_point::Fixed16;
use crate::race_state::{
    CarState, Controls, FourWheelIterationParams, RaceState, SimParams, TraceSample,
    WheelPostprocessError,
};
use crate::replay::{
    ReplayActuators, ReplayControlRecord, decode_replay_control, encode_replay_control,
};
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
    drive_stage_with_step(start, controls, config, |state, control| {
        Ok(state.step(control, params))
    })
    .expect("the simplified step is infallible")
}

/// Drive the complete four-wheel producer over the same vertical path.
///
/// # Errors
///
/// Propagates candidate lookup and geometric postprocess failures.
pub fn drive_stage_with_all_four_wheels(
    start: CarState,
    controls: &[Controls],
    config: &StageConfig,
    wheel: &FourWheelIterationParams<'_>,
) -> Result<StageOutcome, WheelPostprocessError> {
    drive_stage_with_step(start, controls, config, |state, control| {
        state.step_with_all_four_wheels(control, wheel)
    })
}

fn drive_stage_with_step(
    start: CarState,
    controls: &[Controls],
    config: &StageConfig,
    mut step: impl FnMut(&mut RaceState, &Controls) -> Result<TraceSample, WheelPostprocessError>,
) -> Result<StageOutcome, WheelPostprocessError> {
    let mut state = RaceState::new(start);
    // One passed-bitmask per checkpoint (car index 0 here).
    let mut checkpoint_bits = vec![0u32; config.checkpoints.len()];
    let mut passed = 0u32;
    let mut finished = false;
    let mut finish_centiseconds = None;
    let mut ticks = 0usize;
    let mut trace = Vec::with_capacity(controls.len() * 4);
    let mut previous_record = ReplayControlRecord::default();

    for control in controls {
        let sample = step(&mut state, control)?;
        ticks += 1;

        let record = encode_replay_control(
            ReplayActuators {
                throttle: control.forward_force.raw(),
                analog_throttle: true,
                ..ReplayActuators::default()
            },
            previous_record,
            true,
        );
        trace.extend_from_slice(&record.0);
        previous_record = record;

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
            return Ok(StageOutcome {
                finished,
                finish_centiseconds,
                checkpoints_passed: passed,
                final_car: sample.car,
                ticks,
                trace,
            });
        }
    }

    Ok(StageOutcome {
        finished,
        finish_centiseconds,
        checkpoints_passed: passed,
        final_car: state.car(),
        ticks,
        trace,
    })
}

/// Decode native four-byte records back to the harness's quantized throttle
/// channel, rejecting a trailing partial record.
#[must_use]
pub fn decode_trace(trace: &[u8]) -> Option<Vec<u8>> {
    if !trace.len().is_multiple_of(4) {
        return None;
    }
    let mut previous_repeat = 0;
    let mut out = Vec::with_capacity(trace.len() / 4);
    for record in trace.as_chunks::<4>().0 {
        let decoded = decode_replay_control(ReplayControlRecord(*record), previous_repeat);
        out.push((decoded.actuators.throttle >> 10) as u8);
        previous_repeat = decoded.next_repeat;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::{Vec3, build_sine_table};
    use crate::race_state::{
        FirstWheelParams, FourWheelPostprocessParams, SecondWheelParams,
        populate_compiled_contact_offsets, populate_compiled_contact_update_inputs,
        populate_force_pass_scales, populate_surface_table_factor,
    };
    use crate::replay::quantize_actuator;
    use crate::vehicle_dynamics::GeometricPostprocessLane;
    use cmr_formats::pe_surface::{SurfaceCoefficientFamilies, SurfaceCoefficientFamily};

    fn four_wheel_params(trig: &[i32]) -> FourWheelIterationParams<'_> {
        let inactive = GeometricPostprocessLane {
            active: false,
            vector_a: Vec3::ZERO,
            vector_b: Vec3::ZERO,
            vector_c: Vec3::ZERO,
            accumulator: Vec3::ZERO,
        };
        let mut wheel = FourWheelIterationParams {
            first: [FirstWheelParams {
                split: 0x10000,
                wheel_constant: 0,
                wheel_vector: [0; 3],
                state_vector: [0; 3],
                scale_c600: 0x10000,
                limit: 0x10000,
                shift: 0,
                factor_c5c0: 0x10000,
                gear: 1,
                config_40c: 0,
                flag_43b: 0,
                config_158: 1,
                gear_ratio: 1,
                is_front_wheel: true,
            }; 4],
            second: std::array::from_fn(|_| SecondWheelParams {
                axis_dot: 0,
                local_68: [0x10000, 0, 0],
                local_98: [0; 3],
                wheel_e8: 0,
                scale_c5c4: 0x10000,
                residual_flag_147a: false,
                config_2ec: 1,
                gear_ratio: 1,
                coefficient_34: 0x10000,
                arctan_table: &crate::legacy_math::SECOND_WHEEL_ARCTAN_TABLE,
                cos_table: trig,
                config_scale: 1,
                transition_gain: 0,
                transition_base: 0,
                alternate_gain: 0,
                alternate_base: 0,
                correction_gate: 1,
                accumulation_limit: 0x10000,
                correction_alternate: 0x10000,
                surface_vector: [-0x10000, 0, 0],
                table_factor: None,
            }),
            contact_vector_indices: [2, 3, 6, 7],
            contact_offsets: [Vec3::ZERO; 4],
            contact_update_inputs: [crate::vehicle_dynamics::ContactVectorUpdateInput {
                update_gate: 0,
                enabled: false,
                vector: Vec3::ZERO,
                basis: Vec3::ZERO,
                scale_528: Fixed16::ONE,
                extra_vector: Vec3::ZERO,
                limit: Fixed16::ONE,
                threshold: Fixed16::ONE,
            }; 8],
            lane_threshold_inputs: crate::compiled_car_initializer::CompiledLaneThresholdInputs {
                lane_words: [0; 8],
                source_y: [0x199a; 8],
            },
            pedal_response: crate::pedal_response::PedalResponseParams::default(),
            lane_time_step: Fixed16::ZERO,
            lane_direction_angle: 0,
            steering: crate::steering_response::SteeringParams::default(),
            steering_car_factor_e188: 0x1_0000,
            steering_surface_factors: [0; 2],
            gearbox: None,
            brake_balance_e1a4: [0x1_0000; 2],
            lane_rebuild: false,
            lane_exchange: false,
            postprocess: FourWheelPostprocessParams {
                lanes: [inactive; 4],
                current_lane: 0,
                argument_scale: Fixed16::ONE,
                base_force: Vec3::ZERO,
                non_wheel_forces: &[],
                wheel_contacts: [true; 4],
            },
            inverse_mass_4c: Fixed16::ONE,
            angular: crate::vehicle_dynamics::AngularOrientationInput {
                momentum: crate::vehicle_dynamics::AngularMomentumStepInput {
                    prior_moment: Vec3::ZERO,
                    contact_moment: Vec3::ZERO,
                    moment_scale: Fixed16::ONE,
                    inertia_scale: Vec3::new(Fixed16::ONE, Fixed16::ONE, Fixed16::ONE),
                    angular_velocity: Vec3::ZERO,
                    time_step: Fixed16::ZERO,
                },
                clamp_enabled: false,
                control: crate::vehicle_dynamics::ControlAxisFlags {
                    mode_3ec: 0,
                    axis_3f4: 0,
                    axis_3f8: 0,
                },
                basis: [Vec3::ZERO; 3],
            },
            engine_smoothing: crate::drivetrain::SmoothedEngineSpeedInput {
                wheel_speeds: [0; 4],
                config_158: 1,
                gear_ratio: 0x10000,
                engine_2e0: 0,
                redline_15c: 0x10000,
                slip_gain: 0,
                engine_load: 0x10000,
            },
            driven_smoothing: crate::race_state::DrivenWheelUpdateParams::default(),
            engine_force: crate::drivetrain::EngineForceInput {
                limiter_444: false,
                cut_448: 0,
                engine_2e0: 0,
                redline_15c: 0x10000,
                gear: 1,
                config_40c: 0,
                response_2d8: 0,
                response_e1ac: 0,
                engine_brake_144: 0,
                throttle_304: 0,
            },
        };
        let families = SurfaceCoefficientFamilies {
            first: SurfaceCoefficientFamily {
                values: [0x20000; 120],
            },
            second: SurfaceCoefficientFamily { values: [0; 120] },
        };
        for index in 0..4 {
            let _ = populate_force_pass_scales(
                &mut wheel.first[index],
                &mut wheel.second[index],
                Fixed16::from_raw(0x30000),
                Fixed16::from_raw(0x20000),
                Fixed16::from_raw(0x40000),
            );
            populate_surface_table_factor(&mut wheel.second[index], &families, 0).unwrap();
        }
        populate_compiled_contact_offsets(
            &mut wheel,
            &crate::compiled_car_initializer::derive_compiled_half_sign_state([4, -6, 8]),
        );
        populate_compiled_contact_update_inputs(
            &mut wheel,
            &crate::compiled_car_initializer::derive_compiled_control_clear_loop(),
        );
        wheel
    }

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
                raw_steer: Default::default(),
                raw_pedals: Default::default(),
                raw_gear_command_314: 0,
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
    fn the_four_wheel_producer_finishes_and_replays_deterministically() {
        let trig = build_sine_table();
        let wheel = four_wheel_params(&trig);
        let script = full_throttle(400);
        let mut start = CarState::at_rest();
        start.motion.angular_velocity = Vec3::new(Fixed16::ONE, Fixed16::ONE, Fixed16::ONE);
        // Grounded and upright in the harness basis (the negated identity), so
        // the wheel-force producer runs its grounded body.
        start.motion.contact_distance_flags = [true; 4];
        start.motion.contact_count_674 = 4;
        start.motion.projection_axis = Vec3::new(Fixed16::ZERO, Fixed16::ONE.neg(), Fixed16::ZERO);
        let first = drive_stage_with_all_four_wheels(start, &script, &stage(), &wheel).unwrap();
        assert!(first.finished);
        assert!(first.ticks < 400);
        assert!(
            first
                .final_car
                .four_wheel
                .postprocess
                .accumulators
                .iter()
                .any(|force| *force != Vec3::ZERO)
        );
        let replay = drive_stage_with_all_four_wheels(start, &script, &stage(), &wheel).unwrap();
        assert_eq!(first, replay);
    }

    #[test]
    fn the_saved_trace_round_trips_through_the_codec() {
        let outcome = drive_stage(CarState::at_rest(), &full_throttle(32), &stage(), &params());
        let decoded = decode_trace(&outcome.trace).unwrap();
        let expected = quantize_actuator(Fixed16::ONE.raw());
        assert!(decoded.iter().all(|&v| v == expected));
        assert_eq!(outcome.trace.len(), outcome.ticks * 4);
        assert!(
            outcome.trace.as_chunks::<4>().0.iter().all(|record| {
                record[0] == 1 && record[1] & 0xfc == 0 && record[1] & 3 == 1 && record[3] == 0
            }),
            "{:02x?}",
            outcome.trace
        );
        assert_eq!(
            decode_trace(&outcome.trace[..outcome.trace.len() - 1]),
            None
        );
    }
}
