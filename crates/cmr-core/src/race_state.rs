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

use crate::drivetrain::{FirstWheelInputs, execute_first_wheel_iteration};
use crate::fixed_point::{Fixed16, TRIG_TABLE_LEN, Vec3, build_sine_table, cos_bam, sin_bam};
use crate::stage_timing::{advance_stage_clock, elapsed_centiseconds, stage_clock_step};
use crate::vehicle_dynamics::{
    TranslationStepInput, apply_lateral_grip, apply_translation_step, drag, net_accel,
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
        self.car.heading = self.car.heading.wrapping_add(controls.steer_rate as u16);
        let forward = self.forward_unit();

        let drive_force = forward.scale(controls.forward_force);
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

    /// Run the recovered first-wheel force pass, then translate with its force.
    pub fn step_with_first_wheel(
        &mut self,
        controls: &Controls,
        params: &SimParams,
        wheel: &FirstWheelParams,
    ) -> TraceSample {
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
            handbrake: i32::from(controls.handbrake),
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
        self.step(
            &Controls {
                forward_force: Fixed16::from_raw(effects.force_at_540),
                ..*controls
            },
            params,
        )
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
