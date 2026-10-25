//! Exact source-owned reconstruction of the legacy startup math tables.
//!
//! The shipped executable builds these values at startup. This module keeps the
//! recovered lookup behavior behind scalar functions, without loading or
//! embedding bytes from the game.

use crate::fixed_point::Fixed16;

const TRIG_PHASE_MASK: u16 = 0x0fff;
const TRIG_QUARTER_TURN: u16 = 0x0400;
const TRIG_QUARTER: [i32; 1025] = include!("root_menu_c3d_trig_quarter.in");
const SIGNED_ARCSINE_TABLE: [u16; 4096] = include!("signed_arcsine_table.in");
const SIGNED_RATIO_ANGLE_TABLE: [u16; 512] = include!("signed_ratio_angle_table.in");

/// Input-domain failure for [`signed_arcsine_phase`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignedArcsinePhaseError {
    /// Wrapping negation cannot produce a valid magnitude for `i32::MIN`.
    MinimumInput,
}

/// Reconstructs the signed arcsine-table phase kernel used by
/// `FUN_0040fbe0`.
///
/// The input is a signed Q16.16 scalar. Its wrapping absolute value is clamped
/// to `0x10000`, shifted right by four, and used as a u16 table index. A
/// negative input negates the recovered phase after lookup. Exact magnitude
/// `0x10000` produces index 4096: the shipped layout reads the adjacent signed
/// ratio table's first word, which is zero, and this helper represents that
/// sentinel explicitly instead of indexing out of bounds.
///
/// This is a neutral compatibility primitive, not a general floating-point
/// arcsine API. Its phase uses `0x1000` per full period and `0x400` per quarter.
///
/// # Errors
///
/// Returns [`SignedArcsinePhaseError::MinimumInput`] for `i32::MIN`, whose
/// wrapping negation remains negative and would send the shipped unchecked
/// logical-shift index far outside the recovered table span.
pub fn signed_arcsine_phase(value: i32) -> Result<i16, SignedArcsinePhaseError> {
    if value == i32::MIN {
        return Err(SignedArcsinePhaseError::MinimumInput);
    }

    let negative = value.is_negative();
    let magnitude = value.abs().min(Fixed16::ONE.raw());
    let index = (magnitude as usize) >> 4;
    let mut phase = match index {
        0..=4095 => SIGNED_ARCSINE_TABLE[index],
        // DAT_00831380 is immediately followed by DAT_00833380, whose first
        // generated word is zero.
        4096 => 0,
        _ => unreachable!("the clamped magnitude cannot exceed index 4096"),
    };
    if negative {
        phase = phase.wrapping_neg();
    }
    Ok(phase as i16)
}

/// Input-domain failure for [`signed_ratio_angle`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignedRatioAngleError {
    /// The legacy wrapping absolute-value sequence is not defined as a safe
    /// lookup operation for `i32::MIN`.
    MinimumOperand,
    /// Checked arithmetic produced a ratio or table index outside the
    /// recovered lookup domain.
    ArithmeticDomain,
}

/// Reconstructs the legacy signed ratio-to-angle lookup.
///
/// The result uses a wrapping phase where `0x1000` is one full period and
/// `0x0400` is one quarter-period. This is deliberately not an `atan2` API:
/// it restores only the XOR of the operand signs. The asymmetric zero cases
/// and the zero sentinel for equal nonzero magnitudes are compatibility
/// behavior.
///
/// # Errors
///
/// Returns [`SignedRatioAngleError::MinimumOperand`] when either nonzero
/// operand is `i32::MIN`. The recovered zero gates run first. If both operands
/// are nonzero, the legacy `NEG` instruction cannot make the minimum value
/// positive, after which its signed divide and logical index shift can fault or
/// address outside the recovered table.
pub fn signed_ratio_angle(first: i32, second: i32) -> Result<i16, SignedRatioAngleError> {
    if first == 0 {
        return Ok(0);
    }
    if second == 0 {
        return Ok(TRIG_QUARTER_TURN as i16);
    }
    if first == i32::MIN || second == i32::MIN {
        return Err(SignedRatioAngleError::MinimumOperand);
    }

    let mut first_magnitude = first.abs();
    let mut second_magnitude = second.abs();
    let negative = first.is_negative() ^ second.is_negative();
    let swapped = first_magnitude > second_magnitude;
    if swapped {
        core::mem::swap(&mut first_magnitude, &mut second_magnitude);
    }

    let numerator = i64::from(first_magnitude)
        .checked_shl(16)
        .ok_or(SignedRatioAngleError::ArithmeticDomain)?;
    let ratio = numerator
        .checked_div(i64::from(second_magnitude))
        .ok_or(SignedRatioAngleError::ArithmeticDomain)?;
    let index = usize::try_from(ratio >> 7).map_err(|_| SignedRatioAngleError::ArithmeticDomain)?;
    let mut angle = match index {
        0..=511 => SIGNED_RATIO_ANGLE_TABLE[index],
        // The legacy index lands on the adjacent generated table here. Its
        // first entry is zero, so preserve the result without coupling the
        // two source-owned representations.
        512 => 0,
        _ => return Err(SignedRatioAngleError::ArithmeticDomain),
    };

    if swapped {
        angle = TRIG_QUARTER_TURN.wrapping_sub(angle);
    }
    if negative {
        angle = angle.wrapping_neg();
    }
    Ok(angle as i16)
}

