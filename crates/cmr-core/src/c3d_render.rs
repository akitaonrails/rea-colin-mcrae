//! C3D Direct3D-IM submission model: ordering, depth, transparency, blending
//! (reconstruction item E06).
//!
//! # Decisive recovered fact
//!
//! The shipped game renders through **Direct3D Immediate Mode on
//! `IDirect3DDevice2` (DirectX 5)**, layered over DirectDraw. Triangles reach the
//! device via `DrawIndexedPrimitive` (`D3DPT_TRIANGLELIST`, `D3DFVF`-less
//! `D3DLVERTEX`). Depth is a **hardware 16-bit Z-buffer** with `ZFUNC`
//! `LESSEQUAL`; there is **no CPU span-filler and no painter's triangle sort**.
//! Ordering is a per-object **back-to-front** distance sort (farthest object
//! first, by 2D top-down XZ distance; `FUN_004a2070`) plus scene-graph /
//! world-section submission order: the opaque pass draws with Z-write enabled,
//! then deferred transparent batches draw with Z-write disabled. Triangles
//! within an object and the transparent prims themselves are *not* individually
//! back-to-front sorted - unsorted transparent blends are original behavior,
//! relying on the object-level order and the Z-buffer.
//!
//! This module models the backend-neutral submission contract recovered from the
//! dispatcher `FUN_004a07d0`, the mesh emitters `FUN_004a2700`/`FUN_004a28a0`, the
//! render-state bank, the blend selectors `FUN_004a2290`/`FUN_004a2540`, and the
//! LVERTEX finalizer `FUN_004b1e10` + batch builder `FUN_004b2340`, so the
//! software oracle (E07) and a modern backend implement the *same* contract and
//! pass the *same* tests. The numeric render-state codes are public DirectX 5 SDK
//! values, reproduced here (as with the force-feedback DirectInput constants); no
//! game content is encoded.
//!
//! See evidence `rea.official-eu.c3d-rasterizer`.

// --- D3DRENDERSTATETYPE codes actually used (public DX5 values) ---------------

/// `D3DRENDERSTATE_TEXTUREHANDLE` - current texture bind.
pub const RS_TEXTUREHANDLE: u32 = 1;
/// `D3DRENDERSTATE_ZENABLE` - depth test.
pub const RS_ZENABLE: u32 = 7;
/// `D3DRENDERSTATE_ZWRITEENABLE` - depth write.
pub const RS_ZWRITEENABLE: u32 = 14;
/// `D3DRENDERSTATE_SRCBLEND`.
pub const RS_SRCBLEND: u32 = 19;
/// `D3DRENDERSTATE_DESTBLEND`.
pub const RS_DESTBLEND: u32 = 20;
/// `D3DRENDERSTATE_TEXTUREMAPBLEND`.
pub const RS_TEXTUREMAPBLEND: u32 = 21;
/// `D3DRENDERSTATE_CULLMODE`.
pub const RS_CULLMODE: u32 = 22;
/// `D3DRENDERSTATE_ZFUNC`.
pub const RS_ZFUNC: u32 = 23;
/// `D3DRENDERSTATE_FOGENABLE` (global fog; kept off - fog is per-vertex specular).
pub const RS_FOGENABLE: u32 = 28;
/// `D3DRENDERSTATE_ALPHABLENDENABLE`.
pub const RS_ALPHABLENDENABLE: u32 = 27;
/// `D3DRENDERSTATE_STIPPLEDALPHA` - screen-door fallback where HW alpha is absent.
pub const RS_STIPPLEDALPHA: u32 = 33;
/// `D3DRENDERSTATE_COLORKEYENABLE` - index/color-keyed transparency.
pub const RS_COLORKEYENABLE: u32 = 41;

// --- Render-state values ------------------------------------------------------

