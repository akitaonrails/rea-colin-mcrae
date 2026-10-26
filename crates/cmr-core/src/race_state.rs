//! A deterministic offline race-state trace (G09): composes the recovered
//! fixed-step pieces into a reproducible per-tick integration.
//!
//! This is a **composition / determinism harness** over the recovered Q16.16
//! formulas - [`crate::fixed_point`] (the BAM trig and `Vec3`),
//! [`crate::vehicle_dynamics`] (lateral grip, net-accel, drag, velocity
//! integration), and [`crate::stage_timing`] (the fixed-step stage clock). Its
//! purpose is to verify that the subsystems interoperate and that identical
//! inputs yield a bit-for-bit identical trace (all state is integer `Fixed16`,
//! so there is no float non-determinism).
//!
//! It is **not** a claim of trajectory accuracy against the original game. The
//! force that drives the car, the inverse mass, and gravity are explicit harness
//! parameters ([`SimParams`]), because the original force-production wiring
//! (drive torque -> contact force) remains a G02/G03 unknown. The recovered
//! translation half-delta kernel is wired with zero damping after acceleration
//! is computed, so the existing lateral-grip order is preserved while position
//! uses `new_velocity - delta_velocity / 2`. The force model remains a documented
//! harness choice, not a recovered trajectory claim.
//!
//! See evidence `impl.sim-race-state-trace`.

use crate::drivetrain::{
    FirstWheelEffects, FirstWheelInputs, SecondWheelCoreEffects, SecondWheelInputs,
    execute_first_wheel_iteration, execute_second_wheel_core,
};
use crate::fixed_point::{Fixed16, TRIG_TABLE_LEN, Vec3, build_sine_table, cos_bam, sin_bam};
use crate::stage_timing::{advance_stage_clock, elapsed_centiseconds, stage_clock_step};
use crate::vehicle_dynamics::{
    GeometricPostprocessError, GeometricPostprocessLane, LookupDependentCandidateError,
    TranslationStepInput, apply_geometric_lane_postprocess, apply_lateral_grip,
    apply_translation_step, drag, net_accel, sum_translation_lane_forces,
};

/// Fixed per-tick parameters of the integration harness (not recovered game
/// data - explicit inputs that stand in for the unrecovered force wiring).
#[derive(Clone, Copy, Debug)]
pub struct SimParams {
    /// Reciprocal of the car mass applied in [`net_accel`].
    pub inverse_mass: Fixed16,
    /// Constant acceleration added each tick (e.g. gravity / downforce).
    pub gravity: Vec3,
}

/// A driver's per-tick inputs for the harness.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Controls {
    /// Signed BAM steering rate added to the heading each tick.
    pub steer_rate: i16,
    /// Forward driving force magnitude applied along the heading.
    pub forward_force: Fixed16,
    /// Whether the handbrake is held (halves lateral grip).
    pub handbrake: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FirstWheelState {
    pub force: i32,
    pub wheel_speed: i32,
}

/// Runtime inputs for the recovered first-wheel force pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirstWheelParams {
    pub split: i32,
    pub wheel_constant: i32,
    pub wheel_vector: [i32; 3],
    pub state_vector: [i32; 3],
    pub scale_c600: i32,
    pub limit: i32,
    pub shift: u32,
    pub factor_c5c0: i32,
    pub gear: i32,
    pub config_40c: i32,
    pub config_158: i32,
    pub gear_ratio: i32,
    pub is_front_wheel: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SecondWheelState {
    pub ratio_at_3b0: i32,
    pub angle: i16,
    pub component: i32,
    pub accumulators: [i32; 3],
}

/// Runtime inputs for the second per-wheel core after the first iteration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecondWheelParams<'a> {
    pub axis_dot: i32,
    pub local_68: [i32; 3],
    pub local_98: [i32; 3],
    pub wheel_e8: i32,
    pub scale_c5c4: i32,
    pub residual_flag_147a: bool,
    pub config_2ec: i32,
    pub gear_ratio: i32,
    pub coefficient_34: i32,
    pub arctan_table: &'a [i16],
    pub cos_table: &'a [i32],
    pub config_scale: i32,
    pub transition_gain: i32,
    pub transition_base: i32,
    pub alternate_gain: i32,
    pub alternate_base: i32,
    pub correction_gate: i32,
    pub accumulation_limit: i32,
    pub correction_alternate: i32,
    pub surface_vector: [i32; 3],
    pub table_factor: Option<i32>,
}

