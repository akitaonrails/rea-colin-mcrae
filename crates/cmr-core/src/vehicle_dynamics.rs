//! Reconstruction of the suspension/wheel/grip response and the velocity
//! integrator (the confidently-recovered Q16.16 formulas of G03).
//!
//! From the dispatcher `FUN_00407690` solver passes:
//!
//! - **Lateral-grip constraint** (`FUN_0041b270`, runs before forces): removes
//!   the sideways component of the car velocity each step - `perp = v -
//!   fwd * dot(fwd, v)`, then `v -= perp * K`, with `K = 0x51e` (~0.02) normally
//!   and `0x28f` (~0.01) while the handbrake flag is set (half lateral grip ->
//!   slides).
//! - **Slope grip** (`FUN_00408f50`): grip is a factor of the surface-normal Y
//!   component (the slope cosine, `1.0` flat). Flat ground (`normal_y >= 0xf0a3`,
//!   ~0.9399) grips at the ceiling `0xe666` (0.90); between `0xb333` (0.70) and
//!   that ceiling it ramps linearly - `(normal_y - 0xb333) * 0x42ab5` (slope
//!   ~4.1667), floored at 0, then *multiplied* by the `0xe666` ceiling; at or
//!   below `0xb333` (steeper than ~45 degrees) grip is zero. The lateral slip a
//!   surface *retains* is `1.0 - grip`.
//! - **Integrator** (`FUN_0040a4a0`): `accel = force * invMass + gravity`;
//!   quadratic drag `drag = v * speed` (drag coefficient `1.0`); velocity
//!   integrates as `v += accel * 0x3333` (0.2 gain, the 40 ms step folded into
//!   the tuned coefficient). The recovered translation step uses the cached
//!   prior speed for damping, snapshots the old velocity, and advances position
//!   by `new_velocity - delta_velocity * 0x8000`.
//! - **Surface grip**: a per-wheel surface id `ctrl[0x42a + k*2]` indexes 8-byte
//!   grip-table records; the recovered coefficient tiers are `0x3d70` (~0.24,
//!   high grip) and `0x2666` (~0.15, low grip). The full per-surface table and
//!   the per-car-type parameter block (`DAT_0050e150`, stride 0xcd0, BSS) are
//!   game data loaded at runtime - they are not vendored here.
//! - **Signed force-candidate selection** (`FUN_0040d810`): one recovered branch uses
//!   a signed raw comparison between caller-derived candidates. A current
//!   candidate at or below the transition candidate is retained; above it, an
//!   alternate candidate is selected verbatim. This comparison alone does
//!   not establish a physical model or the meaning of either candidate.
//! - **Lookup-free candidate branches** (`FUN_0040d810`): when the wrapping
//!   absolute first component is below `0x6666`, transition/alternate gains are
//!   multiplied by their caller-derived bases. Otherwise a second component
//!   below `0x6666` selects both gains verbatim. When neither component is below
//!   the boundary, the paired candidate derivation composes the recovered
//!   angle, trig, and square-root lookup behavior.
//! - **Gated correction arithmetic** (`FUN_0040d810`): a nonzero control gate
//!   admits a correction only when the wrapping difference between the
//!   smoothed/current and raw wheel values has the same strict sign as the
//!   selected gear ratio. The admitted scalar is formed by two recovered
//!   Q16.16 factors, then passes through a signed wrapping-absolute limit and
//!   an optional second-scalar budget rule.
//! - **Postprocess correction scalar** (`FUN_0040fbe0`): one to four qualified
//!   lanes share `0x140000`; each strict-above-threshold angle measure feeds a
//!   wrapping Q16.16 ramp with an upper-only clamp, then scales and negates an
//!   opaque caller argument. The surrounding geometric pass applies the exact
//!   active/angle/Y/residual gates, legacy normalization, alignment clamp, and
//!   wrapping accumulator adds over four neutral vector lanes.
//! - **Surface-indexed lane scalar** (`FUN_0040d810`): an opaque nonzero
//!   per-lane override selects `0x8000`; otherwise a caller-resolved surface
//!   coefficient is used. The projected input is arithmetic-shifted by two,
//!   multiplied through an opaque global factor, and wrapping-negated.
//! - **Lane accumulation tail** (`FUN_0040d810`): the already-selected scalar
//!   first replaces the accumulator with its component-wise Q16.16 product.
//!   An admitted first correction is added along one direction; only then is
//!   the updated accumulator projected onto a second direction for the
//!   remaining-budget addition. The independent surface contribution projects
//!   a caller-provided vector and adds along the first direction.
//! - **Lane-direction construction** (`FUN_0040d810`): five caller vectors are
//!   rejected from a shared axis and normalized with the legacy square-root
//!   path. The resulting pair groups are preserved, exchanged, or partially
//!   rebuilt according to an already-resolved opaque mode.
//!
//! Values are signed Q16.16 ([`crate::fixed_point`]). See evidence
//! `rea.official-eu.sim-grip` and
//! `rea.official-eu.sim-wheel-correction-arithmetic`.
//!
//! Observations: the formulas and constants above, read from the solver passes.
//! Inferences: the slope ramp raises grip as the surface flattens (output is a
//! grip factor, high on flat ground); the two surface tiers are high/low grip.
//! Unknowns (not reproduced): the physical units of all candidates and
//! directions, the physical meaning of the nonzero control `+0x420` gate, the
//! physical meaning of a distinct suspension-damper source, runtime-loaded
//! per-surface/per-car table contents, the contributors to the accumulated
//! force/additive-acceleration inputs, and the opaque globals/callback that
//! select the direction-construction mode.

use crate::{
    fixed_point::{Fixed16, Vec3},
    legacy_math::{
        SignedArcsinePhaseError, SignedRatioAngleError, cos_q16, signed_arcsine_phase,
        signed_ratio_angle, sin_q16, sqrt_q16,
    },
};

/// Velocity-integration gain applied to acceleration (`0x3333` ~ 0.2); the
/// fixed 40 ms step is folded into this tuned coefficient.
pub const VELOCITY_GAIN: Fixed16 = Fixed16::from_raw(0x3333);

/// Half of the official-EU unit step scalar, applied to the recovered velocity
/// delta before it is subtracted from the newly integrated velocity.
pub const POSITION_HALF_DELTA: Fixed16 = Fixed16::from_raw(0x8000);

/// Lateral-grip strength removed from the sideways velocity each step
/// (`0x51e` ~ 0.02).
pub const LATERAL_GRIP_K: Fixed16 = Fixed16::from_raw(0x51e);

/// Lateral-grip strength while the handbrake is held (`0x28f` ~ 0.01, half grip).
pub const LATERAL_GRIP_K_HANDBRAKE: Fixed16 = Fixed16::from_raw(0x28f);

/// Surface-normal Y at and below which [`slope_grip`] yields zero grip (`0xb333`
/// ~ 0.70, i.e. steeper than ~45 degrees).
pub const SLOPE_NORMAL_Y_MIN: Fixed16 = Fixed16::from_raw(0xb333);

/// Surface-normal Y at and above which [`slope_grip`] saturates at the flat
/// ceiling (`0xf0a3` ~ 0.9399).
pub const SLOPE_NORMAL_Y_MAX: Fixed16 = Fixed16::from_raw(0xf0a3);

/// Slope of the grip ramp (`0x42ab5` ~ 4.1667), mapping the normal-Y band onto
/// `[0, ~1]` before the flat-grip scale.
pub const SLOPE_GRIP_RAMP: Fixed16 = Fixed16::from_raw(0x42ab5);

/// Grip factor on flat ground and the ceiling the ramp is scaled by (`0xe666`
/// ~ 0.90).
pub const SLOPE_GRIP_FLAT: Fixed16 = Fixed16::from_raw(0xe666);

/// Recovered high-grip surface coefficient tier (`0x3d70` ~ 0.24).
pub const SURFACE_GRIP_HIGH: Fixed16 = Fixed16::from_raw(0x3d70);

/// Recovered low-grip surface coefficient tier (`0x2666` ~ 0.15).
pub const SURFACE_GRIP_LOW: Fixed16 = Fixed16::from_raw(0x2666);

/// Stride, in bytes, of a surface grip-table record (the `surfId * 8` index).
pub const SURFACE_GRIP_RECORD_BYTES: usize = 8;

/// Number of signed entries in the extracted coefficient family consumed by
/// the recovered surface-indexed lane scalar.
pub const SURFACE_COEFFICIENT_COUNT: usize = 120;

/// Checked lookup failure for
/// [`surface_indexed_lane_scalar_from_first_family`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceCoefficientIndexError {
    pub index: usize,
}

/// Signed component boundary used by the two lookup-free candidate branches
/// in `FUN_0040d810` (`0x40f24f..0x40f2c7`).
pub const CANDIDATE_COMPONENT_THRESHOLD: Fixed16 = Fixed16::from_raw(0x6666);

/// Control mode for [`construct_lane_directions`].
///
/// These names describe only the observed movement of the two packed lane
/// groups. [`resolve_lane_direction_mode`] implements the native precedence;
/// their physical meanings remain unassigned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaneDirectionMode {
    /// Retain the initially constructed lane groups.
    Preserve,
    /// Exchange packed lanes 0/1 with lanes 2/3 in both direction arrays.
    Exchange,
    /// Replace the second A/B group using the recovered dependent transform.
    Rebuild,
}

/// Resolve the native selector with Rebuild taking precedence over Exchange.
#[must_use]
pub const fn resolve_lane_direction_mode(rebuild: bool, exchange: bool) -> LaneDirectionMode {
    if rebuild {
        LaneDirectionMode::Rebuild
    } else if exchange {
        LaneDirectionMode::Exchange
    } else {
        LaneDirectionMode::Preserve
    }
}

/// Produce P/R from S and `+0x264`: copy S for a zero angle, otherwise rotate it
/// through the recovered axis-angle matrix, then form the unnormalized raw cross.
#[must_use]
pub fn derive_lane_direction_pair(
    source_s: Vec3,
    cross_operand: Vec3,
    angle: i16,
    trig: &[i32],
) -> Option<(Vec3, Vec3)> {
    let source_p = if angle == 0 {
        source_s
    } else {
        integrate_orientation([source_s; 3], [0, angle, 0], trig)?[0]
    };
    Some((source_p, wrapping_cross(source_p, cross_operand)))
}

/// Neutral vector sources consumed by [`construct_lane_directions`].
///
/// The field names express only their roles in the recovered arithmetic. No
/// physical interpretation or unit-length invariant is assigned to them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaneDirectionSources {
    pub projection_axis: Vec3,
    pub source_a0: Vec3,
    pub source_b0: Vec3,
    pub source_a2: Vec3,
    pub cross_operand: Vec3,
}

/// Observable result of [`construct_lane_directions`].
///
/// `pre_mode_a_valid` records whether the wrapping square sum of each initial
/// A candidate was nonzero. The shipped routine neither exchanges nor
/// recomputes these flags after applying the selected mode, so they can differ
/// from the final vectors for degenerate inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaneDirectionOutcome {
    pub direction_a: [Vec3; 4],
    pub direction_b: [Vec3; 4],
    pub pre_mode_a_valid: [bool; 4],
}

/// Already-resolved translational inputs consumed by the official-EU
/// `FUN_0040a4a0` integration chain.
///
/// `cached_speed` is the magnitude cached by the preceding tick. The shipped
/// function reads it before updating velocity and recomputes the cache only
/// afterward, so this contract deliberately does not derive it from
/// `velocity`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TranslationStepInput {
    pub position: Vec3,
    pub velocity: Vec3,
    pub cached_speed: Fixed16,
    pub contacts_active: bool,
    pub accumulated_force: Vec3,
    pub inverse_mass: Fixed16,
    pub additive_acceleration: Vec3,
}

/// Observable values produced by [`apply_translation_step`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TranslationStepOutcome {
    pub position: Vec3,
    pub velocity: Vec3,
    pub prior_velocity: Vec3,
    pub acceleration: Vec3,
}

impl LaneDirectionOutcome {
    /// Return the four unique-vector slots in their recovered publication
    /// order: A lane 0 (`+0x380`), A lane 2 (`+0x38c`), B lane 0 (`+0x398`),
    /// and B lane 2 (`+0x3a4`).
    #[must_use]
    pub const fn published_unique_vectors(self) -> [Vec3; 4] {
        [
            self.direction_a[0],
            self.direction_a[2],
            self.direction_b[0],
            self.direction_b[2],
        ]
    }
}

/// Construct the four packed direction pairs consumed by the recovered lane
/// accumulation path in `FUN_0040d810` (`0x40d9b6..0x40e45d`).
///
/// Each source is rejected from `projection_axis` as `v + axis * -dot(v,
/// axis)`, preserving the observed Q16.16 truncation, negation, and wrapping
/// order. The second B source is the raw cross product `source_a2 x
/// cross_operand`; it deliberately does not use the rejected/normalized A2.
/// Every normalization uses the recovered square-root lookup and maps a zero
/// root to the zero vector.
///
/// The caller supplies [`LaneDirectionMode`], commonly through
/// [`resolve_lane_direction_mode`]. No physical meaning is assigned to the
/// modes or vectors.
#[must_use]
pub fn construct_lane_directions(
    sources: LaneDirectionSources,
    mode: LaneDirectionMode,
) -> LaneDirectionOutcome {
    let (a0, a0_valid) =
        normalize_lane_direction(reject_from_axis(sources.source_a0, sources.projection_axis));
    let (b0, _) =
        normalize_lane_direction(reject_from_axis(sources.source_b0, sources.projection_axis));
    let (a2, a2_valid) =
        normalize_lane_direction(reject_from_axis(sources.source_a2, sources.projection_axis));
    let raw_b2 = wrapping_cross(sources.source_a2, sources.cross_operand);
    let (b2, _) = normalize_lane_direction(reject_from_axis(raw_b2, sources.projection_axis));

    let mut direction_a = [a0, a0, a2, a2];
    let mut direction_b = [b0, b0, b2, b2];
    let pre_mode_a_valid = [a0_valid, a0_valid, a2_valid, a2_valid];

    match mode {
        LaneDirectionMode::Preserve => {}
        LaneDirectionMode::Exchange => {
            direction_a = [a2, a2, a0, a0];
            direction_b = [b2, b2, b0, b0];
        }
        LaneDirectionMode::Rebuild => {
            let doubled_alignment = b2.dot(a0).mul(Fixed16::from_raw(0x20000));
            let rebuilt_a2 = a0.sub(b2.scale(doubled_alignment));
            let alignment = b2.dot(rebuilt_a2);
            let (rebuilt_b2, _) = normalize_lane_direction(b2.sub(rebuilt_a2.scale(alignment)));
            direction_a = [a0, a0, rebuilt_a2, rebuilt_a2];
            direction_b = [b0, b0, rebuilt_b2, rebuilt_b2];
        }
    }

    LaneDirectionOutcome {
        direction_a,
        direction_b,
        pre_mode_a_valid,
    }
}

fn reject_from_axis(source: Vec3, axis: Vec3) -> Vec3 {
    let negative_projection = source.dot(axis).neg();
    source.add(axis.scale(negative_projection))
}

fn wrapping_cross(lhs: Vec3, rhs: Vec3) -> Vec3 {
    Vec3::new(
        lhs.y.mul(rhs.z).sub(lhs.z.mul(rhs.y)),
        lhs.z.mul(rhs.x).sub(lhs.x.mul(rhs.z)),
        lhs.x.mul(rhs.y).sub(lhs.y.mul(rhs.x)),
    )
}

fn normalize_lane_direction(direction: Vec3) -> (Vec3, bool) {
    let square_sum = direction.dot(direction).raw() as u32;
    let root = sqrt_q16(square_sum);
    if root == 0 {
        return (Vec3::ZERO, square_sum != 0);
    }

    let reciprocal = ((1_u64 << 32) / u64::from(root)) as i32;
    (
        direction.scale(Fixed16::from_raw(reciprocal)),
        square_sum != 0,
    )
}

/// Rejection from the lookup-dependent candidate derivation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LookupDependentCandidateError {
    /// One of the wrapping-absolute components belongs to an earlier branch.
    LookupFreePath,
    /// The recovered signed ratio lookup cannot safely represent an operand.
    Angle(SignedRatioAngleError),
}

/// Already-resolved inputs to one lane's accumulation tail in
/// `FUN_0040d810` (`0x40f5ee..0x40f9ff`).
///
/// The neutral field names reflect only their roles in the recovered
/// arithmetic. Candidate derivation, table lookup, direction construction,
/// inactive-lane handling, and physical interpretation remain upstream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaneAccumulationInput {
    pub accumulator: Vec3,
    pub selected_scalar: Fixed16,
    pub direction_a: Vec3,
    pub direction_b: Vec3,
    pub surface_vector: Vec3,
    pub correction_gate: i32,
    pub smoothed_value: Fixed16,
    pub raw_value: Fixed16,
    pub gear_ratio: Fixed16,
    pub global_factor: Fixed16,
    pub correction_threshold: Fixed16,
    pub correction_alternate: Fixed16,
    pub second_budget: Fixed16,
    pub surface_coefficient: Fixed16,
    pub surface_override: i32,
}

/// Observable result of [`apply_lane_accumulation`].
///
/// Optional scalars are `None` exactly when the corresponding recovered branch
/// skipped its vector addition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaneAccumulationOutcome {
    pub accumulator: Vec3,
    pub first_correction_scalar: Option<Fixed16>,
    pub second_correction_scalar: Option<Fixed16>,
    pub surface_scalar: Option<Fixed16>,
}

/// Strict signed angle boundary for the qualified-lane postprocess in
/// `FUN_0040fbe0`.
pub const POSTPROCESS_ANGLE_THRESHOLD: Fixed16 = Fixed16::from_raw(0xa0000);

