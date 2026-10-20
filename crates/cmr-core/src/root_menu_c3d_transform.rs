//! Exact root-menu C3D transform, projection, and viewport words.
//!
//! The shipped renderer consumes row-major Q16.16 actor matrices and converts
//! each word through binary64 `2^-16` scaling followed by one f32 store.  Its
//! trigonometric and square-root lookup tables were generated at startup.  This
//! reconstruction retains the exact mathematical table values without relying
//! on a host `sin` implementation, so Linux and Windows produce identical
//! command words.

use std::fmt;

use crate::display::DisplayExtent;

const Q16_ONE: i32 = 0x1_0000;
const ROOT_TRANSLATION_Q16: [i32; 3] = [0x4ccc, 0x1333, 0x14_0000];
const TRIG_PHASE_MASK: u16 = 0x0fff;
const TRIG_QUARTER_TURN: u16 = 0x0400;
const TRIG_QUARTER: [i32; 1025] = include!("root_menu_c3d_trig_quarter.in");

/// Exact f32 bit pattern for one row-major 4×4 compatibility matrix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dMatrixWords([u32; 16]);

impl C3dMatrixWords {
    #[must_use]
    pub const fn words(self) -> [u32; 16] {
        self.0
    }
}

/// Exact 44-byte viewport structure submitted by the legacy renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dViewportWords([u32; 11]);

impl C3dViewportWords {
    #[must_use]
    pub const fn words(self) -> [u32; 11] {
        self.0
    }
}

/// Safe public-boundary rejection for dimensions the recovered signed-i16
/// viewport constructor cannot represent as a positive extent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootMenuC3dViewportError {
    pub extent: DisplayExtent,
}

impl fmt::Display for RootMenuC3dViewportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "root-menu C3D viewport extent {}x{} is not a positive signed-16-bit rectangle",
            self.extent.width, self.extent.height
        )
    }
}

impl std::error::Error for RootMenuC3dViewportError {}

/// Recovered identity camera transform.
pub const ROOT_MENU_C3D_VIEW: C3dMatrixWords = C3dMatrixWords([
    0x3f80_0000,
    0,
    0,
    0,
    0,
    0x3f80_0000,
    0,
    0,
    0,
    0,
    0x3f80_0000,
    0,
    0,
    0,
    0,
    0x3f80_0000,
]);

/// Recovered fixed root-menu projection, retained as submitted f32 words.
pub const ROOT_MENU_C3D_PROJECTION: C3dMatrixWords = C3dMatrixWords([
    0x4000_0000,
    0,
    0,
    0,
    0,
    0x4046_6660,
    0,
    0,
    0,
    0,
    0x3f80_20cd,
    0x3f80_0000,
    0,
    0,
    0xbf80_20cd,
    0,
]);

/// Builds the exact root actor world matrix for the complete wrapping angle.
///
/// Only raw angle zero skips the legacy normalization branch.  In particular,
/// `0x1000` has lookup phase zero but still normalizes all three basis rows.
#[must_use]
pub fn root_menu_c3d_world_q16(raw_angle: u16) -> [i32; 16] {
    let basis = if raw_angle == 0 {
        [Q16_ONE, 0, 0, 0, Q16_ONE, 0, 0, 0, Q16_ONE]
    } else {
        let negative_sine = trig_lookup(0_u16.wrapping_sub(raw_angle));
        let cosine = trig_lookup(TRIG_QUARTER_TURN.wrapping_sub(raw_angle));
        let first = normalize_q16([cosine, 0, negative_sine]);
        let second = normalize_q16([0, Q16_ONE, 0]);
        let third = normalize_q16([negative_sine.wrapping_neg(), 0, cosine]);
        [
            first[0], first[1], first[2], second[0], second[1], second[2], third[0], third[1],
            third[2],
        ]
    };

    [
        basis[0],
        basis[1],
        basis[2],
        0,
        basis[3],
        basis[4],
        basis[5],
        0,
        basis[6],
        basis[7],
        basis[8],
        0,
        ROOT_TRANSLATION_Q16[0],
        ROOT_TRANSLATION_Q16[1],
        ROOT_TRANSLATION_Q16[2],
        Q16_ONE,
    ]
}

/// Converts the exact Q16 world transform to the f32 words submitted by the
/// recovered renderer.
#[must_use]
pub fn root_menu_c3d_world(raw_angle: u16) -> C3dMatrixWords {
    C3dMatrixWords(root_menu_c3d_world_q16(raw_angle).map(q16_to_f32_bits))
}

/// Builds the recovered viewport fields for a safe positive frontend extent.
pub fn root_menu_c3d_viewport(
    extent: DisplayExtent,
) -> Result<C3dViewportWords, RootMenuC3dViewportError> {
    if extent.width == 0
        || extent.height == 0
        || extent.width > i16::MAX as u16
        || extent.height > i16::MAX as u16
    {
        return Err(RootMenuC3dViewportError { extent });
    }

    let width = f64::from(extent.width);
    let height = f64::from(extent.height);
    let clip_y = (width / height) as f32;
    let clip_height = ((width * 2.0) / height) as f32;
    Ok(C3dViewportWords([
        44,
        0,
        0,
        u32::from(extent.width),
        u32::from(extent.height),
        (-1.0_f32).to_bits(),
        clip_y.to_bits(),
        2.0_f32.to_bits(),
        clip_height.to_bits(),
        0,
        1.0_f32.to_bits(),
    ]))
}

