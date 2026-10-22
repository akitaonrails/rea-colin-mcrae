//! C3D runtime mesh layout and the per-node draw-call expansion
//! (reconstruction item E06/E07).
//!
//! When the draw walker `FUN_004a28a0` reaches a drawable node (its render-pass
//! bit is set, its kind `node+0x180` is `0`, and its mesh pointer `node+0x0c` is
//! non-null), it submits the node's world matrix and then expands the node's
//! mesh into Direct3D `DrawIndexedPrimitive` calls:
//!
//! - The active level of detail is `mesh+0x80`. Each LOD has a fixed-stride
//!   (`0x18`) record pair: a **draw-group** header at `mesh+0x20 + lod*0x18`
//!   (its `+0x10` is the `DrawIndexedPrimitive` vertex type) and a **prim-list
//!   head** pointer at `mesh+0x34 + lod*0x18`.
//! - The prim list is singly linked through each record's `+0x18`. Each prim
//!   record carries the resource ordinal (`+0x00`, `-1` = untextured), the
//!   vertex count (`+0x08`) and buffer (`+0x0c`), and the index count (`+0x10`)
//!   and buffer (`+0x14`) - the five `DrawIndexedPrimitive` arguments after the
//!   primitive type.
//! - Each prim is drawn as `DrawIndexedPrimitive(D3DPT_TRIANGLELIST, vertex_type,
//!   vbuf, vcount, ibuf, icount, D3DDP_DONOTUPDATEEXTENTS)`, subject to the
//!   translucent per-prim gate [`crate::c3d_scene_graph::node_prims_draw`], and
//!   fog is baked per prim when the global fog flag is on and the vertex type is
//!   not `1`.
//!
//! This module recovers that layout and the parameter assembly as pure data; the
//! buffers themselves (`vertex_buffer`/`index_buffer`) are opaque runtime handles
//! produced by the batch builder (see [`crate::c3d_batch`]).
//!
//! # Runtime mesh-record assembly (the loader node loop)
//!
//! The draw-time layout above is populated when the C3D loader `FUN_004a2b70`
//! instantiates a mesh. For each on-disk group-1 mesh record it allocates a
//! `0xa4`-byte runtime record (`FUN_004ae1d0`) and fills:
//!
//! - `mesh+0x0c` the vertex count and `mesh+0x10` the triangle count (from the
//!   group-1 `+0x10`/`+0x12` fields). The triangle count is **doubled** for the
//!   two-sided menu mode (`3`) when the renderer reports no hardware two-sided
//!   handling (`renderer+0x28 == 0`) - see [`runtime_face_count`].
//! - `mesh+0x18` the runtime triangle array (stride `0x1c`), each entry built
//!   from one on-disk face: see [`RuntimeTriangle`].
//! - `mesh+0x00` the dequantized/staging vertex array (stride `0x20`), each
//!   component `raw * 2^-16`. For the stage/car modes (`5`/`6`/`7`/`8`) the
//!   first two LODs' vertex streams are also preserved at `mesh+0x04` (LOD 0)
//!   and `mesh+0x08` (LOD 1) - see [`preserves_lod_vertex_streams`] and
//!   [`lod_stream_backup_offset`].
//!
//! The per-LOD finalize `FUN_004b1e10` then writes the draw-group vertex type
//! (`1` vs `2`, see [`vertex_type_has_per_vertex_diffuse`]) and the prim list is
//! built by `FUN_004b2340`, whose group-by-material-run / vertex-dedup / compact
//! `u16` index topology is **exactly** [`crate::c3d_batch::plan_consecutive_batches`]
//! keyed on each face's material ordinal - that builder is reused here rather
//! than re-derived.
//!
//! Decisive constraint: `FUN_004b2340` allocates the vbuf, ibuf and per-vertex
//! color buffer from the system-memory arena `FUN_004990d0`, not from a Direct3D
//! vertex/index buffer. C3D geometry is therefore drawn from system-memory
//! streams (user-pointer draw calls); the loader creates a Direct3D resource
//! only for textures (`FUN_004b01d0`). The compatibility interface models
//! geometry as plain slices, which is why the software path consumes
//! [`crate::c3d_batch`] vertices/indices directly.
//!
//! See evidence `rea.official-eu.c3d-rasterizer`, `rea.official-eu.c3d-mesh-instantiate`
//! and `impl.c3d-mesh-draw`.

