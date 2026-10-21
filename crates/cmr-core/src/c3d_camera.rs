//! C3D camera, projection, viewport, clipping, and culling (item E05).
//!
//! # Scope
//!
//! The shipped game renders with Direct3D 5 Immediate Mode (see
//! [`crate::c3d_render`]). This module reconstructs the view/projection stage
//! that feeds the E06 submission: the perspective projection matrix builder
//! (`FUN_004a71b0`) and its parallel Q16.16 coefficient set, the D3DVIEWPORT2
//! clip volume (`FUN_004a6ed0`), the 2D top-down (XZ) frustum cull
//! (`FUN_004a1c00`), the cull-mode / winding rule (`FUN_004a2460`), the LOD
//! record selection, and the camera VIEW matrix. All values are the recovered
//! formulas/constants.
//!
//! The camera VIEW matrix is `inverse(camera-world)` ([`invert_affine_q16`],
//! `FUN_0048f460`) - a Q16.16 affine matrix inverse, not a look-at and not a
//! BAM-angle composition. The recovered routine is pure integer arithmetic (an
//! `i64` multiply reduced by `>> 16`, a `2^32 / det` integer reciprocal), so it
//! reproduces byte-exact with no driver floating-point dependency.
//!
//! Matrices are `[i32; 16]` column-major Q16.16: the 3x3 basis occupies columns
//! `0..=2`/`4..=6`/`8..=10`, the translation `12..=14`, and the homogeneous row
//! `3`/`7`/`11 = 0`, `15 = 1.0`. This matches the affine transform `FUN_0048fa60`
//! (`out[i] = m[12+i] + sum_j (m[4*j+i] * v[j])`).
//!
//! See evidence `rea.official-eu.c3d-camera`.

use crate::display::DisplayExtent;
use crate::root_menu_c3d_transform::{
    C3dMatrixWords, C3dViewportWords, RootMenuC3dViewportError, root_menu_c3d_viewport,
};

// --- Perspective projection (FUN_004a71b0) ------------------------------------

/// Baseline projection inputs `(w, h, z_near, z_far)` the D3D init sets
/// (`FUN_004a6950`): a fixed ~90 degree horizontal FOV and a 1.0..250.0 depth
/// range. Track load and the mirror/menu/sky callers vary these.
pub const PROJECTION_BASELINE: (f32, f32, f32, f32) = (2.0, 2.4, 1.0, 250.0);
/// The near plane, fixed at 1.0 at every call site.
pub const Z_NEAR: f32 = 1.0;
/// The default far plane (overridden per track and scaled by the view-distance
/// setting).
pub const Z_FAR_DEFAULT: f32 = 250.0;

/// Per-detail-level far-plane multipliers (`FUN_00435d10`, cases 0..6).
pub const DETAIL_FAR_MULTIPLIERS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// Builds the left-handed D3D perspective matrix (`FUN_004a71b0`) as a row-major
/// `D3DMATRIX`: `m00 = w/2`, `m11 = h/2`, `m22 = zf/(zf-zn)`, `_34 = 1`,
/// `_43 = -zn*zf/(zf-zn)`, `_44 = 0`.
pub fn perspective_lh(w: f32, h: f32, z_near: f32, z_far: f32) -> [f32; 16] {
    let mut m = [0.0f32; 16];
    let range = z_far - z_near;
    m[0] = w * 0.5; // _11
    m[5] = h * 0.5; // _22
    m[10] = z_far / range; // _33
    m[11] = 1.0; // _34 (w = z)
    m[14] = -z_near * z_far / range; // _43
    m[15] = 0.0; // _44
    m
}

/// A mirror/reflection view flips the horizontal scale, realized by negating the
/// projection width (`FUN_00435d10`: `uVar3 = -uVar3`).
pub fn mirror_width(w: f32) -> f32 {
    -w
}

/// Truncates a float coefficient to Q16.16 the way the engine stores its
/// parallel fixed-point copy (`__ftol(coef * 65536)`, truncating toward zero).
pub fn coeff_to_q16(coef: f32) -> i32 {
    (coef * 65_536.0) as i32
}