fn q16_to_f32_bits(value: i32) -> u32 {
    ((f64::from(value) * (1.0 / 65_536.0)) as f32).to_bits()
}

fn trig_lookup(raw_phase: u16) -> i32 {
    let phase = usize::from(raw_phase & TRIG_PHASE_MASK);
    let quadrant = phase >> 10;
    let offset = phase & 0x03ff;
    match quadrant {
        0 => TRIG_QUARTER[offset],
        1 => TRIG_QUARTER[1024 - offset],
        2 => -TRIG_QUARTER[offset],
        3 => -TRIG_QUARTER[1024 - offset],
        _ => unreachable!("masked trig phase has exactly four quadrants"),
    }
}

fn qmul_low(left: i32, right: i32) -> i32 {
    (((i64::from(left) * i64::from(right)) >> 16) & i64::from(u32::MAX)) as i32
}

fn normalize_q16(vector: [i32; 3]) -> [i32; 3] {
    let sum = vector.into_iter().fold(0_u32, |sum, component| {
        sum.wrapping_add(qmul_low(component, component) as u32)
    });
    if sum == 0 {
        return [0; 3];
    }

    let highest_bit = 31_i8 - sum.leading_zeros() as i8;
    let mut exponent = highest_bit - 15;
    if exponent & 1 != 0 {
        exponent += 1;
    }

    let index_shift = exponent + 4;
    let index = if index_shift < 0 {
        sum << u32::from(index_shift.unsigned_abs())
    } else {
        sum >> u32::from(index_shift as u8)
    };
    debug_assert!((1024..=4095).contains(&index));

    let scale = exponent >> 1;
    let mut divisor = u32::from(sqrt_lut(index as usize));
    if scale < 0 {
        divisor >>= u32::from(scale.unsigned_abs());
    } else {
        divisor <<= u32::from(scale as u8);
    }
    let reciprocal = ((1_u64 << 32) / u64::from(divisor)) as i32;
    vector.map(|component| qmul_low(component, reciprocal))
}

