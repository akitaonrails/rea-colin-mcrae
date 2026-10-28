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

use crate::compiled_car_initializer::{
    CompiledControlClearLoop, CompiledControlFamilies, CompiledFourWheelConstants,
    CompiledHalfSignState, CompiledLaneThresholdInputs, CompiledPostLaneBasisEffects,
    derive_compiled_lane_thresholds,
};
use crate::drivetrain::{
    DrivenSmoothingScalarInput, DrivenSmoothingScalars, DrivenWheelSmoothingInput,
    EngineForceInput, FirstWheelEffects, FirstWheelInputs, ForcePassScales, SecondWheelCoreEffects,
    SecondWheelInputs, SmoothedEngineSpeedInput, SmoothedSpeedGateInput, advance_idle_oscillation,
    advance_idle_reset, advance_residual_enable, apply_rev_limiter,
    derive_driven_smoothing_scalars, derive_engine_force, derive_force_pass_scales,
    derive_gear_ratio, derive_smoothed_engine_speed, equalize_wheel_speeds,
    execute_first_wheel_iteration, execute_second_wheel_core, integrate_driven_wheel_speed,
    integrate_engine_from_raw_wheels, integrate_idle_engine, smoothed_speed_uses_tracking,
    track_smoothed_wheel_speed,
};
use crate::fixed_point::{Fixed16, TRIG_TABLE_LEN, Vec3, build_sine_table, cos_bam, sin_bam};
use crate::legacy_math::sqrt_q16;
use crate::live_static_collision_car_frame::{
    ActiveCellRecordBuildError, LiveStaticCollisionCarFrameInput,
    LiveStaticCollisionFrameBridgeError, PreviousCarXz, UpperHullDeformationScalars,
    build_active_cell_record, build_live_static_collision_car_frame,
};
use crate::pedal_response::{
    PedalInputs, PedalResponseParams, PedalResponseState, RawPedals, apply_pedal_responses,
};
use crate::stage_timing::{advance_stage_clock, elapsed_centiseconds, stage_clock_step};
use crate::static_collision_kernel::{
    RecoveredStaticCollisionKernel, StaticKernelContext, StaticKernelError,
};
use crate::static_collision_response::{ResponseBasis, StaticDamageOwner};
use crate::static_collision_scene_builder::ParsedStaticCollisionScene;
use crate::static_world_collision::{
    StaticCollisionExecutionError, StaticContactHistory, resolve_static_world_collisions,
};
use crate::steering_response::{SteeringInputs, SteeringParams, SteeringState, apply_steering};
use crate::vehicle_dynamics::{
    AngularOrientationInput, ContactBasisInput, ContactDeadzoneGate, ContactVectorUpdateInput,
    GeometricPostprocessError, GeometricPostprocessLane, LaneDirectionSources,
    LookupDependentCandidateError, OutOfControlState, RawSteer, SurfaceCoefficientIndexError,
    TranslationStepInput, apply_angular_orientation_pipeline, apply_contact_vector_updates,
    apply_geometric_lane_postprocess, apply_lateral_grip, apply_low_speed_steer_cut,
    apply_translation_step, build_contact_basis_vectors, construct_lane_directions,
    contact_count_scale, contact_global_update_gate, derive_lane_direction_pair, drag, net_accel,
    resolve_lane_direction_mode, resting_flag_700, stabilize_lane_source_s, sum_contact_moments,
    sum_translation_lane_forces, transform_contact_force, upright_flag_404,
};
use cmr_formats::pe_surface::SurfaceCoefficientFamilies;

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
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Controls {
    /// Signed BAM steering rate added to the heading each tick.
    pub steer_rate: i16,
    /// Forward driving force magnitude applied along the heading.
    pub forward_force: Fixed16,
    /// Whether the handbrake is held (halves lateral grip).
    pub handbrake: bool,
    /// The recovered raw steer magnitudes `+0x2fc/+0x300`, which the
    /// four-wheel integrator's deadzones read after the low-speed cut.
    pub raw_steer: RawSteer,
    /// The recovered raw pedal words `+0x304/+0x308` and analog flags
    /// `+0x414/+0x418`, which the four-wheel control processor ramps.
    pub raw_pedals: RawPedals,
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
    pub flag_43b: i32,
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

/// Select a compiled control-family word by the first-wheel gear, derive its
/// ratio, and update both wheel parameter sets. Invalid gears change nothing.
#[must_use]
pub fn populate_compiled_gear_ratio(
    first: &mut FirstWheelParams,
    second: &mut SecondWheelParams<'_>,
    families: &CompiledControlFamilies,
    factor_c860: Fixed16,
) -> Option<Fixed16> {
    let raw = *families.raw_copy.get(usize::try_from(first.gear).ok()?)?;
    let ratio = derive_gear_ratio(Fixed16::from_raw(raw), factor_c860);
    first.gear_ratio = ratio.raw();
    second.gear_ratio = ratio.raw();
    Some(ratio)
}

/// Derive the main force-pass scales once and apply them to both wheel inputs.
#[must_use]
pub fn populate_force_pass_scales(
    first: &mut FirstWheelParams,
    second: &mut SecondWheelParams<'_>,
    state_52c: Fixed16,
    scale_eac70: Fixed16,
    step_eac6c: Fixed16,
) -> ForcePassScales {
    let scales = derive_force_pass_scales(state_52c, scale_eac70, step_eac6c);
    first.scale_c600 = scales.scale_c600.raw();
    first.factor_c5c0 = scales.factor_c5c0.raw();
    second.scale_c5c4 = scales.scale_c5c4.raw();
    scales
}

fn raw_dot(left: [i32; 3], right: [i32; 3]) -> i32 {
    (0..3).fold(0_i32, |sum, index| {
        sum.wrapping_add(
            Fixed16::from_raw(left[index])
                .mul(Fixed16::from_raw(right[index]))
                .raw(),
        )
    })
}

/// Populate the shared first/second-loop vectors from constructed directions
/// and the current per-wheel contact vector.
pub fn populate_wheel_vector_inputs(
    first: &mut FirstWheelParams,
    second: &SecondWheelParams<'_>,
    direction_a: Vec3,
) {
    first.wheel_vector = [
        direction_a.x.raw(),
        direction_a.y.raw(),
        direction_a.z.raw(),
    ];
    first.state_vector = second.surface_vector;
}

/// Derive second-core wheel projection and `0x147a` inputs from native state.
pub fn populate_derived_second_inputs(
    first: &FirstWheelParams,
    second: &mut SecondWheelParams<'_>,
    direction_b: Vec3,
    lane: usize,
) {
    second.wheel_e8 = raw_dot(first.wheel_vector, first.state_vector);
    second.axis_dot = direction_b
        .dot(Vec3::new(
            Fixed16::from_raw(second.surface_vector[0]),
            Fixed16::from_raw(second.surface_vector[1]),
            Fixed16::from_raw(second.surface_vector[2]),
        ))
        .raw();
    let config_gate = if lane < 2 {
        first.config_158 != 0
    } else {
        first.config_158 != 0x10000
    };
    second.residual_flag_147a = first.flag_43b != 0 && config_gate;
}

/// Populate the second-core surface coefficient from the reviewed first family.
/// Invalid surface indices leave the existing table/override selection unchanged.
///
/// # Errors
///
/// Returns the shared surface-index error for indices outside the 120-entry table.
pub fn populate_surface_table_factor(
    second: &mut SecondWheelParams<'_>,
    families: &SurfaceCoefficientFamilies,
    surface_index: usize,
) -> Result<(), SurfaceCoefficientIndexError> {
    let coefficient =
        *families
            .first
            .values
            .get(surface_index)
            .ok_or(SurfaceCoefficientIndexError {
                index: surface_index,
            })?;
    second.table_factor = Some(coefficient);
    Ok(())
}

/// Build the contact vectors and select one for a second-wheel surface input.
#[must_use]
pub fn populate_contact_basis_vector(
    second: &mut SecondWheelParams<'_>,
    input: ContactBasisInput,
    vector_index: usize,
) -> Option<Vec3> {
    if vector_index >= 8 {
        return None;
    }
    let vector = build_contact_basis_vectors(input)[vector_index];
    second.surface_vector = [vector.x.raw(), vector.y.raw(), vector.z.raw()];
    Some(vector)
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

/// Runtime gates and scalar sources for the driven `FUN_004088c0` smoothing path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrivenWheelUpdateParams {
    pub scalars: DrivenSmoothingScalarInput,
    pub mode_408: i32,
    pub throttle_304: i32,
    pub brake_308: i32,
    pub split_170: i32,
    pub time_step: i32,
}

impl Default for DrivenWheelUpdateParams {
    fn default() -> Self {
        Self {
            scalars: DrivenSmoothingScalarInput {
                suspension_298: [0; 4],
                longitudinal: 0,
                gear_index: 0,
                gear_ratio: 0,
                gear_ratio_190: 0,
                redline_15c: 0,
                response_1f0: 0,
                engine_2e0: 0,
                response_160: 0,
            },
            mode_408: 0,
            throttle_304: 0,
            brake_308: 0,
            split_170: 0,
            time_step: 0x10000,
        }
    }
}

/// Complete per-wheel runtime inputs for one native-order force tick.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FourWheelIterationParams<'a> {
    pub first: [FirstWheelParams; 4],
    pub second: [SecondWheelParams<'a>; 4],
    pub contact_vector_indices: [usize; 4],
    pub contact_offsets: [Vec3; 4],
    /// Per-tick `FUN_0040d400` scalar/gate inputs; vectors are replaced from the
    /// current contact-basis output before the reverse traversal.
    pub contact_update_inputs: [ContactVectorUpdateInput; 8],
    /// Lane/source-Y values for the native `FUN_004126b0` enable/count scan.
    pub lane_threshold_inputs: CompiledLaneThresholdInputs,
    /// Per-car scalars for the control processor's pedal ramps
    /// (`+0x148/+0x150/+0x16c/+0x174/+0x17c/+0x184`).
    pub pedal_response: PedalResponseParams,
    /// Timestep used by the persistent `+0x5d4` stabilizer.
    pub lane_time_step: Fixed16,
    /// The per-car-type base angle `DAT_0050e4fc[type]`; the step adds this
    /// tick's wheel steer angle `+0x3e0` before rotating the lane source.
    pub lane_direction_angle: i16,
    /// Steering scalars, the per-car-type factor `DAT_0050e188[type]` and the
    /// two front-wheel `DAT_004f74ac` values for the control processor.
    pub steering: SteeringParams,
    pub steering_car_factor_e188: i32,
    pub steering_surface_factors: [i32; 2],
    /// Resolved global/callback inputs; Rebuild takes precedence over Exchange.
    pub lane_rebuild: bool,
    pub lane_exchange: bool,
    pub postprocess: FourWheelPostprocessParams<'a>,
    /// State `+0x4c`, the common prefix's reciprocal of `+0x48`, which the
    /// integrator multiplies into the damped force (`0x40a9f0`).
    pub inverse_mass_4c: Fixed16,
    /// Moment scales/base contact input and angular gates; stateful vectors come
    /// from `MotionState` at each step.
    pub angular: AngularOrientationInput,
    /// Explicit engine redline/slip/load inputs; speeds and gear/mode fields are
    /// replaced from current native state before each update.
    pub engine_smoothing: SmoothedEngineSpeedInput,
    /// Suspension/engine/gate inputs; wheel forces, speeds, gear, limiter and
    /// handbrake come from the current tick.
    pub driven_smoothing: DrivenWheelUpdateParams,
    /// Explicit engine response/brake inputs for the `state+0x530` producer.
    pub engine_force: EngineForceInput,
}

/// Populate wheel contact offsets from the compiled sign vectors at native state
/// offsets `+0x5c`, `+0x68`, `+0x74`, and `+0x80`.
pub fn populate_compiled_contact_offsets(
    wheel: &mut FourWheelIterationParams<'_>,
    state: &CompiledHalfSignState,
) {
    const INDICES: [[usize; 3]; 4] = [[6, 7, 8], [3, 4, 5], [9, 10, 11], [12, 13, 14]];
    wheel.contact_offsets = INDICES.map(|[x, y, z]| {
        Vec3::new(
            Fixed16::from_raw(state.words[x]),
            Fixed16::from_raw(state.words[y]),
            Fixed16::from_raw(state.words[z]),
        )
    });
}

/// Populate the integrator's mass, moment scale and inertia, and the control
/// processor's pedal scalars and `+0x408` mode, from the recovered initializer
/// constants.
pub fn populate_compiled_four_wheel_constants(
    wheel: &mut FourWheelIterationParams<'_>,
    constants: &CompiledFourWheelConstants,
) {
    wheel.inverse_mass_4c = Fixed16::from_raw(constants.state_word_4c);
    wheel.angular.momentum.moment_scale = Fixed16::from_raw(constants.state_word_48);
    let [x, y, z] = constants.inertia_50_54_58.map(Fixed16::from_raw);
    wheel.angular.momentum.inertia_scale = Vec3::new(x, y, z);
    wheel.pedal_response = PedalResponseParams {
        throttle_max_148: constants.control_word_148,
        throttle_rate_150: constants.control_word_150,
        brake_max_16c: constants.control_word_16c,
        brake_rate_174: constants.control_word_174,
        handbrake_rate_17c: constants.control_word_17c,
        handbrake_max_184: constants.control_word_184,
    };
    wheel.driven_smoothing.mode_408 = constants.control_word_408;
    wheel.steering.centering_188 = constants.control_word_188;
    wheel.steering.max_angle_3e4 = constants.control_u16_3e4 as i16;
    wheel.steering.flags_440 = constants.control_byte_440;
}

/// Populate each update's limit and threshold from the interleaved compiled
/// control-clear words at `+0x31c..+0x358`.
pub fn populate_compiled_contact_update_inputs(
    wheel: &mut FourWheelIterationParams<'_>,
    state: &CompiledControlClearLoop,
) {
    for (index, input) in wheel.contact_update_inputs.iter_mut().enumerate() {
        input.limit = Fixed16::from_raw(state.words[index * 2]);
        input.threshold = Fixed16::from_raw(state.words[index * 2 + 1]);
    }
}

/// Transform current postprocessed wheel forces through the pre-update basis and
/// accumulate their contact moments in native lane order.
#[must_use]
pub fn derive_wheel_contact_moment(
    basis: [Vec3; 3],
    lanes: &[GeometricPostprocessLane; 4],
    contacts: [bool; 4],
    offsets: [Vec3; 4],
) -> Vec3 {
    sum_contact_moments(
        &[],
        std::array::from_fn(|index| {
            (
                contacts[index],
                transform_contact_force(basis, lanes[index].accumulator),
                offsets[index],
            )
        }),
    )
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FourWheelState {
    pub first: [FirstWheelState; 4],
    pub second: [SecondWheelState; 4],
    /// Separate smoothed speeds at `control+0x2b8..+0x2c4`.
    pub smoothed_wheel_speeds: [i32; 4],
    pub postprocess: FourWheelPostprocessState,
    /// Persistent `FUN_0040d400` destinations, including skipped records.
    pub contact_updates: [Vec3; 8],
    /// Prior native velocity magnitude at `state+0xec`.
    pub speed_ec: i32,
    /// Per-frame residual-enable countdown at `control+0x43b`.
    pub residual_enable: u8,
    /// Engine-speed input at `control+0x2e0` and smoothed output at `+0x2e8`.
    pub engine_2e0: i32,
    pub engine_speed_2e8: i32,
    /// Rev-limiter state and cut at `control+0x444/+0x448`.
    pub limiter_444: bool,
    pub limiter_cut_448: i32,
    /// Engine force and neutral/idle state at `state+0x530/+0x6e8/+0x6ec`.
    pub engine_force_530: i32,
    pub idle_6e8: i32,
    pub idle_countdown_6ec: i16,
    /// The control processor's ramps and responses, including the throttle
    /// response `control+0x2d8` read by the engine force and lane stabilizer.
    pub pedals: PedalResponseState,
    /// The control processor's steering words (`+0x318/+0x114/+0x3e0/+0x3e2`).
    pub steering: SteeringState,
    /// Control `+0x404`, recomputed at the start of each four-wheel tick.
    pub upright_404: bool,
    /// Resting flag at `state+0x700`, written by the integrator each tick.
    pub resting_700: bool,
}

/// Apply neutral-gear zeroing or the ordinary post-step tracking update.
pub fn update_smoothed_wheel_speeds(state: &mut FourWheelState, gear_ratio: i32) {
    if gear_ratio == 0 {
        state.smoothed_wheel_speeds = [0; 4];
        return;
    }
    for index in 0..4 {
        state.smoothed_wheel_speeds[index] = track_smoothed_wheel_speed(
            state.first[index].wheel_speed,
            state.smoothed_wheel_speeds[index],
        );
    }
}

fn driven_smoothing_scalars(
    state: &FourWheelState,
    first: &FirstWheelParams,
    params: &DrivenWheelUpdateParams,
) -> DrivenSmoothingScalars {
    derive_driven_smoothing_scalars(DrivenSmoothingScalarInput {
        gear_index: first.gear,
        gear_ratio: first.gear_ratio,
        engine_2e0: state.engine_2e0,
        ..params.scalars
    })
}

fn update_driven_wheel_speeds(
    state: &mut FourWheelState,
    first: &[FirstWheelParams; 4],
    handbrake: bool,
    params: &DrivenWheelUpdateParams,
    scalars: DrivenSmoothingScalars,
) {
    if first[0].gear_ratio == 0 {
        state.smoothed_wheel_speeds = [0; 4];
        return;
    }
    for (lane, wheel) in first.iter().enumerate() {
        let raw = state.first[lane].wheel_speed;
        let current = state.smoothed_wheel_speeds[lane];
        state.smoothed_wheel_speeds[lane] =
            if smoothed_speed_uses_tracking(SmoothedSpeedGateInput {
                lane,
                mode_408: params.mode_408,
                throttle_304: params.throttle_304,
                brake_308: params.brake_308,
                handbrake_30c: i32::from(handbrake),
                config_158: wheel.config_158,
                split_170: params.split_170,
                limiter_444: i32::from(state.limiter_444),
            }) {
                track_smoothed_wheel_speed(raw, current)
            } else {
                integrate_driven_wheel_speed(DrivenWheelSmoothingInput {
                    raw,
                    current,
                    drive_force_2c8: state.first[lane].force,
                    slip_gain: scalars.slip_gain,
                    direction_gain: scalars.direction_gain,
                    time_step: params.time_step,
                    signed_speed_limit: scalars.signed_speed_limit,
                    gear: wheel.gear,
                    config_40c: wheel.config_40c,
                    config_158: wheel.config_158,
                    gear_ratio: wheel.gear_ratio,
                    is_rear_wheel: lane >= 2,
                })
            };
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WheelPostprocessError {
    LaneIndex(usize),
    ContactVectorIndex { lane: usize, vector_index: usize },
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

/// Orientation and angular inputs shared by the native motion/contact passes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MotionState {
    /// The three axes published through physics `+0x2a0`.
    pub basis: [Vec3; 3],
    /// The distinct AK11 projection axis at `+0x324/+0x328/+0x32c`.
    pub projection_axis: Vec3,
    /// Primary angular velocity at `+0xf0/+0xf4/+0xf8`.
    pub angular_velocity: Vec3,
    /// Persistent prior moment at `+0x3d8/+0x3dc/+0x3e0`.
    pub moment: Vec3,
    /// Arithmetic-half state at `+0x2c/+0x30/+0x34` from the compiled initializer.
    pub compiled_half_words: Vec3,
    /// Persistent stabilized direction source at `+0x5d4/+0x5d8/+0x5dc`.
    pub lane_source_s: Vec3,
    /// Four strict contact-distance flags at `+0x68c..+0x698`.
    pub contact_distance_flags: [bool; 4],
    /// Count of those flags at `+0x674`, consumed by static response mode 4.
    pub contact_count_674: i32,
    /// Setup/live collision deformation scalars at `+0x638/+0x63c/+0x640`.
    pub upper_hull_deformation: UpperHullDeformationScalars,
}

/// Cross-tick static-collision state owned by the live car.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveCollisionState {
    /// Linear correction at `+0x3cc..+0x3d4`; angular `+0x3d8` has the single
    /// owner [`MotionState::moment`].
    pub pending_linear_3cc: Vec3,
    pub history: StaticContactHistory,
    pub response_state_680: bool,
    pub recovery_latch_688: bool,
    /// `+0xbc/+0xc0/+0xc4`, written by the angular pass's moment-overflow
    /// trigger when it sets `+0x680/+0x688`.
    pub out_of_control_axis_bc: Vec3,
}

impl LiveCollisionState {
    /// The out-of-control words in the shape the angular pass reads and sets.
    #[must_use]
    pub fn out_of_control(&self) -> OutOfControlState {
        OutOfControlState {
            out_of_control_680: self.response_state_680,
            recovery_latch_688: self.recovery_latch_688,
            axis_snapshot_bc: self.out_of_control_axis_bc,
        }
    }

    /// Publish the angular pass's out-of-control result back into the owners.
    pub fn set_out_of_control(&mut self, state: OutOfControlState) {
        self.response_state_680 = state.out_of_control_680;
        self.recovery_latch_688 = state.recovery_latch_688;
        self.out_of_control_axis_bc = state.axis_snapshot_bc;
    }

    /// Snapshot and clear the three linear words before any collision pass can
    /// accumulate their replacements for the next tick.
    #[must_use]
    pub fn consume_pending_linear(&mut self) -> Vec3 {
        std::mem::take(&mut self.pending_linear_3cc)
    }
}

impl MotionState {
    /// Neutral harness values; no physical orientation or trajectory is claimed.
    pub const fn harness_default() -> Self {
        Self {
            basis: [
                Vec3::new(Fixed16::ONE.neg(), Fixed16::ZERO, Fixed16::ZERO),
                Vec3::new(Fixed16::ZERO, Fixed16::ONE.neg(), Fixed16::ZERO),
                Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE.neg()),
            ],
            projection_axis: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            moment: Vec3::ZERO,
            compiled_half_words: Vec3::new(Fixed16::ONE, Fixed16::ONE, Fixed16::ONE),
            lane_source_s: Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
            contact_distance_flags: [false; 4],
            contact_count_674: 0,
            upper_hull_deformation: UpperHullDeformationScalars {
                scale_638: Fixed16::ZERO,
                scale_63c: Fixed16::ZERO,
                scale_640: Fixed16::ZERO,
            },
        }
    }

    /// Copy the first three recovered half-words from the compiled state record.
    pub fn apply_compiled_half_state(&mut self, state: &CompiledHalfSignState) {
        self.compiled_half_words = Vec3::new(
            Fixed16::from_raw(state.words[0]),
            Fixed16::from_raw(state.words[1]),
            Fixed16::from_raw(state.words[2]),
        );
    }

    /// Publish the compiled AK11 normal and rebuilt orientation basis.
    pub fn apply_compiled_post_lane_basis(
        &mut self,
        normal: [i32; 3],
        effects: &CompiledPostLaneBasisEffects,
    ) {
        self.projection_axis = Vec3::new(
            Fixed16::from_raw(normal[0]),
            Fixed16::from_raw(normal[1]),
            Fixed16::from_raw(normal[2]),
        );
        self.basis = [effects.vector_258, effects.vector_264, effects.vector_270].map(|vector| {
            Vec3::new(
                Fixed16::from_raw(vector[0]),
                Fixed16::from_raw(vector[1]),
                Fixed16::from_raw(vector[2]),
            )
        });
    }

    /// Form the six angular inputs and add the current native velocity field.
    #[must_use]
    pub fn contact_basis(self, velocity: Vec3) -> ContactBasisInput {
        ContactBasisInput {
            rows: self.basis,
            angular: [
                self.angular_velocity.x,
                self.angular_velocity.y,
                self.angular_velocity.z,
                self.compiled_half_words.x,
                self.compiled_half_words.y,
                self.compiled_half_words.z,
            ],
            velocity,
        }
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
    /// Orientation and angular state consumed by contact production.
    pub motion: MotionState,
    /// Persistent collision corrections, history, and recovery flags.
    pub collision: LiveCollisionState,
    /// Recovered first-wheel force and speed state.
    pub first_wheel: FirstWheelState,
    /// Residual, angle, component and accumulators from the second core.
    pub second_wheel: SecondWheelState,
    /// All four lane accumulators after `FUN_0040fbe0`.
    pub postprocess: FourWheelPostprocessState,
    /// Independent state for the complete four-wheel producer path.
    pub four_wheel: FourWheelState,
}

impl CarState {
    /// A car at the world origin, at rest, facing +x (heading 0).
    pub const fn at_rest() -> Self {
        CarState {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            heading: 0,
            motion: MotionState::harness_default(),
            collision: LiveCollisionState {
                pending_linear_3cc: Vec3::ZERO,
                history: StaticContactHistory {
                    previous_ids: [0; crate::static_world_collision::STATIC_CONTACT_CAPACITY],
                    previous_len: 0,
                    current_ids: [0; crate::static_world_collision::STATIC_CONTACT_CAPACITY],
                    current_len: 0,
                    current_overflow: false,
                },
                response_state_680: false,
                recovery_latch_688: false,
                out_of_control_axis_bc: Vec3::ZERO,
            },
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
            four_wheel: FourWheelState {
                first: [FirstWheelState {
                    force: 0,
                    wheel_speed: 0,
                }; 4],
                second: [SecondWheelState {
                    ratio_at_3b0: 0,
                    angle: 0,
                    component: 0,
                    accumulators: [0; 3],
                }; 4],
                smoothed_wheel_speeds: [0; 4],
                postprocess: FourWheelPostprocessState {
                    accumulators: [Vec3::ZERO; 4],
                    qualified: [false; 4],
                    angle_measures: [Fixed16::ZERO; 4],
                },
                contact_updates: [Vec3::ZERO; 8],
                speed_ec: 0,
                residual_enable: 10,
                engine_2e0: 0,
                engine_speed_2e8: 0,
                limiter_444: false,
                limiter_cut_448: 0,
                engine_force_530: 0,
                idle_6e8: 0,
                idle_countdown_6ec: 0,
                pedals: PedalResponseState::ZERO,
                steering: SteeringState::ZERO,
                upright_404: false,
                resting_700: false,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveStaticCollisionStepError<E> {
    ActiveCell(ActiveCellRecordBuildError),
    Frame(LiveStaticCollisionFrameBridgeError),
    Execute(StaticCollisionExecutionError<StaticKernelError<E>>),
}

/// The deterministic offline race state: a stage clock plus one car, stepped at
/// the recovered fixed 40 ms / 25 Hz tick.
#[derive(Clone)]
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

    /// Resolve one post-integration static-scene pass and atomically publish
    /// depenetration, alternate velocity, next-tick corrections, history, and
    /// persistent response flags into the live car.
    pub fn resolve_static_collision_after_step<D: StaticDamageOwner>(
        &mut self,
        previous_center_xz: PreviousCarXz,
        parsed: &ParsedStaticCollisionScene,
        damage_owner: &mut D,
    ) -> Result<usize, LiveStaticCollisionStepError<D::Error>> {
        let car = self.car;
        let primary_cell =
            build_active_cell_record(&parsed.grid, &parsed.main_cell_centers, car.position)
                .map_err(LiveStaticCollisionStepError::ActiveCell)?;
        let deformation = car
            .motion
            .upper_hull_deformation
            .bind(car.motion.basis[0], car.motion.basis[2]);
        let live = build_live_static_collision_car_frame(LiveStaticCollisionCarFrameInput {
            previous_center_xz,
            current_center: car.position,
            current_basis: car.motion.basis,
            half_extents: car.motion.compiled_half_words,
            projection_axis_258: car.motion.basis[0],
            projection_axis_264: car.motion.basis[1],
            deformation: Some(deformation),
            primary_cell,
        });
        let frame = live
            .into_static_frame(
                car.collision.pending_linear_3cc,
                car.motion.moment,
                Vec3::ZERO,
                car.motion.compiled_half_words,
            )
            .map_err(LiveStaticCollisionStepError::Frame)?;
        let speed_ec =
            Fixed16::from_raw(sqrt_q16(car.velocity.dot(car.velocity).raw() as u32) as i32);
        let projection = live.projection;
        let context = StaticKernelContext {
            projected_corners: projection.projected_corners,
            projection_axis: projection.axis,
            projection_perpendicular: projection.perpendicular,
            projection_axis_radius: projection.axis_radius,
            projection_perpendicular_radius: projection.perpendicular_radius,
            y_gate_552f60: projection.min_y,
            y_gate_552f80: projection.max_y,
            velocity_d4: car.velocity,
            alternate_axis_324: car.motion.projection_axis,
            projected_axis_270: car.motion.basis[2],
            response_basis_2a0: ResponseBasis {
                rows: car.motion.basis,
            },
            out_of_control_680: car.collision.response_state_680,
            out_of_control_688: car.collision.recovery_latch_688,
            // The windowed compatibility session currently owns player slot
            // zero and the fresh-process subtype byte remains zero.
            car_type_738: 0,
            car_subtype_739: 0,
            contact_count_674: car.motion.contact_count_674,
            velocity_normalizer_ec: speed_ec,
            fallback_axis_258: car.motion.basis[0],
        };
        let mut kernel =
            RecoveredStaticCollisionKernel::with_damage_owner(context, [], damage_owner);
        let effects = resolve_static_world_collisions(
            &parsed.scene,
            &frame,
            car.collision.history,
            &mut kernel,
        )
        .map_err(LiveStaticCollisionStepError::Execute)?;

        let mut committed = car;
        committed.collision.pending_linear_3cc = effects.linear_impulse_3cc;
        committed.motion.moment = effects.angular_impulse_3d8;
        committed.collision.history = effects.history;
        if let Some(kernel_state) = kernel.state() {
            committed.position = kernel_state.position;
            committed.velocity = kernel_state.velocity_d4;
            committed.collision.response_state_680 = kernel_state.out_of_control_680;
            committed.collision.recovery_latch_688 = kernel_state.out_of_control_688;
        }
        let contact_count = effects.contacts.len();
        self.car = committed;
        Ok(contact_count)
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
        let mut sample = self.step_with_optional_world_force(controls, params, None, false);
        self.finish_mechanics_tick();
        sample.car = self.car;
        sample
    }

    /// The end-of-tick clear the per-car mechanics driver `FUN_00407690` applies
    /// after `FUN_0040a4a0` and the drivetrain passes `FUN_0040d0c0`/`FUN_004088c0`:
    /// it zeroes `+0x3cc..+0x3e0`, so the next static collision pass accumulates
    /// both corrections from zero. The linear words are already consumed and
    /// cleared by the translation step; the angular moment `+0x3d8` is cleared
    /// here. Neither drivetrain pass reads `+0x3d8`, so the decayed value
    /// `FUN_0040a4a0` writes back is dead once the angular pass has used it.
    fn finish_mechanics_tick(&mut self) {
        self.car.motion.moment = Vec3::ZERO;
    }

    fn step_with_optional_world_force(
        &mut self,
        controls: &Controls,
        params: &SimParams,
        world_force: Option<Vec3>,
        contacts_active: bool,
    ) -> TraceSample {
        self.car.heading = self.car.heading.wrapping_add(controls.steer_rate as u16);
        let forward = self.forward_unit();

        let recovered_force_path = world_force.is_some();
        let drive_force = world_force.unwrap_or_else(|| forward.scale(controls.forward_force));
        let speed = self.car.velocity.length();
        let net_force = drive_force.sub(drag(self.car.velocity, speed));

        let accel = net_accel(net_force, params.inverse_mass, params.gravity);
        let pending_collision_acceleration = self.car.collision.consume_pending_linear();
        let translation = apply_translation_step(TranslationStepInput {
            position: self.car.position,
            velocity: self.car.velocity,
            cached_speed: if recovered_force_path {
                speed
            } else {
                Fixed16::ZERO
            },
            contacts_active,
            accumulated_force: accel,
            inverse_mass: Fixed16::ONE,
            additive_acceleration: pending_collision_acceleration,
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

    fn execute_first_wheel_state(
        state: &mut FirstWheelState,
        handbrake: bool,
        wheel: &FirstWheelParams,
    ) -> FirstWheelEffects {
        let effects = execute_first_wheel_iteration(FirstWheelInputs {
            force: state.force,
            wheel_speed: state.wheel_speed,
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
        *state = FirstWheelState {
            force: effects.force_at_540,
            wheel_speed: effects.wheel_speed,
        };
        effects
    }

    fn run_first_wheel(&mut self, handbrake: bool, wheel: &FirstWheelParams) -> FirstWheelEffects {
        Self::execute_first_wheel_state(&mut self.car.first_wheel, handbrake, wheel)
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

    fn execute_second_wheel_state(
        state: &mut SecondWheelState,
        prior_speed: i32,
        current_speed: i32,
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
            speed_at_2b8: current_speed,
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
        *state = SecondWheelState {
            ratio_at_3b0: effects.ratio_at_3b0,
            angle: effects.angle,
            component: effects.component,
            accumulators: effects.accumulators,
        };
        Ok(effects)
    }

    fn run_second_wheel(
        &mut self,
        prior_speed: i32,
        first: &FirstWheelParams,
        second: &SecondWheelParams<'_>,
    ) -> Result<SecondWheelCoreEffects, LookupDependentCandidateError> {
        Self::execute_second_wheel_state(
            &mut self.car.second_wheel,
            prior_speed,
            self.car.first_wheel.wheel_speed,
            first,
            second,
        )
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
        let mut sample = self.step_with_optional_world_force(
            controls,
            params,
            Some(force),
            postprocess.wheel_contacts.contains(&true),
        );
        self.finish_mechanics_tick();
        sample.car = self.car;
        Ok(sample)
    }

    /// Rebuild contact vectors from the pre-step velocity, run all four first
    /// iterations in reverse order, equalize each axle, run all second iterations
    /// in reverse order, postprocess, update all eight contacts, then translate.
    ///
    /// # Errors
    ///
    /// Propagates contact-index, candidate lookup and geometric postprocess failures.
    pub fn step_with_all_four_wheels(
        &mut self,
        controls: &Controls,
        wheel: &FourWheelIterationParams<'_>,
    ) -> Result<TraceSample, WheelPostprocessError> {
        if wheel.postprocess.current_lane >= wheel.postprocess.lanes.len() {
            return Err(WheelPostprocessError::LaneIndex(
                wheel.postprocess.current_lane,
            ));
        }
        let contact_vectors =
            build_contact_basis_vectors(self.car.motion.contact_basis(self.car.velocity));
        for (lane, vector_index) in wheel.contact_vector_indices.into_iter().enumerate() {
            if vector_index >= contact_vectors.len() {
                return Err(WheelPostprocessError::ContactVectorIndex { lane, vector_index });
            }
        }
        self.car.four_wheel.residual_enable = advance_residual_enable(
            self.car.four_wheel.residual_enable,
            wheel.first[0].config_158,
            self.car.four_wheel.first[0].wheel_speed,
            self.car.four_wheel.first[2].wheel_speed,
            self.car.four_wheel.speed_ec,
        );
        let thresholds = derive_compiled_lane_thresholds(wheel.lane_threshold_inputs);
        let scale_528 =
            contact_count_scale(wheel.angular.momentum.moment_scale, thresholds.count_742);
        // The mechanics driver `FUN_00407690` cuts the steer magnitudes against
        // the previous tick's `+0xec`, then recomputes `+0x404` from the same
        // start-of-tick body axis, before any integrator runs.
        let steer = apply_low_speed_steer_cut(controls.raw_steer, self.car.four_wheel.speed_ec);
        // The control processor `FUN_0041b5b0` follows the cut: its steering
        // half turns the cut magnitudes into the wheel angle `+0x3e0`, then its
        // pedal half ramps the raw pedals into this tick's responses.
        self.car.four_wheel.steering = apply_steering(
            self.car.four_wheel.steering,
            wheel.steering,
            SteeringInputs {
                steer,
                analog_410: steer.analog_410,
                car_factor_e188: wheel.steering_car_factor_e188,
                front_surface_factors: wheel.steering_surface_factors,
                forward_axis: self.car.motion.basis[0],
                velocity: self.car.velocity,
            },
        );
        let pedal_inputs = PedalInputs::from_raw(
            controls.raw_pedals,
            controls.handbrake,
            wheel.driven_smoothing.mode_408,
        );
        self.car.four_wheel.pedals = apply_pedal_responses(
            self.car.four_wheel.pedals,
            pedal_inputs,
            wheel.pedal_response,
            &self.trig,
        )
        .expect("the race-state trig table is complete");
        let body_axis =
            transform_contact_force(self.car.motion.basis, self.car.motion.projection_axis);
        self.car.four_wheel.upright_404 = upright_flag_404(body_axis);
        let update_gate = contact_global_update_gate(body_axis);
        let mut contact_update_inputs = wheel.contact_update_inputs;
        for (index, (input, vector)) in contact_update_inputs
            .iter_mut()
            .zip(contact_vectors)
            .enumerate()
        {
            input.vector = vector;
            input.basis = self.car.motion.projection_axis;
            input.scale_528 = scale_528;
            input.extra_vector = Vec3::ZERO;
            input.update_gate = update_gate;
            input.enabled = thresholds.flags_73a[index] != 0;
        }
        let source_s = stabilize_lane_source_s(
            self.car.motion.lane_source_s,
            self.car.motion.basis[0],
            self.car.motion.basis[1],
            Fixed16::from_raw(self.car.four_wheel.pedals.response_2d8),
            wheel.lane_time_step,
        );
        self.car.motion.lane_source_s = source_s;
        let (source_p, source_r) = derive_lane_direction_pair(
            source_s,
            self.car.motion.basis[1],
            wheel
                .lane_direction_angle
                .wrapping_add(self.car.four_wheel.steering.angle_3e0),
            &self.trig,
        )
        .expect("the race-state trig table is complete");
        let directions = construct_lane_directions(
            LaneDirectionSources {
                projection_axis: self.car.motion.projection_axis,
                source_a0: source_p,
                source_b0: source_r,
                source_a2: source_s,
                cross_operand: self.car.motion.basis[1],
            },
            resolve_lane_direction_mode(wheel.lane_rebuild, wheel.lane_exchange),
        );
        let mut first = wheel.first;
        for input in &mut first {
            input.flag_43b = i32::from(self.car.four_wheel.residual_enable);
        }
        let mut second = wheel.second.clone();
        for input in &mut second {
            // The second loop's gear-ratio gate reads the brake response.
            input.config_2ec = self.car.four_wheel.pedals.brake_response_2ec;
        }
        for (lane, vector_index) in wheel.contact_vector_indices.into_iter().enumerate() {
            let Some(vector) = contact_vectors.get(vector_index) else {
                return Err(WheelPostprocessError::ContactVectorIndex { lane, vector_index });
            };
            second[lane].surface_vector = [vector.x.raw(), vector.y.raw(), vector.z.raw()];
            second[lane].local_68 = [
                directions.direction_a[lane].x.raw(),
                directions.direction_a[lane].y.raw(),
                directions.direction_a[lane].z.raw(),
            ];
            second[lane].local_98 = [
                directions.direction_b[lane].x.raw(),
                directions.direction_b[lane].y.raw(),
                directions.direction_b[lane].z.raw(),
            ];
            populate_wheel_vector_inputs(
                &mut first[lane],
                &second[lane],
                directions.direction_a[lane],
            );
        }
        let mut prior_speeds = [0; 4];
        for index in (0..4).rev() {
            prior_speeds[index] = self.car.four_wheel.first[index].wheel_speed;
            Self::execute_first_wheel_state(
                &mut self.car.four_wheel.first[index],
                controls.handbrake,
                &first[index],
            );
        }
        for axle in [0, 2] {
            let (left, right) = equalize_wheel_speeds(
                Fixed16::from_raw(self.car.four_wheel.first[axle].wheel_speed),
                Fixed16::from_raw(self.car.four_wheel.first[axle + 1].wheel_speed),
            );
            self.car.four_wheel.first[axle].wheel_speed = left.raw();
            self.car.four_wheel.first[axle + 1].wheel_speed = right.raw();
        }
        let mut lanes = wheel.postprocess.lanes;
        for (lane, output) in lanes.iter_mut().enumerate() {
            output.vector_a = directions.direction_a[lane];
            output.vector_b = directions.direction_b[lane];
        }
        for index in (0..4).rev() {
            populate_derived_second_inputs(
                &first[index],
                &mut second[index],
                directions.direction_b[index],
                index,
            );
            Self::execute_second_wheel_state(
                &mut self.car.four_wheel.second[index],
                prior_speeds[index],
                self.car.four_wheel.smoothed_wheel_speeds[index],
                &first[index],
                &second[index],
            )?;
            let current = self.car.four_wheel.second[index].accumulators;
            lanes[index].accumulator = Vec3::new(
                Fixed16::from_raw(current[0]),
                Fixed16::from_raw(current[1]),
                Fixed16::from_raw(current[2]),
            );
        }
        let outcome =
            apply_geometric_lane_postprocess(&mut lanes, wheel.postprocess.argument_scale)?;
        self.car.four_wheel.postprocess = FourWheelPostprocessState {
            accumulators: lanes.map(|lane| lane.accumulator),
            qualified: outcome.qualified,
            angle_measures: outcome.angle_measures,
        };
        self.car.four_wheel.contact_updates = apply_contact_vector_updates(
            contact_update_inputs,
            self.car.four_wheel.contact_updates,
        );
        let force = sum_translation_lane_forces(
            wheel.postprocess.base_force,
            wheel.postprocess.non_wheel_forces,
            std::array::from_fn(|index| {
                (
                    wheel.postprocess.wheel_contacts[index],
                    lanes[index].accumulator,
                )
            }),
        );
        // The integrator's own translation: damping by the previous `+0xec`,
        // inverse mass `+0x4c`, the pending collision correction, then the new
        // `+0xec`. The low-speed suppression is gated on the threshold scan's
        // `+0x742 > 0` (`0x40afcc`), not on the wheel flags. Unlike the planar
        // harness there is no heading-rate steering, no separate drag, and no
        // lateral grip: lateral and drive forces arrive through the lanes.
        let translation = apply_translation_step(TranslationStepInput {
            position: self.car.position,
            velocity: self.car.velocity,
            cached_speed: Fixed16::from_raw(self.car.four_wheel.speed_ec),
            contacts_active: thresholds.count_742 > 0,
            accumulated_force: force,
            inverse_mass: wheel.inverse_mass_4c,
            additive_acceleration: self.car.collision.consume_pending_linear(),
        });
        self.car.position = translation.position;
        self.car.velocity = translation.velocity;
        self.car.four_wheel.speed_ec = translation.speed.raw();
        self.clock = advance_stage_clock(self.clock, stage_clock_step(false));
        let mut sample = TraceSample {
            clock: self.clock,
            centiseconds: elapsed_centiseconds(self.clock, self.start_clock),
            car: self.car,
        };
        let engine_force = derive_engine_force(EngineForceInput {
            limiter_444: self.car.four_wheel.limiter_444,
            cut_448: self.car.four_wheel.limiter_cut_448,
            engine_2e0: self.car.four_wheel.engine_2e0,
            redline_15c: wheel.engine_smoothing.redline_15c,
            gear: first[0].gear,
            config_40c: first[0].config_40c,
            response_2d8: self.car.four_wheel.pedals.response_2d8,
            throttle_304: pedal_inputs.throttle_304,
            ..wheel.engine_force
        });
        self.car.four_wheel.engine_force_530 = engine_force.force_530;
        self.car.four_wheel.engine_2e0 = engine_force.engine_2e0;
        if first[0].gear_ratio != 0 && first[0].config_40c == 0 {
            self.car.four_wheel.engine_2e0 = integrate_engine_from_raw_wheels(
                self.car.four_wheel.engine_2e0,
                std::array::from_fn(|index| self.car.four_wheel.first[index].wheel_speed),
                first[0].config_158,
                first[0].gear_ratio,
            );
        } else if self.car.four_wheel.idle_6e8 == 0 && self.car.four_wheel.idle_countdown_6ec == 0 {
            self.car.four_wheel.engine_2e0 = integrate_idle_engine(
                self.car.four_wheel.engine_2e0,
                self.car.four_wheel.engine_force_530,
            );
        } else if self.car.four_wheel.idle_6e8 == 0 && self.car.four_wheel.idle_countdown_6ec > 0 {
            let oscillation = advance_idle_oscillation(
                wheel.engine_smoothing.redline_15c,
                self.car.four_wheel.idle_countdown_6ec,
                &self.trig,
            );
            self.car.four_wheel.engine_2e0 = oscillation.engine_2e0;
            self.car.four_wheel.idle_countdown_6ec = oscillation.countdown_6ec;
            if let Some(response) = oscillation.response_2d8 {
                self.car.four_wheel.pedals.response_2d8 = response;
            }
        } else if self.car.four_wheel.idle_6e8 != 0 {
            (
                self.car.four_wheel.idle_6e8,
                self.car.four_wheel.idle_countdown_6ec,
            ) = advance_idle_reset(
                self.car.four_wheel.idle_6e8,
                self.car.four_wheel.idle_countdown_6ec,
            );
        }
        let rev_limit = apply_rev_limiter(
            Fixed16::from_raw(self.car.four_wheel.engine_2e0),
            Fixed16::from_raw(wheel.engine_smoothing.redline_15c),
        );
        self.car.four_wheel.engine_2e0 = rev_limit.rpm.raw();
        self.car.four_wheel.limiter_444 = rev_limit.active;
        self.car.four_wheel.limiter_cut_448 = rev_limit.cut.raw();
        let driven_scalars =
            driven_smoothing_scalars(&self.car.four_wheel, &first[0], &wheel.driven_smoothing);
        let driven_smoothing = DrivenWheelUpdateParams {
            throttle_304: pedal_inputs.throttle_304,
            brake_308: pedal_inputs.brake_308,
            ..wheel.driven_smoothing
        };
        update_driven_wheel_speeds(
            &mut self.car.four_wheel,
            &first,
            controls.handbrake,
            &driven_smoothing,
            driven_scalars,
        );
        if first[0].gear_ratio == 0 {
            self.car.four_wheel.engine_speed_2e8 = self.car.four_wheel.engine_2e0;
        } else {
            let engine = derive_smoothed_engine_speed(SmoothedEngineSpeedInput {
                wheel_speeds: self.car.four_wheel.smoothed_wheel_speeds,
                config_158: first[0].config_158,
                gear_ratio: first[0].gear_ratio,
                engine_2e0: self.car.four_wheel.engine_2e0,
                slip_gain: driven_scalars.slip_gain,
                engine_load: driven_scalars.engine_load,
                ..wheel.engine_smoothing
            });
            self.car.four_wheel.engine_speed_2e8 = engine.speed_2e8;
        }
        let mut angular = wheel.angular;
        angular.basis = self.car.motion.basis;
        angular.momentum.angular_velocity = self.car.motion.angular_velocity;
        angular.momentum.prior_moment = self.car.motion.moment;
        angular.momentum.contact_moment =
            angular
                .momentum
                .contact_moment
                .add(derive_wheel_contact_moment(
                    self.car.motion.basis,
                    &lanes,
                    wheel.postprocess.wheel_contacts,
                    wheel.contact_offsets,
                ));
        let orientation = apply_angular_orientation_pipeline(
            angular,
            self.car.collision.out_of_control(),
            self.car.motion.projection_axis,
            ContactDeadzoneGate {
                contacts_active_742: thresholds.count_742 > 0,
                steer,
                upright_404: self.car.four_wheel.upright_404,
            },
            &self.trig,
        )
        .expect("the race-state trig table is complete");
        self.car.four_wheel.resting_700 =
            resting_flag_700(orientation.angle_increments, translation.speed);
        self.car.motion.basis = orientation.basis;
        self.car.motion.angular_velocity = orientation.angular_velocity;
        // The angular pass consumed the decayed moment
        // (`orientation.momentum.scaled_prior_moment`); the mechanics driver
        // then clears it.
        self.car
            .collision
            .set_out_of_control(orientation.out_of_control);
        self.finish_mechanics_tick();
        sample.car = self.car;
        Ok(sample)
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
            flag_43b: 0,
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
                raw_steer: Default::default(),
                raw_pedals: Default::default(),
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
    fn planar_step_clears_a_collision_moment_at_the_end_of_the_tick() {
        // The live planar session runs mechanics then collision. A moment the
        // collision pass published must not survive into the next pass, or
        // impulses compound across ticks; `FUN_00407690` zeroes it each tick.
        let mut car = CarState::at_rest();
        car.motion.moment = Vec3::new(
            Fixed16::from_raw(0x4_0000),
            Fixed16::from_raw(-0x2_0000),
            Fixed16::from_raw(0x1_0000),
        );
        let mut state = RaceState::new(car);
        let sample = state.step(&straight_throttle(1)[0], &params());
        assert_eq!(sample.car.motion.moment, Vec3::ZERO);
        assert_eq!(state.car().motion.moment, Vec3::ZERO);
    }

    #[test]
    fn live_collision_out_of_control_words_round_trip_through_their_owners() {
        let mut collision = CarState::at_rest().collision;
        assert_eq!(collision.out_of_control(), OutOfControlState::default());
        let tripped = OutOfControlState {
            out_of_control_680: true,
            recovery_latch_688: true,
            axis_snapshot_bc: Vec3::new(
                Fixed16::from_raw(1),
                Fixed16::from_raw(-2),
                Fixed16::from_raw(3),
            ),
        };
        collision.set_out_of_control(tripped);
        assert!(collision.response_state_680);
        assert!(collision.recovery_latch_688);
        assert_eq!(collision.out_of_control_axis_bc, tripped.axis_snapshot_bc);
        assert_eq!(collision.out_of_control(), tripped);
    }

    #[test]
    fn live_collision_pending_linear_is_consumed_once_without_clearing_persistent_state() {
        let pending = Vec3::new(
            Fixed16::from_raw(1),
            Fixed16::from_raw(-2),
            Fixed16::from_raw(3),
        );
        let mut collision = LiveCollisionState {
            pending_linear_3cc: pending,
            history: StaticContactHistory {
                previous_len: 1,
                ..StaticContactHistory::default()
            },
            response_state_680: true,
            recovery_latch_688: true,
            out_of_control_axis_bc: Vec3::ZERO,
        };
        assert_eq!(collision.consume_pending_linear(), pending);
        assert_eq!(collision.pending_linear_3cc, Vec3::ZERO);
        assert_eq!(collision.history.previous_len, 1);
        assert!(collision.response_state_680);
        assert!(collision.recovery_latch_688);
        assert_eq!(collision.consume_pending_linear(), Vec3::ZERO);
    }

    #[test]
    fn race_step_consumes_pending_collision_acceleration_before_clearing_it() {
        let mut car = CarState::at_rest();
        car.collision.pending_linear_3cc =
            Vec3::new(Fixed16::from_raw(0x10000), Fixed16::ZERO, Fixed16::ZERO);
        let mut race = RaceState::new(car);
        let sample = race.step(
            &Controls {
                steer_rate: 0,
                forward_force: Fixed16::ZERO,
                handbrake: false,
                raw_steer: Default::default(),
                raw_pedals: Default::default(),
            },
            &SimParams {
                inverse_mass: Fixed16::ZERO,
                gravity: Vec3::ZERO,
            },
        );
        assert_eq!(sample.car.velocity.x, Fixed16::from_raw(0x3333));
        assert_eq!(sample.car.position.x, Fixed16::from_raw(0x199a));
        assert_eq!(sample.car.collision.pending_linear_3cc, Vec3::ZERO);

        let next = race.step(
            &Controls {
                steer_rate: 0,
                forward_force: Fixed16::ZERO,
                handbrake: false,
                raw_steer: Default::default(),
                raw_pedals: Default::default(),
            },
            &SimParams {
                inverse_mass: Fixed16::ZERO,
                gravity: Vec3::ZERO,
            },
        );
        assert_eq!(next.car.velocity.x, sample.car.velocity.x);
    }

    #[test]
    fn live_static_pass_publishes_depenetration_and_next_tick_state() {
        use crate::c3d_terrain_cull::CellGrid;
        use crate::static_collision_kernel::NoStaticDamageOwner;
        use crate::static_world_collision::{
            StaticCollisionNodeId, StaticCollisionRecord, StaticCollisionScene, StaticNarrowObb,
        };

        let one = Fixed16::ONE;
        let identity = [
            Vec3::new(one, Fixed16::ZERO, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, one, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, one),
        ];
        let object = StaticNarrowObb {
            center: Vec3::ZERO,
            projected_corners: [[-128, -128], [128, -128], [128, 128], [-128, 128]],
            radius: Fixed16::from_raw(0x20000),
            max_y_shift9: 128,
            min_y_shift9: -128,
            axis_radius: one,
            perpendicular_radius: one,
            axis: identity[0],
            perpendicular: identity[2],
        };
        let node = StaticCollisionNodeId::new(0).unwrap();
        let parsed = ParsedStaticCollisionScene {
            grid: CellGrid {
                width: 1,
                height: 1,
                cell_count: 1,
                cell_size: 0x20_0000,
                min_x: 0,
                half_cell: 0x10_0000,
                max_z: 0,
            },
            main_cell_centers: vec![Vec3::ZERO],
            scene: StaticCollisionScene::new(
                vec![StaticCollisionRecord {
                    collision_ordinal: 7,
                    raw_material_flags: 0,
                    geometry: object,
                }],
                [(node, 0)],
            )
            .unwrap(),
        };
        let mut car = CarState::at_rest();
        car.motion.basis = identity;
        car.motion.projection_axis = identity[0];
        car.motion.compiled_half_words = Vec3::new(one, one, one);
        car.motion.contact_count_674 = 3;
        car.velocity = Vec3::new(one.neg(), Fixed16::ZERO, one);
        let mut race = RaceState::new(car);
        let previous = PreviousCarXz {
            x: Fixed16::from_raw(-0xa_0000),
            z: Fixed16::ZERO,
        };
        let contacts = race
            .resolve_static_collision_after_step(previous, &parsed, &mut NoStaticDamageOwner)
            .unwrap();
        let committed = race.car();
        assert_eq!(contacts, 1);
        assert_ne!(committed.position, car.position);
        assert_ne!(committed.collision.pending_linear_3cc, Vec3::ZERO);
        assert_eq!(committed.collision.history.current_len, 1);
    }

    fn second_wheel_params(trig: &[i32]) -> SecondWheelParams<'_> {
        SecondWheelParams {
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
            surface_vector: [0x10000, 0, 0],
            table_factor: None,
        }
    }

    #[test]
    fn contact_basis_vectors_populate_second_wheel_with_checked_indices() {
        let trig = build_sine_table();
        let mut second = second_wheel_params(&trig);
        let input = ContactBasisInput {
            rows: [
                Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
                Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO),
                Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE),
            ],
            angular: [Fixed16::ONE; 6],
            velocity: Vec3::ZERO,
        };
        assert_eq!(
            populate_contact_basis_vector(&mut second, input, 5),
            Some(Vec3::new(
                Fixed16::ZERO,
                Fixed16::from_raw(0x20000),
                Fixed16::from_raw(-0x20000),
            ))
        );
        assert_eq!(second.surface_vector, [0, 0x20000, -0x20000]);
        second.surface_vector = [7, 8, 9];
        assert_eq!(populate_contact_basis_vector(&mut second, input, 8), None);
        assert_eq!(second.surface_vector, [7, 8, 9]);
    }

    #[test]
    fn wheel_vector_inputs_share_direction_and_contact_vector() {
        let trig = build_sine_table();
        let mut first = first_wheel_params();
        let mut second = second_wheel_params(&trig);
        second.surface_vector = [3, 4, 5];
        populate_wheel_vector_inputs(
            &mut first,
            &second,
            Vec3::new(
                Fixed16::from_raw(0x10000),
                Fixed16::from_raw(-0x20000),
                Fixed16::from_raw(0x30000),
            ),
        );
        assert_eq!(first.wheel_vector, [0x10000, -0x20000, 0x30000]);
        assert_eq!(first.state_vector, [3, 4, 5]);
    }

    #[test]
    fn neutral_gear_zeroes_smoothed_speeds_before_any_tracking() {
        let mut state = FourWheelState {
            first: [FirstWheelState {
                force: 0,
                wheel_speed: 0x20000,
            }; 4],
            smoothed_wheel_speeds: [0x10000; 4],
            ..FourWheelState::default()
        };
        update_smoothed_wheel_speeds(&mut state, 0);
        assert_eq!(state.smoothed_wheel_speeds, [0; 4]);
        update_smoothed_wheel_speeds(&mut state, 1);
        assert_eq!(state.smoothed_wheel_speeds, [0x10000; 4]);
    }

    #[test]
    fn driven_smoothing_consumes_each_first_loop_force_and_derived_scalars() {
        let first = [FirstWheelParams {
            gear: 4,
            gear_ratio: 0x10000,
            config_158: 1,
            ..first_wheel_params()
        }; 4];
        let mut state = FourWheelState {
            first: [FirstWheelState {
                force: 0x20000,
                wheel_speed: 0x10000,
            }; 4],
            smoothed_wheel_speeds: [0x10000; 4],
            ..FourWheelState::default()
        };
        let params = DrivenWheelUpdateParams {
            scalars: DrivenSmoothingScalarInput {
                redline_15c: 0x10000,
                response_1f0: 0x20000,
                gear_ratio_190: 0x10000,
                suspension_298: [0x40000, 0, 0, 0],
                ..DrivenWheelUpdateParams::default().scalars
            },
            throttle_304: 1,
            split_170: 0x10000,
            ..DrivenWheelUpdateParams::default()
        };
        let scalars = driven_smoothing_scalars(&state, &first[0], &params);
        assert_eq!(scalars.slip_gain, 0x9999);
        assert_eq!(scalars.direction_gain, 0x10000);
        assert_eq!(scalars.signed_speed_limit, 0x20000);
        update_driven_wheel_speeds(&mut state, &first, false, &params, scalars);
        assert!(
            state
                .smoothed_wheel_speeds
                .iter()
                .all(|speed| *speed > 0x10000)
        );

        state.smoothed_wheel_speeds = [0; 4];
        update_driven_wheel_speeds(
            &mut state,
            &first,
            false,
            &DrivenWheelUpdateParams {
                throttle_304: 0,
                ..params
            },
            scalars,
        );
        assert_eq!(state.smoothed_wheel_speeds, [0x10000; 4]);

        let neutral = [FirstWheelParams {
            gear_ratio: 0,
            ..first[0]
        }; 4];
        update_driven_wheel_speeds(&mut state, &neutral, false, &params, scalars);
        assert_eq!(state.smoothed_wheel_speeds, [0; 4]);
    }

    #[test]
    fn derived_second_inputs_use_native_dot_products_and_residual_gates() {
        let trig = build_sine_table();
        let mut first = first_wheel_params();
        first.wheel_vector = [0x10000, 0, 0];
        first.state_vector = [0x20000, 0, 0];
        first.flag_43b = 1;
        let mut second = second_wheel_params(&trig);
        second.surface_vector = [3, 0, 0];
        populate_derived_second_inputs(
            &first,
            &mut second,
            Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
            0,
        );
        assert_eq!(second.wheel_e8, 0x20000);
        assert_eq!(second.axis_dot, 3);
        assert!(second.residual_flag_147a);
        first.config_158 = 0x10000;
        populate_derived_second_inputs(
            &first,
            &mut second,
            Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
            2,
        );
        assert!(!second.residual_flag_147a);
    }

    #[test]
    fn motion_state_forms_contact_inputs_from_native_fields() {
        let mut motion = MotionState::harness_default();
        motion.angular_velocity = Vec3::new(
            Fixed16::from_raw(3),
            Fixed16::from_raw(4),
            Fixed16::from_raw(5),
        );
        motion.apply_compiled_half_state(
            &crate::compiled_car_initializer::derive_compiled_half_sign_state([4, -6, 8]),
        );
        motion.apply_compiled_post_lane_basis(
            [7, 8, 9],
            &CompiledPostLaneBasisEffects {
                vector_258: [1, 2, 3],
                vector_264: [4, 5, 6],
                vector_270: [7, 8, 9],
            },
        );
        assert_eq!(
            motion.projection_axis,
            Vec3::new(
                Fixed16::from_raw(7),
                Fixed16::from_raw(8),
                Fixed16::from_raw(9),
            )
        );
        assert_eq!(motion.basis[0].x, Fixed16::from_raw(1));
        assert_eq!(motion.basis[1].y, Fixed16::from_raw(5));
        assert_eq!(motion.basis[2].z, Fixed16::from_raw(9));
        let velocity = Vec3::new(
            Fixed16::from_raw(9),
            Fixed16::from_raw(10),
            Fixed16::from_raw(11),
        );
        let input = motion.contact_basis(velocity);
        assert_eq!(input.rows, motion.basis);
        assert_eq!(input.angular, [3, 4, 5, 2, -3, 4].map(Fixed16::from_raw));
        assert_eq!(input.velocity, velocity);
    }

    #[test]
    fn reviewed_surface_family_populates_table_factor_with_checked_indices() {
        let families = SurfaceCoefficientFamilies {
            first: cmr_formats::pe_surface::SurfaceCoefficientFamily {
                values: [0x20000; 120],
            },
            second: cmr_formats::pe_surface::SurfaceCoefficientFamily { values: [0; 120] },
        };
        let trig = build_sine_table();
        let mut second = second_wheel_params(&trig);
        populate_surface_table_factor(&mut second, &families, 3).unwrap();
        assert_eq!(second.table_factor, Some(0x20000));
        second.table_factor = None;
        assert_eq!(
            populate_surface_table_factor(&mut second, &families, 120),
            Err(SurfaceCoefficientIndexError { index: 120 })
        );
        assert_eq!(second.table_factor, None);
    }

    #[test]
    #[ignore = "requires an authenticated surface store under CMR_RUNTIME_DATA"]
    fn official_surface_store_feeds_second_core_without_exposing_values() {
        let root = std::env::var_os("CMR_RUNTIME_DATA").expect("CMR_RUNTIME_DATA");
        let reviewed = cmr_assets::ReviewedSurfaceProfile::from_json(
            &std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../profiles/official-eu-surface-tables-v1.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let store = cmr_assets::SurfaceTableStore::open(
            &std::path::Path::new(&root).join("official-eu-surface-coefficients"),
            &reviewed,
        )
        .unwrap();
        let trig = build_sine_table();
        let mut second = second_wheel_params(&trig);
        populate_surface_table_factor(&mut second, store.families(), 0).unwrap();
        assert_eq!(second.table_factor, Some(store.families().first.values[0]));
    }

    #[test]
    fn force_pass_scales_populate_both_wheel_parameter_sets() {
        let trig = build_sine_table();
        let mut first = first_wheel_params();
        let mut second = second_wheel_params(&trig);
        let scales = populate_force_pass_scales(
            &mut first,
            &mut second,
            Fixed16::from_raw(0x30000),
            Fixed16::from_raw(0x20000),
            Fixed16::from_raw(0x40000),
        );
        assert_eq!(first.scale_c600, 0x1e000);
        assert_eq!(first.factor_c5c0, 0x4444c);
        assert_eq!(second.scale_c5c4, 0x60000);
        assert_eq!(scales.scale_c5c4, Fixed16::from_raw(0x60000));
    }

    #[test]
    fn compiled_families_populate_the_selected_gear_ratio_atomically() {
        let families = CompiledControlFamilies {
            post_processed_raw: [11, 22, 33, 44, 55, 66, 77, 88],
            reciprocal: [0; 8],
            raw_copy: [11, 22, 33, 44, 55, 66, 77, 88],
            reciprocal_copy: [0; 8],
        };
        let trig = build_sine_table();
        let mut first = first_wheel_params();
        first.gear = 2;
        let mut second = second_wheel_params(&trig);
        assert_eq!(
            populate_compiled_gear_ratio(&mut first, &mut second, &families, Fixed16::ONE),
            Some(Fixed16::from_raw(33))
        );
        assert_eq!(first.gear_ratio, 33);
        assert_eq!(second.gear_ratio, 33);

        first.gear = 8;
        first.gear_ratio = 99;
        second.gear_ratio = 98;
        assert_eq!(
            populate_compiled_gear_ratio(&mut first, &mut second, &families, Fixed16::ONE),
            None
        );
        assert_eq!(first.gear_ratio, 99);
        assert_eq!(second.gear_ratio, 98);
    }

    #[test]
    fn second_wheel_core_composes_after_the_first_iteration() {
        let mut car = CarState::at_rest();
        car.first_wheel.wheel_speed = -1;
        let mut state = RaceState::new(car);
        let trig = build_sine_table();
        let second = second_wheel_params(&trig);
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
        let second = second_wheel_params(&trig);
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
        assert_eq!(sample.car.position, Vec3::ZERO);

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
    fn all_four_wheels_run_both_iterations_before_postprocess() {
        let vector = |components: [i32; 3]| {
            Vec3::new(
                Fixed16::from_raw(components[0]),
                Fixed16::from_raw(components[1]),
                Fixed16::from_raw(components[2]),
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
            vector_a: vector([0x8000, -1, 0]),
            vector_b: vector([0x10000, 0, 0]),
            vector_c: vector([0, 0x10000, 0x10000]),
            accumulator: Vec3::ZERO,
        };
        let trig = build_sine_table();
        let mut wheel = FourWheelIterationParams {
            first: [first_wheel_params(); 4],
            second: std::array::from_fn(|_| second_wheel_params(&trig)),
            contact_vector_indices: [2, 3, 6, 7],
            contact_offsets: [Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, vector([0, 0x10000, 0])],
            contact_update_inputs: std::array::from_fn(|index| ContactVectorUpdateInput {
                update_gate: 0,
                enabled: index == 0,
                vector: Vec3::ZERO,
                basis: Vec3::ZERO,
                scale_528: Fixed16::ONE,
                extra_vector: Vec3::ZERO,
                limit: Fixed16::ONE,
                threshold: Fixed16::from_raw(0x7fff0000),
            }),
            lane_threshold_inputs: CompiledLaneThresholdInputs {
                lane_words: [0; 8],
                source_y: [0x199a; 8],
            },
            pedal_response: PedalResponseParams {
                throttle_max_148: 0x8000,
                ..PedalResponseParams::default()
            },
            lane_time_step: crate::vehicle_dynamics::SOLVER_GLOBAL_CONSTANTS.time_step,
            lane_direction_angle: 0,
            steering: SteeringParams::default(),
            steering_car_factor_e188: 0x1_0000,
            steering_surface_factors: [0; 2],
            lane_rebuild: false,
            lane_exchange: false,
            postprocess: FourWheelPostprocessParams {
                lanes,
                current_lane: 3,
                argument_scale: Fixed16::ONE,
                // A caller-supplied non-wheel force: the four-wheel step has no
                // harness throttle push, so it drives the fixture.
                base_force: vector([0x1_0000, 0, 0]),
                non_wheel_forces: &[],
                wheel_contacts: [true; 4],
            },
            inverse_mass_4c: Fixed16::ONE,
            angular: AngularOrientationInput {
                momentum: crate::vehicle_dynamics::AngularMomentumStepInput {
                    prior_moment: Vec3::ZERO,
                    contact_moment: vector([1, 2, 3]),
                    moment_scale: Fixed16::ONE,
                    inertia_scale: Vec3::new(Fixed16::ONE, Fixed16::ONE, Fixed16::ONE),
                    angular_velocity: Vec3::ZERO,
                    time_step: crate::vehicle_dynamics::SOLVER_GLOBAL_CONSTANTS.time_step,
                },
                clamp_enabled: false,
                control: crate::vehicle_dynamics::ControlAxisFlags {
                    mode_3ec: 0,
                    axis_3f4: 0,
                    axis_3f8: 0,
                },
                basis: [Vec3::ZERO; 3],
            },
            engine_smoothing: SmoothedEngineSpeedInput {
                wheel_speeds: [0; 4],
                config_158: 1,
                gear_ratio: 0x10000,
                engine_2e0: 0,
                redline_15c: 0x10000,
                slip_gain: 0,
                engine_load: 0x10000,
            },
            driven_smoothing: DrivenWheelUpdateParams::default(),
            engine_force: EngineForceInput {
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
        populate_compiled_contact_offsets(
            &mut wheel,
            &crate::compiled_car_initializer::derive_compiled_half_sign_state([
                0x40000, -0x60000, 0x80000,
            ]),
        );
        populate_compiled_contact_update_inputs(
            &mut wheel,
            &CompiledControlClearLoop {
                words: [
                    0x10000, 0x7fff0000, 0x10000, 0x7fff0000, 0x10000, 0x7fff0000, 0x10000,
                    0x7fff0000, 0x10000, 0x7fff0000, 0x10000, 0x7fff0000, 0x10000, 0x7fff0000,
                    0x10000, 0x7fff0000,
                ],
            },
        );
        assert_eq!(wheel.contact_update_inputs[0].limit, Fixed16::ONE);
        assert_eq!(
            wheel.contact_update_inputs[0].threshold,
            Fixed16::from_raw(0x7fff0000)
        );
        assert_eq!(
            wheel.contact_offsets,
            [
                vector([0x20000, 0x30000, 0x40000]),
                vector([0x20000, 0x30000, -0x40000]),
                vector([-0x20000, 0x30000, 0x40000]),
                vector([-0x20000, 0x30000, -0x40000]),
            ]
        );
        let mut car = CarState::at_rest();
        car.motion.apply_compiled_post_lane_basis(
            [0, 0, 0],
            &CompiledPostLaneBasisEffects {
                vector_258: [0x10000, 0, 0],
                vector_264: [0, 0x10000, 0],
                vector_270: [0, 0, 0x10000],
            },
        );
        car.motion.angular_velocity = vector([0x10000, 0x10000, 0x10000]);
        // A nonzero collision moment from the previous tick: the angular pass
        // must consume it (decayed) and the mechanics driver must clear it.
        car.motion.moment = vector([0x2_0000, -0x1_0000, 0x3_0000]);
        for (index, speed) in [-1, 0x10000, -1, -1].into_iter().enumerate() {
            car.four_wheel.first[index].wheel_speed = speed;
        }
        let mut state = RaceState::new(car);
        let sample = state
            .step_with_all_four_wheels(&straight_throttle(1)[0], &wheel)
            .unwrap();
        assert_eq!(
            sample.car.four_wheel.first[0].wheel_speed,
            sample.car.four_wheel.first[1].wheel_speed
        );
        assert_eq!(
            sample.car.four_wheel.first[2].wheel_speed,
            sample.car.four_wheel.first[3].wheel_speed
        );
        assert_eq!(car.four_wheel.residual_enable, 10);
        assert_eq!(sample.car.four_wheel.residual_enable, 0);
        assert_eq!(
            sample.car.four_wheel.speed_ec,
            sqrt_q16(sample.car.velocity.dot(sample.car.velocity).raw() as u32) as i32
        );
        for index in 0..4 {
            assert_eq!(
                sample.car.four_wheel.smoothed_wheel_speeds[index],
                track_smoothed_wheel_speed(sample.car.four_wheel.first[index].wheel_speed, 0)
            );
        }
        let expected_engine = apply_rev_limiter(
            Fixed16::from_raw(integrate_engine_from_raw_wheels(
                0,
                std::array::from_fn(|index| sample.car.four_wheel.first[index].wheel_speed),
                wheel.first[0].config_158,
                wheel.first[0].gear_ratio,
            )),
            Fixed16::from_raw(wheel.engine_smoothing.redline_15c),
        )
        .rpm
        .raw();
        assert_eq!(sample.car.four_wheel.engine_2e0, expected_engine);
        assert_eq!(
            sample.car.four_wheel.engine_speed_2e8,
            derive_smoothed_engine_speed(SmoothedEngineSpeedInput {
                wheel_speeds: sample.car.four_wheel.smoothed_wheel_speeds,
                config_158: wheel.first[0].config_158,
                gear_ratio: wheel.first[0].gear_ratio,
                engine_2e0: expected_engine,
                ..wheel.engine_smoothing
            })
            .speed_2e8
        );
        let contact_vectors = build_contact_basis_vectors(car.motion.contact_basis(car.velocity));
        let thresholds = derive_compiled_lane_thresholds(wheel.lane_threshold_inputs);
        let update_gate = contact_global_update_gate(transform_contact_force(
            car.motion.basis,
            car.motion.projection_axis,
        ));
        let mut contact_inputs = wheel.contact_update_inputs;
        for (index, (input, vector)) in contact_inputs.iter_mut().zip(contact_vectors).enumerate() {
            input.vector = vector;
            input.basis = car.motion.projection_axis;
            input.scale_528 =
                contact_count_scale(wheel.angular.momentum.moment_scale, thresholds.count_742);
            input.extra_vector = Vec3::ZERO;
            input.update_gate = update_gate;
            input.enabled = thresholds.flags_73a[index] != 0;
        }
        assert_eq!(
            sample.car.four_wheel.contact_updates,
            apply_contact_vector_updates(contact_inputs, [Vec3::ZERO; 8])
        );
        let mut expected_lanes = lanes;
        let expected_s = stabilize_lane_source_s(
            car.motion.lane_source_s,
            car.motion.basis[0],
            car.motion.basis[1],
            // No pedal is pressed, so the processor's `+0x2d8` is zero.
            Fixed16::ZERO,
            wheel.lane_time_step,
        );
        let (source_p, source_r) = derive_lane_direction_pair(
            expected_s,
            car.motion.basis[1],
            wheel.lane_direction_angle,
            &trig,
        )
        .unwrap();
        let expected_directions = construct_lane_directions(
            LaneDirectionSources {
                projection_axis: car.motion.projection_axis,
                source_a0: source_p,
                source_b0: source_r,
                source_a2: expected_s,
                cross_operand: car.motion.basis[1],
            },
            resolve_lane_direction_mode(wheel.lane_rebuild, wheel.lane_exchange),
        );
        for (index, lane) in expected_lanes.iter_mut().enumerate() {
            lane.vector_a = expected_directions.direction_a[index];
            lane.vector_b = expected_directions.direction_b[index];
            lane.accumulator = vector(sample.car.four_wheel.second[index].accumulators);
        }
        let outcome = apply_geometric_lane_postprocess(&mut expected_lanes, Fixed16::ONE).unwrap();
        assert_eq!(
            sample.car.four_wheel.postprocess.accumulators,
            expected_lanes.map(|lane| lane.accumulator)
        );
        assert_eq!(
            sample.car.four_wheel.postprocess.qualified,
            outcome.qualified
        );
        assert!(sample.car.velocity.x > Fixed16::ZERO);
        // The integrator has no harness throttle push: the planar
        // `forward_force` leaves the four-wheel tick unchanged.
        let coasting = RaceState::new(car)
            .step_with_all_four_wheels(&Controls::default(), &wheel)
            .unwrap();
        assert_eq!(coasting.car, sample.car);

        // An analog full throttle goes through the control processor: `+0x2d8`
        // becomes `qmul(+0x148, 0x10000)`, the lane stabilizer reads it this
        // tick, and the raw `+0x304` reaches the driven-wheel gate, which now
        // takes the integrating branch on every lane (`config_158 = 1`).
        let pressed_controls = Controls {
            raw_pedals: RawPedals {
                throttle_304: 0x1_0000,
                throttle_analog_414: true,
                ..RawPedals::default()
            },
            ..straight_throttle(1)[0]
        };
        let pressed = RaceState::new(car)
            .step_with_all_four_wheels(&pressed_controls, &wheel)
            .unwrap();
        assert_eq!(pressed.car.four_wheel.pedals.response_2d8, 0x8000);

        // A held digital left above the low-speed cut runs the steering half:
        // the wheel angle slews to -0x22, the lane source is rotated by base +
        // angle, and the lane outputs change. At rest the cut removes the steer
        // and the tick matches the unsteered one.
        let mut steerable = wheel.clone();
        steerable.steering = SteeringParams {
            max_angle_3e4: 0x2aa,
            rate_44c: 0x4000,
            time_step: 0x1_0000,
            ..SteeringParams::default()
        };
        let left = Controls {
            raw_steer: RawSteer {
                left_2fc: 0x1_0000,
                right_300: 0,
                analog_410: false,
            },
            ..straight_throttle(1)[0]
        };
        let mut rolling = car;
        rolling.four_wheel.speed_ec = 0x1000;
        let steered = RaceState::new(rolling)
            .step_with_all_four_wheels(&left, &steerable)
            .unwrap();
        let straight = RaceState::new(rolling)
            .step_with_all_four_wheels(&straight_throttle(1)[0], &steerable)
            .unwrap();
        assert_eq!(steered.car.four_wheel.steering.position_318, 0x4000);
        assert_eq!(steered.car.four_wheel.steering.angle_3e0, -0x22);
        assert_eq!(straight.car.four_wheel.steering, SteeringState::ZERO);
        assert_ne!(
            steered.car.four_wheel.second[0].accumulators,
            straight.car.four_wheel.second[0].accumulators
        );
        let parked = RaceState::new(car)
            .step_with_all_four_wheels(&left, &steerable)
            .unwrap();
        assert_eq!(parked.car.four_wheel.steering, SteeringState::ZERO);
        assert_eq!(
            pressed.car.motion.lane_source_s,
            stabilize_lane_source_s(
                car.motion.lane_source_s,
                car.motion.basis[0],
                car.motion.basis[1],
                Fixed16::from_raw(0x8000),
                wheel.lane_time_step,
            )
        );
        for lane in 0..4 {
            assert!(!smoothed_speed_uses_tracking(SmoothedSpeedGateInput {
                lane,
                mode_408: wheel.driven_smoothing.mode_408,
                throttle_304: 0x1_0000,
                brake_308: 0,
                handbrake_30c: 0,
                config_158: wheel.first[lane].config_158,
                split_170: wheel.driven_smoothing.split_170,
                limiter_444: 0,
            }));
        }
        assert_ne!(
            pressed.car.four_wheel.smoothed_wheel_speeds,
            sample.car.four_wheel.smoothed_wheel_speeds
        );
        // Every lane passes `0 + 0x1999 < 0x199a`, so `+0x742 == 0` and the
        // native low-speed suppression is off even though the wheel flags are
        // set: the position advances.
        assert_eq!(
            derive_compiled_lane_thresholds(wheel.lane_threshold_inputs).count_742,
            0
        );
        assert_ne!(sample.car.position, Vec3::ZERO);

        // With contacting lanes (`+0x742 > 0`) and a new speed below `0xccc`, the
        // same tick keeps velocity but suppresses the position increment.
        let mut contacting = wheel.clone();
        contacting.lane_threshold_inputs.source_y = [0; 8];
        assert!(derive_compiled_lane_thresholds(contacting.lane_threshold_inputs).count_742 > 0);
        let suppressed = RaceState::new(car)
            .step_with_all_four_wheels(&straight_throttle(1)[0], &contacting)
            .unwrap();
        assert!(suppressed.car.velocity.x > Fixed16::ZERO);
        assert!(
            suppressed.car.velocity.length()
                < crate::vehicle_dynamics::LOW_SPEED_POSITION_SUPPRESSION
        );
        assert_eq!(suppressed.car.position, Vec3::ZERO);
        let mut expected_angular = wheel.angular;
        expected_angular.basis = car.motion.basis;
        expected_angular.momentum.angular_velocity = car.motion.angular_velocity;
        expected_angular.momentum.prior_moment = car.motion.moment;
        expected_angular.momentum.contact_moment =
            wheel
                .angular
                .momentum
                .contact_moment
                .add(derive_wheel_contact_moment(
                    car.motion.basis,
                    &expected_lanes,
                    wheel.postprocess.wheel_contacts,
                    wheel.contact_offsets,
                ));
        // `wheel` has no contacting lane, so the deadzones are skipped.
        assert_eq!(
            derive_compiled_lane_thresholds(wheel.lane_threshold_inputs).count_742,
            0
        );
        let expected_orientation = apply_angular_orientation_pipeline(
            expected_angular,
            car.collision.out_of_control(),
            car.motion.projection_axis,
            ContactDeadzoneGate::default(),
            &trig,
        )
        .unwrap();
        let start_axis = transform_contact_force(car.motion.basis, car.motion.projection_axis);
        assert_eq!(
            sample.car.four_wheel.upright_404,
            upright_flag_404(start_axis)
        );
        // An upright car with no rotation or throttle: on contact the
        // deadzones zero the one-LSB residual angles, the basis holds, and the
        // slow new speed marks it at rest. Without contact the same residual
        // rotates the basis and the flag stays clear.
        let mut calm_car = car;
        calm_car.motion.projection_axis = vector([0, 0x10000, 0]);
        calm_car.motion.angular_velocity = Vec3::ZERO;
        calm_car.motion.moment = Vec3::ZERO;
        let calm_step = |wheel: &FourWheelIterationParams<'_>| {
            let mut unforced = wheel.clone();
            unforced.postprocess.base_force = Vec3::ZERO;
            RaceState::new(calm_car)
                .step_with_all_four_wheels(&Controls::default(), &unforced)
                .unwrap()
        };
        let calm = calm_step(&contacting);
        assert!(calm.car.four_wheel.upright_404);
        assert_eq!(calm.car.motion.basis, calm_car.motion.basis);
        assert!(calm.car.four_wheel.resting_700);
        let loose = calm_step(&wheel);
        assert!(loose.car.four_wheel.upright_404);
        assert_ne!(loose.car.motion.basis, calm_car.motion.basis);
        assert!(!loose.car.four_wheel.resting_700);

        // With contacting lanes the deadzones run on the cut steer and the
        // start-of-tick `+0x404`; the fixture starts at rest, so any steer is cut
        // and a steered tick matches an unsteered one.
        let steered = Controls {
            raw_steer: RawSteer {
                left_2fc: 0xfc00,
                right_300: 0,
                analog_410: false,
            },
            ..straight_throttle(1)[0]
        };
        assert_eq!(car.four_wheel.speed_ec, 0);
        let steered_contacting = RaceState::new(car)
            .step_with_all_four_wheels(&steered, &contacting)
            .unwrap();
        assert_eq!(steered_contacting.car.motion, suppressed.car.motion);
        assert_eq!(steered_contacting.car.four_wheel, suppressed.car.four_wheel);
        assert_eq!(sample.car.motion.basis, expected_orientation.basis);
        assert_eq!(
            sample.car.motion.angular_velocity,
            expected_orientation.angular_velocity
        );
        assert_ne!(
            expected_orientation.momentum.scaled_prior_moment,
            Vec3::ZERO,
            "the fixture must exercise a nonzero decayed moment"
        );
        assert_eq!(sample.car.motion.moment, Vec3::ZERO);
        assert_eq!(state.car().motion.moment, Vec3::ZERO);
        assert_eq!(
            sample.car.collision.out_of_control(),
            expected_orientation.out_of_control
        );
        assert_ne!(sample.car.motion.basis, car.motion.basis);
        assert_ne!(
            sample.car.motion.angular_velocity,
            car.motion.angular_velocity
        );
        assert_ne!(
            sample.car.motion.contact_basis(sample.car.velocity).rows,
            car.motion.contact_basis(car.velocity).rows
        );

        let mut moving_car = car;
        moving_car.velocity = vector([0x20000, 0, 0]);
        let moving = RaceState::new(moving_car)
            .step_with_all_four_wheels(&straight_throttle(1)[0], &wheel)
            .unwrap();
        assert_ne!(
            moving.car.four_wheel.second[0].accumulators,
            sample.car.four_wheel.second[0].accumulators
        );

        let mut invalid = wheel.clone();
        invalid.contact_vector_indices[1] = 8;
        let before = state.car();
        assert_eq!(
            state.step_with_all_four_wheels(&straight_throttle(1)[0], &invalid),
            Err(WheelPostprocessError::ContactVectorIndex {
                lane: 1,
                vector_index: 8,
            })
        );
        assert_eq!(state.car(), before);
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
                raw_steer: Default::default(),
                raw_pedals: Default::default(),
            };
            16
        ];
        let mut state = RaceState::new(CarState::at_rest());
        let trace = state.run_trace(&script, &params());
        let last = trace.last().unwrap().car;
        assert_ne!(last.velocity.z, Fixed16::ZERO);
    }
}