/// The sparse Q16.16 projection coefficients the engine keeps alongside the
/// float matrix for its own fixed-point vertex/distance path: `[m00, m11, m22,
/// 1.0, m32]` (stored at `ctx+0x40c0/40d4/40e8/40ec/40f8`).
pub fn projection_coeffs_q16(w: f32, h: f32, z_near: f32, z_far: f32) -> [i32; 5] {
    let m = perspective_lh(w, h, z_near, z_far);
    [
        coeff_to_q16(m[0]),
        coeff_to_q16(m[5]),
        coeff_to_q16(m[10]),
        0x1_0000, // the constant 1.0 (w = z) coefficient
        coeff_to_q16(m[14]),
    ]
}

// --- Viewport (FUN_004a6ed0, D3DVIEWPORT2) ------------------------------------

/// The recovered D3DVIEWPORT2 fields: full back buffer at the origin, a clip
/// volume whose X span is fixed and whose Y span carries the `W/H` aspect, and
/// depth `0..1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub clip_x: f32,
    pub clip_width: f32,
    pub clip_y: f32,
    pub clip_height: f32,
    pub min_z: f32,
    pub max_z: f32,
}

/// Builds the viewport for a logical render size (e.g. 640x480). Aspect enters
/// only through the clip rectangle; the projection FOV is independent.
pub fn viewport(width: u32, height: u32) -> Viewport {
    let aspect = width as f32 / height as f32;
    Viewport {
        x: 0,
        y: 0,
        width,
        height,
        clip_x: -1.0,
        clip_width: 2.0,
        clip_y: aspect,
        clip_height: 2.0 * aspect,
        min_z: 0.0,
        max_z: 1.0,
    }
}

// --- 2D top-down frustum cull (FUN_004a1c00) ----------------------------------

/// Default near/far extents of the two XZ frustum triangles (`DAT_005022e8` /
/// `DAT_005022ec`); overridden per track.
pub const FRUSTUM_NEAR_EXTENT: f32 = 100.0;
pub const FRUSTUM_FAR_EXTENT: f32 = 500.0;
/// Distance below which a visible node goes to the full-detail deferred near
/// list (`DAT_00502774`), and that list's cap.
pub const NEAR_FAR_SPLIT: f32 = 7.0;
pub const NEAR_LIST_CAP: usize = 25;

/// Point-in-triangle test on the XZ (ground) plane (`FUN_004a1c00`): the point
/// is inside when it is on the non-negative side of all three edges (the
/// original pre-scales the deltas by `1/100` to fit 32-bit; the sign is
/// unchanged, so this computes the edge cross products directly in `i64`).
/// Vertices are `[x, z]`.
pub fn point_in_triangle_xz(point: [i64; 2], tri: [[i64; 2]; 3]) -> bool {
    (0..3).all(|edge| {
        let a = tri[edge];
        let b = tri[(edge + 1) % 3];
        let cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
        cross >= 0
    })
}

// --- Cull mode / winding (FUN_004a2460) ---------------------------------------

/// `D3DCULL_NONE` - two-sided (drawn for a record whose two-sided bit is set).
pub const CULL_NONE: u32 = 1;
/// `D3DCULL_CW`.
pub const CULL_CW: u32 = 2;
/// `D3DCULL_CCW` - the default (`ctx+0x70`), so front faces are wound clockwise.
pub const CULL_CCW: u32 = 3;
/// Bit in a mesh LOD record's byte `+0x20` marking the record two-sided.
pub const TWO_SIDED_FLAG: u8 = 0x01;

/// The cull mode for a draw record: `CULL_NONE` when its two-sided bit is set,
/// otherwise the default back-face cull.
pub fn cull_mode(record_flag_20: u8) -> u32 {
    if record_flag_20 & TWO_SIDED_FLAG != 0 {
        CULL_NONE
    } else {
        CULL_CCW
    }
}

// --- LOD / detail selection ---------------------------------------------------

