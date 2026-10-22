//! C3D gameplay scene-graph traversal and per-frame draw orchestration
//! (reconstruction item E07).
//!
//! # What this recovers
//!
//! The gameplay 3D renderer walks a scene graph of entity nodes and submits
//! each node's mesh through the Direct3D-IM device for a single render-pass
//! selector per frame. Three shipped routines define the contract:
//!
//! - `FUN_004a07d0` - the per-frame dispatcher: `BeginScene`, then for each
//!   object category it optionally distance-sorts the entity list
//!   (`FUN_004a2070`, [`crate::c3d_render::object_sort_key`]) and emits every
//!   entity through the root emitter; near/flagged entities are deferred into a
//!   bounded list and flushed last under an alpha-blend bracket; `EndScene`.
//! - `FUN_004a2700` - the entity-root emitter: draws the root node's mesh when
//!   it passes the render-pass mask and is not culled, then descends into the
//!   child chain via the recursive walker.
//! - `FUN_004a28a0` - the recursive sibling/child walker: a pass-gated
//!   pre-order traversal with per-node cull and deferred-translucency handling.
//!
//! The traversal below is transcribed directly from the shipped control flow
//! (`FUN_004a28a0` was cross-checked against its disassembly for the
//! translucent-sibling branch). Everything here is backend-neutral (it issues
//! no device calls), so the software oracle (E07) and a later modern backend
//! drive the *same* traversal and pass the *same* tests.
//!
//! See evidence `rea.official-eu.c3d-scene-graph`.
//!
//! ## Decisive corrections to the E06 submission note
//!
//! The dispatcher toggles **no** `ZWRITEENABLE`/`ZENABLE` render state: the
//! opaque-vs-transparent separation is implemented by **deferral** - a bounded
//! near list ([`DEFERRED_NEAR_LIST_CAP`]) drawn last under an
//! `ALPHABLENDENABLE`/`STIPPLEDALPHA` bracket - not by disabling depth writes
//! for a transparent pass. Depth-write state is established at device setup and
//! left alone across the frame. `FOGENABLE` is set for the whole 3D scene (on
//! before `BeginScene`, off after). There is **no pass-index loop**: a single
//! render-pass mask (the emitter's `param_5`) selects which nodes draw this
//! frame.

use cmr_formats::c3d::C3dGroup0Record;

// --- Scene-node layout (byte offsets, observed) -------------------------------

/// `node+0x00` - pointer to the next sibling in the chain (list terminator 0).
pub const NODE_NEXT_SIBLING: usize = 0x00;
/// `node+0x04` - pointer to the first child (subtree root, 0 if a leaf).
pub const NODE_FIRST_CHILD: usize = 0x04;
/// `node+0x0c` - pointer to the node's mesh (0 = no geometry; the walker skips
/// such nodes, the root emitter presumes it non-null).
pub const NODE_MESH: usize = 0x0c;
/// `node+0x2c` - the render/translucency class; [`TRANSLUCENT_CLASS`] marks a
/// deferred-transparent node.
pub const NODE_TRANSLUCENCY_CLASS: usize = 0x2c;
/// `node+0x114` - the node's world matrix, submitted via `SetTransform(WORLD)`.
pub const NODE_WORLD_MATRIX: usize = 0x114;
/// `node+0x180` - the cull/hidden flag; nonzero suppresses this node's own draw
/// (its children are still traversed).
pub const NODE_CULL_FLAG: usize = 0x180;
/// `node+0x184` - the render-pass bitmask byte; bit `p` set means "emit in pass
/// `p`". A node is visited in pass `p` iff `(mask >> p) & 1`.
pub const NODE_RENDER_PASS_MASK: usize = 0x184;

/// The translucency-class sentinel (`node+0x2c == 0x6f`): a deferred-transparent
/// node whose primitives draw only when blending is enabled or the override is
/// set, and which the walker re-enters under a blend-state bracket.
pub const TRANSLUCENT_CLASS: i32 = 0x6f;

// --- Scene-entity (object) layout for the dispatcher's sort/deferral ----------

/// `object+0x104` - the entity's world X, the sort key's X term (`FUN_004a2070`).
pub const OBJECT_SORT_X: usize = 0x104;
/// `object+0x10c` - the entity's world Z, the sort key's Z term (`FUN_004a2070`).
pub const OBJECT_SORT_Z: usize = 0x10c;
/// `object+0x16c` - the per-category entity list thread (next object), the field
/// the distance sort relinks and the dispatcher walks.
pub const OBJECT_LIST_NEXT: usize = 0x16c;

// --- Renderer device-wrapper vtable offsets (observed) ------------------------
//
// The renderer device lives at `PTR_DAT_0050283c + 0x10`; these are the method
// slots the dispatcher and emitters call. They are the game's own device-wrapper
// vtable, not raw `IDirect3DDevice2` slots.

