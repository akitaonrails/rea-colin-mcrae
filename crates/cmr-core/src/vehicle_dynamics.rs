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
//!   the tuned coefficient).
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
//!   opaque caller argument. Geometric lane qualification remains separate.
//! - **Surface-indexed lane scalar** (`FUN_0040d810`): an opaque nonzero
//!   per-lane override selects `0x8000`; otherwise a caller-resolved surface
//!   coefficient is used. The projected input is arithmetic-shifted by two,
//!   multiplied through an opaque global factor, and wrapping-negated.
//!
//! Values are signed Q16.16 ([`crate::fixed_point`]). See evidence
//! `rea.official-eu.sim-grip` and
//! `rea.official-eu.sim-wheel-correction-arithmetic`.
//!
//! Observations: the formulas and constants above, read from the solver passes.
//! Inferences: the slope ramp raises grip as the surface flattens (output is a
//! grip factor, high on flat ground); the two surface tiers are high/low grip.
//! Unknowns (not reproduced): the physical units of all candidates, the
//! direction vectors and surrounding
//! per-wheel accumulation, the physical meaning of the nonzero control
//! `+0x420` gate, the surface-indexed direction/projection and stateful add, the
//! vector qualification and normalization around `FUN_0040fbe0`, the exact
//! semi-implicit position update's half-accel term, the suspension damper
//! source, and runtime-loaded per-surface/per-car table contents.

use crate::{
    fixed_point::{Fixed16, Vec3},
    legacy_math::{SignedRatioAngleError, cos_q16, signed_ratio_angle, sin_q16, sqrt_q16},
};

/// Velocity-integration gain applied to acceleration (`0x3333` ~ 0.2); the
/// fixed 40 ms step is folded into this tuned coefficient.
pub const VELOCITY_GAIN: Fixed16 = Fixed16::from_raw(0x3333);

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

/// Signed component boundary used by the two lookup-free candidate branches
/// in `FUN_0040d810` (`0x40f24f..0x40f2c7`).
pub const CANDIDATE_COMPONENT_THRESHOLD: Fixed16 = Fixed16::from_raw(0x6666);

/// Rejection from the lookup-dependent candidate derivation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LookupDependentCandidateError {
    /// One of the wrapping-absolute components belongs to an earlier branch.
    LookupFreePath,
    /// The recovered signed ratio lookup cannot safely represent an operand.
    Angle(SignedRatioAngleError),
}

/// Strict signed angle boundary for the qualified-lane postprocess in
/// `FUN_0040fbe0`.
pub const POSTPROCESS_ANGLE_THRESHOLD: Fixed16 = Fixed16::from_raw(0xa0000);

/// Truncated Q16.16 reciprocal of the postprocess ramp span:
/// `q16_div(1.0, 0x320000) == 0x51e`.
pub const POSTPROCESS_RAMP_RECIPROCAL: Fixed16 = Fixed16::from_raw(0x51e);

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
    fn surface_grip_tiers_match_recovery() {
        assert_eq!(SURFACE_GRIP_HIGH.raw(), 0x3d70);
        assert_eq!(SURFACE_GRIP_LOW.raw(), 0x2666);
        assert!(SURFACE_GRIP_HIGH > SURFACE_GRIP_LOW);
        assert_eq!(SURFACE_GRIP_RECORD_BYTES, 8);
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
}
