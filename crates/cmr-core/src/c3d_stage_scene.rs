//! Platform-neutral C3D stage/car scene aggregate (reconstruction item E08).
//!
//! The individual recovery steps - the node tree
//! ([`crate::c3d_scene_graph::SceneGraph::from_c3d_group0`]), the per-frame world
//! matrices ([`crate::c3d_transform::scene_world_transforms`]), and the per-node
//! multi-LOD geometry ([`crate::c3d_mesh::assemble_scene_meshes`]) - each consume
//! the same ordered Group0 record stream and each emit output **aligned 1:1 with
//! the node index**. [`StageScene`] ties them into one owned structure so a
//! parsed `.c3d` scene becomes a walkable frame: for each drawable node the
//! walker has its world matrix and its active-LOD draw group, in the recovered
//! traversal order.
//!
//! This is the backend-neutral walk half - it produces no pixels and binds no
//! textures (resolving a face's group-2 material ordinal to a texture is the
//! separate E03 step). A software/GPU backend consumes [`StageScene::draw_order`]
//! plus [`StageScene::world`]/[`StageScene::mesh`] to composite a frame.
//!
//! The draw order reproduces the dispatcher `FUN_004a07d0`: the scene roots (the
//! `field_04 == -1` group markers the dispatcher iterates through the per-category
//! object list, not the tree) are each walked by the recovered entity emitter
//! ([`SceneGraph::emit_entity`]) under a **single** render-pass mask (the
//! dispatcher runs no pass-index loop). See evidence `impl.stage-scene`.

use std::fmt;

use cmr_formats::c3d::{C3dFile, C3dGroup1Record};

use crate::c3d_mesh::{AssembleNodeMeshError, AssembledMesh, assemble_scene_meshes};
use crate::c3d_scene_graph::{DrawOp, SceneAssemblyError, SceneGraph};
use crate::c3d_transform::scene_world_transforms;

/// The single render pass the scene walk emits. Every node a freshly parsed C3D
/// yields carries the all-passes default mask ([`SceneGraph::from_c3d_group0`]),
/// and the shipped dispatcher runs one render-pass mask with no pass-index loop,
/// so pass `0` selects the whole scene.
const SCENE_RENDER_PASS: u32 = 0;

/// A parsed C3D scene resolved into the three node-aligned tables a frame walk
/// needs. Borrows the parsed file's bytes (`'a`) through the assembled meshes.
#[derive(Debug)]
pub struct StageScene<'a> {
    graph: SceneGraph,
    roots: Vec<usize>,
    worlds: Vec<[i32; 16]>,
    meshes: Vec<Option<AssembledMesh<'a>>>,
}

impl<'a> StageScene<'a> {
    /// Resolves a parsed C3D scene into its node tree, world matrices, and
    /// per-node geometry.
    ///
    /// `root_world` is the world matrix of the external container the scene roots
    /// hang under (pass [`crate::c3d_camera::MATRIX_IDENTITY_Q16`] for a stage at
    /// the origin); `vertex_type` is the draw-group vertex type the loader fixes
    /// per load call (`2` = lit, `1` = pre-lit - see
    /// [`crate::c3d_mesh::node_mesh_vertex_type`]). The three tables are built
    /// from the same record stream, so `graph`/`world`/`mesh` share the node
    /// index. A forward/out-of-range parent or a bad mesh index is rejected.
    pub fn from_c3d(
        scene: &'a C3dFile,
        root_world: &[i32; 16],
        vertex_type: u32,
    ) -> Result<Self, StageSceneError> {
        let graph = SceneGraph::from_c3d_group0(scene.group0_records())?;
        let worlds = scene_world_transforms(scene.group0_records(), root_world);
        let roots: Vec<usize> = scene
            .group0_records()
            .enumerate()
            .filter(|(_, record)| record.parent_index().is_none())
            .map(|(index, _)| index)
            .collect();
        let group1: Vec<C3dGroup1Record<'a>> = scene.group1_records().collect();
        let meshes = assemble_scene_meshes(scene.group0_records(), &group1, vertex_type)?;
        Ok(StageScene {
            graph,
            roots,
            worlds,
            meshes,
        })
    }

    /// The number of scene nodes (one per Group0 record).
    pub fn node_count(&self) -> usize {
        self.graph.len()
    }

    /// Whether the scene has no nodes.
    pub fn is_empty(&self) -> bool {
        self.graph.is_empty()
    }

    /// The assembled node tree.
    pub fn graph(&self) -> &SceneGraph {
        &self.graph
    }

    /// The scene roots in record order (the `field_04 == -1` group markers).
    pub fn roots(&self) -> &[usize] {
        &self.roots
    }

    /// The world matrix of node `index`.
    pub fn world(&self, index: usize) -> &[i32; 16] {
        &self.worlds[index]
    }