fn sqrt_lut(index: usize) -> u16 {
    debug_assert!(index < 4096);
    let radicand = ((8_u32 + 16 * index as u32) << 16) as u64;
    integer_sqrt(radicand) as u16
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
    use std::fmt::Write as _;

    use sha2::{Digest, Sha256};

    use super::*;

    const ZERO_Q16: [i32; 16] = [
        65_536, 0, 0, 0, 0, 65_536, 0, 0, 0, 0, 65_536, 0, 19_660, 4_915, 1_310_720, 65_536,
    ];
    const ZERO_WORDS: [u32; 16] = [
        0x3f80_0000,
        0,
        0,
        0,
        0,
        0x3f80_0000,
        0,
        0,
        0,
        0,
        0x3f80_0000,
        0,
        0x3e99_9800,
        0x3d99_9800,
        0x41a0_0000,
        0x3f80_0000,
    ];

    #[test]
    fn raw_zero_skips_rotation_and_preserves_translation() {
        assert_eq!(root_menu_c3d_world_q16(0), ZERO_Q16);
        assert_eq!(root_menu_c3d_world(0).words(), ZERO_WORDS);
    }

    #[test]
    fn raw_eight_matches_static_oracle() {
        assert_eq!(
            root_menu_c3d_world_q16(8),
            [
                65_535, 0, -805, 0, 0, 65_522, 0, 0, 804, 0, 65_535, 0, 19_660, 4_915, 1_310_720,
                65_536,
            ]
        );
        assert_eq!(
            root_menu_c3d_world(8).words(),
            [
                0x3f7f_ff00,
                0,
                0xbc49_4000,
                0,
                0,
                0x3f7f_f200,
                0,
                0,
                0x3c49_0000,
                0,
                0x3f7f_ff00,
                0,
                0x3e99_9800,
                0x3d99_9800,
                0x41a0_0000,
                0x3f80_0000,
            ]
        );
    }

    #[test]
    fn raw_full_turn_still_takes_normalization_branch() {
        let matrix = root_menu_c3d_world_q16(0x1000);
        assert_eq!([matrix[0], matrix[5], matrix[10]], [65_522; 3]);
        assert_ne!(matrix, ZERO_Q16);
        assert_eq!(root_menu_c3d_world(0x1000).words()[0], 0x3f7f_f200);
    }

    #[test]
    fn trig_lookup_has_canonical_hash_and_symmetry() {
        assert_eq!(trig_lookup(8), 804);
        assert_eq!(trig_lookup(1016), 65_531);
        assert_eq!(trig_lookup(4088), -804);

        let mut full_bytes = Vec::with_capacity(4096 * 4);
        for phase in 0..4096_u16 {
            full_bytes.extend_from_slice(&trig_lookup(phase).to_le_bytes());
            assert_eq!(trig_lookup(phase), trig_lookup(phase.wrapping_add(4096)));
        }
        assert_eq!(
            hex_sha256(&full_bytes),
            "f8002db15dd3f307df69236e00caa9233d027a059e50846db3e8925348b363aa"
        );

        let quarter_bytes: Vec<_> = TRIG_QUARTER
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect();
        assert_eq!(
            hex_sha256(&quarter_bytes),
            "493588138b6b9a2fb39aff91a5e912eff965d4cf608dc0c672b3d7402324fabe"
        );
    }

    #[test]
    fn generated_sqrt_table_and_combined_tables_have_canonical_hashes() {
        assert_eq!(sqrt_lut(1024), 32_775);
        assert_eq!(sqrt_lut(4095), 65_531);

        let mut sqrt_bytes = Vec::with_capacity(4096 * 2);
        let mut combined = Vec::with_capacity(4096 * 6);
        for phase in 0..4096_u16 {
            combined.extend_from_slice(&trig_lookup(phase).to_le_bytes());
        }
        for index in 0..4096 {
            let value = sqrt_lut(index);
            sqrt_bytes.extend_from_slice(&value.to_le_bytes());
            combined.extend_from_slice(&value.to_le_bytes());
        }
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
    fn normalization_preserves_instruction_level_edges() {
        assert_eq!(normalize_q16([0, 0, 0]), [0, 0, 0]);
        assert_eq!(normalize_q16([i32::MIN, 0, 0]), [0, 0, 0]);
        assert_eq!(normalize_q16([i32::MAX, 0, 0]), [8_388_607, 0, 0]);
        assert_eq!(
            normalize_q16([i32::MAX; 3]),
            [8_388_607, 8_388_607, 8_388_607]
        );
        assert_eq!(normalize_q16([256, 0, 0]), [65_536, 0, 0]);
        assert_eq!(normalize_q16([-256, 0, 0]), [-65_536, 0, 0]);
        assert_eq!(normalize_q16([65_531, 0, -804]), [65_535, 0, -805]);
        assert_eq!(normalize_q16([0, 65_536, 0]), [0, 65_522, 0]);
        assert_eq!(normalize_q16([804, 0, 65_531]), [804, 0, 65_535]);
    }

    #[test]
    fn every_raw_angle_retains_actor_matrix_layout() {
        for raw_angle in 0..=u16::MAX {
            let matrix = root_menu_c3d_world_q16(raw_angle);
            assert_eq!([matrix[3], matrix[7], matrix[11]], [0; 3]);
            assert_eq!(&matrix[12..15], &ROOT_TRANSLATION_Q16);
            assert_eq!(matrix[15], Q16_ONE);
        }
    }

    #[test]
    fn camera_projection_and_frontend_viewport_match_static_oracle() {
        assert_eq!(
            ROOT_MENU_C3D_VIEW.words(),
            [
                0x3f80_0000,
                0,
                0,
                0,
                0,
                0x3f80_0000,
                0,
                0,
                0,
                0,
                0x3f80_0000,
                0,
                0,
                0,
                0,
                0x3f80_0000,
            ]
        );
        assert_eq!(
            ROOT_MENU_C3D_PROJECTION.words(),
            [
                0x4000_0000,
                0,
                0,
                0,
                0,
                0x4046_6660,
                0,
                0,
                0,
                0,
                0x3f80_20cd,
                0x3f80_0000,
                0,
                0,
                0xbf80_20cd,
                0,
            ]
        );
        assert_eq!(
            root_menu_c3d_viewport(DisplayExtent {
                width: 640,
                height: 480,
            })
            .unwrap()
            .words(),
            [
                0x2c,
                0,
                0,
                0x280,
                0x1e0,
                0xbf80_0000,
                0x3faa_aaab,
                0x4000_0000,
                0x402a_aaab,
                0,
                0x3f80_0000,
            ]
        );
    }

    #[test]
    fn viewport_rejects_unsafe_signed_i16_extents() {
        for extent in [
            DisplayExtent {
                width: 0,
                height: 480,
            },
            DisplayExtent {
                width: 640,
                height: 0,
            },
            DisplayExtent {
                width: i16::MAX as u16 + 1,
                height: 480,
            },
            DisplayExtent {
                width: 640,
                height: i16::MAX as u16 + 1,
            },
        ] {
            assert_eq!(
                root_menu_c3d_viewport(extent),
                Err(RootMenuC3dViewportError { extent })
            );
        }
    }

    #[test]
    fn largest_safe_square_viewport_has_exact_ratios() {
        let extent = DisplayExtent {
            width: i16::MAX as u16,
            height: i16::MAX as u16,
        };
        let words = root_menu_c3d_viewport(extent).unwrap().words();
        assert_eq!(words[3..5], [0x7fff, 0x7fff]);
        assert_eq!(words[6], 1.0_f32.to_bits());
        assert_eq!(words[8], 2.0_f32.to_bits());
    }

    fn hex_sha256(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        let mut output = String::with_capacity(digest.len() * 2);
        for byte in digest {
            write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
        }
        output
    }
}