/// `D3DBLEND_ONE`.
pub const BLEND_ONE: u32 = 2;
/// `D3DBLEND_INVSRCCOLOR` (the additive mode's destination factor).
pub const BLEND_INVSRCCOLOR: u32 = 4;
/// `D3DBLEND_SRCALPHA`.
pub const BLEND_SRCALPHA: u32 = 5;
/// `D3DBLEND_INVSRCALPHA`.
pub const BLEND_INVSRCALPHA: u32 = 6;
/// `D3DTBLEND_MODULATE` - texel * diffuse (RGB), diffuse alpha ignored.
pub const TBLEND_MODULATE: u32 = 2;
/// `D3DTBLEND_MODULATEALPHA` - texel * diffuse including the diffuse alpha.
pub const TBLEND_MODULATEALPHA: u32 = 4;
/// `D3DCMP_LESSEQUAL` - the Z comparison.
pub const CMP_LESSEQUAL: u32 = 4;
/// `D3DCULL_NONE` - two-sided (the car render class).
pub const CULL_NONE: u32 = 1;

// --- DrawIndexedPrimitive arguments -------------------------------------------

/// `D3DPT_TRIANGLELIST` - the only primitive kind submitted.
pub const PRIMTYPE_TRIANGLELIST: u32 = 4;
/// `D3DVT_LVERTEX` - the vertex type (pre-lit, with specular).
pub const VERTEXTYPE_LVERTEX: u32 = 2;
/// `D3DDP_DONOTUPDATEEXTENTS` - the draw flag used.
pub const DP_DONOTUPDATEEXTENTS: u32 = 8;

/// Stride of one `D3DLVERTEX` (position, reserved, diffuse, specular, tu, tv).
pub const LVERTEX_STRIDE: usize = 0x20;
/// The baked diffuse color of an untextured primitive (opaque black, so the
/// vertex color shows through MODULATE); `FUN_004b1e10` writes this when the
/// material ordinal is the untextured sentinel.
pub const UNTEXTURED_DIFFUSE: u32 = 0xff00_0000;
/// The alpha byte the transparent render class forces into each per-vertex color
/// (`FUN_004a2b70`), consumed by MODULATEALPHA + SRCALPHA.
pub const TRANSPARENT_ALPHA: u8 = 0x96;

/// Builds a `D3DCOLOR` (`0xAARRGGBB`) from channel bytes - the LVERTEX diffuse
/// format the finalizer writes.
pub fn d3dcolor_argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Byte offsets of the three per-vertex colors within a runtime primitive record
/// (stride [`RUNTIME_PRIM_STRIDE`]), as read by the finalizer `FUN_004b1e10`.
pub const PRIM_VERTEX_COLOR_OFFSETS: [usize; 3] = [0x08, 0x0c, 0x10];
/// Stride of one runtime primitive record (the stride-`0x1c` prim the finalizer
/// walks and the batch builder `FUN_004b2340` groups). This is the per-frame
/// runtime record, distinct from the on-disk 24-byte C3D face entry (E02).
pub const RUNTIME_PRIM_STRIDE: usize = 0x1c;

/// Packs a runtime per-vertex color into the LVERTEX diffuse. The finalizer
/// `FUN_004b1e10` reads the 4 source bytes in ascending order as `[R, G, B, A]`
/// and repacks them into `0xAARRGGBB` (byte 0 -> red, byte 1 -> green, byte 2 ->
/// blue, byte 3 -> alpha). This resolves the E02/E06 channel-order question: the
/// stored bytes are RGBA, not BGRA. Faces with the untextured material sentinel
/// instead get [`UNTEXTURED_DIFFUSE`].
pub fn diffuse_from_rgba_bytes(rgba: [u8; 4]) -> u32 {
    d3dcolor_argb(rgba[3], rgba[0], rgba[1], rgba[2])
}

// --- Transparency / blend selectors -------------------------------------------

/// Whether hardware alpha blending is available; selects which enable state the
/// transparent path sets (`FUN_004a2290`): true -> `ALPHABLENDENABLE`, false ->
/// the `STIPPLEDALPHA` screen-door fallback.
pub fn transparency_enable_state(hardware_alpha: bool) -> u32 {
    if hardware_alpha {
        RS_ALPHABLENDENABLE
    } else {
        RS_STIPPLEDALPHA
    }
}