/// Runtime vector lanes for the recovered four-lane postprocess. The current
/// second-core result replaces `lanes[current_lane].accumulator` before it runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FourWheelPostprocessParams<'a> {
    pub lanes: [GeometricPostprocessLane; 4],
    pub current_lane: usize,
    pub argument_scale: Fixed16,
    pub base_force: Vec3,
    pub non_wheel_forces: &'a [Vec3],
    pub wheel_contacts: [bool; 4],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FourWheelPostprocessState {
    pub accumulators: [Vec3; 4],
    pub qualified: [bool; 4],
    pub angle_measures: [Fixed16; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WheelPostprocessError {
    LaneIndex(usize),
    Candidate(LookupDependentCandidateError),
    Geometry(GeometricPostprocessError),
}

impl From<LookupDependentCandidateError> for WheelPostprocessError {
    fn from(error: LookupDependentCandidateError) -> Self {
        Self::Candidate(error)
    }
}

impl From<GeometricPostprocessError> for WheelPostprocessError {
    fn from(error: GeometricPostprocessError) -> Self {
        Self::Geometry(error)
    }
}

/// The minimal per-car state the harness integrates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CarState {
    /// World position (Q16.16 on the x/z ground plane, y up).
    pub position: Vec3,
    /// World velocity.
    pub velocity: Vec3,
    /// Heading as a 16-bit BAM angle about the y axis.
    pub heading: u16,
    /// Recovered first-wheel force and speed state.
    pub first_wheel: FirstWheelState,
    /// Residual, angle, component and accumulators from the second core.
    pub second_wheel: SecondWheelState,
    /// All four lane accumulators after `FUN_0040fbe0`.
    pub postprocess: FourWheelPostprocessState,
}

impl CarState {
    /// A car at the world origin, at rest, facing +x (heading 0).
    pub const fn at_rest() -> Self {
        CarState {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            heading: 0,
            first_wheel: FirstWheelState {
                force: 0,
                wheel_speed: 0,
            },
            second_wheel: SecondWheelState {
                ratio_at_3b0: 0,
                angle: 0,
                component: 0,
                accumulators: [0; 3],
            },
            postprocess: FourWheelPostprocessState {
                accumulators: [Vec3::ZERO; 4],
                qualified: [false; 4],
                angle_measures: [Fixed16::ZERO; 4],
            },
        }
    }
}

/// A snapshot emitted per tick for the trace.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceSample {
    /// The stage clock after the tick (Q16.16 seconds).
    pub clock: Fixed16,
    /// Elapsed time in centiseconds since the trace started.
    pub centiseconds: i32,
    /// The car state after the tick.
    pub car: CarState,
}

/// The deterministic offline race state: a stage clock plus one car, stepped at
/// the recovered fixed 40 ms / 25 Hz tick.
pub struct RaceState {
    clock: Fixed16,
    start_clock: Fixed16,
    car: CarState,
    trig: [i32; TRIG_TABLE_LEN],
}

impl RaceState {
    /// Start a race from a given car state with the stage clock at zero.
    pub fn new(car: CarState) -> Self {
        RaceState {
            clock: Fixed16::ZERO,
            start_clock: Fixed16::ZERO,
            car,
            trig: build_sine_table(),
        }
    }

    /// The current stage clock (Q16.16 seconds).
    pub fn clock(&self) -> Fixed16 {
        self.clock
    }

    /// The current car state.
    pub fn car(&self) -> CarState {
        self.car
    }

    /// The unit forward vector for the current heading on the x/z plane.
    fn forward_unit(&self) -> Vec3 {
        Vec3::new(
            cos_bam(&self.trig, self.car.heading),
            Fixed16::ZERO,
            sin_bam(&self.trig, self.car.heading),
        )
    }

    /// Advance one fixed tick, composing the recovered formulas in their
    /// observed order: steer, build the forward vector, form the net force
    /// (drive force minus the recovered quadratic drag), integrate acceleration
    /// into velocity, apply the recovered half-delta position update, apply the
    /// lateral-grip constraint, and advance the stage clock.
    pub fn step(&mut self, controls: &Controls, params: &SimParams) -> TraceSample {
        self.step_with_optional_world_force(controls, params, None)
    }

