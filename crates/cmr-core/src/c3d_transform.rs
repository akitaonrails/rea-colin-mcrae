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
}