/// Exact Q16.16 sine lookup for a wrapping 16-bit BAM phase.
#[must_use]
pub const fn sin_q16(raw_phase: u16) -> Fixed16 {
    let phase = (raw_phase & TRIG_PHASE_MASK) as usize;
    let quadrant = phase >> 10;
    let offset = phase & 0x03ff;
    let value = match quadrant {
        0 => TRIG_QUARTER[offset],
        1 => TRIG_QUARTER[1024 - offset],
        2 => -TRIG_QUARTER[offset],
        3 => -TRIG_QUARTER[1024 - offset],
        _ => unreachable!(),
    };
    Fixed16::from_raw(value)
}

/// Exact Q16.16 cosine lookup, a quarter-turn phase shift of [`sin_q16`].
#[must_use]
pub const fn cos_q16(raw_phase: u16) -> Fixed16 {
    sin_q16(raw_phase.wrapping_add(TRIG_QUARTER_TURN))
}

/// Exact fixed-point square-root behavior backed by the recovered startup
/// lookup formula.
///
/// `value` is the unsigned, wrapping Q16.16 squared sum consumed by the legacy
/// normalization and distance paths. The result is an unsigned Q16.16 word.
#[must_use]
pub const fn sqrt_q16(value: u32) -> u32 {
    if value == 0 {
        return 0;
    }
    let highest_bit = 31_i8 - value.leading_zeros() as i8;
    let mut exponent = highest_bit - 15;
    if exponent & 1 != 0 {
        exponent += 1;
    }
    let index_shift = exponent + 4;
    let index = if index_shift < 0 {
        value << index_shift.unsigned_abs()
    } else {
        value >> index_shift as u8
    };
    debug_assert!(index >= 1024 && index <= 4095);

    let scale = exponent >> 1;
    let result = sqrt_table_entry(index);
    if scale < 0 {
        result >> scale.unsigned_abs()
    } else {
        result << scale as u8
    }
}

const fn sqrt_table_entry(index: u32) -> u32 {
    debug_assert!(index < 4096);
    integer_sqrt(((8 + 16 * index) << 16) as u64) as u32
}

