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
                "Type-2 fog vertex {vertex_index} word {word_index} (f32 bits {bits:#010x}) cannot be represented by the recovered truncating x87 i64 conversion"
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
) -> Result<Box<[C3dType2PreDrawVertex]>, C3dType2FogError> {
    let mut vertices = source_words
        .into_iter()
        .map(|words| C3dType2PreDrawVertex { words })
        .collect::<Vec<_>>();

    if !state.enabled {
        return Ok(vertices.into_boxed_slice());
    }

    // The recovered shared-distance branch maps its argument before checking
    // the vertex count. Preserve that fault boundary even for an empty batch.
    let shared_level = if state.shared_distance_q16 != 0 {
        Some(fog_level(state, state.shared_distance_q16)?)
    } else {
        None
    };
    for (vertex_index, vertex) in vertices.iter_mut().enumerate() {
        let level = if let Some(level) = shared_level {
            level
        } else {
            let distance = vertex_distance_q16(vertex.words, state.center_q16, vertex_index)?;
            fog_level(state, distance)?
        };
        vertex.words[5] = ((level as u32) & 0xffff_0000).wrapping_shl(8);
    }

    Ok(vertices.into_boxed_slice())
}

fn fog_level(state: C3dType2FogState, distance: i32) -> Result<i32, C3dType2FogError> {
    if distance < state.near_q16 {
        Ok(0x00ff_0000)
    } else if distance > state.far_q16 {
        Ok(state.far_level)
    } else {
        let slope = fog_slope(state)?;
        let remaining = state.far_q16.wrapping_sub(distance);
        let candidate = ((i64::from(slope) * i64::from(remaining)) >> 16) as i32;
        Ok(candidate.max(state.far_level))
    }
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
    vertex_index: usize,
) -> Result<i32, C3dType2FogError> {
    let qx = scaled_coordinate(words[0]).ok_or(C3dType2FogError::CoordinateConversion {
        vertex_index,
        word_index: 0,
        bits: words[0],
    })?;
    let qz = scaled_coordinate(words[2]).ok_or(C3dType2FogError::CoordinateConversion {
        vertex_index,
        word_index: 2,
        bits: words[2],
    })?;
    let dx = center[0].wrapping_neg().wrapping_sub(qx);
    let dz = center[2].wrapping_neg().wrapping_sub(qz);
    let sum = qmul_low(dx, dx).wrapping_add(qmul_low(dz, dz)) as u32;
    Ok(fixed_sqrt_q16(sum) as i32)
}

fn scaled_coordinate(bits: u32) -> Option<i32> {
    let value = f64::from(f32::from_bits(bits)) * -65_536.0;
    if !value.is_finite() {
        return None;
    }
    // The private legacy __ftol helper forces x87 RC=11, stores a signed
    // qword, restores the caller's control word, and the caller consumes EAX.
    let integral = value.trunc();
    const I64_MIN_F64: f64 = -9_223_372_036_854_775_808.0;
    const I64_END_F64: f64 = 9_223_372_036_854_775_808.0;
    if !(I64_MIN_F64..I64_END_F64).contains(&integral) {
        None
    } else {
        Some((integral as i64) as i32)
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
        let prepared =
            prepare_c3d_type2_pre_draw_vertices([source], C3dType2FogState::DISABLED).unwrap();

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
        let prepared = prepare_c3d_type2_pre_draw_vertices([source], state).unwrap();

        // The recovered lookup samples the middle of a bin, so this is the
        // table's exact answer rather than a host square-root result.
        assert_eq!(fixed_sqrt_q16(25 << 16), 327_728);
        assert_eq!(prepared[0].word5(), 0xff00_0000);
        assert_eq!(source[5], 0xdead_beef);
    }

    #[test]
    fn copies_are_independent_and_only_word5_changes() {
        let source = words(-45.0, 0.0, 0x0102_0304);
        let prepared = prepare_c3d_type2_pre_draw_vertices([source, source], state()).unwrap();

        for vertex in prepared.iter() {
            let result = vertex.words();
            assert_eq!(result[5], 0x7f00_0000);
            assert_eq!(&result[..5], &source[..5]);
            assert_eq!(&result[6..], &source[6..]);
        }
        assert_eq!(source[5], 0x0102_0304);
    }

    #[test]
    fn recovered_ftol_truncates_and_returns_the_low_i32_word() {
        let bits = (-(1.5_f32 / 65_536.0)).to_bits();
        assert_eq!(scaled_coordinate(bits), Some(1));
        assert_eq!(scaled_coordinate((-65_536.0_f32).to_bits()), Some(0));
    }

    #[test]
    fn enabled_per_vertex_path_rejects_unmodelled_x87_invalid_conversion() {
        let error =
            prepare_c3d_type2_pre_draw_vertices([words(f32::NAN, 0.0, 0)], state()).unwrap_err();
        assert_eq!(
            error,
            C3dType2FogError::CoordinateConversion {
                vertex_index: 0,
                word_index: 0,
                bits: f32::NAN.to_bits(),
            }
        );
    }

    fn overflowing_slope_state(distance: i32) -> C3dType2FogState {
        C3dType2FogState {
            enabled: true,
            near_q16: 10 << 16,
            far_q16: (10 << 16) + 1,
            far_level: 0x0033_0000,
            shared_distance_q16: distance,
            center_q16: [0; 3],
        }
    }

    #[test]
    fn empty_shared_distance_path_preserves_pre_count_slope_failure() {
        let state = overflowing_slope_state(10 << 16);
        assert_eq!(
            prepare_c3d_type2_pre_draw_vertices([], state),
            Err(C3dType2FogError::SlopeOverflow {
                near_q16: state.near_q16,
                far_q16: state.far_q16,
            })
        );
    }

    #[test]
    fn empty_per_vertex_path_checks_count_before_coordinates_or_slope() {
        let mut state = overflowing_slope_state(0);
        state.center_q16 = [i32::MIN; 3];
        assert!(
            prepare_c3d_type2_pre_draw_vertices([], state)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn outside_threshold_paths_do_not_evaluate_an_unused_overflowing_slope() {
        for (distance, expected_word5) in
            [((10 << 16) - 1, 0xff00_0000), ((10 << 16) + 2, 0x3300_0000)]
        {
            let prepared = prepare_c3d_type2_pre_draw_vertices(
                [words(f32::NAN, f32::NAN, 0xdead_beef)],
                overflowing_slope_state(distance),
            )
            .unwrap();

            assert_eq!(prepared[0].word5(), expected_word5);
        }
    }

    #[test]
    fn interpolation_still_reports_an_overflowing_slope() {
        let state = overflowing_slope_state(10 << 16);
        let error =
            prepare_c3d_type2_pre_draw_vertices([words(f32::NAN, f32::NAN, 0xdead_beef)], state)
                .unwrap_err();

        assert_eq!(
            error,
            C3dType2FogError::SlopeOverflow {
                near_q16: state.near_q16,
                far_q16: state.far_q16,
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
        let prepared = prepare_c3d_type2_pre_draw_vertices([words(0.0, 0.0, 0)], state).unwrap();
        assert_eq!(prepared[0].word5(), 0);
    }
}