    /// The assembled multi-LOD mesh of node `index`, or `None` for a transform-
    /// only node (an empty mesh-index list).
    pub fn mesh(&self, index: usize) -> Option<&AssembledMesh<'a>> {
        self.meshes[index].as_ref()
    }

    /// The scene's draw order: the recovered entity emitter walked from each root
    /// in turn, under the single scene render pass. Each [`DrawOp`] names a
    /// drawable node (pass-selected, un-culled, mesh-carrying) and whether it was
    /// reached inside a deferred-translucent bracket. Roots are disjoint subtrees
    /// (a root never parents another), so a node is emitted at most once.
    pub fn draw_order(&self) -> Vec<DrawOp> {
        let mut ops = Vec::new();
        for &root in &self.roots {
            ops.extend(self.graph.emit_entity(root, SCENE_RENDER_PASS));
        }
        ops
    }
}

/// Why [`StageScene::from_c3d`] rejected a parsed scene.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageSceneError {
    /// The Group0 node tree could not be threaded (a forward/out-of-range
    /// parent).
    Assembly(SceneAssemblyError),
    /// A node's mesh-index list could not be resolved against the mesh LUT.
    Mesh(AssembleNodeMeshError),
}

impl From<SceneAssemblyError> for StageSceneError {
    fn from(error: SceneAssemblyError) -> Self {
        StageSceneError::Assembly(error)
    }
}

impl From<AssembleNodeMeshError> for StageSceneError {
    fn from(error: AssembleNodeMeshError) -> Self {
        StageSceneError::Mesh(error)
    }
}

impl fmt::Display for StageSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageSceneError::Assembly(error) => write!(f, "C3D scene node tree: {error}"),
            StageSceneError::Mesh(error) => write!(f, "C3D scene mesh: {error}"),
        }
    }
}