const fn integer_sqrt(value: u64) -> u64 {
    let mut remainder = value;
    let mut root = 0_u64;
    let mut bit = 1_u64 << 62;
    while bit > remainder {
        bit >>= 2;
    }
    while bit != 0 {
        if remainder >= root + bit {
            remainder -= root + bit;
            root = (root >> 1) + bit;
        } else {
            root >>= 1;
        }
        bit >>= 2;
    }
    root
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn trig_lookup_has_exact_anchors_symmetry_and_wrap() {
        assert_eq!(sin_q16(0).raw(), 0);
        assert_eq!(sin_q16(1).raw(), 100);
        assert_eq!(sin_q16(8).raw(), 804);
        assert_eq!(sin_q16(1016).raw(), 65_531);
        assert_eq!(sin_q16(0x400).raw(), 65_536);
        assert_eq!(cos_q16(0).raw(), 65_536);

        for phase in 0..4096_u16 {
            assert_eq!(sin_q16(phase), sin_q16(phase.wrapping_add(4096)));
            assert_eq!(sin_q16(phase.wrapping_add(2048)), -sin_q16(phase));
            assert_eq!(cos_q16(phase), sin_q16(phase.wrapping_add(0x400)));
        }
    }

    #[test]
    fn signed_arcsine_phase_pins_zero_sign_and_minimum_domain() {
        assert_eq!(signed_arcsine_phase(0), Ok(0));
        assert_eq!(signed_arcsine_phase(15), Ok(0));
        assert_eq!(signed_arcsine_phase(-15), Ok(0));
        assert_eq!(signed_arcsine_phase(0x8000), Ok(341));
        assert_eq!(signed_arcsine_phase(-0x8000), Ok(-341));
        assert_eq!(
            signed_arcsine_phase(i32::MIN),
            Err(SignedArcsinePhaseError::MinimumInput)
        );
    }

    #[test]
    fn signed_arcsine_phase_pins_threshold_neighbor_indices() {
        assert_eq!(signed_arcsine_phase(4032 << 4), Ok(909));
        assert_eq!(signed_arcsine_phase(4033 << 4), Ok(910));
        assert_eq!(signed_arcsine_phase(4034 << 4), Ok(911));
        assert_eq!(signed_arcsine_phase(-(4033 << 4)), Ok(-910));
        assert_eq!(signed_arcsine_phase(-(4034 << 4)), Ok(-911));
    }

    #[test]
    fn signed_arcsine_phase_keeps_index_4096_adjacent_zero() {
        assert_eq!(signed_arcsine_phase(0xffff), Ok(1023));
        assert_eq!(signed_arcsine_phase(-0xffff), Ok(-1023));
        assert_eq!(signed_arcsine_phase(0x10000), Ok(0));
        assert_eq!(signed_arcsine_phase(-0x10000), Ok(0));
        assert_eq!(signed_arcsine_phase(i32::MAX), Ok(0));
        assert_eq!(signed_arcsine_phase(i32::MIN + 1), Ok(0));
    }

    #[test]
    fn signed_arcsine_table_has_canonical_shape_and_hash() {
        assert_eq!(SIGNED_ARCSINE_TABLE.len(), 4096);
        assert_eq!(SIGNED_ARCSINE_TABLE[0], 0);
        assert_eq!(SIGNED_ARCSINE_TABLE[2048], 341);
        assert_eq!(SIGNED_ARCSINE_TABLE[4033], 910);
        assert_eq!(SIGNED_ARCSINE_TABLE[4034], 911);
        assert_eq!(SIGNED_ARCSINE_TABLE[4095], 1023);
        assert!(
            SIGNED_ARCSINE_TABLE
                .windows(2)
                .all(|pair| pair[0] <= pair[1])
        );

        let bytes: Vec<_> = SIGNED_ARCSINE_TABLE
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        assert_eq!(
            hex_sha256(&bytes),
            "ddf0d95fbd29aedd9287193c16fac32b628f29507e479220785ce00f1e02745b"
        );
    }

    #[test]
    fn signed_ratio_angle_keeps_zero_asymmetry_and_rejects_minimum() {
        assert_eq!(signed_ratio_angle(0, 0), Ok(0));
        assert_eq!(signed_ratio_angle(0, 7), Ok(0));
        assert_eq!(signed_ratio_angle(0, -7), Ok(0));
        assert_eq!(signed_ratio_angle(7, 0), Ok(0x400));
        assert_eq!(signed_ratio_angle(-7, 0), Ok(0x400));

        assert_eq!(
            signed_ratio_angle(i32::MIN, 1),
            Err(SignedRatioAngleError::MinimumOperand)
        );
        assert_eq!(
            signed_ratio_angle(1, i32::MIN),
            Err(SignedRatioAngleError::MinimumOperand)
        );
        assert_eq!(signed_ratio_angle(0, i32::MIN), Ok(0));
        assert_eq!(signed_ratio_angle(i32::MIN, 0), Ok(0x400));
    }

    #[test]
    fn signed_ratio_angle_pins_index_boundaries_and_equal_sentinel() {
        assert_eq!(signed_ratio_angle(127, 65_536), Ok(0));
        assert_eq!(signed_ratio_angle(128, 65_536), Ok(1));
        assert_eq!(signed_ratio_angle(65_408, 65_536), Ok(511));
        assert_eq!(signed_ratio_angle(65_535, 65_536), Ok(511));

        for magnitude in [1, 127, 128, 65_535, 65_536, i32::MAX] {
            assert_eq!(signed_ratio_angle(magnitude, magnitude), Ok(0));
            assert_eq!(signed_ratio_angle(-magnitude, -magnitude), Ok(0));
        }
    }

    #[test]
    fn signed_ratio_angle_restores_swap_and_xor_sign() {
        let magnitudes = [1, 127, 128, 1024, 32_768, 65_408, 65_535, 65_536];
        for first in magnitudes {
            for second in magnitudes {
                let positive = signed_ratio_angle(first, second).unwrap();
                assert_eq!(signed_ratio_angle(-first, -second), Ok(positive));
                assert_eq!(signed_ratio_angle(-first, second), Ok(-positive));
                assert_eq!(signed_ratio_angle(first, -second), Ok(-positive));

                if first != second {
                    let swapped = signed_ratio_angle(second, first).unwrap();
                    assert_eq!(i32::from(positive) + i32::from(swapped), 0x400);
                }
            }
        }
    }

    #[test]
    fn signed_ratio_angle_feeds_wrapping_trig_phase() {
        let positive = signed_ratio_angle(32_768, 65_536).unwrap();
        let negative = signed_ratio_angle(-32_768, 65_536).unwrap();
        assert_eq!(negative, -positive);
        assert_eq!(
            sin_q16(negative as u16),
            sin_q16((i32::from(negative) + 0x1000) as u16)
        );
        assert_eq!(sin_q16(negative as u16), -sin_q16(positive as u16));
        assert_eq!(cos_q16(negative as u16), cos_q16(positive as u16));
    }

    #[test]
    fn signed_ratio_angle_table_has_canonical_shape_and_hash() {
        assert_eq!(SIGNED_RATIO_ANGLE_TABLE.len(), 512);
        assert_eq!(SIGNED_RATIO_ANGLE_TABLE[0], 0);
        assert_eq!(SIGNED_RATIO_ANGLE_TABLE[511], 511);
        assert!(
            SIGNED_RATIO_ANGLE_TABLE
                .windows(2)
                .all(|pair| pair[0] <= pair[1])
        );

        let bytes: Vec<_> = SIGNED_RATIO_ANGLE_TABLE
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        assert_eq!(
            hex_sha256(&bytes),
            "50604018fd3b52b579c3a26dc571d1d71f0db3a3f08faf706367fc84eb278150"
        );
    }

    #[test]
    fn startup_tables_keep_the_canonical_hashes() {
        let mut trig_bytes = Vec::with_capacity(4096 * 4);
        let mut sqrt_bytes = Vec::with_capacity(4096 * 2);
        let mut combined = Vec::with_capacity(4096 * 6);
        for phase in 0..4096_u16 {
            let bytes = sin_q16(phase).raw().to_le_bytes();
            trig_bytes.extend_from_slice(&bytes);
            combined.extend_from_slice(&bytes);
        }
        for index in 0..4096_u32 {
            let value = sqrt_table_entry(index) as u16;
            let bytes = value.to_le_bytes();
            sqrt_bytes.extend_from_slice(&bytes);
            combined.extend_from_slice(&bytes);
        }

        assert_eq!(
            hex_sha256(&trig_bytes),
            "f8002db15dd3f307df69236e00caa9233d027a059e50846db3e8925348b363aa"
        );
        assert_eq!(
            hex_sha256(&sqrt_bytes),
            "0750dc14e5f73204cdf69f0f7fe5a977788d970594fc5d3ab39b6914c85aa383"
        );
        assert_eq!(
            hex_sha256(&combined),
            "8f15233f6da6cb427bc88f913df1a3f35bc8035968a21300672b59fd56a07068"
        );
    }

    #[test]
    fn legacy_square_root_keeps_lookup_anchors_and_boundaries() {
        assert_eq!(sqrt_table_entry(1024), 32_775);
        assert_eq!(sqrt_table_entry(4095), 65_531);
        assert_eq!(sqrt_q16(0), 0);
        assert_eq!(sqrt_q16(25 << 16), 327_728);
        assert_eq!(sqrt_q16(u32::MAX), 16_775_936);
    }

    #[test]
    fn legacy_square_root_pins_normalization_regime_edges() {
        // These straddle log2/exponent changes in both right-scaled and
        // left-scaled result regimes. They protect the shared normalization
        // contract independently of the raw 4096-entry table digest.
        let cases = [
            (1, 256),
            (3, 443),
            (4, 512),
            (0x03ff, 8_188),
            (0x0400, 8_193),
            (0x0401, 8_197),
            (0x3fff, 32_765),
            (0x4000, 32_775),
            (0x4001, 32_775),
            (0x3fff_ffff, 8_387_968),
            (0x4000_0000, 8_390_400),
            (0x4000_0001, 8_390_400),
        ];
        for (input, expected) in cases {
            assert_eq!(sqrt_q16(input), expected, "input {input:#010x}");
        }
    }

    fn hex_sha256(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        digest.iter().fold(String::new(), |mut out, byte| {
            write!(&mut out, "{byte:02x}").unwrap();
            out
        })
    }
}
