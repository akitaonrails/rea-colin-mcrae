//! C3D scene-graph world-transform propagation (reconstruction item E07/E08).
//!
//! The loader (`FUN_004a2b70`, see [`crate::c3d_scene_graph`]) stores only each
//! node's **local** matrix (node `+0x54`/`+0x94`); the per-frame world matrix is
//! computed by a separate pre-pass that walks the tree. Two shipped routines
//! implement the same rule - `FUN_00492520` (the recursive sibling/child walk)
//! and `FUN_00492190` (the single-node form) - both writing the composed world
//! matrix to node `+0xd4`:
//!
//! ```text
//! parent_class == 1000            -> world = local            (a world-root parent)
//! node +0x188 != 0                -> world = parent_world     (inherit the parent world)
//! otherwise                       -> world = local * parent_world   (FUN_0048ecb0)
//! ```
//!
//! where `*` is the Q16.16 matrix concatenation [`matmul_q16`] under the
//! row-vector convention (`world = apply local, then parent_world`). The focus
//! entity (`DAT_00b77f4c`) additionally has its world rotation reset to identity
//! (translation kept) - an axis-aligned/billboard transform. The draw walker
//! `FUN_004a28a0` submits a copy of this world matrix (node `+0x114`) as the
//! Direct3D `WORLD` transform.
//!
//! The gates `FUN_00492190` applies before recomputing (`node +0x180` kind in
//! `0..=3`, and a dirty flag `node +0x170 == 1` on the node or its parent) are
//! runtime scheduling concerns; this reconstruction computes the transform for
//! every node and leaves the dirty-flag bookkeeping to the caller.
//!
//! See evidence `rea.official-eu.c3d-world-transform`.

use crate::c3d_camera::{MATRIX_IDENTITY_Q16, matmul_q16};
use cmr_formats::c3d::C3dGroup0Record;

/// The parent class value (`node+0x2c == 1000`) marking a *world-root* parent:
/// a child of such a parent takes its local matrix directly as its world matrix
/// (no concatenation), so the subtree is positioned in world space already.
pub const WORLD_ROOT_CLASS: i32 = 1000;

/// One node's inputs to world-transform propagation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeWorldInput {
    /// The parent's index (earlier in the slice), or `None` for a node attached
    /// under the external container (its world composes against `root_world`).
    pub parent: Option<usize>,
    /// The node's local Q16.16 matrix (node `+0x94`).
    pub local: [i32; 16],
    /// Whether the parent carries [`WORLD_ROOT_CLASS`] (`parent+0x2c == 1000`):
    /// the node's world is its local matrix, not `local * parent_world`.
    pub parent_is_world_root: bool,
    /// Whether the node inherits the parent world verbatim (`node+0x188 != 0`):
    /// the node's world is the parent world, ignoring its local matrix.
    pub inherit_parent_world: bool,
    /// Whether this is the focus entity (`DAT_00b77f4c`): its world rotation is
    /// reset to identity (translation kept) after composition.
    pub is_focus: bool,
}

impl NodeWorldInput {
    /// A plain node that composes `local * parent_world` with no special casing.
    pub fn new(parent: Option<usize>, local: [i32; 16]) -> Self {
        NodeWorldInput {
            parent,
            local,
            parent_is_world_root: false,
            inherit_parent_world: false,
            is_focus: false,
        }
    }
}

/// Resets the upper-left 3x3 rotation of a Q16.16 matrix to identity while
/// leaving the translation row and the homogeneous column untouched
/// (`FUN_00492190`'s focus-entity branch).
fn reset_rotation_to_identity(m: &mut [i32; 16]) {
    const ONE: i32 = 0x1_0000;
    m[0] = ONE;
    m[1] = 0;
    m[2] = 0;
    m[4] = 0;
    m[5] = ONE;
    m[6] = 0;
    m[8] = 0;
    m[9] = 0;
    m[10] = ONE;
}

/// Propagates world matrices down a scene tree (`FUN_00492190`/`FUN_00492520`).
///
/// `nodes` is the tree in parent-before-child order (as the loader emits it, and
/// as [`crate::c3d_scene_graph::SceneGraph::assemble`] requires); `root_world`
/// is the world matrix of the external container the roots hang under (identity
/// for a stage loaded at the origin). Returns each node's world matrix in the
/// same order, so `world[i]` is the matrix the draw walker submits for node `i`.
///
/// # Panics (debug only)
///
/// Debug-asserts that every parent index precedes its child; a forward
/// reference would read an uncomputed parent world. Callers build `nodes` from
/// the same validated stream as `assemble`, which already rejects such input.
pub fn propagate_world_transforms(
    nodes: &[NodeWorldInput],
    root_world: &[i32; 16],
) -> Vec<[i32; 16]> {
    let mut world = vec![MATRIX_IDENTITY_Q16; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        let parent_world = match node.parent {
            Some(parent) => {
                debug_assert!(parent < index, "parent must precede its child");
                world[parent]
            }
            None => *root_world,
        };
        let mut composed = if node.parent_is_world_root {
            node.local
        } else if node.inherit_parent_world {
            parent_world
        } else {
            matmul_q16(&node.local, &parent_world)
        };
        if node.is_focus {
            reset_rotation_to_identity(&mut composed);
        }
        world[index] = composed;
    }
    world
}