/// `BeginScene()` - opens the frame (dispatcher `FUN_004a07d0`).
pub const DEVICE_BEGIN_SCENE: usize = 0x28;
/// `EndScene()` - closes the frame (dispatcher `FUN_004a07d0`).
pub const DEVICE_END_SCENE: usize = 0x2c;
/// `SetRenderState(state, value)` (dispatcher scene-state toggles).
pub const DEVICE_SET_RENDER_STATE: usize = 0x5c;
/// `SetTransform(state, matrix)` (emitters set `WORLD` per node).
pub const DEVICE_SET_TRANSFORM: usize = 0x68;
/// `DrawIndexedPrimitive(...)` (emitters submit each prim batch).
pub const DEVICE_DRAW_INDEXED_PRIMITIVE: usize = 0x78;

// --- Dispatcher orchestration constants ---------------------------------------

/// The bounded deferred near-object list cap (`local_30 < 0x19` in
/// `FUN_004a07d0`): at most 25 entities are held back for the post-walk
/// transparent flush. This is the same 25 the near/far frustum split records as
/// [`crate::c3d_camera::NEAR_LIST_CAP`].
pub const DEFERRED_NEAR_LIST_CAP: usize = 0x19;

/// The top render-capability level (`DAT_00a23994` ranges 0..=7, read via
/// `FUN_00498af0`). At this level hardware alpha blending is forced off
/// (`FUN_004aa040`).
pub const RENDER_CAP_MAX: u32 = 7;

/// The render-capability level at which the dispatcher distance-sorts every
/// object category (`FUN_00498af0() == 2`).
pub const RENDER_CAP_SORTED: u32 = 2;

/// The render-pass mask applied to every node built from a C3D file: the file
/// carries no mask (`FUN_004a2b70` never writes node `+0x184`), so a load-time
/// node is drawable in all passes until the runtime assigns its real mask.
pub const ALL_RENDER_PASSES: u8 = 0xff;

// --- Capability and per-node draw gates ---------------------------------------

/// Whether hardware alpha blending is currently enabled for translucent geometry
/// (`FUN_004aa040` = `-(cap != 7) & DAT_00bb320c`): true iff the render
/// capability level is not the top value [`RENDER_CAP_MAX`] and the global blend
/// flag is set.
pub fn alpha_blend_enabled(cap: u32, blend_flag: bool) -> bool {
    cap != RENDER_CAP_MAX && blend_flag
}

/// Whether a node's primitives are actually submitted, given its translucency
/// class and the current runtime state - the per-prim gate in both emitters:
/// `(class != 0x6f) || FUN_004aa040() || FUN_00498370()`. Opaque nodes always
/// draw; a translucent node draws only when alpha blending is enabled
/// ([`alpha_blend_enabled`]) or the debug override (`FUN_00498370`,
/// `DAT_00a23988`) is set. A node can be *visited* (its `SetTransform` and prim
/// loop run) yet draw nothing when this gate is false.
pub fn node_prims_draw(class: i32, cap: u32, blend_flag: bool, override_flag: bool) -> bool {
    class != TRANSLUCENT_CLASS || alpha_blend_enabled(cap, blend_flag) || override_flag
}

/// The transparency render state the walker establishes before re-entering a
/// deferred-translucent sibling (`FUN_004a28a0` tail, verified against the
/// disassembly at `0x4a2a59`-`0x4a2a76`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferredTranslucentState {
    /// `FUN_004a2540(3)` - additive blend; chosen when the capability level is
    /// nonzero *and* alpha blending is enabled.
    Additive,
    /// `FUN_004a2290(1)` - enable the transparency state (hardware alpha or the
    /// stippled-alpha fallback); chosen otherwise.
    Enable,
}

/// Selects the deferred-translucent render state (`FUN_004a28a0`):
/// `if cap != 0 && FUN_004aa040() -> additive(3) else enable(1)`.
pub fn deferred_translucent_state(cap: u32, blend_flag: bool) -> DeferredTranslucentState {
    if cap != 0 && alpha_blend_enabled(cap, blend_flag) {
        DeferredTranslucentState::Additive
    } else {
        DeferredTranslucentState::Enable
    }
}

/// Whether the dispatcher distance-sorts an object category before walking it
/// (`FUN_004a07d0`: `FUN_00498af0() == 2 || *(renderer+0x28) == 0`). The sort is
/// the back-to-front relink of [`crate::c3d_render::object_sort_key`].
pub fn should_distance_sort(cap: u32, renderer_flag_0x28: i32) -> bool {
    cap == RENDER_CAP_SORTED || renderer_flag_0x28 == 0
}