use crate::c3d_render::{DP_DONOTUPDATEEXTENTS, PRIMTYPE_TRIANGLELIST, VERTEXTYPE_LVERTEX};

/// `mesh+0x80`: the active level-of-detail index selecting the draw-group pair.
pub const MESH_LOD_INDEX_OFFSET: usize = 0x80;
/// Base of the per-LOD draw-group header table (`mesh+0x20 + lod*stride`).
pub const MESH_DRAW_GROUP_BASE: usize = 0x20;
/// Base of the per-LOD prim-list-head table (`mesh+0x34 + lod*stride`).
pub const MESH_PRIM_HEAD_BASE: usize = 0x34;
/// Stride between successive LOD records in both tables.
pub const MESH_LOD_STRIDE: usize = 0x18;
/// Offset of the `DrawIndexedPrimitive` vertex type within a draw-group header.
pub const DRAW_GROUP_VERTEX_TYPE_OFFSET: usize = 0x10;

/// Prim record: resource ordinal (`-1` = untextured).
pub const PRIM_RESOURCE_ORDINAL_OFFSET: usize = 0x00;
/// Prim record: vertex count.
pub const PRIM_VERTEX_COUNT_OFFSET: usize = 0x08;
/// Prim record: vertex buffer handle.
pub const PRIM_VERTEX_BUFFER_OFFSET: usize = 0x0c;
/// Prim record: index count.
pub const PRIM_INDEX_COUNT_OFFSET: usize = 0x10;
/// Prim record: index buffer handle.
pub const PRIM_INDEX_BUFFER_OFFSET: usize = 0x14;
/// Prim record: next-prim link (singly linked list).
pub const PRIM_NEXT_OFFSET: usize = 0x18;

/// The sentinel resource ordinal marking an untextured prim (`prim+0x00 == -1`).
pub const PRIM_UNTEXTURED_ORDINAL: i32 = -1;

/// Byte offset of a LOD's draw-group header within the mesh.
pub fn draw_group_offset(lod: u8) -> usize {
    MESH_DRAW_GROUP_BASE + usize::from(lod) * MESH_LOD_STRIDE
}

/// Byte offset of a LOD's prim-list-head pointer within the mesh.
pub fn prim_head_offset(lod: u8) -> usize {
    MESH_PRIM_HEAD_BASE + usize::from(lod) * MESH_LOD_STRIDE
}

/// One prim record's draw fields (the five `DrawIndexedPrimitive` arguments after
/// the primitive type, plus the resource ordinal that selects the texture bind).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimBatch {
    /// `prim+0x00`: resource ordinal, or [`PRIM_UNTEXTURED_ORDINAL`].
    pub resource_ordinal: i32,
    /// `prim+0x08`: vertex count.
    pub vertex_count: u32,
    /// `prim+0x0c`: opaque vertex-buffer handle.
    pub vertex_buffer: u32,
    /// `prim+0x10`: index count.
    pub index_count: u32,
    /// `prim+0x14`: opaque index-buffer handle.
    pub index_buffer: u32,
}

impl PrimBatch {
    /// Whether this prim is untextured (its ordinal is the `-1` sentinel).
    pub fn is_untextured(&self) -> bool {
        self.resource_ordinal == PRIM_UNTEXTURED_ORDINAL
    }
}

/// The resolved `DrawIndexedPrimitive` call for one prim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrawIndexedPrimitive {
    /// `D3DPRIMITIVETYPE` - always `D3DPT_TRIANGLELIST`.
    pub primitive_type: u32,
    /// `D3DVERTEXTYPE` from the LOD draw-group header (`+0x10`).
    pub vertex_type: u32,
    /// The vertex buffer and its count.
    pub vertex_buffer: u32,
    pub vertex_count: u32,
    /// The index buffer and its count.
    pub index_buffer: u32,
    pub index_count: u32,
    /// `dwFlags` - always `D3DDP_DONOTUPDATEEXTENTS`.
    pub flags: u32,
}