/// Computes the world matrix of every node in a parsed C3D scene.
///
/// This bridges the on-disk group-0 node stream (as parsed by
/// [`cmr_formats::c3d`]) to [`propagate_world_transforms`]: each record supplies
/// its local matrix ([`C3dGroup0Record::affine_matrix_q16`]) and its parent link
/// ([`C3dGroup0Record::parent_index`], an absolute backward index, `None` for a
/// root). The `parent_is_world_root` short-circuit is derived here from the
/// parent record's class ([`C3dGroup0Record::node_class`] `== `[`WORLD_ROOT_CLASS`]).
///
/// The runtime-only toggles are left at their defaults for a static scene
/// snapshot: `inherit_parent_world` (node `+0x188`) and `is_focus`
/// (`DAT_00b77f4c`) are per-frame animation state the loader does not record, so
/// a freshly loaded scene composes `local * parent_world` everywhere except
/// under a world-root parent. `root_world` is the external container the roots
/// hang under (pass [`MATRIX_IDENTITY_Q16`] for a stage loaded at the origin).
///
/// Records must arrive in parent-before-child order (as the loader emits them);
/// the returned vector is in the same order, so `world[i]` is node `i`'s matrix.
pub fn scene_world_transforms<'a>(
    records: impl IntoIterator<Item = C3dGroup0Record<'a>>,
    root_world: &[i32; 16],
) -> Vec<[i32; 16]> {
    scene_world_transforms_with_locals(records, root_world, &[])
}

/// [`scene_world_transforms`] with some records' local matrices replaced, as
/// the render interpolation `FUN_00439ee0` poses the car's wheel nodes before
/// the scene is drawn. `overrides` pairs a record index with its local matrix.
#[must_use]
pub fn scene_world_transforms_with_locals<'a>(
    records: impl IntoIterator<Item = C3dGroup0Record<'a>>,
    root_world: &[i32; 16],
    overrides: &[(usize, [i32; 16])],
) -> Vec<[i32; 16]> {
    let records: Vec<C3dGroup0Record<'a>> = records.into_iter().collect();
    let inputs: Vec<NodeWorldInput> = records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            let parent = record.parent_index();
            let parent_is_world_root = parent
                .and_then(|p| records.get(p))
                .is_some_and(|p| p.node_class() == WORLD_ROOT_CLASS as u32);
            let local = overrides
                .iter()
                .find(|(target, _)| *target == index)
                .map_or_else(|| record.affine_matrix_q16(), |(_, local)| *local);
            NodeWorldInput {
                parent,
                local,
                parent_is_world_root,
                inherit_parent_world: false,
                is_focus: false,
            }
        })
        .collect();
    propagate_world_transforms(&inputs, root_world)
}

/// Margin used by the native spatial-section neighbor test (`FUN_00490bb0`).
pub const SECTION_NEIGHBOR_MARGIN: i32 = 0x4_8000;

/// Compute the two neighboring section links and the unused `-1` sentinel.
/// The native margin appears on both sides of each comparison and cancels, so
/// the cell-center sign determines the transition. Link zero is the Z neighbor;
/// link one is the diagonal with the X direction applied.
#[must_use]
pub fn section_neighbors(
    x: i32,
    z: i32,
    center_x: i32,
    center_z: i32,
    section: i16,
    row_count: i16,
) -> [i16; 3] {
    let x_direction = if x < center_x {
        -1
    } else if x >= center_x {
        1
    } else {
        0
    };
    let z_direction = if center_z < z { -1 } else { 1 };
    let z_neighbor = section + z_direction * row_count;
    [z_neighbor, z_neighbor + x_direction, -1]
}

/// Move an object to the tail of a different section, preserving every other
/// object's order. Invalid targets and same-section moves leave lists unchanged.
pub fn relocate_section(sections: &mut [Vec<usize>], object: usize, target: usize) -> bool {
    let Some(source) = sections
        .iter()
        .position(|objects| objects.contains(&object))
    else {
        return false;
    };
    if source == target || target >= sections.len() {
        return false;
    }
    let position = sections[source]
        .iter()
        .position(|&candidate| candidate == object)
        .unwrap();
    sections[source].remove(position);
    sections[target].push(object);
    true
}

