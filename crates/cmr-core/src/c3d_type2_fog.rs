//! Pure pre-draw Type-2 fog mutation.
//!
//! The shipped renderer recomputes Type-2 vertex word 5 immediately before
//! each eligible draw. This module deliberately produces a separate owned
//! copy: prepared C3D commands and their source-format vertices are immutable.

use std::fmt;

/// Retained process-global inputs consumed by the Type-2 pre-draw mutation.
///
/// A nonzero `shared_distance_q16` selects one distance for every vertex. Zero
/// selects the per-vertex X/Z path. The Y center is retained because it is part
/// of the recovered ABI even though that path contributes no Y term.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dType2FogState {
    pub enabled: bool,
    pub near_q16: i32,
    pub far_q16: i32,
    pub far_level: i32,
    pub shared_distance_q16: i32,
    pub center_q16: [i32; 3],
}

impl C3dType2FogState {
    /// Explicit caller-selected disabled runtime state.
    pub const DISABLED: Self = Self {
        enabled: false,
        near_q16: 0,
        far_q16: 0,
        far_level: 0,
        shared_distance_q16: 0,
        center_q16: [0; 3],
    };
}

/// Explicit x87 integral-conversion mode for the per-vertex distance path.
///
/// The ordinary game startup uses round-to-nearest-even. Keeping the mode in
/// the call makes the ambient x87 control-word boundary visible and testable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dType2FogRounding {
    NearestEven,
    Down,
    Up,
    TowardZero,
}

/// One backend-neutral, owned Type-2 vertex ready for a draw submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dType2PreDrawVertex {
    words: [u32; 8],
}

impl C3dType2PreDrawVertex {
    #[must_use]
    pub const fn words(self) -> [u32; 8] {
        self.words
    }

    #[must_use]
    pub const fn word5(self) -> u32 {
        self.words[5]
    }
}

/// Unsupported input at the bounded compatibility boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dType2FogError {
    CoordinateConversion {
        vertex_index: usize,
        word_index: usize,
        bits: u32,
    },
    SlopeOverflow {
        near_q16: i32,
        far_q16: i32,
    },
}

impl fmt::Display for C3dType2FogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoordinateConversion {
                vertex_index,
                word_index,
                bits,
            } => write!(
                formatter,
                "Type-2 fog vertex {vertex_index} word {word_index} (f32 bits {bits:#010x}) cannot be represented by the selected x87 i32 conversion profile"
            ),
            Self::SlopeOverflow { near_q16, far_q16 } => write!(
                formatter,
                "Type-2 fog slope for near {near_q16:#010x} and far {far_q16:#010x} exceeds signed i32"
            ),
        }
    }
}

impl std::error::Error for C3dType2FogError {}

/// Copies Type-2 vertex words and applies the optional fog mutation to word 5.
///
/// Input arrays may come from draw-command vertices via their public `words`
/// accessor. They are copied before any mutation, including when fog is
/// disabled. This function never mutates source commands or retained batches.
pub fn prepare_c3d_type2_pre_draw_vertices(
    source_words: impl IntoIterator<Item = [u32; 8]>,
    state: C3dType2FogState,
    rounding: C3dType2FogRounding,
) -> Result<Box<[C3dType2PreDrawVertex]>, C3dType2FogError> {
    let mut vertices = source_words
        .into_iter()
        .map(|words| C3dType2PreDrawVertex { words })
        .collect::<Vec<_>>();

    if !state.enabled {
        return Ok(vertices.into_boxed_slice());
    }

    let slope = fog_slope(state)?;
    for (vertex_index, vertex) in vertices.iter_mut().enumerate() {
        let distance = if state.shared_distance_q16 != 0 {
            state.shared_distance_q16
        } else {
            vertex_distance_q16(vertex.words, state.center_q16, rounding, vertex_index)?
        };
        let level = if distance < state.near_q16 {
            0x00ff_0000
        } else if distance > state.far_q16 {
            state.far_level
        } else {
            let remaining = state.far_q16.wrapping_sub(distance);
            let candidate = ((i64::from(slope) * i64::from(remaining)) >> 16) as i32;
            candidate.max(state.far_level)
        };
        vertex.words[5] = ((level as u32) & 0xffff_0000).wrapping_shl(8);
    }

    Ok(vertices.into_boxed_slice())
}