/// Assembles the `DrawIndexedPrimitive` call for one prim under a LOD draw-group's
/// `vertex_type` (`FUN_004a28a0`: `DrawIndexedPrimitive(TRIANGLELIST, vertex_type,
/// prim.vbuf, prim.vcount, prim.ibuf, prim.icount, DONOTUPDATEEXTENTS)`).
pub fn draw_call(vertex_type: u32, prim: &PrimBatch) -> DrawIndexedPrimitive {
    DrawIndexedPrimitive {
        primitive_type: PRIMTYPE_TRIANGLELIST,
        vertex_type,
        vertex_buffer: prim.vertex_buffer,
        vertex_count: prim.vertex_count,
        index_buffer: prim.index_buffer,
        index_count: prim.index_count,
        flags: DP_DONOTUPDATEEXTENTS,
    }
}

/// Whether per-vertex fog is baked into this prim before drawing (`FUN_004a28a0`:
/// the global fog query `FUN_004b01c0()` is set and the draw-group vertex type is
/// not `1`). The vertex type `1` (pre-lit/unfogged) skips the fog bake.
pub fn prim_takes_fog(global_fog_enabled: bool, vertex_type: u32) -> bool {
    global_fog_enabled && vertex_type != 1
}

/// Whether a draw-group vertex type carries per-vertex diffuse color unpacked
/// from the triangle's color bytes. The lit vertex type (`2`, [`VERTEXTYPE_LVERTEX`])
/// does; the pre-lit type (`1`) instead forces an opaque specular (`0xff000000`)
/// and rescales only the UVs (`FUN_004b1e10`), which is also why type `1` skips
/// the fog bake in [`prim_takes_fog`].
pub fn vertex_type_has_per_vertex_diffuse(vertex_type: u32) -> bool {
    vertex_type == VERTEXTYPE_LVERTEX
}

// --- Runtime mesh-record assembly (the loader node loop) ---

/// Loader mode selecting a two-sided menu mesh (`FUN_004a2b70` `param_2 == 3`).
pub const C3D_MODE_MENU_TWO_SIDED: i32 = 3;

/// `mesh+0x00`: dequantized/staging vertex array (`vertex_count * 0x20` bytes).
pub const MESH_STAGING_VERTEX_ARRAY_OFFSET: usize = 0x00;
/// `mesh+0x04`: preserved LOD-0 vertex stream (stage/car modes only).
pub const MESH_LOD0_VERTEX_STREAM_OFFSET: usize = 0x04;
/// `mesh+0x08`: preserved LOD-1 vertex stream (stage/car modes only).
pub const MESH_LOD1_VERTEX_STREAM_OFFSET: usize = 0x08;
/// `mesh+0x0c`: vertex count (from group-1 `+0x10`).
pub const MESH_VERTEX_COUNT_OFFSET: usize = 0x0c;
/// `mesh+0x10`: triangle count (from group-1 `+0x12`, doubled when two-sided).
pub const MESH_FACE_COUNT_OFFSET: usize = 0x10;
/// `mesh+0x18`: runtime triangle array (`triangle_count * 0x1c` bytes).
pub const MESH_TRIANGLE_ARRAY_OFFSET: usize = 0x18;
/// `mesh+0x81`: number of LODs stored in the record.
pub const MESH_LOD_COUNT_OFFSET: usize = 0x81;

/// Stride of one vertex in the staging and preserved LOD vertex streams.
pub const MESH_VERTEX_STRIDE: usize = 0x20;
/// Stride of one runtime triangle record in the `mesh+0x18` array.
pub const MESH_TRIANGLE_STRIDE: usize = 0x1c;