/// Truncated Q16.16 reciprocal of the postprocess ramp span:
/// `q16_div(1.0, 0x320000) == 0x51e`.
pub const POSTPROCESS_RAMP_RECIPROCAL: Fixed16 = Fixed16::from_raw(0x51e);

/// One neutral vector lane consumed by the geometric postprocess in
/// `FUN_0040fbe0`.
///
/// The vector labels intentionally mirror only their roles in the recovered
/// arithmetic. Their physical meanings are not established.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeometricPostprocessLane {
    pub active: bool,
    pub vector_a: Vec3,
    pub vector_b: Vec3,
    pub vector_c: Vec3,
    pub accumulator: Vec3,
}

/// Observable qualification result of [`apply_geometric_lane_postprocess`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeometricPostprocessOutcome {
    pub qualified: [bool; 4],
    pub angle_measures: [Fixed16; 4],
}

/// Safe-domain failure while reproducing the geometric lane postprocess.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometricPostprocessError {
    /// The signed arcsine lookup cannot safely represent this lane's input.
    Angle {
        lane: u8,
        source: SignedArcsinePhaseError,
    },
}

/// Convert the recovered A/B dot product to the Q16.16 angle measure used by
/// `FUN_0040fbe0` qualification.
///
/// A signed dot at or above `1.0` maps directly to zero. Smaller values use
/// the signed arcsine phase, complement it about `0x400`, and multiply by
/// `0x1680` with wrapping i32 arithmetic. This preserves the legacy exact
/// negative-one adjacency sentinel through [`signed_arcsine_phase`].
///
/// # Errors
///
/// Propagates a signed-arcsine lookup domain failure.
pub fn geometric_lane_angle_measure(dot_ab: Fixed16) -> Result<Fixed16, SignedArcsinePhaseError> {
    if dot_ab.raw() >= Fixed16::ONE.raw() {
        return Ok(Fixed16::ZERO);
    }

    let phase = i32::from(signed_arcsine_phase(dot_ab.raw())?);
    Ok(Fixed16::from_raw(
        0x400_i32.wrapping_sub(phase).wrapping_mul(0x1680),
    ))
}

/// Apply the recovered four-lane geometric qualification, normalization, and
/// stateful accumulator update from `FUN_0040fbe0`.
///
/// Qualification is exact and ordered: a lane must be active, its A/B angle
/// must be strictly above [`POSTPROCESS_ANGLE_THRESHOLD`], `A.y` must be
/// strictly below `B.y`, and `dot(A, C - B*dot(B,C))` must be strictly
/// negative. Qualified lanes share the recovered `0x140000` budget. Each
/// direction is `A - B*dot(B,A)`, normalized through the legacy square-root
/// lookup and signed reciprocal, aligned to C with the signed
/// wrapping-absolute limit (including its `i32::MIN` pass-through), scaled by
/// the existing postprocess gain, then wrapping-added to its accumulator.
///
/// Updates are published only after every active lane has passed the safe arc
/// lookup domain, so an error leaves all accumulators unchanged.
///
/// # Errors
///
/// Returns a lane-indexed lookup failure rather than reproducing an unchecked
/// table read.
pub fn apply_geometric_lane_postprocess(
    lanes: &mut [GeometricPostprocessLane; 4],
    argument_scale: Fixed16,
) -> Result<GeometricPostprocessOutcome, GeometricPostprocessError> {
    let mut qualified = [false; 4];
    let mut angle_measures = [Fixed16::ZERO; 4];

    for (index, lane) in lanes.iter().enumerate() {
        if !lane.active {
            continue;
        }

        let angle =
            geometric_lane_angle_measure(lane.vector_b.dot(lane.vector_a)).map_err(|source| {
                GeometricPostprocessError::Angle {
                    lane: index as u8,
                    source,
                }
            })?;
        angle_measures[index] = angle;
        if angle.raw() <= POSTPROCESS_ANGLE_THRESHOLD.raw()
            || lane.vector_a.y.raw() >= lane.vector_b.y.raw()
        {
            continue;
        }

        let projection = lane.vector_b.dot(lane.vector_c);
        let residual = lane.vector_c.sub(lane.vector_b.scale(projection));
        if lane.vector_a.dot(residual).raw() < 0 {
            qualified[index] = true;
        }
    }

    let qualified_count = qualified.iter().filter(|&&value| value).count() as u8;
    if qualified_count == 0 {
        return Ok(GeometricPostprocessOutcome {
            qualified,
            angle_measures,
        });
    }

    let mut pending = [Vec3::ZERO; 4];
    for (index, lane) in lanes.iter().enumerate() {
        if !qualified[index] {
            continue;
        }

        let projection = lane.vector_b.dot(lane.vector_a);
        let residual = lane.vector_a.sub(lane.vector_b.scale(projection));
        let direction = normalize_geometric_residual(residual);
        let alignment = direction.dot(lane.vector_c);
        let bounded_alignment = bound_geometric_alignment(alignment);
        let aligned_direction = direction.scale(bounded_alignment);
        let Some(gain) = postprocess_lane_correction_gain(
            angle_measures[index],
            argument_scale,
            qualified_count,
        ) else {
            continue;
        };
        pending[index] = aligned_direction.scale(gain);
    }

    for (lane, delta) in lanes.iter_mut().zip(pending) {
        lane.accumulator = lane.accumulator.add(delta);
    }
    Ok(GeometricPostprocessOutcome {
        qualified,
        angle_measures,
    })
}

fn normalize_geometric_residual(residual: Vec3) -> Vec3 {
    let square_sum = residual.dot(residual).raw() as u32;
    let root = sqrt_q16(square_sum);
    if root == 0 {
        return Vec3::ZERO;
    }

    // For every nonzero u32 square sum, `sqrt_q16` returns a positive root in
    // the bounded legacy normalization range. The recovered 2^32/root
    // reciprocal therefore fits a positive i32; no divide-error state is
    // reachable after the explicit zero-root branch.
    let reciprocal = ((1_u64 << 32) / u64::from(root)) as i32;
    residual.scale(Fixed16::from_raw(reciprocal))
}

const fn bound_geometric_alignment(alignment: Fixed16) -> Fixed16 {
    let absolute_alignment = alignment.raw().wrapping_abs();
    if absolute_alignment <= Fixed16::ONE.raw() {
        alignment
    } else if alignment.raw() > 0 {
        Fixed16::ONE
    } else {
        Fixed16::ONE.neg()
    }
}

/// Recover the per-lane share used when `FUN_0040fbe0` has qualified one to
/// four lanes.
///
/// The shipped code performs signed Q16.16 division of `0x140000` by the
/// qualified count represented as Q16.16. Counts outside the four-lane domain
/// cannot arise from that loop and are rejected rather than assigned behavior.
#[must_use]
pub const fn postprocess_qualified_lane_share(qualified_lane_count: u8) -> Option<Fixed16> {
    if qualified_lane_count == 0 || qualified_lane_count > 4 {
        return None;
    }

    let numerator = 0x140000_i64 << 16;
    let denominator = (qualified_lane_count as i64) << 16;
    Some(Fixed16::from_raw((numerator / denominator) as i32))
}

/// Apply the raw signed ramp arithmetic recovered from `FUN_0040fbe0`.
///
/// Subtraction and multiplication are modular Q16.16 operations. The final
/// comparison is a signed upper-only clamp: negative results are deliberately
/// retained. Qualification against [`POSTPROCESS_ANGLE_THRESHOLD`] is kept in
/// [`postprocess_lane_correction_gain`], outside this raw arithmetic helper.
#[must_use]
pub const fn postprocess_correction_ramp(angle_measure: Fixed16) -> Fixed16 {
    let ramp = angle_measure
        .sub(POSTPROCESS_ANGLE_THRESHOLD)
        .mul(POSTPROCESS_RAMP_RECIPROCAL);
    if ramp.raw() > Fixed16::ONE.raw() {
        Fixed16::ONE
    } else {
        ramp
    }
}

/// Derive the final qualified-lane correction gain from `FUN_0040fbe0`.
///
/// The strict angle gate rejects equality. Invalid lane counts are also
/// rejected. For admitted inputs, each Q16.16 multiply and the final negation
/// preserve the recovered wrapping behavior.
#[must_use]
pub const fn postprocess_lane_correction_gain(
    angle_measure: Fixed16,
    argument_scale: Fixed16,
    qualified_lane_count: u8,
) -> Option<Fixed16> {
    if angle_measure.raw() <= POSTPROCESS_ANGLE_THRESHOLD.raw() {
        return None;
    }

    let share = match postprocess_qualified_lane_share(qualified_lane_count) {
        Some(share) => share,
        None => return None,
    };
    let scaled_argument = argument_scale.mul(share);
    Some(
        postprocess_correction_ramp(angle_measure)
            .mul(scaled_argument)
            .neg(),
    )
}

/// Derive the surface-indexed scalar at `FUN_0040d810`
/// `0x40f890..0x40f986` from already-resolved caller inputs.
///
/// A nonzero opaque override selects the literal coefficient `0x8000`;
/// otherwise `surface_coefficient` is used. The path is skipped only when both
/// coefficient inputs are zero, or when `projection` is zero. The arithmetic
/// right shift, Q16.16 products, and final negation retain signed wrapping
/// behavior. No physical meaning is assigned to this scalar or its override.
#[must_use]
pub const fn surface_indexed_lane_scalar(
    global_factor: Fixed16,
    surface_coefficient: Fixed16,
    override_value: i32,
    projection: Fixed16,
) -> Option<Fixed16> {
    if (surface_coefficient.raw() == 0 && override_value == 0) || projection.raw() == 0 {
        return None;
    }

    let selected_coefficient = if override_value != 0 {
        Fixed16::from_raw(0x8000)
    } else {
        surface_coefficient
    };
    let scaled_factor = global_factor.mul(selected_coefficient);
    let quarter_projection = Fixed16::from_raw(projection.raw() >> 2);
    Some(quarter_projection.mul(scaled_factor).neg())
}

/// Resolve the coefficient from the first extracted family and apply the
/// recovered surface-indexed lane scalar.
///
/// Static evidence pins `FUN_0040d810` to the first family rooted at
/// `DAT_004f72cc`. Unlike the shipped unchecked table access, this safe
/// boundary reports an out-of-range surface index. The adjacent family is not
/// part of this compatibility contract because its consumer is unknown.
pub const fn surface_indexed_lane_scalar_from_first_family(
    first_family: &[i32; SURFACE_COEFFICIENT_COUNT],
    surface_index: usize,
    global_factor: Fixed16,
    override_value: i32,
    projection: Fixed16,
) -> Result<Option<Fixed16>, SurfaceCoefficientIndexError> {
    if surface_index >= SURFACE_COEFFICIENT_COUNT {
        return Err(SurfaceCoefficientIndexError {
            index: surface_index,
        });
    }
    let coefficient = Fixed16::from_raw(first_family[surface_index]);
    Ok(surface_indexed_lane_scalar(
        global_factor,
        coefficient,
        override_value,
        projection,
    ))
}

/// Select between caller-derived force candidates using the recovered signed
/// raw comparison in `FUN_0040d810`.
///
/// Values at or below `transition` are retained. Values above it select
/// `alternate` verbatim. Candidate derivation and physical
/// meaning remain outside this narrow recovered rule.
#[must_use]
pub const fn select_signed_force_candidate(
    candidate: Fixed16,
    transition: Fixed16,
    alternate: Fixed16,
) -> Fixed16 {
    if candidate.raw() > transition.raw() {
        alternate
    } else {
        candidate
    }
}

/// Derive the candidate pair for either lookup-free branch recovered from
/// `FUN_0040d810`.
///
/// Components use wrapping absolute value followed by signed comparison with
/// [`CANDIDATE_COMPONENT_THRESHOLD`]. When `component_a` is below the boundary,
/// that first branch has priority and returns the Q16.16 products of each gain
/// and base. Otherwise, a `component_b` below the boundary returns both gains
/// verbatim. `None` denotes the remaining branch, whose derivation depends on
/// lookup tables and is intentionally outside this helper.
#[must_use]
pub const fn derive_lookup_free_candidates(
    component_a: Fixed16,
    component_b: Fixed16,
    transition_gain: Fixed16,
    transition_base: Fixed16,
    alternate_gain: Fixed16,
    alternate_base: Fixed16,
) -> Option<(Fixed16, Fixed16)> {
    let absolute_a = Fixed16::from_raw(component_a.raw().wrapping_abs());
    if absolute_a.raw() < CANDIDATE_COMPONENT_THRESHOLD.raw() {
        return Some((
            transition_gain.mul(transition_base),
            alternate_gain.mul(alternate_base),
        ));
    }

    let absolute_b = Fixed16::from_raw(component_b.raw().wrapping_abs());
    if absolute_b.raw() < CANDIDATE_COMPONENT_THRESHOLD.raw() {
        Some((transition_gain, alternate_gain))
    } else {
        None
    }
}

/// Derive the paired candidates for the lookup-dependent branch of
/// `FUN_0040d810` (`0x40f2cc..0x40f5de`).
///
/// The components first use the same wrapping absolute value and signed
/// `0x6666` gates as [`derive_lookup_free_candidates`]. Admitted inputs then
/// pass through the recovered signed-ratio phase lookup, exact sine/cosine
/// tables, wrapping Q16.16 square sum, legacy square-root lookup, and the two
/// caller-provided gains. The returned order is `(transition, alternate)` for
/// [`select_signed_force_candidate`]. No physical meaning is assigned to the
/// inputs or outputs.
///
/// # Errors
///
/// Returns [`LookupDependentCandidateError::LookupFreePath`] when either
/// component belongs to an earlier branch, and propagates unsafe angle input
/// domains as [`LookupDependentCandidateError::Angle`]. A zero square sum is
/// valid here: this path takes a square root and performs no reciprocal divide.
pub fn derive_lookup_dependent_candidates(
    component_a: Fixed16,
    component_b: Fixed16,
    transition_gain: Fixed16,
    transition_base: Fixed16,
    alternate_gain: Fixed16,
    alternate_base: Fixed16,
) -> Result<(Fixed16, Fixed16), LookupDependentCandidateError> {
    let absolute_a = Fixed16::from_raw(component_a.raw().wrapping_abs());
    let absolute_b = Fixed16::from_raw(component_b.raw().wrapping_abs());
    if absolute_a.raw() < CANDIDATE_COMPONENT_THRESHOLD.raw()
        || absolute_b.raw() < CANDIDATE_COMPONENT_THRESHOLD.raw()
    {
        return Err(LookupDependentCandidateError::LookupFreePath);
    }

    let transition = derive_one_lookup_dependent_candidate(
        absolute_a,
        absolute_b,
        transition_base,
        transition_gain,
    )?;
    let alternate = derive_one_lookup_dependent_candidate(
        absolute_a,
        absolute_b,
        alternate_base,
        alternate_gain,
    )?;
    Ok((transition, alternate))
}

fn derive_one_lookup_dependent_candidate(
    component_a: Fixed16,
    component_b: Fixed16,
    base: Fixed16,
    gain: Fixed16,
) -> Result<Fixed16, LookupDependentCandidateError> {
    let projected = component_a.mul(base);
    let phase = match signed_ratio_angle(projected.raw(), component_b.raw()) {
        Ok(phase) => phase as u16,
        Err(error) => return Err(LookupDependentCandidateError::Angle(error)),
    };
    let scaled_cosine = base.mul(cos_q16(phase));
    let sine = sin_q16(phase);
    let square_sum = scaled_cosine
        .mul(scaled_cosine)
        .raw()
        .wrapping_add(sine.mul(sine).raw()) as u32;
    let root = Fixed16::from_raw(sqrt_q16(square_sum) as i32);
    Ok(root.mul(gain))
}

/// Derive the correction candidate admitted by the recovered nonzero gate and
/// same-strict-sign rule in `FUN_0040d810`.
///
/// `smoothed.sub(raw)` is intentionally modular. A zero gate, zero difference,
/// zero ratio, or opposite signs skips the path. The physical meaning of the
/// gate and inputs remains outside this arithmetic contract.
#[must_use]
pub const fn gated_correction_candidate(
    gate: i32,
    smoothed: Fixed16,
    raw: Fixed16,
    gear_ratio: Fixed16,
    global_factor: Fixed16,
) -> Option<Fixed16> {
    if gate == 0 {
        return None;
    }

    let difference = smoothed.sub(raw);
    let same_strict_sign = (difference.raw() > 0 && gear_ratio.raw() > 0)
        || (difference.raw() < 0 && gear_ratio.raw() < 0);
    if !same_strict_sign {
        return None;
    }

    let quarter_difference = difference.mul(Fixed16::from_raw(0x4000));
    let scaled_global_factor = global_factor.mul(Fixed16::from_raw(0x1999));
    Some(quarter_difference.mul(scaled_global_factor))
}

/// Apply the recovered signed wrapping-absolute limit to a correction
/// candidate.
///
/// A candidate whose wrapping absolute value is at or below `threshold` is
/// retained. Above it, a positive candidate selects `alternate` verbatim and
/// a nonpositive candidate selects its modular negation. Signed ordering and
/// `INT_MIN` wrapping are intentional.
#[must_use]
pub const fn limit_signed_correction_candidate(
    candidate: Fixed16,
    threshold: Fixed16,
    alternate: Fixed16,
) -> Fixed16 {
    let absolute = Fixed16::from_raw(candidate.raw().wrapping_abs());
    if absolute.raw() <= threshold.raw() {
        return candidate;
    }

    if candidate.raw() > 0 {
        alternate
    } else {
        alternate.neg()
    }
}

