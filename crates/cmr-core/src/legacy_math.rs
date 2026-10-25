//! Exact source-owned reconstruction of the legacy startup math tables.
//!
//! The shipped executable builds these values at startup. This module keeps the
//! recovered lookup behavior behind scalar functions, without loading or
//! embedding bytes from the game.

use crate::fixed_point::Fixed16;

const TRIG_PHASE_MASK: u16 = 0x0fff;
const TRIG_QUARTER_TURN: u16 = 0x0400;
const TRIG_QUARTER: [i32; 1025] = include!("root_menu_c3d_trig_quarter.in");

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