/// Stride of one mesh draw-group (LOD) record (base `mesh+0x20`).
pub const LOD_RECORD_STRIDE: usize = 0x18;
/// Record-relative offset of the D3D vertex type.
pub const LOD_VERTEX_TYPE_OFFSET: usize = 0x10;
/// Record-relative offset of the primitive-list head (`mesh+0x34` = base + 0x14).
pub const LOD_PRIMITIVE_HEAD_OFFSET: usize = 0x14;
/// Base offset of the LOD record array within a mesh.
pub const LOD_RECORD_BASE: usize = 0x20;
/// Global detail cap: a node with detail byte (`node+0x191`) above this is
/// skipped (`DAT_0050278c`).
pub const DETAIL_CAP: u8 = 3;
/// Billboard sentinel value: a mesh whose `+0x84/+0x88/+0x8c` all equal `-1.0`
/// Q16.16 draws with an identity WORLD transform instead of its node matrix.
pub const BILLBOARD_SENTINEL_Q16: i32 = -0x1_0000;

/// Mesh-relative byte offset of LOD record `lod`.
pub fn lod_record_offset(lod: usize) -> usize {
    LOD_RECORD_BASE + lod * LOD_RECORD_STRIDE
}

/// Whether a mesh's `+0x84/+0x88/+0x8c` triple marks it as a billboard (all
/// three equal to the `-1.0` sentinel).
pub fn is_billboard(xyz_q16: [i32; 3]) -> bool {
    xyz_q16 == [BILLBOARD_SENTINEL_Q16; 3]
}

/// Whether a node is within the global detail cap (drawn) or above it (skipped).
pub fn node_within_detail_cap(node_detail: u8) -> bool {
    node_detail <= DETAIL_CAP
}

// --- Camera VIEW matrix: Q16.16 affine inverse (FUN_0048f460) -----------------

/// A Q16.16 column-major identity matrix (`FUN_0048ed80`), returned for a
/// singular (zero-determinant) basis.
pub const MATRIX_IDENTITY_Q16: [i32; 16] = [
    0x1_0000, 0, 0, 0, //
    0, 0x1_0000, 0, 0, //
    0, 0, 0x1_0000, 0, //
    0, 0, 0, 0x1_0000,
];

/// Q16.16 fixed-point multiply: `(a * b) >> 16` with an `i64` intermediate and
/// truncation toward zero, matching the shipped `imul`/`shrd` sequence.
fn fmul_q16(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64) >> 16) as i32
}

/// Inverts a Q16.16 affine matrix (`FUN_0048f460`) to produce the camera VIEW
/// matrix from the camera-world matrix. The 3x3 basis is inverted via the
/// cofactor adjugate over the determinant (reciprocal `2^32 / det`), and the
/// translation becomes `inverse_basis * (-translation)`. A singular basis
/// (`det == 0`) yields [`MATRIX_IDENTITY_Q16`], exactly as the shipped routine.
pub fn invert_affine_q16(m: &[i32; 16]) -> [i32; 16] {
    // Cofactor row paired with basis column 0/1/2 for the determinant.
    let cof0 = fmul_q16(m[5], m[10]) - fmul_q16(m[9], m[6]);
    let cof1 = fmul_q16(m[9], m[2]) - fmul_q16(m[1], m[10]);
    let cof2 = fmul_q16(m[1], m[6]) - fmul_q16(m[5], m[2]);
    let det = fmul_q16(cof2, m[8]) + fmul_q16(cof1, m[4]) + fmul_q16(cof0, m[0]);
    if det == 0 {
        return MATRIX_IDENTITY_Q16;
    }
    let recip = (0x1_0000_0000_i64 / det as i64) as i32;

    let mut out = [0_i32; 16];
    // Inverse basis = adjugate (transposed cofactors) scaled by 1/det.
    out[0] = fmul_q16(recip, cof0);
    out[1] = fmul_q16(recip, cof1);
    out[2] = fmul_q16(recip, cof2);
    out[4] = fmul_q16(recip, fmul_q16(m[8], m[6]) - fmul_q16(m[4], m[10]));
    out[5] = fmul_q16(recip, fmul_q16(m[0], m[10]) - fmul_q16(m[8], m[2]));
    out[6] = fmul_q16(recip, fmul_q16(m[4], m[2]) - fmul_q16(m[0], m[6]));
    out[8] = fmul_q16(recip, fmul_q16(m[4], m[9]) - fmul_q16(m[8], m[5]));
    out[9] = fmul_q16(recip, fmul_q16(m[8], m[1]) - fmul_q16(m[0], m[9]));
    out[10] = fmul_q16(recip, fmul_q16(m[0], m[5]) - fmul_q16(m[4], m[1]));
    out[15] = 0x1_0000;

    // Inverse translation = inverse_basis applied to the negated translation.
    let neg = [-m[12], -m[13], -m[14]];
    out[12] = fmul_q16(neg[2], out[8]) + fmul_q16(neg[1], out[4]) + fmul_q16(neg[0], out[0]);
    out[13] = fmul_q16(neg[2], out[9]) + fmul_q16(neg[1], out[5]) + fmul_q16(neg[0], out[1]);
    out[14] = fmul_q16(neg[2], out[10]) + fmul_q16(neg[1], out[6]) + fmul_q16(neg[0], out[2]);
    out
}