/// Select the optional second correction scalar from the first scalar, a
/// signed projection, and the caller-provided budget.
///
/// The projection's wrapping absolute value is subtracted modularly from the
/// budget. A nonpositive remainder skips the correction. Otherwise the first
/// scalar's wrapping absolute value is bounded by that remainder using signed
/// ordering. Positive projections retain the bounded sign; zero and negative
/// projections take its modular negation.
#[must_use]
pub const fn second_correction_scalar(
    first_scalar: Fixed16,
    projection: Fixed16,
    budget: Fixed16,
) -> Option<Fixed16> {
    let absolute_projection = Fixed16::from_raw(projection.raw().wrapping_abs());
    let remaining = budget.sub(absolute_projection);
    if remaining.raw() <= 0 {
        return None;
    }

    let absolute_first = Fixed16::from_raw(first_scalar.raw().wrapping_abs());
    let bounded = if absolute_first.raw() <= remaining.raw() {
        absolute_first
    } else {
        remaining
    };
    if projection.raw() > 0 {
        Some(bounded)
    } else {
        Some(bounded.neg())
    }
}

/// Apply one lane's recovered accumulation tail from `FUN_0040d810`.
///
/// Ordering is part of the contract. The selected scalar first replaces the
/// accumulator with its component-wise Q16.16 product. When the gated first
/// correction is admitted, it is limited and added along `direction_a`; the
/// second projection then reads that updated accumulator and may add along
/// `direction_b`. Finally, the surface path is considered independently of the
/// correction gate and may add along `direction_a`.
///
/// All vector products, dot products, and additions use the existing modular
/// Q16.16 operations. Inputs that require checked lookup or direction
/// construction are deliberately resolved by the caller before this total,
/// pure value transformation.
#[must_use]
pub const fn apply_lane_accumulation(input: LaneAccumulationInput) -> LaneAccumulationOutcome {
    let mut accumulator = input.accumulator.scale(input.selected_scalar);
    let mut first_correction_scalar = None;
    let mut selected_second_scalar = None;

    if let Some(candidate) = gated_correction_candidate(
        input.correction_gate,
        input.smoothed_value,
        input.raw_value,
        input.gear_ratio,
        input.global_factor,
    ) {
        let first_scalar = limit_signed_correction_candidate(
            candidate,
            input.correction_threshold,
            input.correction_alternate,
        );
        accumulator = accumulator.add(input.direction_a.scale(first_scalar));
        first_correction_scalar = Some(first_scalar);

        let projection = input.direction_b.dot(accumulator);
        selected_second_scalar =
            second_correction_scalar(first_scalar, projection, input.second_budget);
        if let Some(second_scalar) = selected_second_scalar {
            accumulator = accumulator.add(input.direction_b.scale(second_scalar));
        }
    }

    let surface_projection = input.direction_a.dot(input.surface_vector);
    let surface_scalar = surface_indexed_lane_scalar(
        input.global_factor,
        input.surface_coefficient,
        input.surface_override,
        surface_projection,
    );
    if let Some(scalar) = surface_scalar {
        accumulator = accumulator.add(input.direction_a.scale(scalar));
    }

    LaneAccumulationOutcome {
        accumulator,
        first_correction_scalar,
        second_correction_scalar: selected_second_scalar,
        surface_scalar,
    }
}

/// Remove the sideways velocity component, reproducing `FUN_0041b270`:
/// `perp = v - fwd * dot(fwd, v); v -= perp * K`. `forward_unit` is the car's
/// forward direction (a unit vector); `handbrake` halves the grip.
pub fn apply_lateral_grip(velocity: Vec3, forward_unit: Vec3, handbrake: bool) -> Vec3 {
    let along = forward_unit.dot(velocity);
    let perpendicular = velocity.sub(forward_unit.scale(along));
    let k = if handbrake {
        LATERAL_GRIP_K_HANDBRAKE
    } else {
        LATERAL_GRIP_K
    };
    velocity.sub(perpendicular.scale(k))
}

/// The slope grip factor for a surface-normal Y component, reproducing
/// `FUN_00408f50`: at or above [`SLOPE_NORMAL_Y_MAX`] it is the flat ceiling
/// [`SLOPE_GRIP_FLAT`]; between [`SLOPE_NORMAL_Y_MIN`] and that limit it ramps as
/// `(normal_y - min) * `[`SLOPE_GRIP_RAMP`] (floored at 0) scaled by the ceiling;
/// at or below the min it is zero.
pub fn slope_grip(surface_normal_y: Fixed16) -> Fixed16 {
    if surface_normal_y >= SLOPE_NORMAL_Y_MAX {
        return SLOPE_GRIP_FLAT;
    }
    let ramp = surface_normal_y
        .sub(SLOPE_NORMAL_Y_MIN)
        .mul(SLOPE_GRIP_RAMP);
    if ramp.raw() <= 0 {
        return Fixed16::ZERO;
    }
    ramp.mul(SLOPE_GRIP_FLAT)
}

/// Net acceleration from the summed force, reproducing the integrator's
/// `accel = force * invMass + gravity`.
pub fn net_accel(force: Vec3, inverse_mass: Fixed16, gravity: Vec3) -> Vec3 {
    force.scale(inverse_mass).add(gravity)
}

/// Quadratic drag `drag = v * speed` (drag coefficient `1.0`), as subtracted
/// from the accumulated force in the integrator.
pub fn drag(velocity: Vec3, speed: Fixed16) -> Vec3 {
    velocity.scale(speed)
}

/// Integrate velocity by one step: `v += accel * 0.2` ([`VELOCITY_GAIN`]).
pub fn integrate_velocity(velocity: Vec3, accel: Vec3) -> Vec3 {
    velocity.add(accel.scale(VELOCITY_GAIN))
}

/// Sum the lane forces consumed by `FUN_0040a4a0` before translational damping.
///
/// Non-wheel lanes are included unconditionally. Wheel lanes are included only
/// when their contact flag at `+0x698+4*wheel` is set; iteration and addition
/// follow native wheel order. The moment/cross-product accumulation is separate.
#[must_use]
pub fn sum_translation_lane_forces(
    base: Vec3,
    non_wheel_lanes: &[Vec3],
    wheel_lanes: [(bool, Vec3); 4],
) -> Vec3 {
    non_wheel_lanes
        .iter()
        .copied()
        .chain(
            wheel_lanes
                .into_iter()
                .filter_map(|(contact, force)| contact.then_some(force)),
        )
        .fold(base, Vec3::add)
}

/// Apply the three row-wise Q16.16 dot products of `FUN_0048e100`.
#[must_use]
pub fn transform_contact_force(rows: [Vec3; 3], force: Vec3) -> Vec3 {
    Vec3::new(rows[0].dot(force), rows[1].dot(force), rows[2].dot(force))
}

/// Rotate and renormalize a basis about one axis using the runtime trig table.
/// Zero-angle stages preserve the basis exactly. `None` rejects a short table.
#[must_use]
pub fn rotate_basis_axis_angle(
    basis: [Vec3; 3],
    axis: Vec3,
    angle: i16,
    trig: &[i32],
) -> Option<[Vec3; 3]> {
    if angle == 0 {
        return Some(basis);
    }
    if trig.len() < 0x1000 {
        return None;
    }
    let angle = i32::from(angle);
    let sine = Fixed16::from_raw(trig[(-angle & 0xfff) as usize]);
    let cosine = Fixed16::from_raw(trig[((0x400 - angle) & 0xfff) as usize]);
    let one_minus_cosine = Fixed16::ONE.sub(cosine);
    let xx = axis.x.mul(axis.x);
    let yy = axis.y.mul(axis.y);
    let zz = axis.z.mul(axis.z);
    let xy = axis.x.mul(axis.y).mul(one_minus_cosine);
    let xz = axis.x.mul(axis.z).mul(one_minus_cosine);
    let zy = axis.z.mul(axis.y).mul(one_minus_cosine);
    let rows = [
        Vec3::new(
            cosine.mul(Fixed16::ONE.sub(xx)).add(xx),
            xy.sub(axis.z.mul(sine)),
            xz.add(axis.y.mul(sine)),
        ),
        Vec3::new(
            xy.add(axis.z.mul(sine)),
            cosine.mul(Fixed16::ONE.sub(yy)).add(yy),
            zy.sub(axis.x.mul(sine)),
        ),
        Vec3::new(
            xz.sub(axis.y.mul(sine)),
            zy.add(axis.x.mul(sine)),
            cosine.mul(Fixed16::ONE.sub(zz)).add(zz),
        ),
    ];
    Some(basis.map(|vector| normalize_geometric_residual(transform_contact_force(rows, vector))))
}

/// Apply the recovered `FUN_004184e0` stage order: basis axes 1, 2, then 0.
#[must_use]
pub fn integrate_orientation(
    mut basis: [Vec3; 3],
    angles: [i16; 3],
    trig: &[i32],
) -> Option<[Vec3; 3]> {
    for index in [1, 2, 0] {
        basis = rotate_basis_axis_angle(basis, basis[index], angles[index], trig)?;
    }
    Some(basis)
}

/// Convert one angular-velocity component to the native 16-bit angle increment:
/// `(qmul(current,scale) - qmul(delta,scale/2)) * 0x28b >> 16`.
#[must_use]
pub fn orientation_angle_increment(current: Fixed16, delta: Fixed16, scale: Fixed16) -> i16 {
    let half_delta = delta.mul(Fixed16::from_raw(scale.raw() / 2));
    let half_delta_velocity = current.mul(scale).sub(half_delta);
    (half_delta_velocity.raw().wrapping_mul(0x28b) >> 16) as i16
}

/// Compose angular-velocity conversion with the recovered three-stage update.
#[must_use]
pub fn integrate_orientation_from_velocities(
    basis: [Vec3; 3],
    current: Vec3,
    delta: Vec3,
    scale: Fixed16,
    trig: &[i32],
) -> Option<[Vec3; 3]> {
    let angles = [
        orientation_angle_increment(current.x, delta.x, scale),
        orientation_angle_increment(current.y, delta.y, scale),
        orientation_angle_increment(current.z, delta.z, scale),
    ];
    integrate_orientation(basis, angles, trig)
}

/// Transform a contact force before its recovered moment cross product.
#[must_use]
pub fn transformed_contact_moment(rows: [Vec3; 3], force: Vec3, offset: Vec3) -> Vec3 {
    wrapping_cross(transform_contact_force(rows, force), offset)
}

/// Accumulate the moment portion of `FUN_0040a4a0` as
/// `transformed_force x contact_offset` with wrapping Q16.16 products.
/// Non-wheel contacts are unconditional; wheel contacts use their contact flag.
#[must_use]
pub fn sum_contact_moments(
    non_wheel_contacts: &[(Vec3, Vec3)],
    wheel_contacts: [(bool, Vec3, Vec3); 4],
) -> Vec3 {
    non_wheel_contacts
        .iter()
        .copied()
        .chain(
            wheel_contacts
                .into_iter()
                .filter_map(|(contact, force, offset)| contact.then_some((force, offset))),
        )
        .fold(Vec3::ZERO, |moment, (force, offset)| {
            moment.add(wrapping_cross(force, offset))
        })
}

/// Inputs to the angular momentum update in `FUN_0040a4a0`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AngularMomentumStepInput {
    pub prior_moment: Vec3,
    pub contact_moment: Vec3,
    pub moment_scale: Fixed16,
    pub inertia_scale: Vec3,
    pub angular_velocity: Vec3,
    pub time_step: Fixed16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AngularMomentumStepOutcome {
    pub scaled_prior_moment: Vec3,
    pub acceleration: Vec3,
    pub delta_velocity: Vec3,
    pub angular_velocity: Vec3,
}

/// Scale the prior moment, add the contact moment, apply componentwise inertia,
/// and negatively integrate the result into angular velocity.
#[must_use]
pub fn apply_angular_momentum_step(input: AngularMomentumStepInput) -> AngularMomentumStepOutcome {
    let scaled_prior_moment = input.prior_moment.scale(input.moment_scale);
    let combined = scaled_prior_moment.add(input.contact_moment);
    let acceleration = Vec3::new(
        combined.x.mul(input.inertia_scale.x),
        combined.y.mul(input.inertia_scale.y),
        combined.z.mul(input.inertia_scale.z),
    );
    let scaled_delta = acceleration.scale(input.time_step);
    let delta_velocity = Vec3::new(
        scaled_delta.x.neg(),
        scaled_delta.y.neg(),
        scaled_delta.z.neg(),
    );
    AngularMomentumStepOutcome {
        scaled_prior_moment,
        acceleration,
        delta_velocity,
        angular_velocity: input.angular_velocity.add(delta_velocity),
    }
}

/// Symmetric angular-velocity limit applied when either `+0x734` or `+0x736`
/// is nonzero in `FUN_0040a4a0`.
pub const ANGULAR_VELOCITY_LIMIT: Fixed16 = Fixed16::from_raw(0x3333);

/// Apply the recovered per-component angular clamp. The comparison uses
/// wrapping absolute value, so `i32::MIN` is deliberately left unchanged.
#[must_use]
pub fn clamp_angular_velocity(velocity: Vec3, enabled: bool) -> Vec3 {
    if !enabled {
        return velocity;
    }
    let clamp = |value: Fixed16| {
        if value.raw().wrapping_abs() > ANGULAR_VELOCITY_LIMIT.raw() {
            if value.raw() < 1 {
                ANGULAR_VELOCITY_LIMIT.neg()
            } else {
                ANGULAR_VELOCITY_LIMIT
            }
        } else {
            value
        }
    };
    Vec3::new(clamp(velocity.x), clamp(velocity.y), clamp(velocity.z))
}

/// Control flags selecting the angular reset branches in `FUN_0040a4a0`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlAxisFlags {
    pub mode_3ec: i32,
    pub axis_3f4: i32,
    pub axis_3f8: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlAxisResetOutcome {
    pub angular_velocity: Vec3,
    pub angle_increments: [i16; 3],
}

/// Apply the recovered control-axis reset priority. The `+0x3f4` branch clears
/// X/Z velocity and angles 0/2; otherwise `+0x3f8` clears X/Y velocity and
/// angles 0/1. Both require nonzero `+0x3ec`.
#[must_use]
pub fn apply_control_axis_resets(
    mut angular_velocity: Vec3,
    mut angle_increments: [i16; 3],
    flags: ControlAxisFlags,
) -> ControlAxisResetOutcome {
    if flags.mode_3ec == 0 {
        return ControlAxisResetOutcome {
            angular_velocity,
            angle_increments,
        };
    }
    if flags.axis_3f4 != 0 {
        angular_velocity.x = Fixed16::ZERO;
        angular_velocity.z = Fixed16::ZERO;
        angle_increments[0] = 0;
        angle_increments[2] = 0;
    } else if flags.axis_3f8 != 0 {
        angular_velocity.x = Fixed16::ZERO;
        angular_velocity.y = Fixed16::ZERO;
        angle_increments[0] = 0;
        angle_increments[1] = 0;
    }
    ControlAxisResetOutcome {
        angular_velocity,
        angle_increments,
    }
}

/// Inputs for the recovered momentum-to-orientation pipeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AngularOrientationInput {
    pub momentum: AngularMomentumStepInput,
    pub clamp_enabled: bool,
    pub control: ControlAxisFlags,
    pub basis: [Vec3; 3],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AngularOrientationOutcome {
    pub momentum: AngularMomentumStepOutcome,
    pub angular_velocity: Vec3,
    pub angle_increments: [i16; 3],
    pub basis: [Vec3; 3],
}

/// Compose momentum integration, optional clamping, angle conversion, selective
/// resets, and the three-stage orientation update in native order.
#[must_use]
pub fn apply_angular_orientation_pipeline(
    input: AngularOrientationInput,
    trig: &[i32],
) -> Option<AngularOrientationOutcome> {
    let momentum = apply_angular_momentum_step(input.momentum);
    let current = clamp_angular_velocity(momentum.angular_velocity, input.clamp_enabled);
    let angles = [
        orientation_angle_increment(
            current.x,
            momentum.delta_velocity.x,
            input.momentum.time_step,
        ),
        orientation_angle_increment(
            current.y,
            momentum.delta_velocity.y,
            input.momentum.time_step,
        ),
        orientation_angle_increment(
            current.z,
            momentum.delta_velocity.z,
            input.momentum.time_step,
        ),
    ];
    let reset = apply_control_axis_resets(current, angles, input.control);
    let basis = integrate_orientation(input.basis, reset.angle_increments, trig)?;
    Some(AngularOrientationOutcome {
        momentum,
        angular_velocity: reset.angular_velocity,
        angle_increments: reset.angle_increments,
        basis,
    })
}

/// Result of the four-record distance gate in `FUN_0040d740`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContactDistanceGateOutcome {
    pub contacts: [bool; 4],
    pub count_674: u8,
    pub scale_52c: Fixed16,
}

/// Apply the strict `0xcccc` distance gate and the `+0x52c` count scaling.
#[must_use]
pub fn evaluate_contact_distance_gate(
    deltas: [i32; 4],
    mass_48: Fixed16,
) -> ContactDistanceGateOutcome {
    let contacts = deltas.map(|delta| delta.wrapping_abs() < 0xcccc);
    let count_674 = contacts.iter().filter(|&&contact| contact).count() as u8;
    ContactDistanceGateOutcome {
        contacts,
        count_674,
        scale_52c: contact_count_scale(mass_48, count_674),
    }
}

/// Reproduce the shared count scaling: zero for 0, `>>2` for 4, `>>1` for 2,
/// identity for 1, and signed division otherwise.
#[must_use]
pub fn contact_count_scale(mass_48: Fixed16, count: u8) -> Fixed16 {
    match count {
        0 => Fixed16::ZERO,
        1 => mass_48,
        2 => Fixed16::from_raw(mass_48.raw() >> 1),
        4 => Fixed16::from_raw(mass_48.raw() >> 2),
        _ => Fixed16::from_raw(mass_48.raw() / i32::from(count)),
    }
}

