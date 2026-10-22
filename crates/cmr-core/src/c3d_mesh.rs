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
//! See evidence `rea.official-eu.c3d-rasterizer` and `impl.c3d-mesh-draw`.

use crate::c3d_render::{DP_DONOTUPDATEEXTENTS, PRIMTYPE_TRIANGLELIST};

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
}