/// Runtime triangle record: triangle flag/color word.
pub const RUNTIME_TRI_FLAG_OFFSET: usize = 0x00;
/// Runtime triangle record: material ordinal (`-1` = untextured).
pub const RUNTIME_TRI_MATERIAL_OFFSET: usize = 0x04;
/// Runtime triangle record: first per-vertex ARGB color.
pub const RUNTIME_TRI_COLOR0_OFFSET: usize = 0x08;
/// Runtime triangle record: second per-vertex ARGB color.
pub const RUNTIME_TRI_COLOR1_OFFSET: usize = 0x0c;
/// Runtime triangle record: third per-vertex ARGB color.
pub const RUNTIME_TRI_COLOR2_OFFSET: usize = 0x10;
/// Runtime triangle record: first vertex index.
pub const RUNTIME_TRI_INDEX0_OFFSET: usize = 0x14;
/// Runtime triangle record: second vertex index.
pub const RUNTIME_TRI_INDEX1_OFFSET: usize = 0x16;
/// Runtime triangle record: third vertex index.
pub const RUNTIME_TRI_INDEX2_OFFSET: usize = 0x18;

/// The sentinel material ordinal marking an untextured runtime triangle, which
/// takes the per-mode default diffuse instead of a texture bind.
pub const RUNTIME_TRI_UNTEXTURED_MATERIAL: i32 = -1;

/// One runtime triangle record (`mesh+0x18` array, stride `0x1c`), built from an
/// on-disk face (entry24) in the loader node loop. The prim list that the draw
/// walker expands is grouped from this array by [`material_ref`](Self::material_ref)
/// via [`crate::c3d_batch::plan_consecutive_batches`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeTriangle {
    /// `+0x00`: triangle flag/color word.
    pub flag: u16,
    /// `+0x04`: material ordinal, or [`RUNTIME_TRI_UNTEXTURED_MATERIAL`].
    pub material_ref: i32,
    /// `+0x08`/`+0x0c`/`+0x10`: the three per-vertex ARGB colors.
    pub vertex_colors: [u32; 3],
    /// `+0x14`/`+0x16`/`+0x18`: the three vertex indices into the mesh vertices.
    pub vertex_indices: [u16; 3],
}

impl RuntimeTriangle {
    /// Whether this triangle is untextured (its ordinal is the `-1` sentinel).
    pub fn is_untextured(&self) -> bool {
        self.material_ref == RUNTIME_TRI_UNTEXTURED_MATERIAL
    }
}

/// Whether the loader duplicates each face to make the mesh two-sided. Only the
/// two-sided menu mode (`3`) does so, and only when the renderer reports no
/// hardware two-sided handling (`renderer+0x28 == 0`, i.e. `renderer_backface_flag`
/// is `0`).
pub fn mode_duplicates_back_faces(mode: i32, renderer_backface_flag: u32) -> bool {
    mode == C3D_MODE_MENU_TWO_SIDED && renderer_backface_flag == 0
}

/// The runtime triangle-array length: the group-1 face count, doubled when
/// [`mode_duplicates_back_faces`] holds.
pub fn runtime_face_count(group1_face_count: u16, mode: i32, renderer_backface_flag: u32) -> usize {
    let base = usize::from(group1_face_count);
    if mode_duplicates_back_faces(mode, renderer_backface_flag) {
        base * 2
    } else {
        base
    }
}

/// Whether this loader mode preserves the first two LODs' vertex streams in the
/// mesh record (`mesh+0x04`/`+0x08`). The stage/car tree-builder modes
/// (`5`/`6`/`7`/`8`) do; the menu/instance modes (`0`/`1`/`2`/`3`) do not.
pub fn preserves_lod_vertex_streams(mode: i32) -> bool {
    matches!(mode, 5..=8)
}