/// Apply `FUN_00415d20`'s four timestep-scaled response words in source order.
#[must_use]
pub fn scale_contact_response_words(words: [Fixed16; 4], time_step: Fixed16) -> [Fixed16; 4] {
    words.map(|word| word.mul(time_step))
}

/// Matrix and motion inputs for the 24-dword layout in `FUN_00409f90`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContactBasisInput {
    pub rows: [Vec3; 3],
    /// `+0xf0`, `+0xf4`, `+0xf8`, `+0x2c`, `+0x30`, `+0x34`.
    pub angular: [Fixed16; 6],
    pub velocity: Vec3,
}

/// Build twelve primary dwords, mirror them with modular negation, assemble
/// eight vectors, and add velocity to every result.
#[must_use]
pub fn build_contact_basis_vectors(input: ContactBasisInput) -> [Vec3; 8] {
    let mut output = [0; 24];
    for (row_index, row) in input.rows.into_iter().enumerate() {
        let [f0, f4, f8, f2c, f30, f34] = input.angular;
        let a = row.x.mul(f4).mul(f34).raw();
        let b = row.x.mul(f8).mul(f30).raw();
        let c = row.y.mul(f8).mul(f2c).raw();
        let d = row.y.mul(f0).mul(f34).raw();
        let e = row.z.mul(f0).mul(f30).raw();
        let f = row.z.mul(f4).mul(f2c).raw();
        let primary = [
            c.wrapping_sub(f)
                .wrapping_sub(e)
                .wrapping_sub(d)
                .wrapping_add(b)
                .wrapping_add(a),
            d.wrapping_sub(f)
                .wrapping_sub(e)
                .wrapping_sub(a)
                .wrapping_add(c)
                .wrapping_add(b),
            f.wrapping_sub(e)
                .wrapping_sub(d)
                .wrapping_sub(c)
                .wrapping_add(b)
                .wrapping_add(a),
            f.wrapping_sub(e)
                .wrapping_sub(c)
                .wrapping_sub(a)
                .wrapping_add(d)
                .wrapping_add(b),
        ];
        let base = row_index * 4;
        output[base..base + 4].copy_from_slice(&primary);
    }
    for index in 0..12 {
        output[12 + index] = output[11 - index].wrapping_neg();
    }
    std::array::from_fn(|index| {
        let source = (index & 3) + if index < 4 { 0 } else { 12 };
        Vec3::new(
            Fixed16::from_raw(output[source]),
            Fixed16::from_raw(output[source + 4]),
            Fixed16::from_raw(output[source + 8]),
        )
        .add(input.velocity)
    })
}

/// Runtime inputs for one `FUN_0040d400` contact-vector update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContactVectorUpdateInput {
    pub update_gate: i32,
    pub enabled: bool,
    pub vector: Vec3,
    pub basis: Vec3,
    pub scale_528: Fixed16,
    pub extra_vector: Vec3,
    pub limit: Fixed16,
    pub threshold: Fixed16,
}

/// Project and scale one contact vector through the recovered `FUN_0040d400`
/// rule. `None` preserves the destination when the global gate or contact flag
/// rejects the update, or when the projected vector has zero length.
#[must_use]
pub fn apply_contact_vector_update(input: ContactVectorUpdateInput) -> Option<Vec3> {
    if input.update_gate != 0 || !input.enabled {
        return None;
    }
    let projection = input.basis.dot(input.vector);
    let residual = input.vector.sub(input.basis.scale(projection));
    let root = Fixed16::from_raw(sqrt_q16(residual.dot(residual).raw() as u32) as i32);
    if root.raw() <= 0 {
        return None;
    }

    let reciprocal = Fixed16::from_raw(((1_u64 << 32) / u64::from(root.raw() as u32)) as i32);
    let mut magnitude = root.mul(input.scale_528).neg();
    if root < Fixed16::ONE {
        let extra = Fixed16::from_raw(sqrt_q16(
            input.extra_vector.dot(input.extra_vector).raw() as u32
        ) as i32);
        magnitude = magnitude.sub(extra);
    }
    if magnitude.raw().wrapping_abs() > input.threshold.raw() {
        magnitude = if magnitude.raw() < 1 {
            input.limit.neg()
        } else {
            input.limit
        };
    }
    Some(residual.scale(reciprocal).scale(magnitude))
}

/// Apply all eight contact updates in reverse native record order, preserving
/// the previous destination for skipped or zero-residual records.
#[must_use]
pub fn apply_contact_vector_updates(
    inputs: [ContactVectorUpdateInput; 8],
    mut previous: [Vec3; 8],
) -> [Vec3; 8] {
    for (input, destination) in inputs.into_iter().zip(previous.iter_mut()).rev() {
        if let Some(updated) = apply_contact_vector_update(input) {
            *destination = updated;
        }
    }
    previous
}

/// Speed below which an active contact set suppresses the position increment.
pub const LOW_SPEED_POSITION_SUPPRESSION: Fixed16 = Fixed16::from_raw(0xccc);

/// Reproduce the active-contact low-speed position-delta suppression without
/// changing velocity or angular state.
#[must_use]
pub fn suppress_low_speed_position_delta(
    position_delta: Vec3,
    contacts_active: bool,
    speed: Fixed16,
) -> Vec3 {
    if contacts_active && speed < LOW_SPEED_POSITION_SUPPRESSION {
        Vec3::ZERO
    } else {
        position_delta
    }
}