/// The blend classification selected by `FUN_004a2540`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlendMode {
    /// Case 0: opaque - MODULATE, blend off, color-key off.
    Opaque,
    /// Case 1: color-keyed transparency (index/color key).
    ColorKeyed,
    /// Case 2 (default): alpha blend `src*srcA + dst*(1-srcA)` with MODULATEALPHA.
    Alpha,
    /// Case 3: additive (`src*ONE + dst*INVSRCCOLOR`).
    Additive,
}

/// The device state a [`BlendMode`] establishes: whether alpha blending and
/// color-keying are on, the source/destination blend factors (when blending),
/// and the texture-blend mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlendConfig {
    pub alpha_blend: bool,
    pub color_key: bool,
    pub src_blend: Option<u32>,
    pub dest_blend: Option<u32>,
    pub texture_map_blend: u32,
}

impl BlendMode {
    /// The recovered device-state configuration for this mode (`FUN_004a2540`).
    pub fn config(self) -> BlendConfig {
        match self {
            BlendMode::Opaque => BlendConfig {
                alpha_blend: false,
                color_key: false,
                src_blend: None,
                dest_blend: None,
                texture_map_blend: TBLEND_MODULATE,
            },
            BlendMode::ColorKeyed => BlendConfig {
                alpha_blend: false,
                color_key: true,
                src_blend: None,
                dest_blend: None,
                texture_map_blend: TBLEND_MODULATE,
            },
            BlendMode::Alpha => BlendConfig {
                alpha_blend: true,
                color_key: false,
                src_blend: Some(BLEND_SRCALPHA),
                dest_blend: Some(BLEND_INVSRCALPHA),
                texture_map_blend: TBLEND_MODULATEALPHA,
            },
            BlendMode::Additive => BlendConfig {
                alpha_blend: true,
                color_key: false,
                src_blend: Some(BLEND_ONE),
                dest_blend: Some(BLEND_INVSRCCOLOR),
                texture_map_blend: TBLEND_MODULATEALPHA,
            },
        }
    }
}

// --- Pass / depth model -------------------------------------------------------

/// A submission pass. The opaque pass draws first with Z-write enabled; the
/// deferred transparent pass draws after, Z-tested but Z-write disabled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pass {
    Opaque,
    Transparent,
}

impl Pass {
    /// Whether this pass writes to the depth buffer (both passes still Z-test).
    pub fn z_write(self) -> bool {
        matches!(self, Pass::Opaque)
    }
}

/// Squared top-down (XZ-plane) distance from the camera to an object, the
/// per-object painter-sort key (`FUN_004a2070`). The sort is **2D**: the object
/// position supplies only X (`obj+0x104`) and Z (`obj+0x10c`); the vertical (Y)
/// delta is not part of the key, matching the 2D frustum cull. The shipped
/// routine takes the fixed-point square root (LUT `DAT_0082b380`) before
/// comparing, but the square root is monotonic, so comparing this squared key
/// yields the identical order. Depth within an object is the hardware Z-buffer.
pub fn object_sort_key(dx: i64, dz: i64) -> i64 {
    dx * dx + dz * dz
}