fn fog_slope(state: C3dType2FogState) -> Result<i32, C3dType2FogError> {
    let span = state.far_q16.wrapping_sub(state.near_q16);
    if span == 0 {
        return Ok(-1);
    }
    let quotient = (i64::from(0x00ff_0000_i32) << 16) / i64::from(span);
    i32::try_from(quotient).map_err(|_| C3dType2FogError::SlopeOverflow {
        near_q16: state.near_q16,
        far_q16: state.far_q16,
    })
}

fn vertex_distance_q16(
    words: [u32; 8],
    center: [i32; 3],
    rounding: C3dType2FogRounding,
    vertex_index: usize,
) -> Result<i32, C3dType2FogError> {
    let qx =
        scaled_coordinate(words[0], rounding).ok_or(C3dType2FogError::CoordinateConversion {
            vertex_index,
            word_index: 0,
            bits: words[0],
        })?;
    let qz =
        scaled_coordinate(words[2], rounding).ok_or(C3dType2FogError::CoordinateConversion {
            vertex_index,
            word_index: 2,
            bits: words[2],
        })?;
    let dx = center[0].wrapping_neg().wrapping_sub(qx);
    let dz = center[2].wrapping_neg().wrapping_sub(qz);
    let sum = qmul_low(dx, dx).wrapping_add(qmul_low(dz, dz)) as u32;
    Ok(fixed_sqrt_q16(sum) as i32)
}

fn scaled_coordinate(bits: u32, rounding: C3dType2FogRounding) -> Option<i32> {
    let value = f64::from(f32::from_bits(bits)) * -65_536.0;
    if !value.is_finite() {
        return None;
    }
    let integral = match rounding {
        C3dType2FogRounding::NearestEven => value.round_ties_even(),
        C3dType2FogRounding::Down => value.floor(),
        C3dType2FogRounding::Up => value.ceil(),
        C3dType2FogRounding::TowardZero => value.trunc(),
    };
    if integral < f64::from(i32::MIN) || integral > f64::from(i32::MAX) {
        None
    } else {
        Some(integral as i32)
    }
}

fn qmul_low(left: i32, right: i32) -> i32 {
    ((i64::from(left) * i64::from(right)) >> 16) as i32
}

/// Exact counterpart of the shipped startup-generated 4096-entry u16 table.
fn sqrt_lut(index: u32) -> u16 {
    debug_assert!(index < 4096);
    integer_sqrt(u64::from((8 + 16 * index) << 16)) as u16
}

fn fixed_sqrt_q16(value: u32) -> u32 {
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
        value << u32::from(index_shift.unsigned_abs())
    } else {
        value >> u32::from(index_shift as u8)
    };
    debug_assert!((1024..=4095).contains(&index));

    let scale = exponent >> 1;
    let result = u32::from(sqrt_lut(index));
    if scale < 0 {
        result >> u32::from(scale.unsigned_abs())
    } else {
        result << u32::from(scale as u8)
    }
}