    fn step_with_optional_world_force(
        &mut self,
        controls: &Controls,
        params: &SimParams,
        world_force: Option<Vec3>,
    ) -> TraceSample {
        self.car.heading = self.car.heading.wrapping_add(controls.steer_rate as u16);
        let forward = self.forward_unit();

        let drive_force = world_force.unwrap_or_else(|| forward.scale(controls.forward_force));
        let speed = self.car.velocity.length();
        let net_force = drive_force.sub(drag(self.car.velocity, speed));

        let accel = net_accel(net_force, params.inverse_mass, params.gravity);
        let translation = apply_translation_step(TranslationStepInput {
            position: self.car.position,
            velocity: self.car.velocity,
            cached_speed: Fixed16::ZERO,
            accumulated_force: accel,
            inverse_mass: Fixed16::ONE,
            additive_acceleration: Vec3::ZERO,
        });
        self.car.position = translation.position;
        self.car.velocity = apply_lateral_grip(translation.velocity, forward, controls.handbrake);

        self.clock = advance_stage_clock(self.clock, stage_clock_step(false));

        TraceSample {
            clock: self.clock,
            centiseconds: elapsed_centiseconds(self.clock, self.start_clock),
            car: self.car,
        }
    }

    fn run_first_wheel(&mut self, handbrake: bool, wheel: &FirstWheelParams) -> FirstWheelEffects {
        let effects = execute_first_wheel_iteration(FirstWheelInputs {
            force: self.car.first_wheel.force,
            wheel_speed: self.car.first_wheel.wheel_speed,
            split: wheel.split,
            wheel_constant: wheel.wheel_constant,
            wheel_vector: wheel.wheel_vector,
            state_vector: wheel.state_vector,
            scale_c600: wheel.scale_c600,
            limit: wheel.limit,
            shift: wheel.shift,
            handbrake: i32::from(handbrake),
            factor_c5c0: wheel.factor_c5c0,
            gear: wheel.gear,
            config_40c: wheel.config_40c,
            config_158: wheel.config_158,
            gear_ratio: wheel.gear_ratio,
            is_front_wheel: wheel.is_front_wheel,
        });
        self.car.first_wheel = FirstWheelState {
            force: effects.force_at_540,
            wheel_speed: effects.wheel_speed,
        };
        effects
    }

    /// Run the recovered first-wheel force pass, then translate with its force.
    pub fn step_with_first_wheel(
        &mut self,
        controls: &Controls,
        params: &SimParams,
        wheel: &FirstWheelParams,
    ) -> TraceSample {
        let effects = self.run_first_wheel(controls.handbrake, wheel);
        self.step(
            &Controls {
                forward_force: Fixed16::from_raw(effects.force_at_540),
                ..*controls
            },
            params,
        )
    }

    fn run_second_wheel(
        &mut self,
        prior_speed: i32,
        first: &FirstWheelParams,
        second: &SecondWheelParams<'_>,
    ) -> Result<SecondWheelCoreEffects, LookupDependentCandidateError> {
        let dot = (0..3).fold(0_i32, |sum, index| {
            sum.wrapping_add(
                Fixed16::from_raw(first.wheel_vector[index])
                    .mul(Fixed16::from_raw(first.state_vector[index]))
                    .raw(),
            )
        });
        let effects = execute_second_wheel_core(SecondWheelInputs {
            dot,
            speed_at_2a8: prior_speed,
            speed_at_2b8: self.car.first_wheel.wheel_speed,
            axis_dot: second.axis_dot,
            local_68: second.local_68,
            local_98: second.local_98,
            wheel_e8: second.wheel_e8,
            scale_c5c4: second.scale_c5c4,
            residual_flag_147a: second.residual_flag_147a,
            config_2ec: second.config_2ec,
            gear_ratio: second.gear_ratio,
            coefficient_34: second.coefficient_34,
            arctan_table: second.arctan_table,
            cos_table: second.cos_table,
            config_scale: second.config_scale,
            transition_gain: second.transition_gain,
            transition_base: second.transition_base,
            alternate_gain: second.alternate_gain,
            alternate_base: second.alternate_base,
            correction_gate: second.correction_gate,
            accumulation_limit: second.accumulation_limit,
            correction_alternate: second.correction_alternate,
            surface_vector: second.surface_vector,
            table_factor: second.table_factor,
        })?;
        self.car.second_wheel = SecondWheelState {
            ratio_at_3b0: effects.ratio_at_3b0,
            angle: effects.angle,
            component: effects.component,
            accumulators: effects.accumulators,
        };
        Ok(effects)
    }