/// Whether an object with sort key `nearer_or_farther` is drawn before one with
/// key `other`. The shipped sort relinks the scene list so a **farther** object
/// precedes a nearer one, i.e. objects draw **back-to-front** (descending
/// distance); equal keys keep their submission order.
pub fn object_drawn_before(a: i64, b: i64) -> bool {
    a > b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_state_codes_are_the_recovered_dx5_values() {
        assert_eq!(RS_TEXTUREHANDLE, 1);
        assert_eq!(RS_ZENABLE, 7);
        assert_eq!(RS_ZWRITEENABLE, 14);
        assert_eq!(RS_FOGENABLE, 28);
        assert_eq!(RS_COLORKEYENABLE, 41);
        assert_eq!(CMP_LESSEQUAL, 4);
        assert_eq!(PRIMTYPE_TRIANGLELIST, 4);
    }

    #[test]
    fn opaque_mode_disables_blend_and_colorkey() {
        let cfg = BlendMode::Opaque.config();
        assert!(!cfg.alpha_blend);
        assert!(!cfg.color_key);
        assert_eq!(cfg.src_blend, None);
        assert_eq!(cfg.texture_map_blend, TBLEND_MODULATE);
    }

    #[test]
    fn color_keyed_mode_enables_only_the_key() {
        let cfg = BlendMode::ColorKeyed.config();
        assert!(cfg.color_key);
        assert!(!cfg.alpha_blend);
    }

    #[test]
    fn alpha_mode_is_srcalpha_invsrcalpha_modulatealpha() {
        let cfg = BlendMode::Alpha.config();
        assert!(cfg.alpha_blend);
        assert_eq!(cfg.src_blend, Some(BLEND_SRCALPHA));
        assert_eq!(cfg.dest_blend, Some(BLEND_INVSRCALPHA));
        assert_eq!(cfg.texture_map_blend, TBLEND_MODULATEALPHA);
    }

    #[test]
    fn additive_mode_is_one_invsrccolor() {
        let cfg = BlendMode::Additive.config();
        assert!(cfg.alpha_blend);
        assert_eq!(cfg.src_blend, Some(BLEND_ONE));
        assert_eq!(cfg.dest_blend, Some(BLEND_INVSRCCOLOR));
    }

    #[test]
    fn transparency_falls_back_to_stipple_without_hardware_alpha() {
        assert_eq!(transparency_enable_state(true), RS_ALPHABLENDENABLE);
        assert_eq!(transparency_enable_state(false), RS_STIPPLEDALPHA);
    }

    #[test]
    fn d3dcolor_packs_argb_and_constants_hold() {
        assert_eq!(d3dcolor_argb(0xff, 0x11, 0x22, 0x33), 0xff11_2233);
        assert_eq!(UNTEXTURED_DIFFUSE, d3dcolor_argb(0xff, 0, 0, 0));
        assert_eq!(TRANSPARENT_ALPHA, 0x96);
    }

    #[test]
    fn per_vertex_color_bytes_are_rgba() {
        // Source bytes [R, G, B, A] -> D3DCOLOR 0xAARRGGBB.
        assert_eq!(
            diffuse_from_rgba_bytes([0x11, 0x22, 0x33, 0xff]),
            0xff11_2233
        );
        // Alpha comes from byte 3, not byte 0 (would be BGRA otherwise).
        assert_eq!(diffuse_from_rgba_bytes([0x96, 0, 0, 0x80]), 0x8096_0000);
        assert_eq!(PRIM_VERTEX_COLOR_OFFSETS, [0x08, 0x0c, 0x10]);
        assert_eq!(RUNTIME_PRIM_STRIDE, 0x1c);
    }

    #[test]
    fn pass_z_write_only_for_opaque() {
        assert!(Pass::Opaque.z_write());
        assert!(!Pass::Transparent.z_write());
    }

    #[test]
    fn object_sort_key_is_squared_xz_distance() {
        assert_eq!(object_sort_key(3, 4), 25);
        assert_eq!(object_sort_key(-2, 0), 4);
        // Farther objects yield larger keys, the ordering basis.
        assert!(object_sort_key(10, 0) > object_sort_key(1, 1));
    }

    #[test]
    fn objects_draw_back_to_front() {
        let near = object_sort_key(1, 0);
        let far = object_sort_key(10, 0);
        // The farther object is drawn first.
        assert!(object_drawn_before(far, near));
        assert!(!object_drawn_before(near, far));
        // Equal distance keeps submission order (no reorder).
        assert!(!object_drawn_before(near, near));
    }
}