/// Whether an entity is held back for the deferred transparent flush
/// (`FUN_004a07d0`: `(int)DAT_00b64374 < DAT_00502774 && local_30 < 0x19`): its
/// near-distance metric is under the threshold and the list is not yet full.
pub fn entity_is_deferred(distance_metric: i32, near_threshold: i32, current_count: usize) -> bool {
    distance_metric < near_threshold && current_count < DEFERRED_NEAR_LIST_CAP
}

// --- Per-object render-state brackets -----------------------------------------
//
// Beyond the scene-wide `FOGENABLE` and the deferred alpha bracket, the
// dispatcher toggles two render states around individual object draws. These
// are the last `FUN_004a07d0` peripheral details; both are transcribed from the
// shipped `SetRenderState` (device vtable `+0x5c`) call sites.

/// The texture-mapping mode the dispatcher selects for an object's draw
/// (`FUN_004a07d0`: `SetRenderState(TEXTUREPERSPECTIVE, 0)` before the inline
/// draw, then `SetRenderState(TEXTUREPERSPECTIVE, *(renderer+0x34))` to restore
/// the global default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexturePerspective {
    /// Perspective correction disabled (affine). Taken only by an inline draw -
    /// an object the primary visibility test (`FUN_004a1c00`) returns `0` for,
    /// i.e. one not held back for the near list. (Inference: these are the more
    /// distant objects, where affine mapping is cheaper and imperceptible.)
    Affine,
    /// The engine's global default (`renderer+0x34`, perspective correction on).
    /// A near object is deferred and drawn later at this default, so it never
    /// takes the affine bracket.
    GlobalDefault,
}

/// The `TEXTUREPERSPECTIVE` value for an object's draw: [`TexturePerspective::Affine`]
/// for an inline (non-deferred) draw, restored to [`TexturePerspective::GlobalDefault`]
/// afterward; a deferred near object draws at the default.
pub fn draw_texture_perspective(is_inline_draw: bool) -> TexturePerspective {
    if is_inline_draw {
        TexturePerspective::Affine
    } else {
        TexturePerspective::GlobalDefault
    }
}

/// Whether the dispatcher force-disables `COLORKEYENABLE` around a single
/// object's draw (`FUN_004a07d0`: `SetRenderState(COLORKEYENABLE, 0)` before,
/// `SetRenderState(COLORKEYENABLE, 1)` after). This per-object override fires
/// only for the distinguished focus entity (`param_3`) at render-capability
/// level [`RENDER_CAP_SORTED`]; every other object keeps the prevailing bracket
/// value. (The focus entity's exact gameplay role - e.g. the viewed car - is
/// not yet pinned; `param_2`/`param_3` are the dispatcher's two special
/// entities.)
pub fn object_colorkey_disabled(cap: u32, is_focus_entity: bool) -> bool {
    cap == RENDER_CAP_SORTED && is_focus_entity
}

// --- Scene-graph traversal ----------------------------------------------------

/// A gameplay scene-graph node - the fields the traversal reads. Pointers are
/// modelled as arena indices into the [`SceneGraph`]'s node slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneNode {
    /// `node+0x00`: next sibling index, or `None` for the chain terminator.
    pub next_sibling: Option<usize>,
    /// `node+0x04`: first child index, or `None` for a leaf.
    pub first_child: Option<usize>,
    /// `node+0x0c != 0`: whether the node carries a mesh to submit.
    pub has_mesh: bool,
    /// `node+0x2c`: the render/translucency class ([`TRANSLUCENT_CLASS`] defers).
    pub translucency_class: i32,
    /// `node+0x180 != 0`: the cull/hidden flag (suppresses this node's own draw).
    pub culled: bool,
    /// `node+0x184`: the render-pass bitmask (bit `p` => emit in pass `p`).
    pub render_pass_mask: u8,
}

impl SceneNode {
    /// A plain opaque leaf that draws in every pass (all mask bits set).
    pub fn leaf() -> Self {
        SceneNode {
            next_sibling: None,
            first_child: None,
            has_mesh: true,
            translucency_class: 0,
            culled: false,
            render_pass_mask: 0xff,
        }
    }

    /// Whether this node is selected by render pass `pass` (`(mask >> pass) & 1`).
    pub fn in_pass(&self, pass: u32) -> bool {
        u32::from(self.render_pass_mask) & (1u32 << (pass & 0x1f)) != 0
    }

    /// Whether this node is translucent/deferred (`class == 0x6f`).
    pub fn is_translucent(&self) -> bool {
        self.translucency_class == TRANSLUCENT_CLASS
    }
}

/// A node submission produced by the traversal: the node was set up for drawing
/// (its `SetTransform(WORLD)` was issued and its primitive loop ran). Whether the
/// individual primitives were actually rasterized is the separate runtime gate
/// [`node_prims_draw`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrawOp {
    /// The emitted node's arena index.
    pub node: usize,
    /// Whether this emission occurred inside a deferred-translucent state
    /// bracket (the node was reached as a translucent sibling).
    pub deferred_translucent: bool,
}

