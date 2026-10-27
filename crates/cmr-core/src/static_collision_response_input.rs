//! Pure recovery of `FUN_0043c6f0`, the per-contact input producer invoked by
//! the successful normal static narrow phase immediately before response.

use crate::{
    fixed_point::{Fixed16, Vec3},
    static_collision_response::ResponseBasis,
};

const PRIMARY_BASE: Fixed16 = Fixed16::from_raw(0x1_578d);
const CORNER_PROJECTED_LIMIT: Fixed16 = Fixed16::from_raw(0x8000);
const FALLBACK_PROJECTED_LIMIT: Fixed16 = Fixed16::from_raw(0xa_0000);
const X_MARGIN: Fixed16 = Fixed16::from_raw(0x1_8000);
const Z_MARGIN: Fixed16 = Fixed16::from_raw(0x9999);
const CANONICAL_SEED: Vec3 = Vec3::new(
    Fixed16::from_raw(0x1_8000),
    Fixed16::ZERO,
    Fixed16::from_raw(0x9999),
);

/// Literal-derived canonical axes written to globals `0x552f28/0x552f34`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticResponseCanonicalAxes {
    pub axis_552f28: Vec3,
    pub axis_552f34: Vec3,
}

/// Rebuild the two canonical response axes without reading game payloads.
///
/// Native `FUN_0043c010` constructs these once during race/session setup from
/// literal seed `(0x18000, 0, 0x9999)`. Regenerating the pure value is
/// equivalent; it is not per-car or per-frame mutable state.
#[must_use]
pub const fn static_response_canonical_axes() -> StaticResponseCanonicalAxes {
    let square_sum = CANONICAL_SEED.dot(CANONICAL_SEED).raw() as u32;
    let length = crate::legacy_math::sqrt_q16(square_sum) as i32;
    let reciprocal = ((1_i64 << 32) / length as i64) as i32;
    let normalized = CANONICAL_SEED.scale(Fixed16::from_raw(reciprocal));
    StaticResponseCanonicalAxes {
        axis_552f28: Vec3::new(normalized.z.neg(), Fixed16::ZERO, normalized.x.neg()),
        axis_552f34: Vec3::new(normalized.z.neg(), Fixed16::ZERO, normalized.x),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticResponseInputProducerState {
    pub existing_normal: Vec3,
    pub contact_3f0: Vec3,
    pub contact_limits_2c: Vec3,
    pub contact_count_674: i32,
    pub velocity_normalizer_ec: Fixed16,
    pub velocity_d4: Vec3,
    pub basis_2a0: ResponseBasis,
    pub canonical_axis_552f28: Vec3,
    pub canonical_axis_552f34: Vec3,
    pub fallback_axis_258: Vec3,
    pub fallback_axis_270: Vec3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticResponseNormalSource {
    Existing,
    Canonical552f28,
    Canonical552f34,
    Fallback258,
    Fallback270,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticResponseProducedInput {
    pub normal: Vec3,
    pub primary_limit: Fixed16,
    pub projected_limit: Fixed16,
    pub normal_source: StaticResponseNormalSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticResponseInputProducerError {
    /// Native signed `IDIV` would raise because `2^32 / divisor` does not fit
    /// in a signed 32-bit quotient. Resolved live car states are expected to
    /// keep the positive normalizer outside this malformed boundary.
    ReciprocalOverflow { divisor: i32 },
}

const fn wrapping_abs(value: Fixed16) -> Fixed16 {
    if value.raw() < 0 { value.neg() } else { value }
}

fn normalized_body_velocity(
    state: StaticResponseInputProducerState,
) -> Result<Vec3, StaticResponseInputProducerError> {
    let divisor = state.velocity_normalizer_ec.raw();
    if divisor <= 0 {
        return Ok(state.basis_2a0.world_to_body(state.velocity_d4));
    }
    let quotient = (1_i64 << 32) / i64::from(divisor);
    let reciprocal = i32::try_from(quotient)
        .map_err(|_| StaticResponseInputProducerError::ReciprocalOverflow { divisor })?;
    Ok(state
        .basis_2a0
        .world_to_body(state.velocity_d4.scale(Fixed16::from_raw(reciprocal))))
}

/// Produce the final response normal and the two shared threshold values.
///
/// When `contact_count_674 != 4`, native code preserves the normal already published by
/// the narrow phase and installs only the fallback thresholds. In mode four,
/// strict x/z side tests either replace the normal with a transformed canonical
/// axis and scale the primary threshold, or copy one of the two car axes.
pub fn produce_static_response_input(
    state: StaticResponseInputProducerState,
) -> Result<StaticResponseProducedInput, StaticResponseInputProducerError> {
    if state.contact_count_674 != 4 {
        return Ok(StaticResponseProducedInput {
            normal: state.existing_normal,
            primary_limit: PRIMARY_BASE,
            projected_limit: FALLBACK_PROJECTED_LIMIT,
            normal_source: StaticResponseNormalSource::Existing,
        });
    }

    let x = state.contact_3f0.x;
    let z = state.contact_3f0.z;
    let x_boundary = state.contact_limits_2c.x.sub(X_MARGIN);
    let z_boundary = state.contact_limits_2c.z.sub(Z_MARGIN);

    let source = if x > x_boundary {
        if z > z_boundary {
            StaticResponseNormalSource::Canonical552f28
        } else if z < z_boundary.neg() {
            StaticResponseNormalSource::Canonical552f34
        } else {
            StaticResponseNormalSource::Fallback258
        }
    } else if x < x_boundary.neg() {
        if z > z_boundary {
            StaticResponseNormalSource::Canonical552f34
        } else if z < z_boundary.neg() {
            StaticResponseNormalSource::Canonical552f28
        } else {
            StaticResponseNormalSource::Fallback258
        }
    } else {
        StaticResponseNormalSource::Fallback270
    };

    match source {
        StaticResponseNormalSource::Canonical552f28
        | StaticResponseNormalSource::Canonical552f34 => {
            let canonical = if source == StaticResponseNormalSource::Canonical552f28 {
                state.canonical_axis_552f28
            } else {
                state.canonical_axis_552f34
            };
            let factor = if state.velocity_normalizer_ec.raw() > 0 {
                wrapping_abs(normalized_body_velocity(state)?.dot(canonical))
            } else {
                Fixed16::ONE
            };
            Ok(StaticResponseProducedInput {
                normal: state.basis_2a0.body_to_world(canonical),
                primary_limit: PRIMARY_BASE.mul(factor),
                projected_limit: CORNER_PROJECTED_LIMIT,
                normal_source: source,
            })
        }
        StaticResponseNormalSource::Fallback258 => Ok(StaticResponseProducedInput {
            normal: state.fallback_axis_258,
            primary_limit: PRIMARY_BASE,
            projected_limit: FALLBACK_PROJECTED_LIMIT,
            normal_source: source,
        }),
        StaticResponseNormalSource::Fallback270 => Ok(StaticResponseProducedInput {
            normal: state.fallback_axis_270,
            primary_limit: PRIMARY_BASE,
            projected_limit: FALLBACK_PROJECTED_LIMIT,
            normal_source: source,
        }),
        StaticResponseNormalSource::Existing => unreachable!("handled before mode-four branch"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: i32 = 0x1_0000;

    const fn q(raw: i32) -> Fixed16 {
        Fixed16::from_raw(raw)
    }

    const fn v(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(q(x), q(y), q(z))
    }

    fn identity() -> ResponseBasis {
        ResponseBasis {
            rows: [v(ONE, 0, 0), v(0, ONE, 0), v(0, 0, ONE)],
        }
    }

    fn state(x: i32, z: i32) -> StaticResponseInputProducerState {
        StaticResponseInputProducerState {
            existing_normal: v(7, 8, 9),
            contact_3f0: v(x, 0, z),
            // These make both reduced boundaries exactly zero.
            contact_limits_2c: v(0x1_8000, 0, 0x9999),
            contact_count_674: 4,
            velocity_normalizer_ec: q(ONE),
            velocity_d4: v(ONE, 0, 0),
            basis_2a0: identity(),
            canonical_axis_552f28: v(ONE, 0, 0),
            canonical_axis_552f34: v(0, 0, ONE),
            fallback_axis_258: v(2, 5, 8),
            fallback_axis_270: v(3, 6, 9),
        }
    }

    #[test]
    fn canonical_axes_match_literal_seed_formula_signs_and_setup_lifetime() {
        let square_sum = CANONICAL_SEED.dot(CANONICAL_SEED).raw() as u32;
        let length = crate::legacy_math::sqrt_q16(square_sum) as i32;
        let reciprocal = ((1_i64 << 32) / i64::from(length)) as i32;
        let normalized = CANONICAL_SEED.scale(q(reciprocal));
        let axes = static_response_canonical_axes();

        assert_eq!(
            axes.axis_552f28,
            v(
                normalized.z.raw().wrapping_neg(),
                0,
                normalized.x.raw().wrapping_neg()
            )
        );
        assert_eq!(
            axes.axis_552f34,
            v(normalized.z.raw().wrapping_neg(), 0, normalized.x.raw())
        );
        assert_eq!(axes.axis_552f28.x, axes.axis_552f34.x);
        assert_eq!(axes.axis_552f28.z, axes.axis_552f34.z.neg());
        assert_eq!(axes.axis_552f28, v(-0x5f13, 0, -0xedb2));
        assert_eq!(axes.axis_552f34, v(-0x5f13, 0, 0xedb2));
        assert_eq!(static_response_canonical_axes(), axes);
    }

    #[test]
    fn strict_nine_region_table_matches_all_corner_and_fallback_paths() {
        use StaticResponseNormalSource::{
            Canonical552f28 as A, Canonical552f34 as B, Fallback258 as F258, Fallback270 as F270,
        };
        let cases = [
            (-1, -1, A),
            (-1, 0, F258),
            (-1, 1, B),
            (0, -1, F270),
            (0, 0, F270),
            (0, 1, F270),
            (1, -1, B),
            (1, 0, F258),
            (1, 1, A),
        ];
        for (x, z, expected) in cases {
            let output = produce_static_response_input(state(x, z)).unwrap();
            assert_eq!(output.normal_source, expected, "x={x}, z={z}");
            match expected {
                A => {
                    assert_eq!(output.normal, v(ONE, 0, 0));
                    assert_eq!(output.primary_limit, PRIMARY_BASE);
                    assert_eq!(output.projected_limit, CORNER_PROJECTED_LIMIT);
                }
                B => {
                    assert_eq!(output.normal, v(0, 0, ONE));
                    assert_eq!(output.primary_limit, Fixed16::ZERO);
                    assert_eq!(output.projected_limit, CORNER_PROJECTED_LIMIT);
                }
                F258 => {
                    assert_eq!(output.normal, v(2, 5, 8));
                    assert_eq!(output.primary_limit, PRIMARY_BASE);
                    assert_eq!(output.projected_limit, FALLBACK_PROJECTED_LIMIT);
                }
                F270 => {
                    assert_eq!(output.normal, v(3, 6, 9));
                    assert_eq!(output.primary_limit, PRIMARY_BASE);
                    assert_eq!(output.projected_limit, FALLBACK_PROJECTED_LIMIT);
                }
                StaticResponseNormalSource::Existing => unreachable!(),
            }
        }
    }

    #[test]
    fn canonical_path_transforms_normal_and_scales_primary_limit() {
        let mut input = state(1, 1);
        input.basis_2a0 = ResponseBasis {
            rows: [v(0, 0, ONE), v(0, ONE, 0), v(ONE, 0, 0)],
        };
        input.velocity_d4 = v(0, 0, 2 * ONE);
        let output = produce_static_response_input(input).unwrap();
        assert_eq!(output.normal, v(0, 0, ONE));
        assert_eq!(output.primary_limit, PRIMARY_BASE.mul(q(2 * ONE)));
        assert_eq!(output.projected_limit, q(0x8000));
    }

    #[test]
    fn velocity_scales_in_world_space_before_nontrivial_body_transform() {
        let mut input = state(1, 1);
        input.velocity_normalizer_ec = q(2 * ONE);
        input.velocity_d4 = v(i32::MAX, i32::MAX, 0);
        input.basis_2a0 = ResponseBasis {
            rows: [v(ONE, ONE, 0), v(0, ONE, 0), v(0, 0, ONE)],
        };
        let output = produce_static_response_input(input).unwrap();

        let reciprocal = q(0x8000);
        let native_factor = input
            .basis_2a0
            .world_to_body(input.velocity_d4.scale(reciprocal))
            .dot(input.canonical_axis_552f28);
        let reordered_factor = input
            .basis_2a0
            .world_to_body(input.velocity_d4)
            .scale(reciprocal)
            .dot(input.canonical_axis_552f28);
        assert_eq!(native_factor, q(0x7fff_fffe));
        assert_eq!(reordered_factor, q(-1));
        assert_ne!(native_factor, reordered_factor);
        assert_eq!(output.primary_limit, PRIMARY_BASE.mul(native_factor));
    }

    #[test]
    fn nonpositive_normalizer_uses_identity_factor_without_division() {
        for divisor in [0, -1, i32::MIN] {
            let mut input = state(1, 1);
            input.velocity_normalizer_ec = q(divisor);
            input.velocity_d4 = v(9 * ONE, 0, 0);
            let output = produce_static_response_input(input).unwrap();
            assert_eq!(output.primary_limit, PRIMARY_BASE);
        }
    }

    #[test]
    fn native_reciprocal_overflow_is_typed_and_boundary_three_succeeds() {
        for divisor in [1, 2] {
            let mut input = state(1, 1);
            input.velocity_normalizer_ec = q(divisor);
            assert_eq!(
                produce_static_response_input(input),
                Err(StaticResponseInputProducerError::ReciprocalOverflow { divisor })
            );
        }
        let mut input = state(1, 1);
        input.velocity_normalizer_ec = q(3);
        assert!(produce_static_response_input(input).is_ok());
    }

    #[test]
    fn mode_bypass_preserves_existing_normal_and_does_not_divide() {
        let mut input = state(1, 1);
        input.contact_count_674 = 3;
        input.velocity_normalizer_ec = q(1);
        let output = produce_static_response_input(input).unwrap();
        assert_eq!(output.normal_source, StaticResponseNormalSource::Existing);
        assert_eq!(output.normal, input.existing_normal);
        assert_eq!(output.primary_limit, PRIMARY_BASE);
        assert_eq!(output.projected_limit, FALLBACK_PROJECTED_LIMIT);
    }

    #[test]
    fn boundary_subtraction_and_negation_wrap_like_native_words() {
        let mut input = state(0, 0);
        input.contact_limits_2c.x = q(i32::MIN);
        // MIN - 0x18000 wraps positive; its negation is negative, so zero is
        // in the center band and selects +270.
        let output = produce_static_response_input(input).unwrap();
        assert_eq!(
            output.normal_source,
            StaticResponseNormalSource::Fallback270
        );
    }

    #[test]
    fn canonical_factor_absolute_and_primary_multiply_wrap_like_native_words() {
        let mut input = state(1, 1);
        input.velocity_d4 = v(i32::MIN, 0, 0);
        let output = produce_static_response_input(input).unwrap();
        assert_eq!(
            output.primary_limit,
            PRIMARY_BASE.mul(Fixed16::from_raw(i32::MIN))
        );
    }

    #[test]
    fn overlapping_wrapped_side_predicates_give_first_x_branch_priority() {
        let mut input = state(0, 0);
        input.contact_limits_2c.x = X_MARGIN.sub(q(1));
        input.contact_limits_2c.z = Z_MARGIN.sub(q(1));
        // Reduced x boundary is -1: x=0 satisfies both x>-1 and x<1. Native
        // evaluates the positive-side branch first, selecting A for z>-1.
        let output = produce_static_response_input(input).unwrap();
        assert_eq!(
            output.normal_source,
            StaticResponseNormalSource::Canonical552f28
        );
    }
}