impl std::error::Error for StageSceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StageSceneError::Assembly(error) => Some(error),
            StageSceneError::Mesh(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c3d_camera::MATRIX_IDENTITY_Q16;
    use crate::c3d_render::VERTEXTYPE_LVERTEX;
    use cmr_formats::c3d::{
        C3D_FORMAT_VERSION, C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN,
        C3D_HEADER_LEN, C3dLimits, parse_c3d,
    };

    const ONE: i32 = 0x1_0000;

    /// One group-0 node for the combined fixture: its `field_04` (`-1` = root),
    /// its Q16.16 local matrix, and its mesh-index (`values`) list.
    struct NodeSpec<'a> {
        field_04: i16,
        matrix: [i32; 16],
        values: &'a [u16],
    }

    /// One group-1 mesh record for the combined fixture: its vertex count and
    /// its faces (each a `(material ordinal, vertex ordinals)` tuple).
    type MeshSpec<'a> = (usize, &'a [(i16, [u16; 3])]);

    /// Builds one CM3D file carrying both a Group0 node table and a Group1 mesh
    /// table (each mesh = `vertex_count` distinct vertices + one face per
    /// `(material, ordinals)`), so [`StageScene::from_c3d`] reads both sections
    /// from the same file. Only the fields the scene pipeline reads are set.
    fn stage_c3d(nodes: &[NodeSpec<'_>], meshes: &[MeshSpec<'_>]) -> C3dFile {
        // Group0 body: variable-stride node records.
        let mut group0 = Vec::new();
        for node in nodes {
            let declared = 0x50 + node.values.len() * 2;
            let mut record = vec![0u8; declared];
            record[0..4].copy_from_slice(&u32::try_from(declared).unwrap().to_le_bytes());
            record[4..6].copy_from_slice(&node.field_04.to_le_bytes());
            for (index, component) in node.matrix.iter().enumerate() {
                let start = 0x08 + index * 4;
                record[start..start + 4].copy_from_slice(&component.to_le_bytes());
            }
            record[0x4c] = u8::try_from(node.values.len()).unwrap();
            for (k, &value) in node.values.iter().enumerate() {
                let start = 0x50 + k * 2;
                record[start..start + 2].copy_from_slice(&value.to_le_bytes());
            }
            group0.extend_from_slice(&record);
        }

        // Group1 body: fixed-stride mesh records.
        let mut group1 = Vec::new();
        for &(vertex_count, faces) in meshes {
            let record_len = C3D_GROUP1_FIXED_LEN
                + vertex_count * C3D_GROUP1_ENTRY32_LEN
                + faces.len() * C3D_GROUP1_ENTRY24_LEN;
            let mut record = vec![0u8; record_len];
            record[12..16].copy_from_slice(&u32::try_from(record_len).unwrap().to_le_bytes());
            record[16..18].copy_from_slice(&u16::try_from(vertex_count).unwrap().to_le_bytes());
            record[18..20].copy_from_slice(&u16::try_from(faces.len()).unwrap().to_le_bytes());
            let entries24_offset = C3D_GROUP1_FIXED_LEN + vertex_count * C3D_GROUP1_ENTRY32_LEN;
            for index in 0..vertex_count {
                let start = C3D_GROUP1_FIXED_LEN + index * C3D_GROUP1_ENTRY32_LEN;
                record[start..start + C3D_GROUP1_ENTRY32_LEN]
                    .copy_from_slice(&[u8::try_from(index + 1).unwrap(); C3D_GROUP1_ENTRY32_LEN]);
            }
            for (face_index, (material, ordinals)) in faces.iter().enumerate() {
                let start = entries24_offset + face_index * C3D_GROUP1_ENTRY24_LEN;
                for (corner, ordinal) in ordinals.iter().enumerate() {
                    let field = start + 2 + corner * 2;
                    record[field..field + 2].copy_from_slice(&ordinal.to_le_bytes());
                }
                record[start + 8..start + 10].copy_from_slice(&material.to_le_bytes());
            }
            group1.extend_from_slice(&record);
        }

        let group0_offset = C3D_HEADER_LEN;
        let group1_offset = group0_offset + group0.len();
        let blob_offset = group1_offset + group1.len();

        let mut buf = vec![0u8; C3D_HEADER_LEN];
        buf[0..4].copy_from_slice(b"CM3D");
        buf[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        buf[8..12].copy_from_slice(&(C3D_HEADER_LEN as u32).to_le_bytes());
        buf[12..14].copy_from_slice(&u16::try_from(nodes.len()).unwrap().to_le_bytes());
        buf[14..16].copy_from_slice(&u16::try_from(meshes.len()).unwrap().to_le_bytes());
        buf[20..24].copy_from_slice(&u32::try_from(group0_offset).unwrap().to_le_bytes());
        buf[24..28].copy_from_slice(&u32::try_from(group1_offset).unwrap().to_le_bytes());
        buf[28..32].copy_from_slice(&u32::try_from(blob_offset).unwrap().to_le_bytes());
        buf.extend_from_slice(&group0);
        buf.extend_from_slice(&group1);

        parse_c3d(&buf, C3dLimits::default()).unwrap()
    }

    fn translate(x: i32, y: i32, z: i32) -> [i32; 16] {
        let mut m = MATRIX_IDENTITY_Q16;
        m[12] = x;
        m[13] = y;
        m[14] = z;
        m
    }

    #[test]
    fn from_c3d_aligns_graph_world_and_mesh_by_node_index() {
        // Node 0: root marker, no mesh. Node 1: mesh child (LUT index 0).
        let scene = stage_c3d(
            &[
                NodeSpec {
                    field_04: -1,
                    matrix: translate(3 * ONE, 0, 0),
                    values: &[],
                },
                NodeSpec {
                    field_04: 0,
                    matrix: translate(0, 0, 4 * ONE),
                    values: &[0],
                },
            ],
            &[(3, &[(0, [0, 1, 2])])],
        );
        let stage = StageScene::from_c3d(&scene, &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX).unwrap();

        assert_eq!(stage.node_count(), 2);
        assert_eq!(stage.roots(), &[0]);
        // Root marker carries no mesh; the child does.
        assert!(stage.mesh(0).is_none());
        let child_mesh = stage.mesh(1).unwrap();
        assert_eq!(child_mesh.active_lod(), 0);
        assert_eq!(child_mesh.active().unwrap().prims()[0].selector(), 0);
        // World matrices line up with the node index: the child composes its
        // local +z=4 onto the root's +x=3.
        assert_eq!(stage.world(0), &translate(3 * ONE, 0, 0));
        assert_eq!(stage.world(1), &translate(3 * ONE, 0, 4 * ONE));
    }

    #[test]
    fn draw_order_walks_drawable_nodes_and_skips_the_meshless_root() {
        // Root marker (no mesh) with two mesh children and one transform-only
        // child between them.
        let scene = stage_c3d(
            &[
                NodeSpec {
                    field_04: -1,
                    matrix: MATRIX_IDENTITY_Q16,
                    values: &[],
                },
                NodeSpec {
                    field_04: 0,
                    matrix: MATRIX_IDENTITY_Q16,
                    values: &[0],
                },
                NodeSpec {
                    field_04: 0,
                    matrix: MATRIX_IDENTITY_Q16,
                    values: &[],
                },
                NodeSpec {
                    field_04: 0,
                    matrix: MATRIX_IDENTITY_Q16,
                    values: &[0],
                },
            ],
            &[(3, &[(0, [0, 1, 2])])],
        );
        let stage = StageScene::from_c3d(&scene, &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX).unwrap();

        // The meshless root (0) and transform-only child (2) are skipped; the two
        // mesh children emit in sibling (stream) order.
        let order: Vec<usize> = stage.draw_order().iter().map(|op| op.node).collect();
        assert_eq!(order, vec![1, 3]);
    }

    #[test]
    fn from_c3d_propagates_a_bad_mesh_index() {
        // Node 1 references LUT index 5, but only one mesh record exists.
        let scene = stage_c3d(
            &[
                NodeSpec {
                    field_04: -1,
                    matrix: MATRIX_IDENTITY_Q16,
                    values: &[],
                },
                NodeSpec {
                    field_04: 0,
                    matrix: MATRIX_IDENTITY_Q16,
                    values: &[5],
                },
            ],
            &[(3, &[(0, [0, 1, 2])])],
        );
        let error =
            StageScene::from_c3d(&scene, &MATRIX_IDENTITY_Q16, VERTEXTYPE_LVERTEX).unwrap_err();
        assert_eq!(
            error,
            StageSceneError::Mesh(AssembleNodeMeshError::MeshIndexOutOfBounds {
                lod: 0,
                index: 5,
                record_count: 1,
            })
        );
    }
}