/// A node described for tree assembly: its own traversal fields plus the
/// batch-relative index of its parent. The C3D Group0 transform table projects
/// to an ordered stream of these (`cmr_formats::c3d::C3dGroup0Record`: the
/// matrix/class/flags per record, `parent` from `field_04`), which
/// [`SceneGraph::assemble`] threads into the arena the traversal walks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneNodeDescriptor {
    /// The parent's index in the stream, or `None` for a root/group marker
    /// (`field_04 == -1`).
    pub parent: Option<usize>,
    /// `node+0x0c != 0`: whether the node carries a mesh.
    pub has_mesh: bool,
    /// `node+0x2c`: the render/translucency class.
    pub translucency_class: i32,
    /// `node+0x180 != 0`: the cull/hidden flag.
    pub culled: bool,
    /// `node+0x184`: the render-pass bitmask.
    pub render_pass_mask: u8,
}

/// Why [`SceneGraph::assemble`] rejected a descriptor stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneAssemblyError {
    /// A descriptor names a parent that is not an earlier node in the stream
    /// (a forward or out-of-range index). The shipped loader only ever
    /// references an already-created node, so this is malformed input.
    ForwardParent {
        /// The child descriptor's index.
        node: usize,
        /// The invalid parent index it named.
        parent: usize,
    },
}

impl core::fmt::Display for SceneAssemblyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SceneAssemblyError::ForwardParent { node, parent } => write!(
                f,
                "C3D scene node {node} names parent {parent}, not an earlier node in the stream"
            ),
        }
    }
}

impl std::error::Error for SceneAssemblyError {}

/// An arena of scene-graph nodes. Indices stand in for the shipped node
/// pointers; the graph must be acyclic (a tree, as the shipped scene data is).
#[derive(Clone, Debug, Default)]
pub struct SceneGraph {
    nodes: Vec<SceneNode>,
}

impl SceneGraph {
    /// Builds a graph from its nodes (index order defines the arena layout).
    pub fn new(nodes: Vec<SceneNode>) -> Self {
        SceneGraph { nodes }
    }

    /// Assembles an arena from an ordered descriptor stream, threading each child
    /// onto its parent as the **last** sibling - the exact rule of the shipped
    /// node attach `FUN_00492410`: set the parent's first-child link (`+0x04`) if
    /// it is empty, else walk the next-sibling chain (`+0x00`) to the tail and
    /// link there. Sibling order therefore follows stream order, as it does when
    /// `FUN_004a2b70` emits the C3D transform table. A parent must precede its
    /// children in the stream (the shipped loader references an already-created
    /// node by a batch-relative index); a forward or out-of-range parent is
    /// rejected with the offending indices. Root descriptors (`parent == None`,
    /// the on-disk `field_04 == -1` group markers) are left unlinked - the
    /// dispatcher iterates them through the per-category object list, not the
    /// tree.
    pub fn assemble(descriptors: &[SceneNodeDescriptor]) -> Result<Self, SceneAssemblyError> {
        let mut nodes: Vec<SceneNode> = Vec::with_capacity(descriptors.len());
        for (index, descriptor) in descriptors.iter().enumerate() {
            nodes.push(SceneNode {
                next_sibling: None,
                first_child: None,
                has_mesh: descriptor.has_mesh,
                translucency_class: descriptor.translucency_class,
                culled: descriptor.culled,
                render_pass_mask: descriptor.render_pass_mask,
            });
            if let Some(parent) = descriptor.parent {
                if parent >= index {
                    return Err(SceneAssemblyError::ForwardParent {
                        node: index,
                        parent,
                    });
                }
                match nodes[parent].first_child {
                    None => nodes[parent].first_child = Some(index),
                    Some(first) => {
                        let mut tail = first;
                        while let Some(next) = nodes[tail].next_sibling {
                            tail = next;
                        }
                        nodes[tail].next_sibling = Some(index);
                    }
                }
            }
        }
        Ok(SceneGraph { nodes })
    }