/// Applies a Q16.16 column-major affine matrix to a point (`FUN_0048fa60`):
/// `out[i] = m[12+i] + sum_j (m[4*j+i] * v[j])`. The companion to
/// [`invert_affine_q16`], used to transform a point by the VIEW matrix.
pub fn apply_affine_q16(m: &[i32; 16], v: [i32; 3]) -> [i32; 3] {
    let mut out = [0_i32; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot =
            m[12 + i] + fmul_q16(m[i], v[0]) + fmul_q16(m[4 + i], v[1]) + fmul_q16(m[8 + i], v[2]);
    }
    out
}

// --- Transform-state submission (FUN_004a7d70, FUN_004a71b0) -------------------

/// `D3DTRANSFORMSTATE_WORLD` - the per-node model matrix (`SetTransform(1)`).
pub const TRANSFORMSTATE_WORLD: u32 = 1;
/// `D3DTRANSFORMSTATE_VIEW` - the camera VIEW matrix (`SetTransform(2)`).
pub const TRANSFORMSTATE_VIEW: u32 = 2;
/// `D3DTRANSFORMSTATE_PROJECTION` - the projection matrix (`SetTransform(3)`).
pub const TRANSFORMSTATE_PROJECTION: u32 = 3;

/// Converts a Q16.16 matrix to the float `D3DMATRIX` the engine submits
/// (`FUN_004a7d70`: each word times `1.5258789e-05`, i.e. `2^-16`). The VIEW
/// matrix is computed as [`invert_affine_q16`] of the camera-world matrix, then
/// converted and handed to `SetTransform(VIEW)`; D3D - not the CPU -
/// concatenates WORLD * VIEW * PROJECTION per vertex.
pub fn matrix_q16_to_f32(m: &[i32; 16]) -> [f32; 16] {
    let mut out = [0.0_f32; 16];
    for (slot, &word) in out.iter_mut().zip(m.iter()) {
        *slot = word as f32 / 65_536.0;
    }
    out
}

// --- Gameplay-scene matrix bridge ---------------------------------------------

/// Wraps a row-major `[f32; 16]` as the `C3dMatrixWords` the software transform
/// path consumes (it stores each slot as f32 bits and reads it back with
/// `f32::from_bits`). The row-vector convention matches [`apply_affine_q16`] and
/// [`perspective_lh`], so these matrices feed the per-vertex transform directly.
fn matrix_words_from_f32(m: [f32; 16]) -> C3dMatrixWords {
    C3dMatrixWords::from_words(m.map(f32::to_bits))
}

/// The gameplay-scene PROJECTION matrix as submission words (see
/// [`perspective_lh`]).
pub fn projection_matrix_words(w: f32, h: f32, z_near: f32, z_far: f32) -> C3dMatrixWords {
    matrix_words_from_f32(perspective_lh(w, h, z_near, z_far))
}

