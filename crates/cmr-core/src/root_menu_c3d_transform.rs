//! Exact root-menu C3D transform, projection, and viewport words.
//!
//! The shipped renderer consumes row-major Q16.16 actor matrices and converts
//! each word through binary64 `2^-16` scaling followed by one f32 store.  Its
//! trigonometric and square-root lookup tables were generated at startup.  This
//! reconstruction retains the exact mathematical table values without relying
//! on a host `sin` implementation, so Linux and Windows produce identical
//! command words.

use std::fmt;

use crate::{
    display::DisplayExtent,
    legacy_math::{cos_q16, sin_q16, sqrt_q16},
};

const Q16_ONE: i32 = 0x1_0000;
const ROOT_TRANSLATION_Q16: [i32; 3] = [0x4ccc, 0x1333, 0x14_0000];

/// Exact f32 bit pattern for one row-major 4×4 compatibility matrix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dMatrixWords([u32; 16]);

impl C3dMatrixWords {
    #[must_use]
    pub(crate) const fn from_words(words: [u32; 16]) -> Self {
        Self(words)
    }

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
    let basis = apply_c3d_angle_tuple_q16(
        [Q16_ONE, 0, 0, 0, Q16_ONE, 0, 0, 0, Q16_ONE],
        [0, raw_angle, 0],
    );

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

/// Applies the recovered actor angle primitive to a row-major Q16.16 basis.
///
/// Tuple words are visited in the original `1, 2, 0` order. Each nonzero
/// word rotates around the corresponding current basis row and normalizes all
/// three result rows using the same integer kernel as the shipped executable.
pub(crate) fn apply_c3d_angle_tuple_q16(mut basis: [i32; 9], angles: [u16; 3]) -> [i32; 9] {
    for axis_index in [1_usize, 2, 0] {
        let angle = angles[axis_index];
        if angle == 0 {
            continue;
        }

        let axis = basis_row(basis, axis_index);
        let rotation = axis_angle_rotation_q16(axis, angle);
        let mut rotated = [0_i32; 9];
        for row in 0..3 {
            let source = basis_row(basis, row);
            let result = [
                qmul_low(source[0], rotation[0])
                    .wrapping_add(qmul_low(source[1], rotation[3]))
                    .wrapping_add(qmul_low(source[2], rotation[6])),
                qmul_low(source[0], rotation[1])
                    .wrapping_add(qmul_low(source[1], rotation[4]))
                    .wrapping_add(qmul_low(source[2], rotation[7])),
                qmul_low(source[0], rotation[2])
                    .wrapping_add(qmul_low(source[1], rotation[5]))
                    .wrapping_add(qmul_low(source[2], rotation[8])),
            ];
            rotated[row * 3..row * 3 + 3].copy_from_slice(&normalize_q16(result));
        }
        basis = rotated;
    }
    basis
}

fn basis_row(basis: [i32; 9], row: usize) -> [i32; 3] {
    [basis[row * 3], basis[row * 3 + 1], basis[row * 3 + 2]]
}

fn axis_angle_rotation_q16(axis: [i32; 3], angle: u16) -> [i32; 9] {
    let [x, y, z] = axis;
    let negative_sine = sin_q16(0_u16.wrapping_sub(angle)).raw();
    let cosine = cos_q16(angle).raw();
    let one_minus_cosine = Q16_ONE.wrapping_sub(cosine);
    let scaled_product = |left, right| qmul_low(qmul_low(left, right), one_minus_cosine);

    let xx = scaled_product(x, x);
    let xy = scaled_product(x, y);
    let xz = scaled_product(x, z);
    let yy = scaled_product(y, y);
    let yz = scaled_product(y, z);
    let zz = scaled_product(z, z);
    let xs = qmul_low(x, negative_sine);
    let ys = qmul_low(y, negative_sine);
    let zs = qmul_low(z, negative_sine);

    [
        xx.wrapping_add(cosine),
        xy.wrapping_sub(zs),
        xz.wrapping_add(ys),
        xy.wrapping_add(zs),
        yy.wrapping_add(cosine),
        yz.wrapping_sub(xs),
        xz.wrapping_sub(ys),
        yz.wrapping_add(xs),
        zz.wrapping_add(cosine),
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
    frontend_c3d_viewport_rect(C3dViewportRect {
        x: 0,
        y: 0,
        width: extent.width,
        height: extent.height,
    })
    .map_err(|_| RootMenuC3dViewportError { extent })
}

/// The front end's C3D viewport rectangle `DAT_00d4ce10` (i16 `x, y, w, h`
/// pixels), which each sub-screen's enter callback sets (the root's covers
/// the display).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dViewportRect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

/// `FUN_004a0010`'s `D3DVIEWPORT2` from the rectangle: `X, Y, W, H` as given
/// (sign-extended), the clip volume `-1, w / h, 2, 2 w / h` and depth `0..1`.
pub fn frontend_c3d_viewport_rect(
    rect: C3dViewportRect,
) -> Result<C3dViewportWords, RootMenuC3dViewportError> {
    if rect.width == 0
        || rect.height == 0
        || rect.width > i16::MAX as u16
        || rect.height > i16::MAX as u16
    {
        return Err(RootMenuC3dViewportError {
            extent: DisplayExtent {
                width: rect.width,
                height: rect.height,
            },
        });
    }

    let width = f64::from(rect.width);
    let height = f64::from(rect.height);
    let clip_y = (width / height) as f32;
    let clip_height = ((width * 2.0) / height) as f32;
    Ok(C3dViewportWords([
        44,
        i32::from(rect.x) as u32,
        i32::from(rect.y) as u32,
        u32::from(rect.width),
        u32::from(rect.height),
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

    let divisor = sqrt_q16(sum);
    let reciprocal = ((1_u64 << 32) / u64::from(divisor)) as i32;
    vector.map(|component| qmul_low(component, reciprocal))
}

#[cfg(test)]
mod tests {
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
}