/// The mesh-record offset that preserves this LOD's vertex stream, when
/// [`preserves_lod_vertex_streams`] holds for the mode. Only LOD `0`
/// (`mesh+0x04`) and LOD `1` (`mesh+0x08`) are backed up; higher LODs are not.
pub fn lod_stream_backup_offset(lod: u8) -> Option<usize> {
    match lod {
        0 => Some(MESH_LOD0_VERTEX_STREAM_OFFSET),
        1 => Some(MESH_LOD1_VERTEX_STREAM_OFFSET),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c3d_render::{DP_DONOTUPDATEEXTENTS, PRIMTYPE_TRIANGLELIST};

    #[test]
    fn lod_table_offsets_step_by_the_record_stride() {
        assert_eq!(draw_group_offset(0), 0x20);
        assert_eq!(draw_group_offset(1), 0x38);
        assert_eq!(prim_head_offset(0), 0x34);
        assert_eq!(prim_head_offset(1), 0x4c);
        // The draw-group header and prim-list head for a LOD are 0x14 apart.
        assert_eq!(prim_head_offset(3) - draw_group_offset(3), 0x14);
    }

    #[test]
    fn draw_call_maps_the_prim_fields_to_draw_indexed_primitive() {
        let prim = PrimBatch {
            resource_ordinal: 7,
            vertex_count: 24,
            vertex_buffer: 0x1000,
            index_count: 36,
            index_buffer: 0x2000,
        };
        let call = draw_call(2, &prim);
        assert_eq!(
            call,
            DrawIndexedPrimitive {
                primitive_type: PRIMTYPE_TRIANGLELIST,
                vertex_type: 2,
                vertex_buffer: 0x1000,
                vertex_count: 24,
                index_buffer: 0x2000,
                index_count: 36,
                flags: DP_DONOTUPDATEEXTENTS,
            }
        );
        assert!(!prim.is_untextured());
    }

    #[test]
    fn the_minus_one_ordinal_marks_an_untextured_prim() {
        let prim = PrimBatch {
            resource_ordinal: PRIM_UNTEXTURED_ORDINAL,
            vertex_count: 3,
            vertex_buffer: 1,
            index_count: 3,
            index_buffer: 2,
        };
        assert!(prim.is_untextured());
    }

    #[test]
    fn fog_is_baked_unless_disabled_or_vertex_type_is_one() {
        assert!(prim_takes_fog(true, 2));
        assert!(!prim_takes_fog(false, 2)); // global fog off
        assert!(!prim_takes_fog(true, 1)); // pre-lit vertex type skips fog
    }

    #[test]
    fn only_the_lit_vertex_type_carries_per_vertex_diffuse() {
        assert!(vertex_type_has_per_vertex_diffuse(2)); // VERTEXTYPE_LVERTEX
        assert!(!vertex_type_has_per_vertex_diffuse(1)); // pre-lit forces opaque
    }

    #[test]
    fn only_two_sided_menu_mode_doubles_faces_and_only_without_hw_two_sided() {
        // Mode 3 with no hardware two-sided handling (flag 0) doubles.
        assert!(mode_duplicates_back_faces(C3D_MODE_MENU_TWO_SIDED, 0));
        assert_eq!(runtime_face_count(10, C3D_MODE_MENU_TWO_SIDED, 0), 20);
        // Mode 3 but the renderer handles two-sided: no doubling.
        assert!(!mode_duplicates_back_faces(C3D_MODE_MENU_TWO_SIDED, 1));
        assert_eq!(runtime_face_count(10, C3D_MODE_MENU_TWO_SIDED, 1), 10);
        // Other modes never double, regardless of the renderer flag.
        for mode in [0, 1, 2, 5, 6, 7, 8] {
            assert!(!mode_duplicates_back_faces(mode, 0));
            assert_eq!(runtime_face_count(7, mode, 0), 7);
        }
    }

    #[test]
    fn runtime_triangle_untextured_sentinel() {
        let tri = RuntimeTriangle {
            flag: 0,
            material_ref: 5,
            vertex_colors: [0xff00_0000, 0xff00_0000, 0xff00_0000],
            vertex_indices: [0, 1, 2],
        };
        assert!(!tri.is_untextured());
        let untextured = RuntimeTriangle {
            material_ref: RUNTIME_TRI_UNTEXTURED_MATERIAL,
            ..tri
        };
        assert!(untextured.is_untextured());
    }

    #[test]
    fn only_stage_and_car_modes_preserve_lod_vertex_streams() {
        for mode in [5, 6, 7, 8] {
            assert!(preserves_lod_vertex_streams(mode));
        }
        for mode in [0, 1, 2, 3] {
            assert!(!preserves_lod_vertex_streams(mode));
        }
    }

    #[test]
    fn only_the_first_two_lods_back_up_their_vertex_stream() {
        assert_eq!(lod_stream_backup_offset(0), Some(0x04));
        assert_eq!(lod_stream_backup_offset(1), Some(0x08));
        assert_eq!(lod_stream_backup_offset(2), None);
        assert_eq!(lod_stream_backup_offset(255), None);
    }
}