/// A node's WORLD matrix as submission words, from its Q16.16 world transform
/// (node `+0x114`).
pub fn world_matrix_words(world_q16: &[i32; 16]) -> C3dMatrixWords {
    matrix_words_from_f32(matrix_q16_to_f32(world_q16))
}

/// The camera VIEW matrix as submission words: the affine inverse of the
/// camera-world matrix ([`invert_affine_q16`]) converted to float. This is the
/// matrix `FUN_004a7d70` builds and submits with `SetTransform(VIEW)`.
pub fn view_matrix_words(camera_world_q16: &[i32; 16]) -> C3dMatrixWords {
    matrix_words_from_f32(matrix_q16_to_f32(&invert_affine_q16(camera_world_q16)))
}

/// The gameplay-scene D3DVIEWPORT2 as submission words. The clip volume is
/// scene-independent - it is the same full-back-buffer viewport the menu path
/// submits (`{clipX=-1, clipW=2, clipY=W/H, clipH=2*(W/H), minZ=0, maxZ=1}`,
/// matching [`viewport`]) - so this reuses the shared builder rather than
/// defining a second representation.
pub fn viewport_words(
    width: u16,
    height: u16,
) -> Result<C3dViewportWords, RootMenuC3dViewportError> {
    root_menu_c3d_viewport(DisplayExtent { width, height })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perspective_matches_the_recovered_baseline() {
        let (w, h, zn, zf) = PROJECTION_BASELINE;
        let m = perspective_lh(w, h, zn, zf);
        assert_eq!(m[0], 1.0); // w/2
        assert_eq!(m[5], 1.2); // h/2
        assert_eq!(m[11], 1.0); // _34
        assert_eq!(m[15], 0.0); // _44
        assert!((m[10] - 250.0 / 249.0).abs() < 1e-6); // zf/(zf-zn)
        assert!((m[14] - (-250.0 / 249.0)).abs() < 1e-6); // -zn*zf/(zf-zn)
    }

    #[test]
    fn projection_q16_coeffs_include_the_unit_w_term() {
        let (w, h, zn, zf) = PROJECTION_BASELINE;
        let c = projection_coeffs_q16(w, h, zn, zf);
        assert_eq!(c[0], 0x1_0000); // m00 = 1.0
        assert_eq!(c[1], coeff_to_q16(1.2)); // m11 = 1.2
        assert_eq!(c[3], 0x1_0000); // the constant 1.0 coefficient
        assert!(c[4] < 0); // m32 is negative
    }

    #[test]
    fn mirror_negates_width_only() {
        assert_eq!(mirror_width(2.0), -2.0);
        let m = perspective_lh(mirror_width(2.0), 2.4, 1.0, 250.0);
        assert_eq!(m[0], -1.0); // flipped X scale
        assert_eq!(m[5], 1.2); // Y unchanged
    }

    #[test]
    fn viewport_puts_aspect_in_the_clip_rectangle_only() {
        let v = viewport(640, 480);
        assert_eq!((v.x, v.y, v.width, v.height), (0, 0, 640, 480));
        assert_eq!(v.clip_x, -1.0);
        assert_eq!(v.clip_width, 2.0);
        assert_eq!(v.clip_y, 640.0 / 480.0);
        assert_eq!(v.clip_height, 2.0 * (640.0 / 480.0));
        assert_eq!((v.min_z, v.max_z), (0.0, 1.0));
    }

    #[test]
    fn point_in_triangle_xz_accepts_inside_rejects_outside() {
        // CCW triangle on the XZ plane.
        let tri = [[0, 0], [10, 0], [0, 10]];
        assert!(point_in_triangle_xz([1, 1], tri));
        assert!(point_in_triangle_xz([0, 0], tri)); // on a vertex (>= 0)
        assert!(!point_in_triangle_xz([9, 9], tri)); // outside the hypotenuse
        assert!(!point_in_triangle_xz([-1, 1], tri));
    }

    #[test]
    fn cull_mode_is_none_only_for_two_sided_records() {
        assert_eq!(cull_mode(0x00), CULL_CCW);
        assert_eq!(cull_mode(TWO_SIDED_FLAG), CULL_NONE);
        assert_eq!(cull_mode(0xfe), CULL_CCW); // the two-sided bit is clear
    }

    #[test]
    fn lod_offsets_and_billboard_sentinel() {
        assert_eq!(lod_record_offset(0), 0x20);
        assert_eq!(lod_record_offset(2), 0x20 + 2 * 0x18);
        // Primitive head sits at mesh+0x34 for LOD 0.
        assert_eq!(lod_record_offset(0) + LOD_PRIMITIVE_HEAD_OFFSET, 0x34);
        assert!(is_billboard([BILLBOARD_SENTINEL_Q16; 3]));
        assert!(!is_billboard([
            BILLBOARD_SENTINEL_Q16,
            0,
            BILLBOARD_SENTINEL_Q16
        ]));
        assert!(node_within_detail_cap(3));
        assert!(!node_within_detail_cap(4));
    }

    const ONE: i32 = 0x1_0000;

    #[test]
    fn inverse_of_identity_is_identity() {
        assert_eq!(invert_affine_q16(&MATRIX_IDENTITY_Q16), MATRIX_IDENTITY_Q16);
    }

    #[test]
    fn inverse_of_pure_translation_negates_it() {
        let mut m = MATRIX_IDENTITY_Q16;
        m[12] = 2 * ONE;
        m[13] = -3 * ONE;
        m[14] = 5 * ONE;
        let inv = invert_affine_q16(&m);
        // Basis stays identity; translation is negated.
        assert_eq!(&inv[0..12], &MATRIX_IDENTITY_Q16[0..12]);
        assert_eq!([inv[12], inv[13], inv[14]], [-2 * ONE, 3 * ONE, -5 * ONE]);
    }

    #[test]
    fn inverse_of_uniform_scale_is_the_reciprocal_scale() {
        let mut m = MATRIX_IDENTITY_Q16;
        m[0] = 2 * ONE;
        m[5] = 2 * ONE;
        m[10] = 2 * ONE;
        let inv = invert_affine_q16(&m);
        assert_eq!([inv[0], inv[5], inv[10]], [ONE / 2, ONE / 2, ONE / 2]);
    }

    #[test]
    fn singular_basis_falls_back_to_identity() {
        let mut m = MATRIX_IDENTITY_Q16;
        m[0] = 0; // zero the (0,0) basis entry -> det collapses to 0
        m[5] = 0;
        m[10] = 0;
        assert_eq!(invert_affine_q16(&m), MATRIX_IDENTITY_Q16);
    }

    #[test]
    fn inverse_undoes_a_rotation_and_translation_round_trip() {
        // +90 degrees about Z (column-major basis), translated by (2, 3, 0).
        let mut world = MATRIX_IDENTITY_Q16;
        world[0] = 0;
        world[1] = ONE; // col0 = (0, 1, 0)
        world[4] = -ONE;
        world[5] = 0; // col1 = (-1, 0, 0)
        world[12] = 2 * ONE;
        world[13] = 3 * ONE;

        let view = invert_affine_q16(&world);
        let point = [ONE, 0, 0];
        let transformed = apply_affine_q16(&world, point);
        // (1,0,0) rotated +90 about Z = (0,1,0), plus (2,3,0) = (2,4,0).
        assert_eq!(transformed, [2 * ONE, 4 * ONE, 0]);
        // The inverse VIEW returns it exactly (integer-exact for this matrix).
        assert_eq!(apply_affine_q16(&view, transformed), point);
    }

    /// The exact row-vector multiply the software raster applies
    /// (`multiply_row_vector`): `out[c] = sum_r v[r] * m[4*r + c]`.
    fn row_vector_mul(v: [f32; 4], words: C3dMatrixWords) -> [f32; 4] {
        let m = words.words().map(f32::from_bits);
        std::array::from_fn(|c| v[0] * m[c] + v[1] * m[4 + c] + v[2] * m[8 + c] + v[3] * m[12 + c])
    }

    #[test]
    fn projection_words_map_near_to_zero_and_far_to_one() {
        let (w, h, zn, zf) = PROJECTION_BASELINE;
        let proj = projection_matrix_words(w, h, zn, zf);
        // A point on the near plane maps to NDC z = 0, the far plane to z = 1.
        let near = row_vector_mul([0.0, 0.0, zn, 1.0], proj);
        let far = row_vector_mul([0.0, 0.0, zf, 1.0], proj);
        assert!((near[2] / near[3]).abs() < 1e-5); // zn -> 0
        assert!((far[2] / far[3] - 1.0).abs() < 1e-5); // zf -> 1
        // w' carries z (the LH perspective divide).
        assert!((far[3] - zf).abs() < 1e-3);
    }

    #[test]
    fn view_words_move_the_world_into_camera_space() {
        // Camera translated to (2, 3, 5), identity orientation.
        let mut camera_world = MATRIX_IDENTITY_Q16;
        camera_world[12] = 2 * ONE;
        camera_world[13] = 3 * ONE;
        camera_world[14] = 5 * ONE;
        let view = view_matrix_words(&camera_world);
        // A world point at the camera origin lands at the view-space origin.
        let at_camera = row_vector_mul([2.0, 3.0, 5.0, 1.0], view);
        assert!(at_camera[0].abs() < 1e-3);
        assert!(at_camera[1].abs() < 1e-3);
        assert!(at_camera[2].abs() < 1e-3);
        // A world point one unit ahead (+Z) stays one unit ahead in view space.
        let ahead = row_vector_mul([2.0, 3.0, 6.0, 1.0], view);
        assert!((ahead[2] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn viewport_words_match_the_e05_clip_volume() {
        let v = viewport(640, 480);
        let words = viewport_words(640, 480).unwrap().words();
        assert_eq!(words[0], 44); // dwSize
        assert_eq!((words[1], words[2]), (0, 0)); // origin
        assert_eq!((words[3], words[4]), (640, 480)); // width, height
        assert_eq!(f32::from_bits(words[5]), v.clip_x);
        assert_eq!(f32::from_bits(words[6]), v.clip_y);
        assert_eq!(f32::from_bits(words[7]), v.clip_width);
        assert_eq!(f32::from_bits(words[8]), v.clip_height);
        assert_eq!(f32::from_bits(words[9]), v.min_z);
        assert_eq!(f32::from_bits(words[10]), v.max_z);
        // A zero or oversized extent is rejected.
        assert!(viewport_words(0, 480).is_err());
        assert!(viewport_words(640, 0).is_err());
    }

    #[test]
    fn world_words_round_trip_the_q16_matrix() {
        let mut world = MATRIX_IDENTITY_Q16;
        world[12] = -4 * ONE;
        let words = world_matrix_words(&world);
        // Translation survives the Q16 -> f32 -> bits wrapping.
        let moved = row_vector_mul([1.0, 0.0, 0.0, 1.0], words);
        assert!((moved[0] - (1.0 - 4.0)).abs() < 1e-3);
    }

    #[test]
    fn view_matrix_converts_to_float_via_2_pow_neg16() {
        let mut m = MATRIX_IDENTITY_Q16;
        m[12] = 3 * ONE; // +3.0 translation
        m[1] = ONE / 2; // +0.5
        let f = matrix_q16_to_f32(&m);
        assert_eq!(f[0], 1.0);
        assert_eq!(f[1], 0.5);
        assert_eq!(f[12], 3.0);
        assert_eq!(f[15], 1.0);
        assert_eq!(f[3], 0.0);
        // Transform states match the D3DTRANSFORMSTATETYPE enum.
        assert_eq!(
            [
                TRANSFORMSTATE_WORLD,
                TRANSFORMSTATE_VIEW,
                TRANSFORMSTATE_PROJECTION
            ],
            [1, 2, 3]
        );
    }
}