fn integer_sqrt(value: u64) -> u64 {
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
    use super::*;

    fn words(x: f32, z: f32, word5: u32) -> [u32; 8] {
        [
            x.to_bits(),
            0x1111_1111,
            z.to_bits(),
            0x3333_3333,
            0x4444_4444,
            word5,
            0x6666_6666,
            0x7777_7777,
        ]
    }

    fn state() -> C3dType2FogState {
        C3dType2FogState {
            enabled: true,
            near_q16: 10 << 16,
            far_q16: 80 << 16,
            far_level: 0x0033_0000,
            shared_distance_q16: 0,
            center_q16: [0; 3],
        }
    }

    #[test]
    fn disabled_state_copies_every_word_without_interpreting_coordinates() {
        let source = words(f32::NAN, f32::INFINITY, 0x1234_5678);
        let prepared = prepare_c3d_type2_pre_draw_vertices(
            [source],
            C3dType2FogState::DISABLED,
            C3dType2FogRounding::NearestEven,
        )
        .unwrap();

        assert_eq!(prepared[0].words(), source);
    }

    #[test]
    fn shared_distance_maps_near_equal_far_equal_and_outside_thresholds() {
        let cases = [
            ((9 << 16), 0xff00_0000),
            ((10 << 16), 0xfe00_0000),
            ((45 << 16), 0x7f00_0000),
            ((80 << 16), 0x3300_0000),
            ((81 << 16), 0x3300_0000),
        ];
        for (distance, expected) in cases {
            let mut state = state();
            state.shared_distance_q16 = distance;
            let prepared = prepare_c3d_type2_pre_draw_vertices(
                [words(f32::NAN, f32::NAN, 0xdead_beef)],
                state,
                C3dType2FogRounding::NearestEven,
            )
            .unwrap();
            assert_eq!(prepared[0].word5(), expected, "distance {distance:#x}");
        }
    }

    #[test]
    fn per_vertex_path_uses_xz_center_and_exact_fixed_sqrt_table() {
        let mut state = state();
        state.center_q16 = [-(3 << 16), i32::MAX, -(4 << 16)];
        let source = words(-6.0, -8.0, 0xdead_beef);
        let prepared =
            prepare_c3d_type2_pre_draw_vertices([source], state, C3dType2FogRounding::NearestEven)
                .unwrap();

        // The recovered lookup samples the middle of a bin, so this is the
        // table's exact answer rather than a host square-root result.
        assert_eq!(fixed_sqrt_q16(25 << 16), 327_728);
        assert_eq!(prepared[0].word5(), 0xff00_0000);
        assert_eq!(source[5], 0xdead_beef);
    }

    #[test]
    fn copies_are_independent_and_only_word5_changes() {
        let source = words(-45.0, 0.0, 0x0102_0304);
        let prepared = prepare_c3d_type2_pre_draw_vertices(
            [source, source],
            state(),
            C3dType2FogRounding::NearestEven,
        )
        .unwrap();

        for vertex in prepared.iter() {
            let result = vertex.words();
            assert_eq!(result[5], 0x7f00_0000);
            assert_eq!(&result[..5], &source[..5]);
            assert_eq!(&result[6..], &source[6..]);
        }
        assert_eq!(source[5], 0x0102_0304);
    }

    #[test]
    fn rounding_profile_is_explicit_at_half_integer_boundaries() {
        let bits = (-(1.5_f32 / 65_536.0)).to_bits();
        assert_eq!(
            scaled_coordinate(bits, C3dType2FogRounding::NearestEven),
            Some(2)
        );
        assert_eq!(scaled_coordinate(bits, C3dType2FogRounding::Down), Some(1));
        assert_eq!(scaled_coordinate(bits, C3dType2FogRounding::Up), Some(2));
        assert_eq!(
            scaled_coordinate(bits, C3dType2FogRounding::TowardZero),
            Some(1)
        );
    }

    #[test]
    fn enabled_per_vertex_path_rejects_unmodelled_x87_invalid_conversion() {
        let error = prepare_c3d_type2_pre_draw_vertices(
            [words(f32::NAN, 0.0, 0)],
            state(),
            C3dType2FogRounding::NearestEven,
        )
        .unwrap_err();
        assert_eq!(
            error,
            C3dType2FogError::CoordinateConversion {
                vertex_index: 0,
                word_index: 0,
                bits: f32::NAN.to_bits(),
            }
        );
    }

    #[test]
    fn zero_width_threshold_uses_recovered_negative_one_slope() {
        let state = C3dType2FogState {
            enabled: true,
            near_q16: 10 << 16,
            far_q16: 10 << 16,
            far_level: 0,
            shared_distance_q16: 10 << 16,
            center_q16: [0; 3],
        };
        let prepared = prepare_c3d_type2_pre_draw_vertices(
            [words(0.0, 0.0, 0)],
            state,
            C3dType2FogRounding::NearestEven,
        )
        .unwrap();
        assert_eq!(prepared[0].word5(), 0);
    }
}