/// Mark a node and every ancestor dirty (`FUN_00492050`). The bounded walk
/// rejects malformed cycles rather than looping forever.
pub fn mark_world_dirty(dirty: &mut [bool], node: usize, parents: &[Option<usize>]) {
    let mut current = Some(node);
    for _ in 0..=parents.len() {
        let Some(index) = current else {
            return;
        };
        if index >= dirty.len() {
            return;
        }
        dirty[index] = true;
        current = parents.get(index).copied().flatten();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: i32 = 0x1_0000;

    fn translate(x: i32, y: i32, z: i32) -> [i32; 16] {
        let mut m = MATRIX_IDENTITY_Q16;
        m[12] = x;
        m[13] = y;
        m[14] = z;
        m
    }

    #[test]
    fn a_root_takes_the_container_world() {
        // With an identity container, a root's world is its own local matrix.
        let nodes = [NodeWorldInput::new(None, translate(2 * ONE, 0, 0))];
        let world = propagate_world_transforms(&nodes, &MATRIX_IDENTITY_Q16);
        assert_eq!(world[0], translate(2 * ONE, 0, 0));
        // A translated container shifts the root by the container translation.
        let world = propagate_world_transforms(&nodes, &translate(0, 5 * ONE, 0));
        assert_eq!(world[0], translate(2 * ONE, 5 * ONE, 0));
    }

    #[test]
    fn a_child_composes_local_then_parent_world() {
        // Parent at +x=3, child local +z=4: child world translation = (3,0,4).
        let nodes = [
            NodeWorldInput::new(None, translate(3 * ONE, 0, 0)),
            NodeWorldInput::new(Some(0), translate(0, 0, 4 * ONE)),
        ];
        let world = propagate_world_transforms(&nodes, &MATRIX_IDENTITY_Q16);
        assert_eq!(world[1], translate(3 * ONE, 0, 4 * ONE));
    }

    #[test]
    fn a_three_level_chain_accumulates_translation() {
        let nodes = [
            NodeWorldInput::new(None, translate(ONE, 0, 0)),
            NodeWorldInput::new(Some(0), translate(0, ONE, 0)),
            NodeWorldInput::new(Some(1), translate(0, 0, ONE)),
        ];
        let world = propagate_world_transforms(&nodes, &MATRIX_IDENTITY_Q16);
        assert_eq!(world[2], translate(ONE, ONE, ONE));
    }

    #[test]
    fn a_world_root_parent_makes_the_child_use_its_local_matrix() {
        let mut child = NodeWorldInput::new(Some(0), translate(0, 0, 4 * ONE));
        child.parent_is_world_root = true;
        let nodes = [NodeWorldInput::new(None, translate(3 * ONE, 0, 0)), child];
        let world = propagate_world_transforms(&nodes, &MATRIX_IDENTITY_Q16);
        // Parent's +x=3 is ignored; the child's world is its own local matrix.
        assert_eq!(world[1], translate(0, 0, 4 * ONE));
    }

    #[test]
    fn inherit_parent_world_ignores_the_local_matrix() {
        let mut child = NodeWorldInput::new(Some(0), translate(9 * ONE, 9 * ONE, 9 * ONE));
        child.inherit_parent_world = true;
        let nodes = [NodeWorldInput::new(None, translate(3 * ONE, 0, 0)), child];
        let world = propagate_world_transforms(&nodes, &MATRIX_IDENTITY_Q16);
        // The child's local is discarded; it takes the parent world verbatim.
        assert_eq!(world[1], translate(3 * ONE, 0, 0));
    }

    #[test]
    fn the_focus_entity_resets_rotation_but_keeps_translation() {
        // A +90-deg-about-Z local with a translation; parent identity.
        let mut local = translate(2 * ONE, 3 * ONE, 0);
        local[0] = 0;
        local[1] = ONE;
        local[4] = -ONE;
        local[5] = 0;
        let mut node = NodeWorldInput::new(None, local);
        node.is_focus = true;
        let world = propagate_world_transforms(&[node], &MATRIX_IDENTITY_Q16);
        // Rotation is reset to identity; translation (2,3,0) survives.
        assert_eq!(world[0], translate(2 * ONE, 3 * ONE, 0));
    }

    #[test]
    fn a_non_focus_node_keeps_its_rotation() {
        let mut local = MATRIX_IDENTITY_Q16;
        local[0] = 0;
        local[1] = ONE;
        local[4] = -ONE;
        local[5] = 0;
        let world =
            propagate_world_transforms(&[NodeWorldInput::new(None, local)], &MATRIX_IDENTITY_Q16);
        assert_eq!(world[0], local);
    }

    /// Minimal valid `CM3D` buffer whose Group0 table holds `records` as
    /// `(field_04, node_class, local_matrix)` tuples (group1/blob empty, no
    /// mesh-index values). Mirrors the scene-graph test fixture shape.
    fn c3d_group0_matrix_bytes(records: &[(i16, u32, [i32; 16])]) -> Vec<u8> {
        let mut body = Vec::new();
        for &(field_04, class, matrix) in records {
            let declared = 80usize; // header only, value_count == 0
            let mut record = vec![0u8; declared];
            record[0..4].copy_from_slice(&(declared as u32).to_le_bytes());
            record[4..6].copy_from_slice(&field_04.to_le_bytes());
            for (index, component) in matrix.iter().enumerate() {
                let start = 0x08 + index * 4;
                record[start..start + 4].copy_from_slice(&component.to_le_bytes());
            }
            record[0x48..0x4c].copy_from_slice(&class.to_le_bytes());
            body.extend_from_slice(&record);
        }
        let mut buf = vec![0u8; 32];
        buf[0..4].copy_from_slice(b"CM3D");
        buf[4..8].copy_from_slice(&100u32.to_le_bytes());
        buf[12..14].copy_from_slice(&(records.len() as u16).to_le_bytes());
        buf[20..24].copy_from_slice(&32u32.to_le_bytes()); // group0 at offset 32
        let end = (32 + body.len()) as u32;
        buf[24..28].copy_from_slice(&end.to_le_bytes()); // empty group1 at end
        buf[28..32].copy_from_slice(&end.to_le_bytes()); // empty blob at end
        buf.extend_from_slice(&body);
        buf
    }

    fn parse_group0(bytes: &[u8]) -> cmr_formats::c3d::C3dFile {
        use cmr_formats::c3d::{C3dLimits, parse_c3d};
        parse_c3d(
            bytes,
            C3dLimits {
                max_payload_bytes: 4096,
                max_group0_records: 16,
                max_group1_records: 16,
                max_blob_records: 16,
            },
        )
        .unwrap()
    }

    #[test]
    fn scene_world_transforms_composes_a_parsed_child_against_its_parent() {
        // Root at +x=3 (plain class), child local +z=4 (plain class, parent 0).
        let bytes = c3d_group0_matrix_bytes(&[
            (-1, 0x10, translate(3 * ONE, 0, 0)),
            (0, 0x20, translate(0, 0, 4 * ONE)),
        ]);
        let file = parse_group0(&bytes);
        let world = scene_world_transforms(file.group0_records(), &MATRIX_IDENTITY_Q16);
        assert_eq!(world[0], translate(3 * ONE, 0, 0));
        assert_eq!(world[1], translate(3 * ONE, 0, 4 * ONE));
    }

    #[test]
    fn spatial_neighbors_and_relink_follow_section_transitions() {
        assert_eq!(section_neighbors(1, 1, 0, 0, 5, 8), [-3, -2, -1]);
        assert_eq!(section_neighbors(-1, -1, 0, 0, 5, 8), [13, 12, -1]);
        assert_eq!(section_neighbors(-1, 1, 0, 0, 5, 8), [-3, -4, -1]);
        assert_eq!(section_neighbors(1, -1, 0, 0, 5, 8), [13, 14, -1]);

        let mut sections = vec![vec![0, 1], vec![2], Vec::new()];
        assert!(relocate_section(&mut sections, 1, 2));
        assert_eq!(sections, vec![vec![0], vec![2], vec![1]]);
        assert!(!relocate_section(&mut sections, 1, 2));
        assert!(!relocate_section(&mut sections, 1, 3));
        assert!(!relocate_section(&mut sections, 99, 0));
    }

    #[test]
    fn dirty_mark_covers_node_and_ancestors() {
        let parents = [None, Some(0), Some(1), Some(0)];
        let mut dirty = [false; 4];
        mark_world_dirty(&mut dirty, 2, &parents);
        assert_eq!(dirty, [true, true, true, false]);
    }

    #[test]
    fn scene_world_transforms_short_circuits_under_a_world_root_parent() {
        // Parent carries WORLD_ROOT_CLASS, so the child takes its local matrix
        // directly, ignoring the parent's +x=3 translation.
        let bytes = c3d_group0_matrix_bytes(&[
            (-1, WORLD_ROOT_CLASS as u32, translate(3 * ONE, 0, 0)),
            (0, 0x20, translate(0, 0, 4 * ONE)),
        ]);
        let file = parse_group0(&bytes);
        let world = scene_world_transforms(file.group0_records(), &MATRIX_IDENTITY_Q16);
        assert_eq!(world[1], translate(0, 0, 4 * ONE));
    }
}