    /// Builds the traversal arena directly from a C3D Group0 transform table
    /// (`FUN_004a2b70`'s node-build loop). Each record maps 1:1 to a node (as it
    /// does in well-formed stage/car files, where record 0 is the root marker):
    /// the parent is `field_04` (`-1` => a root with no in-stream parent), the
    /// translucency class is the `+0x48` dword, and a node has a mesh exactly
    /// when its mesh-index list is non-empty (`value_count > 0`); the shipped
    /// loader then writes node `+0x180` = `3` (culled) for a mesh-less
    /// transform-only node and `0` for a mesh node, so load-time `culled` is the
    /// negation of `has_mesh`. The render-pass mask is **not** in the file
    /// (`FUN_004a2b70` never writes node `+0x184`; the runtime assigns it), so
    /// every node is marked drawable in all passes ([`ALL_RENDER_PASSES`]) as the
    /// pre-runtime default. Rejects a forward/out-of-range parent exactly as
    /// [`assemble`](Self::assemble).
    pub fn from_c3d_group0<'a>(
        records: impl Iterator<Item = C3dGroup0Record<'a>>,
    ) -> Result<Self, SceneAssemblyError> {
        let descriptors: Vec<SceneNodeDescriptor> = records
            .map(|record| {
                let has_mesh = record.value_count() > 0;
                SceneNodeDescriptor {
                    parent: record.parent_index(),
                    has_mesh,
                    translucency_class: record.node_class() as i32,
                    culled: !has_mesh,
                    render_pass_mask: ALL_RENDER_PASSES,
                }
            })
            .collect();
        SceneGraph::assemble(&descriptors)
    }

    /// The node at `index`.
    pub fn node(&self, index: usize) -> &SceneNode {
        &self.nodes[index]
    }

    /// The number of nodes in the arena.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the arena is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Emits one entity's subtree for render pass `pass`, reproducing the
    /// entity-root emitter `FUN_004a2700`: the root draws iff it is selected by
    /// the pass and not culled (the emitter presumes the root carries a mesh),
    /// then its child chain is walked **unconditionally** (the root's own
    /// pass/cull gate does not gate its descendants).
    pub fn emit_entity(&self, root: usize, pass: u32) -> Vec<DrawOp> {
        let mut ops = Vec::new();
        let node = self.nodes[root];
        if !node.culled && node.in_pass(pass) && node.has_mesh {
            ops.push(DrawOp {
                node: root,
                deferred_translucent: false,
            });
        }
        if let Some(child) = node.first_child {
            self.walk_siblings(child, pass, false, &mut ops);
        }
        ops
    }

    /// The recursive sibling/child walker `FUN_004a28a0`: for render pass `pass`,
    /// walk the sibling chain from `start`. A node is visited only when selected
    /// by the pass; a visited, un-culled node with a mesh emits a [`DrawOp`] and
    /// then its children are walked. On advancing to the next sibling, a
    /// translucent sibling ([`TRANSLUCENT_CLASS`]) is re-entered under a deferred
    /// state bracket (modelled by `in_deferred`) before the walk continues past
    /// it - transcribed exactly from the shipped control flow.
    fn walk_siblings(&self, start: usize, pass: u32, in_deferred: bool, ops: &mut Vec<DrawOp>) {
        let mut cursor = Some(start);
        while let Some(index) = cursor {
            let node = self.nodes[index];
            if node.in_pass(pass) {
                if !node.culled && node.has_mesh {
                    ops.push(DrawOp {
                        node: index,
                        deferred_translucent: in_deferred,
                    });
                }
                if let Some(child) = node.first_child {
                    self.walk_siblings(child, pass, in_deferred, ops);
                }
            }

            // Advance to the next sibling. A translucent sibling is handled
            // immediately under the deferred bracket, then skipped past; any
            // other sibling is processed by the next loop iteration.
            cursor = node.next_sibling;
            if let Some(sibling) = cursor
                && self.nodes[sibling].is_translucent()
            {
                self.walk_siblings(sibling, pass, true, ops);
                cursor = self.nodes[sibling].next_sibling;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque_child(next: Option<usize>, child: Option<usize>) -> SceneNode {
        SceneNode {
            next_sibling: next,
            first_child: child,
            ..SceneNode::leaf()
        }
    }

    #[test]
    fn node_layout_offsets_are_the_recovered_values() {
        assert_eq!(NODE_NEXT_SIBLING, 0x00);
        assert_eq!(NODE_FIRST_CHILD, 0x04);
        assert_eq!(NODE_MESH, 0x0c);
        assert_eq!(NODE_TRANSLUCENCY_CLASS, 0x2c);
        assert_eq!(NODE_WORLD_MATRIX, 0x114);
        assert_eq!(NODE_CULL_FLAG, 0x180);
        assert_eq!(NODE_RENDER_PASS_MASK, 0x184);
        assert_eq!(TRANSLUCENT_CLASS, 0x6f);
    }

    #[test]
    fn object_and_device_offsets_hold() {
        assert_eq!(OBJECT_SORT_X, 0x104);
        assert_eq!(OBJECT_SORT_Z, 0x10c);
        assert_eq!(OBJECT_LIST_NEXT, 0x16c);
        assert_eq!(DEVICE_BEGIN_SCENE, 0x28);
        assert_eq!(DEVICE_END_SCENE, 0x2c);
        assert_eq!(DEVICE_SET_RENDER_STATE, 0x5c);
        assert_eq!(DEVICE_SET_TRANSFORM, 0x68);
        assert_eq!(DEVICE_DRAW_INDEXED_PRIMITIVE, 0x78);
        // The deferred near list cap matches the frustum near-list cap (25).
        assert_eq!(DEFERRED_NEAR_LIST_CAP, 25);
        assert_eq!(DEFERRED_NEAR_LIST_CAP, crate::c3d_camera::NEAR_LIST_CAP);
    }

    #[test]
    fn alpha_blend_is_off_at_the_top_capability_level() {
        // Below the top level, the global flag decides.
        assert!(alpha_blend_enabled(0, true));
        assert!(alpha_blend_enabled(6, true));
        assert!(!alpha_blend_enabled(3, false));
        // At the top level 7, blending is forced off even with the flag set.
        assert!(!alpha_blend_enabled(RENDER_CAP_MAX, true));
    }

    #[test]
    fn opaque_nodes_always_draw_translucent_only_when_enabled() {
        // Opaque: draws regardless of blend/override.
        assert!(node_prims_draw(0, RENDER_CAP_MAX, false, false));
        // Translucent: suppressed when blending off and no override.
        assert!(!node_prims_draw(
            TRANSLUCENT_CLASS,
            RENDER_CAP_MAX,
            true,
            false
        ));
        assert!(!node_prims_draw(TRANSLUCENT_CLASS, 3, false, false));
        // Translucent: draws when blending enabled...
        assert!(node_prims_draw(TRANSLUCENT_CLASS, 3, true, false));
        // ...or when the debug override forces it.
        assert!(node_prims_draw(
            TRANSLUCENT_CLASS,
            RENDER_CAP_MAX,
            false,
            true
        ));
    }

    #[test]
    fn deferred_state_is_additive_only_with_nonzero_cap_and_blending() {
        assert_eq!(
            deferred_translucent_state(3, true),
            DeferredTranslucentState::Additive
        );
        // cap == 0 -> enable path even if the flag is set.
        assert_eq!(
            deferred_translucent_state(0, true),
            DeferredTranslucentState::Enable
        );
        // blending disabled (flag off, or top cap) -> enable path.
        assert_eq!(
            deferred_translucent_state(3, false),
            DeferredTranslucentState::Enable
        );
        assert_eq!(
            deferred_translucent_state(RENDER_CAP_MAX, true),
            DeferredTranslucentState::Enable
        );
    }

    #[test]
    fn distance_sort_trigger_and_deferral_bounds() {
        assert!(should_distance_sort(RENDER_CAP_SORTED, 1));
        assert!(should_distance_sort(5, 0));
        assert!(!should_distance_sort(5, 1));
        // Near metric under threshold and room in the list -> deferred.
        assert!(entity_is_deferred(10, 20, 0));
        // Not near enough.
        assert!(!entity_is_deferred(20, 20, 0));
        // List already full (25 entries).
        assert!(!entity_is_deferred(10, 20, DEFERRED_NEAR_LIST_CAP));
    }

    #[test]
    fn pass_mask_selects_nodes_bitwise() {
        let mut n = SceneNode::leaf();
        n.render_pass_mask = 0b0000_1010; // passes 1 and 3
        assert!(!n.in_pass(0));
        assert!(n.in_pass(1));
        assert!(!n.in_pass(2));
        assert!(n.in_pass(3));
        // Bits beyond the byte are never set.
        assert!(!n.in_pass(8));
    }

    #[test]
    fn preorder_root_then_children_then_siblings() {
        // entity 0 -> child 1 -> (child 1's children: 2, 3) ; sibling of 1: 4
        //   0
        //   `- 1
        //   |  `- 2 -> 3
        //   `- 4
        let graph = SceneGraph::new(vec![
            opaque_child(None, Some(1)),    // 0 root
            opaque_child(Some(4), Some(2)), // 1 child of root, sibling 4
            opaque_child(Some(3), None),    // 2 child of 1
            opaque_child(None, None),       // 3 sibling of 2
            opaque_child(None, None),       // 4 sibling of 1
        ]);
        let ops: Vec<usize> = graph.emit_entity(0, 0).iter().map(|op| op.node).collect();
        // Root first, then child subtree (1,2,3) in pre-order, then sibling 4.
        assert_eq!(ops, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn root_children_are_walked_even_when_root_fails_the_pass() {
        let mut root = opaque_child(None, Some(1));
        root.render_pass_mask = 0; // root not in any pass
        let graph = SceneGraph::new(vec![root, opaque_child(None, None)]);
        let ops: Vec<usize> = graph.emit_entity(0, 0).iter().map(|op| op.node).collect();
        // Root suppressed, but its child is still traversed and drawn.
        assert_eq!(ops, vec![1]);
    }

    #[test]
    fn culled_node_is_skipped_but_its_children_are_not() {
        let mut culled = opaque_child(None, Some(1));
        culled.culled = true;
        // Make node 0 a child of an entity root so the walker (not the root
        // emitter) handles it. Root 2 -> child 0(culled) -> child 1.
        let graph = SceneGraph::new(vec![
            culled,                      // 0 culled, has child 1
            opaque_child(None, None),    // 1 child of 0
            opaque_child(None, Some(0)), // 2 entity root
        ]);
        let ops: Vec<usize> = graph.emit_entity(2, 0).iter().map(|op| op.node).collect();
        // Root 2 drawn, node 0 culled (skipped), its child 1 still drawn.
        assert_eq!(ops, vec![2, 1]);
    }

    #[test]
    fn meshless_node_is_visited_for_children_but_emits_no_draw() {
        let mut meshless = opaque_child(None, Some(1));
        meshless.has_mesh = false;
        let graph = SceneGraph::new(vec![
            meshless,                    // 0 no mesh, has child 1
            opaque_child(None, None),    // 1 child
            opaque_child(None, Some(0)), // 2 entity root
        ]);
        let ops: Vec<usize> = graph.emit_entity(2, 0).iter().map(|op| op.node).collect();
        assert_eq!(ops, vec![2, 1]);
    }

    #[test]
    fn translucent_tail_sibling_is_emitted_under_the_deferred_bracket() {
        // Root 0 -> child chain: A(1) -> T(2, translucent, terminal).
        let translucent = SceneNode {
            translucency_class: TRANSLUCENT_CLASS,
            ..opaque_child(None, None)
        };
        let graph = SceneGraph::new(vec![
            opaque_child(None, Some(1)), // 0 root
            opaque_child(Some(2), None), // 1 = A, sibling T
            translucent,                 // 2 = T, terminal translucent
        ]);
        let ops = graph.emit_entity(0, 0);
        assert_eq!(
            ops,
            vec![
                DrawOp {
                    node: 0,
                    deferred_translucent: false
                },
                DrawOp {
                    node: 1,
                    deferred_translucent: false
                },
                DrawOp {
                    node: 2,
                    deferred_translucent: true
                },
            ]
        );
    }

    #[test]
    fn translucent_midchain_reenters_then_reprocesses_the_tail() {
        // Transcribed faithfully: a translucent node mid-chain is re-entered
        // under the bracket (drawing it and its tail deferred), then the walk
        // advances past it and the loop reprocesses the tail opaquely. The
        // shipped scene data avoids this by making translucent nodes terminal.
        // Root 0 -> child chain: A(1) -> T(2, translucent) -> B(3).
        let translucent = SceneNode {
            translucency_class: TRANSLUCENT_CLASS,
            next_sibling: Some(3),
            first_child: None,
            ..SceneNode::leaf()
        };
        let graph = SceneGraph::new(vec![
            opaque_child(None, Some(1)), // 0 root
            opaque_child(Some(2), None), // 1 = A
            translucent,                 // 2 = T (translucent), sibling B
            opaque_child(None, None),    // 3 = B
        ]);
        let ops = graph.emit_entity(0, 0);
        assert_eq!(
            ops,
            vec![
                DrawOp {
                    node: 0,
                    deferred_translucent: false
                }, // root
                DrawOp {
                    node: 1,
                    deferred_translucent: false
                }, // A
                DrawOp {
                    node: 2,
                    deferred_translucent: true
                }, // T (bracket)
                DrawOp {
                    node: 3,
                    deferred_translucent: true
                }, // B (in bracket)
                DrawOp {
                    node: 3,
                    deferred_translucent: false
                }, // B reprocessed
            ]
        );
    }

    fn descriptor(parent: Option<usize>) -> SceneNodeDescriptor {
        SceneNodeDescriptor {
            parent,
            has_mesh: true,
            translucency_class: 0,
            culled: false,
            render_pass_mask: 0xff,
        }
    }

    /// Minimal valid `CM3D` buffer whose Group0 table holds `records` as
    /// `(field_04, node_class, mesh_count)` tuples (group1/blob sections empty).
    fn c3d_group0_bytes(records: &[(i16, u32, u8)]) -> Vec<u8> {
        let mut body = Vec::new();
        for &(field_04, class, mesh_count) in records {
            let declared = 80 + usize::from(mesh_count) * 2;
            let mut record = vec![0u8; declared];
            record[0..4].copy_from_slice(&(declared as u32).to_le_bytes());
            record[4..6].copy_from_slice(&field_04.to_le_bytes());
            record[0x48..0x4c].copy_from_slice(&class.to_le_bytes());
            record[0x4c] = mesh_count; // value_count = mesh-index-list length
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

    #[test]
    fn from_c3d_group0_builds_the_tree_and_maps_node_fields() {
        use cmr_formats::c3d::{C3dLimits, parse_c3d};
        // Root (marker), a mesh child, a translucent mesh child, a transform-only
        // grandchild under node 1.
        let bytes = c3d_group0_bytes(&[
            (-1, 0x10, 0),                    // 0: root marker, no mesh
            (0, 0x20, 3),                     // 1: child of 0, 3 meshes
            (0, TRANSLUCENT_CLASS as u32, 1), // 2: translucent child of 0
            (1, 0x30, 0),                     // 3: transform-only child of 1
        ]);
        let limits = C3dLimits {
            max_payload_bytes: 4096,
            max_group0_records: 16,
            max_group1_records: 16,
            max_blob_records: 16,
        };
        let file = parse_c3d(&bytes, limits).unwrap();
        let graph = SceneGraph::from_c3d_group0(file.group0_records()).unwrap();

        // Tree: 0 -> [1, 2]; 1 -> [3].
        assert_eq!(graph.node(0).first_child, Some(1));
        assert_eq!(graph.node(1).next_sibling, Some(2));
        assert_eq!(graph.node(1).first_child, Some(3));
        // Field mapping: mesh presence from value_count, cull = !has_mesh.
        assert!(graph.node(0).culled && !graph.node(0).has_mesh); // marker, no mesh
        assert!(graph.node(1).has_mesh && !graph.node(1).culled); // 3 meshes
        assert!(graph.node(2).is_translucent());
        assert!(graph.node(3).culled && !graph.node(3).has_mesh); // transform-only
        // Pass mask is the all-passes pre-runtime default.
        assert_eq!(graph.node(1).render_pass_mask, ALL_RENDER_PASSES);
    }

    #[test]
    fn assemble_threads_children_in_stream_order_as_last_siblings() {
        // Root 0; children 1, 2, 3 of 0 (in order); grandchild 4 of 1.
        let graph = SceneGraph::assemble(&[
            descriptor(None),
            descriptor(Some(0)),
            descriptor(Some(0)),
            descriptor(Some(0)),
            descriptor(Some(1)),
        ])
        .unwrap();
        // Parent 0's first child is 1, then the sibling chain is 1 -> 2 -> 3.
        assert_eq!(graph.node(0).first_child, Some(1));
        assert_eq!(graph.node(1).next_sibling, Some(2));
        assert_eq!(graph.node(2).next_sibling, Some(3));
        assert_eq!(graph.node(3).next_sibling, None);
        // Node 1 owns grandchild 4; leaves have no child/sibling.
        assert_eq!(graph.node(1).first_child, Some(4));
        assert_eq!(graph.node(4).first_child, None);
        assert_eq!(graph.node(4).next_sibling, None);
        // A root is left unlinked (no sibling, iterated via the object list).
        assert_eq!(graph.node(0).next_sibling, None);
    }

    #[test]
    fn assemble_preserves_the_descriptor_node_fields() {
        let graph = SceneGraph::assemble(&[
            descriptor(None),
            SceneNodeDescriptor {
                parent: Some(0),
                has_mesh: false,
                translucency_class: TRANSLUCENT_CLASS,
                culled: true,
                render_pass_mask: 0x04,
            },
        ])
        .unwrap();
        let child = graph.node(1);
        assert!(!child.has_mesh);
        assert!(child.is_translucent());
        assert!(child.culled);
        assert!(child.in_pass(2) && !child.in_pass(0));
    }

    #[test]
    fn assemble_rejects_a_forward_or_out_of_range_parent() {
        // Node 0 names parent 1, which does not exist yet.
        assert_eq!(
            SceneGraph::assemble(&[descriptor(Some(1)), descriptor(None)]).unwrap_err(),
            SceneAssemblyError::ForwardParent { node: 0, parent: 1 }
        );
        // A node may not name itself.
        assert_eq!(
            SceneGraph::assemble(&[descriptor(Some(0))]).unwrap_err(),
            SceneAssemblyError::ForwardParent { node: 0, parent: 0 }
        );
    }

    #[test]
    fn texture_perspective_is_affine_only_for_the_inline_draw() {
        assert_eq!(draw_texture_perspective(true), TexturePerspective::Affine);
        assert_eq!(
            draw_texture_perspective(false),
            TexturePerspective::GlobalDefault
        );
    }

    #[test]
    fn colorkey_is_disabled_only_for_the_focus_entity_at_the_sorted_cap() {
        // Fires exactly when cap == RENDER_CAP_SORTED and the object is the focus.
        assert!(object_colorkey_disabled(RENDER_CAP_SORTED, true));
        // A non-focus object, any other cap, or both negated: left untouched.
        assert!(!object_colorkey_disabled(RENDER_CAP_SORTED, false));
        assert!(!object_colorkey_disabled(RENDER_CAP_MAX, true));
        assert!(!object_colorkey_disabled(0, true));
        assert!(!object_colorkey_disabled(0, false));
    }
}