    /// Compose the second-wheel core after the first iteration, then translate.
    ///
    /// # Errors
    ///
    /// Propagates a checked candidate-lookup failure.
    pub fn step_with_second_wheel(
        &mut self,
        controls: &Controls,
        params: &SimParams,
        first: &FirstWheelParams,
        second: &SecondWheelParams<'_>,
    ) -> Result<TraceSample, LookupDependentCandidateError> {
        let prior_speed = self.car.first_wheel.wheel_speed;
        let first_effects = self.run_first_wheel(controls.handbrake, first);
        self.run_second_wheel(prior_speed, first, second)?;
        Ok(self.step(
            &Controls {
                forward_force: Fixed16::from_raw(first_effects.force_at_540),
                ..*controls
            },
            params,
        ))
    }

    /// Feed the current second-core lane and the other three lanes through the
    /// recovered `FUN_0040fbe0` postprocess, preserving every lane accumulator.
    pub fn step_with_four_wheel_postprocess(
        &mut self,
        controls: &Controls,
        params: &SimParams,
        first: &FirstWheelParams,
        second: &SecondWheelParams<'_>,
        postprocess: &FourWheelPostprocessParams,
    ) -> Result<TraceSample, WheelPostprocessError> {
        if postprocess.current_lane >= postprocess.lanes.len() {
            return Err(WheelPostprocessError::LaneIndex(postprocess.current_lane));
        }
        let prior_speed = self.car.first_wheel.wheel_speed;
        self.run_first_wheel(controls.handbrake, first);
        self.run_second_wheel(prior_speed, first, second)?;
        let current = self.car.second_wheel.accumulators;
        let mut lanes = postprocess.lanes;
        lanes[postprocess.current_lane].accumulator = Vec3::new(
            Fixed16::from_raw(current[0]),
            Fixed16::from_raw(current[1]),
            Fixed16::from_raw(current[2]),
        );
        let outcome = apply_geometric_lane_postprocess(&mut lanes, postprocess.argument_scale)?;
        self.car.postprocess = FourWheelPostprocessState {
            accumulators: lanes.map(|lane| lane.accumulator),
            qualified: outcome.qualified,
            angle_measures: outcome.angle_measures,
        };
        self.car.second_wheel.accumulators = {
            let current = self.car.postprocess.accumulators[postprocess.current_lane];
            [current.x.raw(), current.y.raw(), current.z.raw()]
        };
        let force = sum_translation_lane_forces(
            postprocess.base_force,
            postprocess.non_wheel_forces,
            std::array::from_fn(|index| {
                (postprocess.wheel_contacts[index], lanes[index].accumulator)
            }),
        );
        Ok(self.step_with_optional_world_force(controls, params, Some(force)))
    }

