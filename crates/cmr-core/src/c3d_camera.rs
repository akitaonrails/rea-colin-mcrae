//! C3D camera, projection, viewport, clipping, and culling (item E05).
//!
//! # Scope
//!
//! The shipped game renders with Direct3D 5 Immediate Mode (see
//! [`crate::c3d_render`]). This module reconstructs the view/projection stage
//! that feeds the E06 submission: the perspective projection matrix builder
//! (`FUN_004a71b0`) and its parallel Q16.16 coefficient set, the D3DVIEWPORT2
//! clip volume (`FUN_004a6ed0`), the 2D top-down (XZ) frustum cull
//! (`FUN_004a1c00`), the cull-mode / winding rule (`FUN_004a2460`), and the LOD
//! record selection. All values are the recovered formulas/constants.
//!
//! The camera VIEW matrix is `inverse(camera-world)` (`FUN_0048f460`, a Q16.16
//! affine matrix inverse - not a look-at and not a BAM-angle composition). Its
//! structure is recovered and documented, but its fixed-point implementation
//! (the `2^32 / det` reciprocal) is deferred because byte-exact parity cannot be
//! verified here without a differential fixture; it is reconstructed with the
//! deterministic renderer (E07).
//!
//! See evidence `rea.official-eu.c3d-camera`.

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
}