/// Apply the recovered official-EU translational damping, velocity, and
/// position update from `FUN_0040a4a0`.
///
/// Every vector operation retains the observed per-operation Q16.16
/// truncation and 32-bit wrapping. In particular, the cached magnitude uses
/// wrapping absolute value (`i32::MIN` remains negative), and scaling a
/// negative odd velocity delta by [`POSITION_HALF_DELTA`] rounds toward
/// negative infinity rather than toward zero.
#[must_use]
pub fn apply_translation_step(input: TranslationStepInput) -> TranslationStepOutcome {
    let damping_scalar =
        Fixed16::from_raw(input.cached_speed.raw().wrapping_abs()).mul(Fixed16::ONE);
    let damped_force = input
        .accumulated_force
        .sub(input.velocity.scale(damping_scalar));
    let acceleration = damped_force
        .scale(input.inverse_mass)
        .add(input.additive_acceleration);
    let gain_delta = acceleration.scale(VELOCITY_GAIN);
    let delta_velocity = gain_delta.scale(Fixed16::ONE);
    let velocity = input.velocity.add(delta_velocity);
    let scaled_velocity = velocity.scale(Fixed16::ONE);
    let position_delta = scaled_velocity.sub(delta_velocity.scale(POSITION_HALF_DELTA));
    let position_delta = suppress_low_speed_position_delta(
        position_delta,
        input.contacts_active,
        input.cached_speed,
    );

    TranslationStepOutcome {
        position: input.position.add(position_delta),
        velocity,
        prior_velocity: input.velocity,
        acceleration,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(
            Fixed16::from_int(x),
            Fixed16::from_int(y),
            Fixed16::from_int(z),
        )
    }

    fn raw_v(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(
            Fixed16::from_raw(x),
            Fixed16::from_raw(y),
            Fixed16::from_raw(z),
        )
    }

    fn legacy_axis(x: i32, y: i32, z: i32) -> Vec3 {
        raw_v(65_522 * x, 65_522 * y, 65_522 * z)
    }

    fn lane_accumulation_input() -> LaneAccumulationInput {
        LaneAccumulationInput {
            accumulator: Vec3::ZERO,
            selected_scalar: Fixed16::ONE,
            direction_a: Vec3::ZERO,
            direction_b: Vec3::ZERO,
            surface_vector: Vec3::ZERO,
            correction_gate: 0,
            smoothed_value: Fixed16::ZERO,
            raw_value: Fixed16::ZERO,
            gear_ratio: Fixed16::ZERO,
            global_factor: Fixed16::ONE,
            correction_threshold: Fixed16::ZERO,
            correction_alternate: Fixed16::ZERO,
            second_budget: Fixed16::ZERO,
            surface_coefficient: Fixed16::ZERO,
            surface_override: 0,
        }
    }

    fn translation_step_input() -> TranslationStepInput {
        TranslationStepInput {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            cached_speed: Fixed16::ZERO,
            contacts_active: false,
            accumulated_force: Vec3::ZERO,
            inverse_mass: Fixed16::ONE,
            additive_acceleration: Vec3::ZERO,
        }
    }

    fn geometric_lane(
        active: bool,
        vector_a: Vec3,
        vector_b: Vec3,
        vector_c: Vec3,
        accumulator: Vec3,
    ) -> GeometricPostprocessLane {
        GeometricPostprocessLane {
            active,
            vector_a,
            vector_b,
            vector_c,
            accumulator,
        }
    }

    fn inactive_geometric_lane() -> GeometricPostprocessLane {
        geometric_lane(false, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO)
    }

    fn lane_direction_sources(
        projection_axis: Vec3,
        source_a0: Vec3,
        source_b0: Vec3,
        source_a2: Vec3,
        cross_operand: Vec3,
    ) -> LaneDirectionSources {
        LaneDirectionSources {
            projection_axis,
            source_a0,
            source_b0,
            source_a2,
            cross_operand,
        }
    }

    #[test]
    fn lane_direction_mode_resolution_gives_rebuild_priority() {
        assert_eq!(
            resolve_lane_direction_mode(false, false),
            LaneDirectionMode::Preserve
        );
        assert_eq!(
            resolve_lane_direction_mode(false, true),
            LaneDirectionMode::Exchange
        );
        assert_eq!(
            resolve_lane_direction_mode(true, false),
            LaneDirectionMode::Rebuild
        );
        assert_eq!(
            resolve_lane_direction_mode(true, true),
            LaneDirectionMode::Rebuild
        );
    }

    #[test]
    fn lane_direction_pair_copies_rotates_and_crosses_native_sources() {
        let source = raw_v(0x10000, 0, 0);
        let axis = raw_v(0, 0x10000, 0);
        assert_eq!(
            derive_lane_direction_pair(source, axis, 0, &[]),
            Some((source, raw_v(0, 0, 0x10000)))
        );
        let trig = crate::fixed_point::build_sine_table();
        let rotated = integrate_orientation([source; 3], [0, 0x400, 0], &trig).unwrap()[0];
        assert_eq!(
            derive_lane_direction_pair(source, axis, 0x400, &trig),
            Some((rotated, wrapping_cross(rotated, axis)))
        );
        assert_eq!(derive_lane_direction_pair(source, axis, 1, &[]), None);
    }

    #[test]
    fn lane_directions_preserve_initial_pairs_and_raw_cross_order() {
        let outcome = construct_lane_directions(
            lane_direction_sources(Vec3::ZERO, v(1, 0, 0), v(0, 1, 0), v(0, 0, 1), v(0, 1, 0)),
            LaneDirectionMode::Preserve,
        );

        assert_eq!(
            outcome.direction_a,
            [
                legacy_axis(1, 0, 0),
                legacy_axis(1, 0, 0),
                legacy_axis(0, 0, 1),
                legacy_axis(0, 0, 1),
            ]
        );
        // source_a2 x cross_operand is Z x Y = -X. Reversing the raw
        // operands would produce +X.
        assert_eq!(
            outcome.direction_b,
            [
                legacy_axis(0, 1, 0),
                legacy_axis(0, 1, 0),
                legacy_axis(-1, 0, 0),
                legacy_axis(-1, 0, 0),
            ]
        );
        assert_eq!(outcome.pre_mode_a_valid, [true; 4]);
    }

    #[test]
    fn lane_directions_crosses_raw_a2_before_axis_rejection() {
        let outcome = construct_lane_directions(
            lane_direction_sources(v(1, 0, 0), v(1, 0, 0), v(0, 0, 1), v(1, 1, 0), v(0, 0, 1)),
            LaneDirectionMode::Preserve,
        );

        // A2 rejects to +Y. Its raw source crosses Z to (1,-1,0), which then
        // rejects to -Y. Crossing the already-rejected A2 would instead be
        // removed completely by the X axis rejection.
        assert_eq!(
            outcome.direction_a,
            [
                Vec3::ZERO,
                Vec3::ZERO,
                legacy_axis(0, 1, 0),
                legacy_axis(0, 1, 0),
            ]
        );
        assert_eq!(
            outcome.direction_b,
            [
                legacy_axis(0, 0, 1),
                legacy_axis(0, 0, 1),
                legacy_axis(0, -1, 0),
                legacy_axis(0, -1, 0),
            ]
        );
        assert_eq!(outcome.pre_mode_a_valid, [false, false, true, true]);
    }

    #[test]
    fn lane_directions_exchange_pairs_but_not_pre_mode_validity() {
        let outcome = construct_lane_directions(
            lane_direction_sources(Vec3::ZERO, v(1, 0, 0), v(0, 1, 0), Vec3::ZERO, v(0, 0, 1)),
            LaneDirectionMode::Exchange,
        );

        assert_eq!(
            outcome.direction_a,
            [
                Vec3::ZERO,
                Vec3::ZERO,
                legacy_axis(1, 0, 0),
                legacy_axis(1, 0, 0),
            ]
        );
        assert_eq!(
            outcome.direction_b,
            [
                Vec3::ZERO,
                Vec3::ZERO,
                legacy_axis(0, 1, 0),
                legacy_axis(0, 1, 0),
            ]
        );
        assert_eq!(outcome.pre_mode_a_valid, [true, true, false, false]);
    }

    #[test]
    fn lane_directions_rebuild_discards_a2_without_recomputing_validity() {
        let outcome = construct_lane_directions(
            lane_direction_sources(v(1, 0, 0), v(0, 1, 0), v(1, 0, 0), v(1, 0, 0), v(0, 1, 0)),
            LaneDirectionMode::Rebuild,
        );

        // Pre-mode A2 is rejected to zero, while raw X x Y supplies B2=Z.
        // Rebuild replaces A2 with A0=Y and retains B2=Z, but the old false
        // flags remain in lanes 2/3.
        assert_eq!(outcome.direction_a, [legacy_axis(0, 1, 0); 4]);
        assert_eq!(
            outcome.direction_b,
            [
                Vec3::ZERO,
                Vec3::ZERO,
                raw_v(0, 0, 65_534),
                raw_v(0, 0, 65_534),
            ]
        );
        assert_eq!(outcome.pre_mode_a_valid, [true, true, false, false]);
    }

    #[test]
    fn lane_directions_rebuild_uses_doubled_dot_then_new_a2() {
        let outcome = construct_lane_directions(
            lane_direction_sources(Vec3::ZERO, v(1, 2, 0), v(0, 0, 1), v(0, 0, 1), v(1, -1, 0)),
            LaneDirectionMode::Rebuild,
        );

        assert_eq!(outcome.direction_a[0], raw_v(29_303, 58_606, 0));
        assert_eq!(outcome.direction_a[2], raw_v(-58_581, -29_278, 0));
        assert_eq!(outcome.direction_a[3], outcome.direction_a[2]);
        // This literal pins the second dot to the newly rebuilt A2 before B2
        // is normalized again.
        assert_eq!(outcome.direction_b[2], raw_v(-29_056, 58_748, 0));
        assert_eq!(outcome.direction_b[3], outcome.direction_b[2]);
        assert_eq!(outcome.pre_mode_a_valid, [true; 4]);
    }

    #[test]
    fn lane_directions_wrapped_zero_norm_and_rebuild_zero_b2() {
        let outcome = construct_lane_directions(
            lane_direction_sources(
                Vec3::ZERO,
                v(1, 0, 0),
                v(0, 1, 0),
                raw_v(0, 0, 0x0100_0000),
                v(0, 1, 0),
            ),
            LaneDirectionMode::Rebuild,
        );

        // qmul(0x01000000, 0x01000000) wraps to zero. Both the initial A2
        // and its raw-cross-derived B2 therefore normalize to zero, and the
        // A2 validity captured before rebuild is false.
        assert_eq!(outcome.direction_a[2], legacy_axis(1, 0, 0));
        assert_eq!(outcome.direction_b[2], Vec3::ZERO);
        assert_eq!(outcome.pre_mode_a_valid, [true, true, false, false]);
    }

    #[test]
    fn lane_direction_publication_snapshot_uses_unique_slot_order() {
        let outcome = construct_lane_directions(
            lane_direction_sources(Vec3::ZERO, v(1, 0, 0), v(0, 1, 0), v(0, 0, 1), v(0, 1, 0)),
            LaneDirectionMode::Preserve,
        );

        assert_eq!(
            outcome.published_unique_vectors(),
            [
                raw_v(65_522, 0, 0),
                raw_v(0, 0, 65_522),
                raw_v(0, 65_522, 0),
                raw_v(-65_522, 0, 0),
            ]
        );
    }

    #[test]
    fn lane_direction_primitives_pin_truncation_wrapping_and_cross_products() {
        assert_eq!(reject_from_axis(v(1, 0, 0), v(2, 0, 0)), v(-3, 0, 0));

        let rejected = reject_from_axis(
            raw_v(1_078, 42_317, -166_304),
            raw_v(83_647, -68_071, 17_186),
        );
        assert_eq!(rejected, raw_v(111_088, -47_208, -143_702));

        let wrapped = reject_from_axis(
            raw_v(i32::MAX, i32::MAX - 1, -0x7000_0000),
            raw_v(0x7000_0000, -0x6000_0000, 0x5000_0000),
        );
        assert_eq!(wrapped, raw_v(1_560_281_087, -1_644_167_170, 1_996_488_704));

        assert_eq!(
            wrapping_cross(
                raw_v(0x18001, -0x10003, 0x28005),
                raw_v(-0x1c007, 0x3000b, -0x1400d),
            ),
            raw_v(-409_626, -163_846, 180_231)
        );
    }

    #[test]
    fn lateral_grip_leaves_pure_forward_velocity_untouched() {
        let forward = v(1, 0, 0);
        // Velocity entirely along forward: perpendicular is zero, no change.
        assert_eq!(apply_lateral_grip(v(2, 0, 0), forward, false), v(2, 0, 0));
    }

    #[test]
    fn lateral_grip_bleeds_sideways_velocity() {
        let forward = v(1, 0, 0);
        // Sideways unit velocity loses K of itself (0.02 normal).
        let out = apply_lateral_grip(v(0, 1, 0), forward, false);
        assert_eq!(out.x, Fixed16::ZERO);
        assert_eq!(out.y, Fixed16::ONE.sub(Fixed16::ONE.mul(LATERAL_GRIP_K)));
        // Handbrake removes only half as much (0.01), so more sideways speed stays.
        let hb = apply_lateral_grip(v(0, 1, 0), forward, true);
        assert_eq!(
            hb.y,
            Fixed16::ONE.sub(Fixed16::ONE.mul(LATERAL_GRIP_K_HANDBRAKE))
        );
        assert!(hb.y > out.y);
    }

    #[test]
    fn slope_grip_ramps_and_saturates() {
        // Flat ground (normal straight up) saturates at the 0.9 ceiling.
        assert_eq!(slope_grip(Fixed16::ONE), SLOPE_GRIP_FLAT);
        // At and above the max normal-Y it is still the flat ceiling.
        assert_eq!(slope_grip(SLOPE_NORMAL_Y_MAX), SLOPE_GRIP_FLAT);
        // At and below the min (steeper than ~45 deg) grip is zero.
        assert_eq!(slope_grip(SLOPE_NORMAL_Y_MIN), Fixed16::ZERO);
        assert_eq!(slope_grip(Fixed16::from_raw(0x8000)), Fixed16::ZERO);
        // Inside the band: positive, below the ceiling, and monotonic.
        let mid = slope_grip(Fixed16::from_raw(0xe000)); // ~0.875
        assert!(mid.raw() > 0 && mid.raw() < SLOPE_GRIP_FLAT.raw());
        let higher = slope_grip(Fixed16::from_raw(0xf000)); // ~0.9375
        assert!(higher.raw() > mid.raw());
    }

    #[test]
    fn integrator_core_matches_the_recovered_gains() {
        // accel = force * invMass + gravity.
        let accel = net_accel(v(1, 0, 0), Fixed16::from_raw(0x8000), v(0, -1, 0));
        assert_eq!(
            accel,
            Vec3::new(Fixed16::from_raw(0x8000), -Fixed16::ONE, Fixed16::ZERO)
        );
        // v += accel * 0.2.
        let vel = integrate_velocity(Vec3::ZERO, v(1, 0, 0));
        assert_eq!(vel.x, VELOCITY_GAIN);
        // drag = v * speed.
        let d = drag(v(1, 0, 0), Fixed16::from_raw(0x8000));
        assert_eq!(d.x, Fixed16::from_raw(0x8000));
    }

    #[test]
    fn angular_orientation_pipeline_matches_each_independent_stage() {
        let momentum = AngularMomentumStepInput {
            prior_moment: raw_v(0x10000, 0x20000, 0x30000),
            contact_moment: raw_v(0x40000, 0x50000, 0x60000),
            moment_scale: Fixed16::from_raw(0x8000),
            inertia_scale: raw_v(0x10000, 0x8000, 0x20000),
            angular_velocity: raw_v(0x10000, 0x10000, 0x10000),
            time_step: Fixed16::ONE,
        };
        let control = ControlAxisFlags {
            mode_3ec: 1,
            axis_3f4: 1,
            axis_3f8: 0,
        };
        let basis = [
            raw_v(0x10000, 0, 0),
            raw_v(0, 0x10000, 0),
            raw_v(0, 0, 0x10000),
        ];
        let trig = crate::fixed_point::build_sine_table();
        let outcome = apply_angular_orientation_pipeline(
            AngularOrientationInput {
                momentum,
                clamp_enabled: false,
                control,
                basis,
            },
            &trig,
        )
        .unwrap();
        let expected_momentum = apply_angular_momentum_step(momentum);
        let current = clamp_angular_velocity(expected_momentum.angular_velocity, false);
        let angles = [
            orientation_angle_increment(
                current.x,
                expected_momentum.delta_velocity.x,
                Fixed16::ONE,
            ),
            orientation_angle_increment(
                current.y,
                expected_momentum.delta_velocity.y,
                Fixed16::ONE,
            ),
            orientation_angle_increment(
                current.z,
                expected_momentum.delta_velocity.z,
                Fixed16::ONE,
            ),
        ];
        let reset = apply_control_axis_resets(current, angles, control);
        assert_eq!(outcome.momentum, expected_momentum);
        assert_eq!(outcome.angular_velocity, reset.angular_velocity);
        assert_eq!(outcome.angle_increments, reset.angle_increments);
        assert_eq!(
            outcome.basis,
            integrate_orientation(basis, reset.angle_increments, &trig).unwrap()
        );
        assert_eq!(
            apply_angular_orientation_pipeline(
                AngularOrientationInput {
                    momentum,
                    clamp_enabled: false,
                    control,
                    basis,
                },
                &trig[..0x100],
            ),
            None
        );
    }

    #[test]
    fn control_axis_resets_cover_priority_and_every_gate() {
        let velocity = raw_v(1, 2, 3);
        let angles = [4, 5, 6];
        let reset = |mode_3ec, axis_3f4, axis_3f8| {
            apply_control_axis_resets(
                velocity,
                angles,
                ControlAxisFlags {
                    mode_3ec,
                    axis_3f4,
                    axis_3f8,
                },
            )
        };
        assert_eq!(reset(0, 1, 1).angular_velocity, velocity);
        assert_eq!(reset(0, 1, 1).angle_increments, angles);
        assert_eq!(reset(1, 0, 0).angular_velocity, velocity);
        assert_eq!(reset(1, 0, 0).angle_increments, angles);
        let first = reset(1, 1, 0);
        assert_eq!(first.angular_velocity, raw_v(0, 2, 0));
        assert_eq!(first.angle_increments, [0, 5, 0]);
        let second = reset(1, 0, 1);
        assert_eq!(second.angular_velocity, raw_v(0, 0, 3));
        assert_eq!(second.angle_increments, [0, 0, 6]);
        assert_eq!(reset(1, 1, 1), first);
    }

    #[test]
    fn angular_velocity_clamp_respects_boundaries_and_wrapping_abs() {
        let velocity = raw_v(0x3334, -0x3334, i32::MIN);
        assert_eq!(
            clamp_angular_velocity(velocity, true),
            raw_v(0x3333, -0x3333, i32::MIN)
        );
        let boundaries = raw_v(0x3333, -0x3333, 0);
        assert_eq!(clamp_angular_velocity(boundaries, true), boundaries);
        assert_eq!(clamp_angular_velocity(velocity, false), velocity);
    }

    #[test]
    fn angular_momentum_scales_combines_and_negatively_integrates() {
        let outcome = apply_angular_momentum_step(AngularMomentumStepInput {
            prior_moment: raw_v(0x10000, 0x20000, 0x30000),
            contact_moment: raw_v(0x40000, 0x50000, 0x60000),
            moment_scale: Fixed16::from_raw(0x8000),
            inertia_scale: raw_v(0x10000, 0x8000, 0x20000),
            angular_velocity: raw_v(0x10000, 0x10000, 0x10000),
            time_step: Fixed16::ONE,
        });
        assert_eq!(outcome.scaled_prior_moment, raw_v(0x8000, 0x10000, 0x18000));
        assert_eq!(outcome.acceleration, raw_v(0x48000, 0x30000, 0xf0000));
        assert_eq!(outcome.delta_velocity, raw_v(-0x48000, -0x30000, -0xf0000));
        assert_eq!(
            outcome.angular_velocity,
            raw_v(-0x38000, -0x20000, -0xe0000)
        );
    }

    #[test]
    fn angular_velocity_half_delta_feeds_orientation_angles() {
        assert_eq!(
            orientation_angle_increment(Fixed16::ONE, Fixed16::ZERO, Fixed16::ONE),
            0x28b
        );
        assert_eq!(
            orientation_angle_increment(Fixed16::ONE, Fixed16::from_raw(0x20000), Fixed16::ONE,),
            0
        );
        assert_eq!(
            orientation_angle_increment(Fixed16::ZERO, Fixed16::from_raw(0x10001), Fixed16::ONE,),
            -326
        );
        let basis = [
            raw_v(0x10000, 0, 0),
            raw_v(0, 0x10000, 0),
            raw_v(0, 0, 0x10000),
        ];
        let trig = crate::fixed_point::build_sine_table();
        assert_eq!(
            integrate_orientation_from_velocities(
                basis,
                raw_v(0x10000, 0, 0),
                Vec3::ZERO,
                Fixed16::ONE,
                &trig,
            ),
            integrate_orientation(basis, [0x28b, 0, 0], &trig)
        );
    }

    #[test]
    fn orientation_rotation_normalizes_and_preserves_zero_angle_stages() {
        let basis = [
            raw_v(0x10000, 0, 0),
            raw_v(0, 0x10000, 0),
            raw_v(0, 0, 0x10000),
        ];
        assert_eq!(integrate_orientation(basis, [0; 3], &[]), Some(basis));
        assert_eq!(rotate_basis_axis_angle(basis, basis[2], 1, &[]), None);
        let trig = crate::fixed_point::build_sine_table();
        assert_eq!(
            rotate_basis_axis_angle(basis, basis[2], 0x400, &trig),
            Some([
                raw_v(0, -0xfff2, 0),
                raw_v(0xfff2, 0, 0),
                raw_v(0, 0, 0xfff2),
            ])
        );
        assert_eq!(
            integrate_orientation(basis, [0x400, 0, 0], &trig),
            Some([
                raw_v(0xfff2, 0, 0),
                raw_v(0, 0, -0xfff2),
                raw_v(0, 0xfff2, 0),
            ])
        );
    }

    #[test]
    fn contact_force_transform_uses_matrix_rows_before_the_cross_product() {
        let identity = [
            raw_v(0x10000, 0, 0),
            raw_v(0, 0x10000, 0),
            raw_v(0, 0, 0x10000),
        ];
        let force = raw_v(0x10000, 0x20000, 0x30000);
        assert_eq!(transform_contact_force(identity, force), force);
        assert_eq!(
            transform_contact_force(
                [
                    raw_v(0, 0x10000, 0),
                    raw_v(0, 0, 0x10000),
                    raw_v(0x10000, 0, 0),
                ],
                force,
            ),
            raw_v(0x20000, 0x30000, 0x10000)
        );
        assert_eq!(
            transformed_contact_moment(identity, raw_v(0x10000, 0, 0), raw_v(0, 0x10000, 0),),
            raw_v(0, 0, 0x10000)
        );
    }

    #[test]
    fn contact_basis_vectors_preserve_primary_mirror_and_velocity_layout() {
        let identity = [
            raw_v(0x10000, 0, 0),
            raw_v(0, 0x10000, 0),
            raw_v(0, 0, 0x10000),
        ];
        let vectors = build_contact_basis_vectors(ContactBasisInput {
            rows: identity,
            angular: [Fixed16::ONE; 6],
            velocity: Vec3::ZERO,
        });
        assert_eq!(vectors[0], raw_v(0x20000, 0, -0x20000));
        assert_eq!(vectors[1], raw_v(0, 0x20000, -0x20000));
        assert_eq!(vectors[2], raw_v(0x20000, -0x20000, 0));
        assert_eq!(vectors[3], Vec3::ZERO);
        assert_eq!(vectors[4], Vec3::ZERO);
        assert_eq!(vectors[5], raw_v(0, 0x20000, -0x20000));
        assert_eq!(vectors[6], raw_v(0x20000, -0x20000, 0));
        assert_eq!(vectors[7], raw_v(0x20000, 0, -0x20000));

        let velocity = raw_v(1, 2, 3);
        assert_eq!(
            build_contact_basis_vectors(ContactBasisInput {
                rows: identity,
                angular: [Fixed16::ZERO; 6],
                velocity,
            }),
            [velocity; 8]
        );
    }

    #[test]
    fn contact_moments_cross_transformed_forces_and_filter_wheels() {
        let moment = sum_contact_moments(
            &[
                (raw_v(0x10000, 0, 0), raw_v(0, 0x10000, 0)),
                (raw_v(0, 0x10000, 0), raw_v(0x10000, 0, 0)),
            ],
            [
                (true, raw_v(0x10000, 0, 0), raw_v(0, 0x10000, 0)),
                (false, raw_v(i32::MAX, 0, 0), raw_v(0, 0x20000, 0)),
                (false, raw_v(1, 2, 3), raw_v(4, 5, 6)),
                (false, raw_v(7, 8, 9), raw_v(10, 11, 12)),
            ],
        );
        assert_eq!(moment, raw_v(0, 0, 0x10000));
        assert_eq!(
            sum_contact_moments(
                &[(raw_v(i32::MAX, 0, 0), raw_v(0, 0x20000, 0))],
                [(false, Vec3::ZERO, Vec3::ZERO); 4],
            ),
            raw_v(0, 0, -2)
        );
    }

    #[test]
    fn contact_distance_gate_preserves_exact_threshold_and_count_scaling() {
        let outcome = evaluate_contact_distance_gate(
            [0, 0xcccc, -0xcccc, 0xcccc + 1],
            Fixed16::from_raw(0x30000),
        );
        assert_eq!(outcome.contacts, [true, false, false, false]);
        assert_eq!(outcome.count_674, 1);
        assert_eq!(outcome.scale_52c, Fixed16::from_raw(0x30000));
        assert_eq!(
            evaluate_contact_distance_gate([1; 4], Fixed16::from_raw(0x30000)).scale_52c,
            Fixed16::from_raw(0xc000)
        );
        assert_eq!(
            evaluate_contact_distance_gate(
                [0xcccc + 1, 0xcccc + 1, 0, 0],
                Fixed16::from_raw(0x30000),
            )
            .scale_52c,
            Fixed16::from_raw(0x18000)
        );
        assert_eq!(
            evaluate_contact_distance_gate([0xcccc + 1; 4], Fixed16::from_raw(0x30000)).scale_52c,
            Fixed16::ZERO
        );
    }

    #[test]
    fn contact_count_scale_handles_every_special_case() {
        let mass = Fixed16::from_raw(0x30000);
        assert_eq!(contact_count_scale(mass, 0), Fixed16::ZERO);
        assert_eq!(contact_count_scale(mass, 1), mass);
        assert_eq!(contact_count_scale(mass, 2), Fixed16::from_raw(0x18000));
        assert_eq!(contact_count_scale(mass, 3), Fixed16::from_raw(0x10000));
        assert_eq!(contact_count_scale(mass, 4), Fixed16::from_raw(0xc000));
    }

    #[test]
    fn contact_response_words_scale_in_source_order() {
        assert_eq!(
            scale_contact_response_words(
                [
                    Fixed16::from_raw(0x10000),
                    Fixed16::from_raw(0x20000),
                    Fixed16::from_raw(0x30000),
                    Fixed16::from_raw(0x40000),
                ],
                Fixed16::from_raw(0x8000),
            ),
            [
                Fixed16::from_raw(0x8000),
                Fixed16::from_raw(0x10000),
                Fixed16::from_raw(0x18000),
                Fixed16::from_raw(0x20000),
            ]
        );
    }

    #[test]
    fn eight_contact_updates_replace_only_produced_destinations() {
        let input = ContactVectorUpdateInput {
            update_gate: 0,
            enabled: false,
            vector: raw_v(0, 0x10000, 0),
            basis: raw_v(0x10000, 0, 0),
            scale_528: Fixed16::ONE,
            extra_vector: Vec3::ZERO,
            limit: Fixed16::from_raw(0x20000),
            threshold: Fixed16::from_raw(0x30000),
        };
        let previous = [raw_v(7, 8, 9); 8];
        let mut inputs = [input; 8];
        inputs[3].enabled = true;
        let mut expected = previous;
        expected[3] = raw_v(0, -0x10000, 0);
        assert_eq!(apply_contact_vector_updates(inputs, previous), expected);
        assert_eq!(
            apply_contact_vector_updates(
                inputs.map(|input| ContactVectorUpdateInput {
                    update_gate: 1,
                    ..input
                }),
                previous,
            ),
            previous
        );
    }

    #[test]
    fn contact_vector_update_projects_scales_and_preserves_skips() {
        let input = ContactVectorUpdateInput {
            update_gate: 0,
            enabled: true,
            vector: raw_v(0x20000, 0x10000, 0),
            basis: raw_v(0x10000, 0, 0),
            scale_528: Fixed16::ONE,
            extra_vector: Vec3::ZERO,
            limit: Fixed16::from_raw(0x20000),
            threshold: Fixed16::from_raw(0x30000),
        };
        assert_eq!(
            apply_contact_vector_update(input),
            Some(raw_v(0, -0x10000, 0))
        );
        assert_eq!(
            apply_contact_vector_update(ContactVectorUpdateInput {
                threshold: Fixed16::ZERO,
                ..input
            }),
            Some(raw_v(0, -0x1ffe4, 0))
        );
        assert_eq!(
            apply_contact_vector_update(ContactVectorUpdateInput {
                vector: raw_v(0, 0x8000, 0),
                scale_528: Fixed16::ZERO,
                extra_vector: raw_v(0x10000, 0, 0),
                ..input
            }),
            Some(raw_v(0, -0x10000, 0))
        );
        assert_eq!(
            apply_contact_vector_update(ContactVectorUpdateInput {
                enabled: false,
                ..input
            }),
            None
        );
        assert_eq!(
            apply_contact_vector_update(ContactVectorUpdateInput {
                update_gate: 1,
                ..input
            }),
            None
        );
        assert_eq!(
            apply_contact_vector_update(ContactVectorUpdateInput {
                vector: raw_v(0x10000, 0, 0),
                ..input
            }),
            None
        );
    }

    #[test]
    fn translation_force_sum_filters_wheel_contacts_and_wraps() {
        let summed = sum_translation_lane_forces(
            raw_v(i32::MAX, 1, -1),
            &[raw_v(1, 2, 3), raw_v(4, 5, 6)],
            [
                (true, raw_v(10, 20, 30)),
                (false, raw_v(100, 200, 300)),
                (true, raw_v(-10, -20, -30)),
                (false, raw_v(1000, 2000, 3000)),
            ],
        );
        assert_eq!(summed, raw_v(i32::MIN + 4, 8, 8));
        assert_eq!(
            sum_translation_lane_forces(Vec3::ZERO, &[], [(false, raw_v(1, 1, 1)); 4]),
            Vec3::ZERO
        );
    }

    #[test]
    fn low_speed_suppression_only_zeroes_active_contact_position_delta() {
        let delta = raw_v(1, 2, 3);
        assert_eq!(
            suppress_low_speed_position_delta(delta, true, Fixed16::from_raw(0xccb)),
            Vec3::ZERO
        );
        assert_eq!(
            suppress_low_speed_position_delta(delta, true, LOW_SPEED_POSITION_SUPPRESSION),
            delta
        );
        assert_eq!(
            suppress_low_speed_position_delta(delta, false, Fixed16::ZERO),
            delta
        );
    }

    #[test]
    fn translation_step_suppresses_only_position_for_low_speed_contacts() {
        let unsuppressed = apply_translation_step(TranslationStepInput {
            accumulated_force: raw_v(0x10000, 0, 0),
            ..translation_step_input()
        });
        let suppressed = apply_translation_step(TranslationStepInput {
            accumulated_force: raw_v(0x10000, 0, 0),
            contacts_active: true,
            ..translation_step_input()
        });
        assert_eq!(suppressed.velocity, unsuppressed.velocity);
        assert_ne!(suppressed.velocity, Vec3::ZERO);
        assert_eq!(unsuppressed.position, raw_v(0x199a, 0, 0));
        assert_eq!(suppressed.position, Vec3::ZERO);
    }

    #[test]
    fn translation_step_uses_cached_speed_before_moving() {
        let outcome = apply_translation_step(TranslationStepInput {
            velocity: raw_v(65_536, -131_072, 32_768),
            cached_speed: Fixed16::from_raw(32_768),
            ..translation_step_input()
        });

        assert_eq!(outcome.acceleration, raw_v(-32_768, 65_536, -16_384));
        assert_eq!(outcome.velocity, raw_v(58_982, -117_965, 29_491));
        assert_eq!(outcome.position, raw_v(62_259, -124_518, 31_130));
    }

    #[test]
    fn translation_step_does_not_recompute_the_supplied_cached_speed() {
        let base = TranslationStepInput {
            velocity: raw_v(65_536, 0, 0),
            ..translation_step_input()
        };
        let without_damping = apply_translation_step(base);
        let with_damping = apply_translation_step(TranslationStepInput {
            cached_speed: Fixed16::ONE,
            ..base
        });

        assert_eq!(without_damping.position, raw_v(65_536, 0, 0));
        assert_eq!(with_damping.acceleration, raw_v(-65_536, 0, 0));
        assert_eq!(with_damping.position, raw_v(58_983, 0, 0));
    }

    #[test]
    fn translation_step_preserves_wrapping_abs_for_min_cached_speed() {
        let outcome = apply_translation_step(TranslationStepInput {
            velocity: raw_v(65_536, -65_536, 32_768),
            cached_speed: Fixed16::from_raw(i32::MIN),
            ..translation_step_input()
        });

        assert_eq!(
            outcome.acceleration,
            raw_v(i32::MIN, i32::MIN, 1_073_741_824)
        );
        assert_eq!(
            outcome.velocity,
            raw_v(-429_424_640, -429_555_712, 214_777_856)
        );
        assert_eq!(
            outcome.position,
            raw_v(-214_679_552, -214_810_624, 107_405_312)
        );
    }

    #[test]
    fn translation_step_wraps_each_arithmetic_stage() {
        let outcome = apply_translation_step(TranslationStepInput {
            position: raw_v(i32::MAX - 7, i32::MIN + 8, 123),
            velocity: raw_v(i32::MAX - 7, i32::MIN + 8, 1_879_048_192),
            cached_speed: Fixed16::ONE,
            contacts_active: false,
            accumulated_force: raw_v(i32::MIN + 8, i32::MAX - 7, -1_879_048_192),
            inverse_mass: Fixed16::from_raw(i32::MIN),
            additive_acceleration: raw_v(i32::MAX - 7, i32::MIN + 8, i32::MAX),
        });

        assert_eq!(
            outcome.acceleration,
            raw_v(2_146_959_352, -2_146_959_352, i32::MAX)
        );
        assert_eq!(
            outcome.velocity,
            raw_v(-1_718_098_338, 1_718_098_337, -1_986_428_929)
        );
        assert_eq!(
            outcome.position,
            raw_v(214_692_643, -214_692_643, 2_093_793_403)
        );
    }

    #[test]
    fn translation_step_truncates_each_nonunit_inverse_mass_product() {
        let outcome = apply_translation_step(TranslationStepInput {
            accumulated_force: raw_v(65_537, -65_537, 131_075),
            inverse_mass: Fixed16::from_raw(21_845),
            ..translation_step_input()
        });

        assert_eq!(outcome.acceleration, raw_v(21_845, -21_846, 43_690));
        assert_eq!(outcome.velocity, raw_v(4_368, -4_370, 8_737));
        assert_eq!(outcome.position, raw_v(2_184, -2_185, 4_369));
    }

    #[test]
    fn translation_step_half_delta_uses_arithmetic_shift_for_negative_odds() {
        let positive = apply_translation_step(TranslationStepInput {
            additive_acceleration: raw_v(6, 0, 0),
            ..translation_step_input()
        });
        let negative = apply_translation_step(TranslationStepInput {
            additive_acceleration: raw_v(-1, 0, 0),
            ..translation_step_input()
        });

        assert_eq!(positive.velocity.x.raw(), 1);
        assert_eq!(positive.position.x.raw(), 1);
        assert_eq!(negative.velocity.x.raw(), -1);
        assert_eq!(negative.position.x.raw(), 0);
    }

    #[test]
    fn translation_step_position_uses_new_velocity_minus_half_delta() {
        let outcome = apply_translation_step(TranslationStepInput {
            position: raw_v(1_000, -2_000, 3_000),
            velocity: raw_v(4_000, -5_000, 6_000),
            accumulated_force: raw_v(7_000, -8_000, 9_000),
            additive_acceleration: raw_v(100, -200, 300),
            ..translation_step_input()
        });

        assert_eq!(outcome.acceleration, raw_v(7_100, -8_200, 9_300));
        assert_eq!(outcome.velocity, raw_v(5_419, -6_640, 7_859));
        assert_eq!(outcome.position, raw_v(5_710, -7_820, 9_930));
    }

    #[test]
    fn translation_step_snapshots_all_prior_velocity_axes_in_order() {
        let old_velocity = raw_v(11, -22, 33);
        let outcome = apply_translation_step(TranslationStepInput {
            position: raw_v(100, 200, 300),
            velocity: old_velocity,
            ..translation_step_input()
        });

        assert_eq!(outcome.prior_velocity, old_velocity);
        assert_eq!(outcome.velocity, old_velocity);
        assert_eq!(outcome.position, raw_v(111, 178, 333));
    }

    #[test]
    fn translation_step_wraps_final_position_addition() {
        let outcome = apply_translation_step(TranslationStepInput {
            position: raw_v(i32::MAX, i32::MIN, 0),
            velocity: raw_v(1, -1, 0),
            ..translation_step_input()
        });

        assert_eq!(outcome.position, raw_v(i32::MIN, i32::MAX, 0));
    }

    #[test]
    fn surface_grip_tiers_match_recovery() {
        assert_eq!(SURFACE_GRIP_HIGH.raw(), 0x3d70);
        assert_eq!(SURFACE_GRIP_LOW.raw(), 0x2666);
        assert!(SURFACE_GRIP_HIGH > SURFACE_GRIP_LOW);
        assert_eq!(SURFACE_GRIP_RECORD_BYTES, 8);
    }

    #[test]
    fn geometric_angle_pins_arc_indices_strict_threshold_and_adjacency() {
        let index_4033 = geometric_lane_angle_measure(Fixed16::from_raw(4033 << 4)).unwrap();
        let index_4034 = geometric_lane_angle_measure(Fixed16::from_raw(4034 << 4)).unwrap();
        assert_eq!(index_4033, Fixed16::from_raw(0x0a0500));
        assert_eq!(index_4034, Fixed16::from_raw(0x09ee80));
        assert!(index_4033.raw() > POSTPROCESS_ANGLE_THRESHOLD.raw());
        assert!(index_4034.raw() <= POSTPROCESS_ANGLE_THRESHOLD.raw());
        assert_eq!(
            geometric_lane_angle_measure(Fixed16::ONE.neg()),
            Ok(Fixed16::from_raw(0x5a0000))
        );
        assert_eq!(
            geometric_lane_angle_measure(Fixed16::ONE),
            Ok(Fixed16::ZERO)
        );
        assert_eq!(
            geometric_lane_angle_measure(Fixed16::from_raw(i32::MIN)),
            Err(SignedArcsinePhaseError::MinimumInput)
        );
    }

    #[test]
    fn geometric_postprocess_qualifies_and_updates_one_lane() {
        let qualified = geometric_lane(true, v(-1, 0, 0), v(0, 1, 0), v(1, 0, 0), Vec3::ZERO);
        let mut lanes = [inactive_geometric_lane(); 4];
        lanes[0] = qualified;

        let outcome = apply_geometric_lane_postprocess(&mut lanes, Fixed16::ONE).unwrap();

        assert_eq!(outcome.qualified, [true, false, false, false]);
        assert_eq!(outcome.angle_measures[0], Fixed16::from_raw(0x5a0000));
        assert_eq!(
            lanes[0].accumulator,
            Vec3::new(Fixed16::from_raw(-1_310_160), Fixed16::ZERO, Fixed16::ZERO,)
        );
    }

    #[test]
    fn geometric_postprocess_uses_strict_y_and_residual_dot_gates() {
        let equal_y = geometric_lane(true, v(-1, 0, 0), v(0, 0, 1), v(1, 0, 0), v(3, 0, 0));
        let zero_residual_dot =
            geometric_lane(true, v(-1, 0, 0), v(0, 1, 0), v(0, 0, 1), v(4, 0, 0));
        let positive_residual_dot =
            geometric_lane(true, v(-1, 0, 0), v(0, 1, 0), v(-1, 0, 0), v(5, 0, 0));
        let mut lanes = [inactive_geometric_lane(); 4];
        lanes[0] = equal_y;
        lanes[1] = zero_residual_dot;
        lanes[2] = positive_residual_dot;

        let outcome = apply_geometric_lane_postprocess(&mut lanes, Fixed16::ONE).unwrap();

        assert_eq!(outcome.qualified, [false; 4]);
        assert_eq!(lanes[0].accumulator, v(3, 0, 0));
        assert_eq!(lanes[1].accumulator, v(4, 0, 0));
        assert_eq!(lanes[2].accumulator, v(5, 0, 0));
    }

    #[test]
    fn geometric_postprocess_propagates_each_one_to_four_lane_share() {
        let expected_raw = [-1_310_160, -655_080, -436_720, -327_540];
        for qualified_count in 1..=4 {
            let mut lanes = [inactive_geometric_lane(); 4];
            for lane in &mut lanes[..qualified_count] {
                *lane = geometric_lane(true, v(-1, 0, 0), v(0, 1, 0), v(1, 0, 0), Vec3::ZERO);
            }

            let outcome = apply_geometric_lane_postprocess(&mut lanes, Fixed16::ONE).unwrap();

            assert_eq!(
                outcome.qualified.iter().filter(|&&value| value).count(),
                qualified_count
            );
            for lane in &lanes[..qualified_count] {
                assert_eq!(lane.accumulator.x.raw(), expected_raw[qualified_count - 1]);
            }
        }
    }

    #[test]
    fn geometric_postprocess_shares_gain_and_preserves_alignment_steps() {
        let half_aligned = geometric_lane(
            true,
            v(-1, 0, 0),
            v(0, 1, 0),
            Vec3::new(Fixed16::from_raw(0x8000), Fixed16::ZERO, Fixed16::ZERO),
            Vec3::ZERO,
        );
        let over_aligned = geometric_lane(true, v(-1, 0, 0), v(0, 1, 0), v(2, 0, 0), Vec3::ZERO);
        let mut lanes = [inactive_geometric_lane(); 4];
        lanes[0] = half_aligned;
        lanes[1] = over_aligned;

        let outcome = apply_geometric_lane_postprocess(&mut lanes, Fixed16::ONE).unwrap();

        assert_eq!(outcome.qualified, [true, true, false, false]);
        assert_eq!(
            lanes[0].accumulator,
            Vec3::new(Fixed16::from_raw(-327_540), Fixed16::ZERO, Fixed16::ZERO,)
        );
        assert_eq!(
            lanes[1].accumulator,
            Vec3::new(Fixed16::from_raw(-655_220), Fixed16::ZERO, Fixed16::ZERO,)
        );
    }

    #[test]
    fn geometric_normalization_zero_root_returns_zero_direction() {
        assert_eq!(normalize_geometric_residual(Vec3::ZERO), Vec3::ZERO);
    }

    #[test]
    fn geometric_normalization_pins_lookup_reciprocal_and_axis_truncation() {
        let residual = Vec3::new(
            Fixed16::from_raw(0x18000),
            Fixed16::from_raw(-0xc000),
            Fixed16::from_raw(0x4000),
        );

        assert_eq!(residual.dot(residual).raw() as u32, 0x2e000);
        assert_eq!(sqrt_q16(0x2e000), 111_130);
        assert_eq!((1_u64 << 32) / 111_130, 38_648);
        assert_eq!(
            normalize_geometric_residual(residual),
            Vec3::new(
                Fixed16::from_raw(57_972),
                Fixed16::from_raw(-28_986),
                Fixed16::from_raw(9_662),
            )
        );
    }

    #[test]
    fn geometric_alignment_clamp_keeps_signed_wrapping_rules() {
        assert_eq!(bound_geometric_alignment(Fixed16::ONE), Fixed16::ONE);
        assert_eq!(
            bound_geometric_alignment(Fixed16::ONE.neg()),
            Fixed16::ONE.neg()
        );
        assert_eq!(
            bound_geometric_alignment(Fixed16::from_raw(0x10001)),
            Fixed16::ONE
        );
        assert_eq!(
            bound_geometric_alignment(Fixed16::from_raw(-0x10001)),
            Fixed16::ONE.neg()
        );
        assert_eq!(
            bound_geometric_alignment(Fixed16::from_raw(i32::MIN)),
            Fixed16::from_raw(i32::MIN)
        );
    }

    #[test]
    fn geometric_postprocess_wraps_accumulator_addition_on_all_axes() {
        let x_lane = geometric_lane(
            true,
            v(-1, 0, 0),
            v(0, 1, 0),
            v(1, 0, 0),
            Vec3::new(Fixed16::from_raw(i32::MIN), Fixed16::ZERO, Fixed16::ZERO),
        );
        let y_lane = geometric_lane(
            true,
            v(0, -1, 0),
            v(1, 0, 0),
            v(0, 1, 0),
            Vec3::new(Fixed16::ZERO, Fixed16::from_raw(i32::MIN), Fixed16::ZERO),
        );
        let z_lane = geometric_lane(
            true,
            v(0, 0, -1),
            v(0, 1, 0),
            v(0, 0, 1),
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::from_raw(i32::MIN)),
        );
        let mut lanes = [inactive_geometric_lane(); 4];
        lanes[0] = x_lane;
        lanes[1] = y_lane;
        lanes[2] = z_lane;

        apply_geometric_lane_postprocess(&mut lanes, Fixed16::ONE).unwrap();

        let expected = i32::MIN.wrapping_add(-436_720);
        assert_eq!(lanes[0].accumulator.x.raw(), expected);
        assert_eq!(lanes[1].accumulator.y.raw(), expected);
        assert_eq!(lanes[2].accumulator.z.raw(), expected);
    }

    #[test]
    fn geometric_postprocess_inactive_minimum_dot_is_not_evaluated() {
        let mut lanes = [inactive_geometric_lane(); 4];
        lanes[0] = geometric_lane(
            false,
            Vec3::new(Fixed16::from_raw(i32::MIN), Fixed16::ZERO, Fixed16::ZERO),
            v(1, 0, 0),
            Vec3::ZERO,
            v(6, 0, 0),
        );

        let outcome = apply_geometric_lane_postprocess(&mut lanes, Fixed16::ONE).unwrap();

        assert_eq!(outcome.qualified, [false; 4]);
        assert_eq!(lanes[0].accumulator, v(6, 0, 0));
    }

    #[test]
    fn geometric_postprocess_lookup_error_is_atomic_and_lane_indexed() {
        let valid = geometric_lane(true, v(-1, 0, 0), v(0, 1, 0), v(1, 0, 0), v(7, 0, 0));
        let invalid = geometric_lane(
            true,
            Vec3::new(Fixed16::from_raw(i32::MIN), Fixed16::ZERO, Fixed16::ZERO),
            v(1, 0, 0),
            Vec3::ZERO,
            v(9, 0, 0),
        );
        let mut lanes = [inactive_geometric_lane(); 4];
        lanes[0] = valid;
        lanes[1] = invalid;

        assert_eq!(
            apply_geometric_lane_postprocess(&mut lanes, Fixed16::ONE),
            Err(GeometricPostprocessError::Angle {
                lane: 1,
                source: SignedArcsinePhaseError::MinimumInput,
            })
        );
        assert_eq!(lanes[0].accumulator, v(7, 0, 0));
        assert_eq!(lanes[1].accumulator, v(9, 0, 0));
    }

    #[test]
    fn postprocess_lane_shares_match_signed_q16_division() {
        let expected = [0x140000, 0xa0000, 0x6aaaa, 0x50000];
        for (count, expected_raw) in (1_u8..=4).zip(expected) {
            assert_eq!(
                postprocess_qualified_lane_share(count),
                Some(Fixed16::from_raw(expected_raw))
            );
        }
    }

    #[test]
    fn postprocess_lane_share_rejects_counts_outside_the_four_lane_loop() {
        assert_eq!(postprocess_qualified_lane_share(0), None);
        assert_eq!(postprocess_qualified_lane_share(5), None);
        assert_eq!(postprocess_qualified_lane_share(u8::MAX), None);
    }

    #[test]
    fn postprocess_gain_uses_a_strict_angle_gate() {
        assert_eq!(
            postprocess_lane_correction_gain(POSTPROCESS_ANGLE_THRESHOLD, Fixed16::ONE, 1,),
            None
        );
        assert_eq!(
            postprocess_lane_correction_gain(
                POSTPROCESS_ANGLE_THRESHOLD.add(Fixed16::from_raw(1)),
                Fixed16::ONE,
                1,
            ),
            Some(Fixed16::ZERO)
        );
        assert_eq!(
            postprocess_lane_correction_gain(
                POSTPROCESS_ANGLE_THRESHOLD.add(Fixed16::from_raw(1)),
                Fixed16::ONE,
                5,
            ),
            None
        );
    }

    #[test]
    fn postprocess_ramp_has_no_signed_lower_clamp() {
        assert_eq!(
            postprocess_correction_ramp(POSTPROCESS_ANGLE_THRESHOLD.sub(Fixed16::from_raw(1))),
            Fixed16::from_raw(-1)
        );
    }

    #[test]
    fn postprocess_ramp_pins_the_exact_upper_boundary_and_clamp() {
        assert_eq!(
            postprocess_correction_ramp(
                POSTPROCESS_ANGLE_THRESHOLD.add(Fixed16::from_raw(0x320708))
            ),
            Fixed16::from_raw(0xffff)
        );
        assert_eq!(
            postprocess_correction_ramp(
                POSTPROCESS_ANGLE_THRESHOLD.add(Fixed16::from_raw(0x320709))
            ),
            Fixed16::ONE
        );
        assert_eq!(
            postprocess_correction_ramp(
                POSTPROCESS_ANGLE_THRESHOLD.add(Fixed16::from_raw(0x32073c))
            ),
            Fixed16::ONE
        );
    }

    #[test]
    fn postprocess_gain_distributes_the_argument_across_qualified_lanes() {
        let angle = POSTPROCESS_ANGLE_THRESHOLD.add(Fixed16::ONE);
        let expected = [-0x6658, -0x332c, -0x221d, -0x1996];
        for (count, expected_raw) in (1_u8..=4).zip(expected) {
            assert_eq!(
                postprocess_lane_correction_gain(angle, Fixed16::ONE, count),
                Some(Fixed16::from_raw(expected_raw))
            );
        }
        assert_eq!(
            postprocess_lane_correction_gain(angle, Fixed16::from_raw(-0x10000), 1),
            Some(Fixed16::from_raw(0x6658))
        );
    }

    #[test]
    fn postprocess_ramp_and_gain_preserve_wrapping_extremes() {
        assert_eq!(
            postprocess_correction_ramp(Fixed16::from_raw(i32::MAX)),
            Fixed16::ONE
        );
        assert_eq!(
            postprocess_correction_ramp(Fixed16::from_raw(i32::MIN)),
            Fixed16::ONE
        );
        assert_eq!(
            postprocess_lane_correction_gain(
                POSTPROCESS_ANGLE_THRESHOLD.add(Fixed16::ONE),
                Fixed16::from_raw(i32::MIN),
                4,
            ),
            Some(Fixed16::from_raw(0x028f0000))
        );
    }

    #[test]
    fn surface_indexed_scalar_skips_only_the_recovered_zero_gates() {
        assert_eq!(
            surface_indexed_lane_scalar(Fixed16::ONE, Fixed16::ZERO, 0, Fixed16::from_int(4),),
            None
        );
        assert_eq!(
            surface_indexed_lane_scalar(Fixed16::ONE, Fixed16::ONE, 0, Fixed16::ZERO,),
            None
        );
        assert_eq!(
            surface_indexed_lane_scalar(Fixed16::ONE, Fixed16::ZERO, -1, Fixed16::from_int(4),),
            Some(Fixed16::from_raw(-0x8000))
        );
    }

    #[test]
    fn first_family_adapter_rejects_both_out_of_range_boundaries() {
        let first = [0; SURFACE_COEFFICIENT_COUNT];

        for index in [SURFACE_COEFFICIENT_COUNT, usize::from(u16::MAX)] {
            assert_eq!(
                surface_indexed_lane_scalar_from_first_family(
                    &first,
                    index,
                    Fixed16::ONE,
                    0,
                    Fixed16::ONE,
                ),
                Err(SurfaceCoefficientIndexError { index })
            );
        }
    }

    #[test]
    fn first_family_adapter_preserves_negative_and_int_min_arithmetic() {
        let mut first = [0; SURFACE_COEFFICIENT_COUNT];
        first[0] = -1;
        first[119] = i32::MIN;

        assert_eq!(
            surface_indexed_lane_scalar_from_first_family(
                &first,
                0,
                Fixed16::ONE,
                0,
                Fixed16::from_int(4),
            ),
            Ok(Some(Fixed16::from_raw(1)))
        );
        assert_eq!(
            surface_indexed_lane_scalar_from_first_family(
                &first,
                119,
                Fixed16::ONE,
                0,
                Fixed16::from_int(4),
            ),
            Ok(Some(Fixed16::from_raw(i32::MIN)))
        );
    }

    #[test]
    fn first_family_adapter_validates_index_before_nonzero_override() {
        let first = [0; SURFACE_COEFFICIENT_COUNT];

        assert_eq!(
            surface_indexed_lane_scalar_from_first_family(
                &first,
                120,
                Fixed16::ONE,
                1,
                Fixed16::ONE,
            ),
            Err(SurfaceCoefficientIndexError { index: 120 })
        );
        assert_eq!(
            surface_indexed_lane_scalar_from_first_family(
                &first,
                0,
                Fixed16::ONE,
                1,
                Fixed16::from_int(4),
            ),
            Ok(Some(Fixed16::from_raw(-0x8000)))
        );
    }

    #[test]
    fn first_family_adapter_matches_direct_scalar_at_both_valid_boundaries() {
        let mut first = [0; SURFACE_COEFFICIENT_COUNT];
        first[0] = -0x12345;
        first[119] = 0x23456;
        let global_factor = Fixed16::from_raw(-0x18000);
        let projection = Fixed16::from_raw(0x34567);

        for index in [0, 119] {
            assert_eq!(
                surface_indexed_lane_scalar_from_first_family(
                    &first,
                    index,
                    global_factor,
                    0,
                    projection,
                ),
                Ok(surface_indexed_lane_scalar(
                    global_factor,
                    Fixed16::from_raw(first[index]),
                    0,
                    projection,
                ))
            );
        }
    }

    #[test]
    fn first_family_adapter_composition_has_an_explicit_golden() {
        let mut first = [0; SURFACE_COEFFICIENT_COUNT];
        first[7] = 0x4000;

        let adapted = surface_indexed_lane_scalar_from_first_family(
            &first,
            7,
            Fixed16::from_int(2),
            0,
            Fixed16::from_int(4),
        );
        let direct = surface_indexed_lane_scalar(
            Fixed16::from_int(2),
            Fixed16::from_raw(first[7]),
            0,
            Fixed16::from_int(4),
        );

        assert_eq!(adapted, Ok(direct));
        assert_eq!(adapted, Ok(Some(Fixed16::from_raw(-0x8000))));
    }

    #[test]
    fn surface_indexed_scalar_uses_the_looked_up_coefficient_without_override() {
        assert_eq!(
            surface_indexed_lane_scalar(
                Fixed16::from_int(2),
                Fixed16::from_raw(0x4000),
                0,
                Fixed16::from_int(4),
            ),
            Some(Fixed16::from_raw(-0x8000))
        );
    }

    #[test]
    fn surface_indexed_scalar_override_ignores_the_table_value() {
        let projection = Fixed16::from_int(4);
        for surface_coefficient in [Fixed16::ZERO, Fixed16::from_raw(-0x1234)] {
            assert_eq!(
                surface_indexed_lane_scalar(Fixed16::ONE, surface_coefficient, 7, projection,),
                Some(Fixed16::from_raw(-0x8000))
            );
        }
    }

    #[test]
    fn surface_indexed_scalar_preserves_projection_sign() {
        assert_eq!(
            surface_indexed_lane_scalar(
                Fixed16::ONE,
                Fixed16::from_raw(0x8000),
                0,
                Fixed16::from_int(4),
            ),
            Some(Fixed16::from_raw(-0x8000))
        );
        assert_eq!(
            surface_indexed_lane_scalar(
                Fixed16::ONE,
                Fixed16::from_raw(0x8000),
                0,
                Fixed16::from_int(-4),
            ),
            Some(Fixed16::from_raw(0x8000))
        );
    }

    #[test]
    fn surface_indexed_scalar_uses_arithmetic_shift_for_negative_nonmultiple() {
        assert_eq!(-5_i32 >> 2, -2);
        assert_eq!(
            surface_indexed_lane_scalar(Fixed16::ONE, Fixed16::ONE, 0, Fixed16::from_raw(-5),),
            Some(Fixed16::from_raw(2))
        );
    }

    #[test]
    fn surface_indexed_scalar_final_negation_wraps_int_min() {
        assert_eq!(
            surface_indexed_lane_scalar(
                Fixed16::from_raw(i32::MIN),
                Fixed16::ONE,
                0,
                Fixed16::from_int(4),
            ),
            Some(Fixed16::from_raw(i32::MIN))
        );
    }

    #[test]
    fn signed_force_candidate_below_or_equal_to_transition_is_retained() {
        let alternate = Fixed16::from_raw(0x7777);
        assert_eq!(
            select_signed_force_candidate(
                Fixed16::from_raw(0x1233),
                Fixed16::from_raw(0x1234),
                alternate,
            ),
            Fixed16::from_raw(0x1233)
        );
        assert_eq!(
            select_signed_force_candidate(
                Fixed16::from_raw(0x1234),
                Fixed16::from_raw(0x1234),
                alternate,
            ),
            Fixed16::from_raw(0x1234)
        );
    }

    #[test]
    fn force_candidate_comparison_uses_signed_i32_ordering() {
        let alternate = Fixed16::from_raw(0x55);
        assert_eq!(
            select_signed_force_candidate(Fixed16::from_raw(i32::MIN), Fixed16::ZERO, alternate),
            Fixed16::from_raw(i32::MIN)
        );
        assert_eq!(
            select_signed_force_candidate(Fixed16::ZERO, Fixed16::from_raw(i32::MIN), alternate),
            alternate
        );
    }

    #[test]
    fn force_candidate_above_transition_returns_alternate_verbatim() {
        let candidate = Fixed16::from_raw(10);
        let transition = Fixed16::from_raw(9);
        for alternate in [Fixed16::from_raw(-500), Fixed16::from_raw(500)] {
            assert_eq!(
                select_signed_force_candidate(candidate, transition, alternate),
                alternate
            );
        }
    }

    #[test]
    fn lookup_free_candidates_multiply_at_component_a_threshold_minus_one() {
        assert_eq!(
            derive_lookup_free_candidates(
                Fixed16::from_raw(CANDIDATE_COMPONENT_THRESHOLD.raw() - 1),
                CANDIDATE_COMPONENT_THRESHOLD,
                Fixed16::from_raw(0x8000),
                Fixed16::from_raw(0x1_6000),
                Fixed16::from_raw(0x4000),
                Fixed16::from_raw(0xe000),
            ),
            Some((Fixed16::from_raw(0xb000), Fixed16::from_raw(0x3800)))
        );
    }

    #[test]
    fn lookup_free_candidates_exact_component_a_threshold_uses_component_b_branch() {
        let transition_gain = Fixed16::from_raw(0x8000);
        let alternate_gain = Fixed16::from_raw(0x4000);
        assert_eq!(
            derive_lookup_free_candidates(
                CANDIDATE_COMPONENT_THRESHOLD,
                Fixed16::from_raw(CANDIDATE_COMPONENT_THRESHOLD.raw() - 1),
                transition_gain,
                Fixed16::from_raw(0x1_6000),
                alternate_gain,
                Fixed16::from_raw(0xe000),
            ),
            Some((transition_gain, alternate_gain))
        );
    }

    #[test]
    fn lookup_free_candidates_give_component_a_branch_priority_when_both_are_below() {
        assert_eq!(
            derive_lookup_free_candidates(
                Fixed16::ZERO,
                Fixed16::ZERO,
                Fixed16::from_raw(0x8000),
                Fixed16::from_raw(0x1_6000),
                Fixed16::from_raw(0x4000),
                Fixed16::from_raw(0xe000),
            ),
            Some((Fixed16::from_raw(0xb000), Fixed16::from_raw(0x3800)))
        );
    }

    #[test]
    fn lookup_free_candidates_use_signed_wrapping_absolute_for_int_min() {
        let transition_gain = Fixed16::from_raw(0x8000);
        let alternate_gain = Fixed16::from_raw(0x4000);
        assert_eq!(i32::MIN.wrapping_abs(), i32::MIN);
        assert_eq!(
            derive_lookup_free_candidates(
                Fixed16::from_raw(i32::MIN),
                CANDIDATE_COMPONENT_THRESHOLD,
                transition_gain,
                Fixed16::ONE,
                alternate_gain,
                Fixed16::ONE,
            ),
            Some((transition_gain, alternate_gain))
        );
        assert_eq!(
            derive_lookup_free_candidates(
                CANDIDATE_COMPONENT_THRESHOLD,
                Fixed16::from_raw(i32::MIN),
                transition_gain,
                Fixed16::from_raw(0x1_6000),
                alternate_gain,
                Fixed16::from_raw(0xe000),
            ),
            Some((transition_gain, alternate_gain))
        );
    }

    #[test]
    fn lookup_free_candidate_products_retain_modular_q16_results() {
        let widened = (i32::MAX as i64 * i32::MAX as i64) >> 16;
        assert!(widened > i32::MAX as i64);
        assert_eq!(
            derive_lookup_free_candidates(
                Fixed16::ZERO,
                CANDIDATE_COMPONENT_THRESHOLD,
                Fixed16::from_raw(i32::MAX),
                Fixed16::from_raw(i32::MAX),
                Fixed16::from_raw(i32::MIN),
                Fixed16::from_raw(i32::MIN),
            ),
            Some((Fixed16::from_raw(widened as i32), Fixed16::ZERO))
        );
    }

    #[test]
    fn lookup_free_candidates_defer_the_combined_path() {
        assert_eq!(
            derive_lookup_free_candidates(
                CANDIDATE_COMPONENT_THRESHOLD,
                CANDIDATE_COMPONENT_THRESHOLD,
                Fixed16::ONE,
                Fixed16::ONE,
                Fixed16::ONE,
                Fixed16::ONE,
            ),
            None
        );
    }

    #[test]
    fn lookup_dependent_candidates_pin_both_component_gate_edges() {
        let below = Fixed16::from_raw(CANDIDATE_COMPONENT_THRESHOLD.raw() - 1);
        let threshold = CANDIDATE_COMPONENT_THRESHOLD;
        let derive = |component_a, component_b| {
            derive_lookup_dependent_candidates(
                component_a,
                component_b,
                Fixed16::ONE,
                Fixed16::ZERO,
                Fixed16::ONE,
                Fixed16::ZERO,
            )
        };

        assert_eq!(
            derive(below, threshold),
            Err(LookupDependentCandidateError::LookupFreePath)
        );
        assert_eq!(
            derive(threshold, below),
            Err(LookupDependentCandidateError::LookupFreePath)
        );
        assert_eq!(
            derive(threshold.neg(), threshold.neg()),
            Ok((Fixed16::ZERO, Fixed16::ZERO))
        );
        assert_eq!(
            derive(Fixed16::from_raw(i32::MIN), threshold),
            Err(LookupDependentCandidateError::LookupFreePath)
        );
    }

    #[test]
    fn lookup_dependent_candidates_preserve_equal_ratio_adjacent_zero() {
        assert_eq!(
            derive_lookup_dependent_candidates(
                Fixed16::ONE,
                Fixed16::from_int(2),
                Fixed16::ONE,
                Fixed16::from_int(2),
                Fixed16::ONE,
                Fixed16::ONE,
            ),
            Ok((Fixed16::from_raw(131_100), Fixed16::from_raw(65_531)))
        );
    }

    #[test]
    fn lookup_dependent_candidates_resolve_both_repeated_paths() {
        assert_eq!(
            derive_lookup_dependent_candidates(
                Fixed16::from_raw(0x1_2000),
                Fixed16::from_raw(0xc000),
                Fixed16::from_raw(0x8000),
                Fixed16::from_raw(0x1_4000),
                Fixed16::from_raw(0x1_8000),
                Fixed16::from_raw(0xa000),
            ),
            Ok((Fixed16::from_raw(34_748), Fixed16::from_raw(80_803)))
        );
    }

    #[test]
    fn lookup_dependent_candidates_keep_component_and_base_sign_rules() {
        let positive_components = derive_lookup_dependent_candidates(
            Fixed16::from_raw(0x1_2000),
            Fixed16::from_raw(0xc000),
            Fixed16::from_raw(-0x8000),
            Fixed16::from_raw(-0x1_4000),
            Fixed16::from_raw(0x1_8000),
            Fixed16::from_raw(-0xa000),
        );
        let negative_components = derive_lookup_dependent_candidates(
            Fixed16::from_raw(-0x1_2000),
            Fixed16::from_raw(-0xc000),
            Fixed16::from_raw(-0x8000),
            Fixed16::from_raw(-0x1_4000),
            Fixed16::from_raw(0x1_8000),
            Fixed16::from_raw(-0xa000),
        );

        assert_eq!(positive_components, negative_components);
        assert_eq!(
            positive_components,
            Ok((Fixed16::from_raw(-34_748), Fixed16::from_raw(80_803)))
        );
    }

    #[test]
    fn lookup_dependent_candidates_accept_zero_square_root_results() {
        assert_eq!(
            derive_lookup_dependent_candidates(
                Fixed16::ONE,
                Fixed16::ONE,
                Fixed16::from_raw(i32::MAX),
                Fixed16::ZERO,
                Fixed16::from_raw(i32::MIN),
                Fixed16::ZERO,
            ),
            Ok((Fixed16::ZERO, Fixed16::ZERO))
        );
    }

    #[test]
    fn lookup_dependent_candidates_keep_wrapping_square_sum() {
        assert_eq!(
            derive_lookup_dependent_candidates(
                CANDIDATE_COMPONENT_THRESHOLD,
                Fixed16::from_raw(0x100_0000),
                Fixed16::ONE,
                Fixed16::from_raw(0x100_0000),
                Fixed16::ONE,
                Fixed16::ONE,
            ),
            Ok((Fixed16::from_raw(15_587_072), Fixed16::from_raw(65_550)))
        );
    }

    #[test]
    fn lookup_dependent_candidates_propagate_each_angle_error() {
        let expected = Err(LookupDependentCandidateError::Angle(
            SignedRatioAngleError::MinimumOperand,
        ));
        assert_eq!(
            derive_lookup_dependent_candidates(
                Fixed16::ONE,
                Fixed16::ONE,
                Fixed16::ONE,
                Fixed16::from_raw(i32::MIN),
                Fixed16::ONE,
                Fixed16::ZERO,
            ),
            expected
        );
        assert_eq!(
            derive_lookup_dependent_candidates(
                Fixed16::ONE,
                Fixed16::ONE,
                Fixed16::ONE,
                Fixed16::ZERO,
                Fixed16::ONE,
                Fixed16::from_raw(i32::MIN),
            ),
            expected
        );
    }

    #[test]
    fn correction_candidate_requires_a_nonzero_gate_and_same_strict_sign() {
        let smoothed = Fixed16::from_int(5);
        let raw = Fixed16::from_int(1);
        let positive_ratio = Fixed16::ONE;

        assert_eq!(
            gated_correction_candidate(0, smoothed, raw, positive_ratio, Fixed16::ONE),
            None
        );
        assert_eq!(
            gated_correction_candidate(-7, smoothed, raw, positive_ratio, Fixed16::ONE),
            Some(Fixed16::from_raw(0x1999))
        );
        assert_eq!(
            gated_correction_candidate(
                1,
                Fixed16::from_int(-5),
                Fixed16::from_int(-1),
                -Fixed16::ONE,
                Fixed16::ONE,
            ),
            Some(Fixed16::from_raw(-0x1999))
        );
        assert_eq!(
            gated_correction_candidate(1, smoothed, raw, -Fixed16::ONE, Fixed16::ONE),
            None
        );
    }

    #[test]
    fn correction_candidate_rejects_zero_difference_or_ratio() {
        assert_eq!(
            gated_correction_candidate(1, Fixed16::ONE, Fixed16::ONE, Fixed16::ONE, Fixed16::ONE,),
            None
        );
        assert_eq!(
            gated_correction_candidate(
                1,
                Fixed16::from_int(2),
                Fixed16::ONE,
                Fixed16::ZERO,
                Fixed16::ONE,
            ),
            None
        );
    }

    #[test]
    fn correction_candidate_difference_and_products_are_modular() {
        let smoothed = Fixed16::from_raw(i32::MIN);
        let raw = Fixed16::from_raw(1);
        let wrapped_difference = smoothed.sub(raw);
        assert_eq!(wrapped_difference, Fixed16::from_raw(i32::MAX));

        let expected = wrapped_difference
            .mul(Fixed16::from_raw(0x4000))
            .mul(Fixed16::ONE.mul(Fixed16::from_raw(0x1999)));
        assert_eq!(
            gated_correction_candidate(1, smoothed, raw, Fixed16::ONE, Fixed16::ONE),
            Some(expected)
        );
        assert_eq!(
            gated_correction_candidate(
                1,
                Fixed16::from_int(2),
                Fixed16::ONE,
                Fixed16::ONE,
                Fixed16::ZERO,
            ),
            Some(Fixed16::ZERO)
        );

        let large = Fixed16::from_raw(i32::MAX);
        let quarter_raw = ((i32::MAX as i64 * 0x4000_i64) >> 16) as i32;
        let scaled_global_raw = ((i32::MAX as i64 * 0x1999_i64) >> 16) as i32;
        let shifted_product = (quarter_raw as i64 * scaled_global_raw as i64) >> 16;
        assert!(shifted_product > i32::MAX as i64);
        assert_eq!(
            gated_correction_candidate(1, large, Fixed16::ZERO, Fixed16::ONE, large),
            Some(Fixed16::from_raw(shifted_product as i32))
        );
    }

    #[test]
    fn correction_limit_retains_values_below_or_equal_to_threshold() {
        let alternate = Fixed16::from_raw(0x7777);
        assert_eq!(
            limit_signed_correction_candidate(
                Fixed16::from_raw(9),
                Fixed16::from_raw(10),
                alternate,
            ),
            Fixed16::from_raw(9)
        );
        assert_eq!(
            limit_signed_correction_candidate(
                Fixed16::from_raw(-10),
                Fixed16::from_raw(10),
                alternate,
            ),
            Fixed16::from_raw(-10)
        );
    }

    #[test]
    fn correction_limit_uses_signed_alternate_for_both_candidate_signs() {
        assert_eq!(
            limit_signed_correction_candidate(
                Fixed16::from_raw(11),
                Fixed16::from_raw(10),
                Fixed16::from_raw(-3),
            ),
            Fixed16::from_raw(-3)
        );
        assert_eq!(
            limit_signed_correction_candidate(
                Fixed16::from_raw(-11),
                Fixed16::from_raw(10),
                Fixed16::from_raw(3),
            ),
            Fixed16::from_raw(-3)
        );
    }

    #[test]
    fn correction_limit_preserves_wrapping_int_min_and_zero_behavior() {
        assert_eq!(
            limit_signed_correction_candidate(
                Fixed16::from_raw(i32::MIN),
                Fixed16::ZERO,
                Fixed16::from_raw(3),
            ),
            Fixed16::from_raw(i32::MIN)
        );
        assert_eq!(
            limit_signed_correction_candidate(
                Fixed16::ZERO,
                Fixed16::from_raw(-1),
                Fixed16::from_raw(i32::MIN),
            ),
            Fixed16::from_raw(i32::MIN)
        );
    }

    #[test]
    fn second_correction_skips_nonpositive_remainder_boundaries() {
        assert_eq!(
            second_correction_scalar(
                Fixed16::from_raw(4),
                Fixed16::from_raw(3),
                Fixed16::from_raw(3),
            ),
            None
        );
        assert_eq!(
            second_correction_scalar(Fixed16::from_raw(4), Fixed16::ZERO, Fixed16::ZERO,),
            None
        );
    }

    #[test]
    fn second_correction_projection_zero_takes_nonpositive_sign() {
        assert_eq!(
            second_correction_scalar(Fixed16::from_raw(3), Fixed16::ZERO, Fixed16::from_raw(4),),
            Some(Fixed16::from_raw(-3))
        );
    }

    #[test]
    fn second_correction_bounds_both_projection_signs() {
        assert_eq!(
            second_correction_scalar(
                Fixed16::from_raw(-3),
                Fixed16::from_raw(2),
                Fixed16::from_raw(5),
            ),
            Some(Fixed16::from_raw(3))
        );
        assert_eq!(
            second_correction_scalar(
                Fixed16::from_raw(4),
                Fixed16::from_raw(-2),
                Fixed16::from_raw(5),
            ),
            Some(Fixed16::from_raw(-3))
        );
        assert_eq!(
            second_correction_scalar(
                Fixed16::from_raw(2),
                Fixed16::from_raw(2),
                Fixed16::from_raw(5),
            ),
            Some(Fixed16::from_raw(2))
        );
    }

    #[test]
    fn second_correction_uses_wrapping_abs_and_subtraction() {
        assert_eq!(
            second_correction_scalar(
                Fixed16::from_raw(1),
                Fixed16::from_raw(i32::MIN),
                Fixed16::from_raw(i32::MIN + 1),
            ),
            Some(Fixed16::from_raw(-1))
        );
        assert_eq!(
            second_correction_scalar(
                Fixed16::from_raw(i32::MIN),
                Fixed16::from_raw(1),
                Fixed16::from_raw(i32::MIN),
            ),
            Some(Fixed16::from_raw(i32::MIN))
        );
    }

    #[test]
    fn lane_accumulation_scales_first_and_wraps_every_axis() {
        let outcome = apply_lane_accumulation(LaneAccumulationInput {
            accumulator: raw_v(i32::MAX, i32::MIN, 0x7000_0000),
            selected_scalar: Fixed16::from_int(2),
            ..lane_accumulation_input()
        });

        assert_eq!(outcome.accumulator, raw_v(-2, 0, 0xe000_0000_u32 as i32));
        assert_eq!(outcome.first_correction_scalar, None);
        assert_eq!(outcome.second_correction_scalar, None);
        assert_eq!(outcome.surface_scalar, None);
    }

    #[test]
    fn lane_accumulation_keeps_surface_path_outside_correction_gate() {
        let outcome = apply_lane_accumulation(LaneAccumulationInput {
            accumulator: v(1, 2, 3),
            selected_scalar: Fixed16::from_raw(0x8000),
            direction_a: v(1, 0, 0),
            surface_vector: v(4, 0, 0),
            surface_coefficient: Fixed16::ONE,
            ..lane_accumulation_input()
        });

        assert_eq!(outcome.first_correction_scalar, None);
        assert_eq!(outcome.second_correction_scalar, None);
        assert_eq!(outcome.surface_scalar, Some(-Fixed16::ONE));
        assert_eq!(
            outcome.accumulator,
            Vec3::new(
                Fixed16::from_raw(-0x8000),
                Fixed16::ONE,
                Fixed16::from_raw(0x1_8000),
            )
        );
    }

    #[test]
    fn lane_accumulation_skips_both_corrections_after_sign_rejection() {
        let outcome = apply_lane_accumulation(LaneAccumulationInput {
            accumulator: v(2, -3, 4),
            selected_scalar: Fixed16::from_raw(0x8000),
            correction_gate: 1,
            smoothed_value: Fixed16::from_int(5),
            raw_value: Fixed16::ONE,
            gear_ratio: -Fixed16::ONE,
            direction_a: v(7, 8, 9),
            direction_b: v(4, 5, 6),
            second_budget: Fixed16::from_int(100),
            ..lane_accumulation_input()
        });

        assert_eq!(outcome.accumulator, raw_v(0x1_0000, -0x1_8000, 0x2_0000));
        assert_eq!(outcome.first_correction_scalar, None);
        assert_eq!(outcome.second_correction_scalar, None);
    }

    #[test]
    fn lane_accumulation_second_projection_reads_post_first_accumulator() {
        let pre_first_projection = Fixed16::from_int(2);
        let admitted_first = Fixed16::from_raw(0x1999);
        let budget = Fixed16::from_raw(0x2_8000);
        assert_eq!(
            second_correction_scalar(admitted_first, pre_first_projection, budget),
            Some(admitted_first)
        );

        let outcome = apply_lane_accumulation(LaneAccumulationInput {
            accumulator: v(2, 0, 0),
            direction_a: v(10, 0, 0),
            direction_b: v(1, 0, 0),
            correction_gate: 1,
            smoothed_value: Fixed16::from_int(5),
            raw_value: Fixed16::ONE,
            gear_ratio: Fixed16::ONE,
            correction_threshold: Fixed16::from_raw(0x1999),
            correction_alternate: Fixed16::from_raw(123),
            second_budget: budget,
            ..lane_accumulation_input()
        });

        assert_eq!(outcome.first_correction_scalar, Some(admitted_first));
        assert_eq!(outcome.second_correction_scalar, None);
        assert_eq!(outcome.accumulator, raw_v(196_602, 0, 0));
    }

    #[test]
    fn lane_accumulation_composes_limit_sign_and_exact_budget_boundary() {
        for (smoothed, raw, ratio, expected) in [
            (5, 1, Fixed16::ONE, Fixed16::from_raw(3_000)),
            (-5, -1, -Fixed16::ONE, Fixed16::from_raw(-3_000)),
        ] {
            let outcome = apply_lane_accumulation(LaneAccumulationInput {
                direction_a: v(1, 0, 0),
                direction_b: v(1, 0, 0),
                correction_gate: 1,
                smoothed_value: Fixed16::from_int(smoothed),
                raw_value: Fixed16::from_int(raw),
                gear_ratio: ratio,
                correction_threshold: Fixed16::from_raw(0x1998),
                correction_alternate: Fixed16::from_raw(3_000),
                second_budget: Fixed16::from_raw(6_000),
                ..lane_accumulation_input()
            });

            assert_eq!(outcome.first_correction_scalar, Some(expected));
            assert_eq!(outcome.second_correction_scalar, Some(expected));
            assert_eq!(outcome.accumulator.x, expected.add(expected));
        }

        let equality = apply_lane_accumulation(LaneAccumulationInput {
            direction_a: v(1, 0, 0),
            correction_gate: 1,
            smoothed_value: Fixed16::from_int(5),
            raw_value: Fixed16::ONE,
            gear_ratio: Fixed16::ONE,
            correction_threshold: Fixed16::from_raw(0x1999),
            correction_alternate: Fixed16::from_raw(3_000),
            ..lane_accumulation_input()
        });
        assert_eq!(
            equality.first_correction_scalar,
            Some(Fixed16::from_raw(0x1999))
        );
    }

    #[test]
    fn lane_accumulation_surface_uses_arithmetic_shift_and_override_branch() {
        let shifted = apply_lane_accumulation(LaneAccumulationInput {
            direction_a: raw_v(Fixed16::ONE.raw(), 0, 0),
            surface_vector: raw_v(-5, 0, 0),
            surface_coefficient: Fixed16::ONE,
            ..lane_accumulation_input()
        });
        assert_eq!(shifted.surface_scalar, Some(Fixed16::from_raw(2)));
        assert_eq!(shifted.accumulator, raw_v(2, 0, 0));

        let overridden = apply_lane_accumulation(LaneAccumulationInput {
            direction_a: v(1, 0, 0),
            surface_vector: v(4, 0, 0),
            surface_coefficient: Fixed16::from_raw(i32::MIN),
            surface_override: -7,
            ..lane_accumulation_input()
        });
        assert_eq!(overridden.surface_scalar, Some(Fixed16::from_raw(-0x8000)));
        assert_eq!(overridden.accumulator, raw_v(-0x8000, 0, 0));

        let zero_projection = apply_lane_accumulation(LaneAccumulationInput {
            direction_a: v(1, 0, 0),
            surface_vector: v(0, 4, 0),
            surface_coefficient: Fixed16::ONE,
            ..lane_accumulation_input()
        });
        assert_eq!(zero_projection.surface_scalar, None);
    }

    #[test]
    fn lane_accumulation_surface_int_min_wraps_on_all_axes() {
        let outcome = apply_lane_accumulation(LaneAccumulationInput {
            direction_a: v(1, 1, 1),
            surface_vector: v(4, 0, 0),
            global_factor: Fixed16::from_raw(i32::MIN),
            surface_coefficient: Fixed16::ONE,
            ..lane_accumulation_input()
        });

        assert_eq!(outcome.surface_scalar, Some(Fixed16::from_raw(i32::MIN)));
        assert_eq!(outcome.accumulator, raw_v(i32::MIN, i32::MIN, i32::MIN));
    }

    #[test]
    fn lane_accumulation_first_add_wraps_independently_on_all_axes() {
        let outcome = apply_lane_accumulation(LaneAccumulationInput {
            accumulator: raw_v(i32::MAX - 5, i32::MIN + 5, i32::MAX - 5),
            direction_a: raw_v(101, -101, 101),
            correction_gate: 1,
            smoothed_value: Fixed16::from_int(5),
            raw_value: Fixed16::ONE,
            gear_ratio: Fixed16::ONE,
            correction_threshold: Fixed16::from_raw(0x1999),
            ..lane_accumulation_input()
        });

        assert_eq!(
            outcome.first_correction_scalar,
            Some(Fixed16::from_raw(0x1999))
        );
        assert_eq!(
            outcome.accumulator,
            raw_v(i32::MIN + 4, i32::MAX - 5, i32::MIN + 4)
        );
    }
}