    /// Run a scripted control sequence, returning the per-tick trace.
    pub fn run_trace(&mut self, controls: &[Controls], params: &SimParams) -> Vec<TraceSample> {
        controls.iter().map(|c| self.step(c, params)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> SimParams {
        SimParams {
            inverse_mass: Fixed16::from_raw(0x8000), // 0.5
            gravity: Vec3::ZERO,
        }
    }

    fn first_wheel_params() -> FirstWheelParams {
        FirstWheelParams {
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
            config_158: 1,
            gear_ratio: 1,
            is_front_wheel: true,
        }
    }

    fn straight_throttle(ticks: usize) -> Vec<Controls> {
        vec![
            Controls {
                steer_rate: 0,
                forward_force: Fixed16::ONE,
                handbrake: false,
            };
            ticks
        ]
    }

    #[test]
    fn the_trace_is_bit_for_bit_reproducible() {
        let script = straight_throttle(64);
        let mut a = RaceState::new(CarState::at_rest());
        let mut b = RaceState::new(CarState::at_rest());
        let trace_a = a.run_trace(&script, &params());
        let trace_b = b.run_trace(&script, &params());
        assert_eq!(trace_a, trace_b);
        assert_eq!(trace_a.len(), 64);
    }

    #[test]
    fn the_clock_advances_one_fixed_step_per_tick() {
        let mut state = RaceState::new(CarState::at_rest());
        let trace = state.run_trace(&straight_throttle(3), &params());
        let step = stage_clock_step(false);
        assert_eq!(trace[0].clock, step);
        assert_eq!(trace[1].clock, step.add(step));
        assert_eq!(trace[2].clock, step.add(step).add(step));
        // Centiseconds are the elapsed readout of the same clock.
        assert_eq!(
            trace[2].centiseconds,
            elapsed_centiseconds(trace[2].clock, Fixed16::ZERO)
        );
    }

    #[test]
    fn forward_throttle_from_rest_builds_forward_velocity_and_moves() {
        // Heading 0 faces +x, so a forward force accelerates along +x only.
        let mut state = RaceState::new(CarState::at_rest());
        let trace = state.run_trace(&straight_throttle(8), &params());
        let last = trace.last().unwrap().car;
        assert!(last.velocity.x > Fixed16::ZERO);
        assert_eq!(last.velocity.y, Fixed16::ZERO);
        assert_eq!(last.velocity.z, Fixed16::ZERO);
        // The car has moved forward (position integrated from velocity).
        assert!(last.position.x > Fixed16::ZERO);
        assert_eq!(last.position.z, Fixed16::ZERO);
    }

    #[test]
    fn position_uses_the_recovered_half_delta_translation() {
        let mut state = RaceState::new(CarState::at_rest());
        let sample = state.step(&straight_throttle(1)[0], &params());
        let acceleration = net_accel(
            Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
            params().inverse_mass,
            Vec3::ZERO,
        );
        let velocity = crate::vehicle_dynamics::integrate_velocity(Vec3::ZERO, acceleration);
        let delta = velocity.sub(Vec3::ZERO);
        let expected_position = velocity.sub(delta.scale(Fixed16::from_raw(0x8000)));
        assert_eq!(sample.car.position, expected_position);
        assert_eq!(sample.car.velocity, velocity);
    }

    #[test]
    fn first_wheel_force_and_speed_drive_the_translation() {
        let mut car = CarState::at_rest();
        car.first_wheel.wheel_speed = -1;
        let mut state = RaceState::new(car);
        let wheel = first_wheel_params();
        let first = state.step_with_first_wheel(&straight_throttle(1)[0], &params(), &wheel);
        assert_eq!(first.car.first_wheel.force, 0x10004);
        assert_eq!(first.car.first_wheel.wheel_speed, 0x10003);
        assert!(first.car.velocity.x > Fixed16::ZERO);
        let second = state.step_with_first_wheel(&straight_throttle(1)[0], &params(), &wheel);
        assert_eq!(second.car.first_wheel.force, -0xfffc);
        assert_eq!(second.car.first_wheel.wheel_speed, 7);
        assert!(second.car.position.x > first.car.position.x);
    }

    #[test]
    fn second_wheel_core_composes_after_the_first_iteration() {
        let mut car = CarState::at_rest();
        car.first_wheel.wheel_speed = -1;
        let mut state = RaceState::new(car);
        let trig = build_sine_table();
        let second = SecondWheelParams {
            axis_dot: 0,
            local_68: [0x10000, 0, 0],
            local_98: [0; 3],
            wheel_e8: 0,
            scale_c5c4: 0x10000,
            residual_flag_147a: false,
            config_2ec: 1,
            gear_ratio: 1,
            coefficient_34: 0x10000,
            arctan_table: &[],
            cos_table: &trig,
            config_scale: 1,
            transition_gain: 0,
            transition_base: 0,
            alternate_gain: 0,
            alternate_base: 0,
            correction_gate: 1,
            accumulation_limit: 0x10000,
            correction_alternate: 0x10000,
            surface_vector: [0x10000, 0, 0],
            table_factor: None,
        };
        let sample = state
            .step_with_second_wheel(
                &straight_throttle(1)[0],
                &params(),
                &first_wheel_params(),
                &second,
            )
            .unwrap();
        assert_eq!(sample.car.second_wheel.ratio_at_3b0, -0x10003);
        assert_eq!(sample.car.second_wheel.angle, 0);
        assert_eq!(sample.car.second_wheel.component, 1);
        assert_eq!(sample.car.second_wheel.accumulators, [-6554, 0, 0]);
        assert!(sample.car.velocity.x > Fixed16::ZERO);
    }

    #[test]
    fn four_lane_postprocess_consumes_the_current_second_core_lane() {
        let vector = |x, y, z| {
            Vec3::new(
                Fixed16::from_raw(x),
                Fixed16::from_raw(y),
                Fixed16::from_raw(z),
            )
        };
        let inactive = GeometricPostprocessLane {
            active: false,
            vector_a: Vec3::ZERO,
            vector_b: Vec3::ZERO,
            vector_c: Vec3::ZERO,
            accumulator: Vec3::ZERO,
        };
        let mut lanes = [inactive; 4];
        lanes[3] = GeometricPostprocessLane {
            active: true,
            vector_a: vector(0x8000, -1, 0),
            vector_b: vector(0x10000, 0, 0),
            vector_c: vector(0, 0x10000, 0x10000),
            accumulator: vector(-6554, 0, 0),
        };
        let postprocess = FourWheelPostprocessParams {
            lanes,
            current_lane: 3,
            argument_scale: Fixed16::ONE,
            base_force: Vec3::ZERO,
            non_wheel_forces: &[],
            wheel_contacts: [false, false, false, true],
        };
        let trig = build_sine_table();
        let second = SecondWheelParams {
            axis_dot: 0,
            local_68: [0x10000, 0, 0],
            local_98: [0; 3],
            wheel_e8: 0,
            scale_c5c4: 0x10000,
            residual_flag_147a: false,
            config_2ec: 1,
            gear_ratio: 1,
            coefficient_34: 0x10000,
            arctan_table: &[],
            cos_table: &trig,
            config_scale: 1,
            transition_gain: 0,
            transition_base: 0,
            alternate_gain: 0,
            alternate_base: 0,
            correction_gate: 1,
            accumulation_limit: 0x10000,
            correction_alternate: 0x10000,
            surface_vector: [0x10000, 0, 0],
            table_factor: None,
        };
        let mut car = CarState::at_rest();
        car.first_wheel.wheel_speed = -1;
        let mut state = RaceState::new(car);
        let sample = state
            .step_with_four_wheel_postprocess(
                &straight_throttle(1)[0],
                &params(),
                &first_wheel_params(),
                &second,
                &postprocess,
            )
            .unwrap();
        let outcome = apply_geometric_lane_postprocess(&mut lanes, Fixed16::ONE).unwrap();
        assert!(outcome.qualified[3]);
        assert_eq!(sample.car.postprocess.qualified, outcome.qualified);
        assert_eq!(
            sample.car.postprocess.angle_measures,
            outcome.angle_measures
        );
        assert_eq!(
            sample.car.postprocess.accumulators,
            lanes.map(|lane| lane.accumulator)
        );
        let current = lanes[3].accumulator;
        assert_eq!(
            sample.car.second_wheel.accumulators,
            [current.x.raw(), current.y.raw(), current.z.raw()]
        );

        assert!(sample.car.velocity.x < Fixed16::ZERO);
        assert!(sample.car.position.x < Fixed16::ZERO);

        let before = state.car;
        let invalid = FourWheelPostprocessParams {
            current_lane: 4,
            ..postprocess
        };
        assert_eq!(
            state.step_with_four_wheel_postprocess(
                &straight_throttle(1)[0],
                &params(),
                &first_wheel_params(),
                &second,
                &invalid,
            ),
            Err(WheelPostprocessError::LaneIndex(4))
        );
        assert_eq!(state.car, before);
    }

    #[test]
    fn drag_bounds_the_speed_under_constant_force() {
        // With quadratic drag opposing a constant force, speed stops growing
        // without bound: a later tick's acceleration is smaller than an early one.
        let mut state = RaceState::new(CarState::at_rest());
        let trace = state.run_trace(&straight_throttle(200), &params());
        let early_gain = trace[1].car.velocity.x.sub(trace[0].car.velocity.x);
        let late_gain = trace[199].car.velocity.x.sub(trace[198].car.velocity.x);
        assert!(late_gain < early_gain);
    }

    #[test]
    fn steering_rotates_the_forward_axis_into_z() {
        // Steering a quarter turn (0x400 BAM) over the run turns the car so
        // velocity gains a z component.
        let script = vec![
            Controls {
                steer_rate: 0x40,
                forward_force: Fixed16::ONE,
                handbrake: false,
            };
            16
        ];
        let mut state = RaceState::new(CarState::at_rest());
        let trace = state.run_trace(&script, &params());
        let last = trace.last().unwrap().car;
        assert_ne!(last.velocity.z, Fixed16::ZERO);
    }
}
