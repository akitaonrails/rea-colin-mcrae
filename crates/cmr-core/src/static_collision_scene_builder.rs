//! Pure bridge from parsed main-C3D/OBJ data to the recovered static-collision
//! grid and record arena. It performs no asset I/O and owns no game payload.

use core::fmt;

use cmr_formats::c3d::C3dFile;

use crate::{
    c3d_camera::matmul_q16,
    c3d_scene_graph::{SceneAssemblyError, SceneGraph},
    c3d_terrain_cull::{CellGrid, CellGridDeriveError, derive_cell_grid_from_main_translations},
    compiled_car_initializer::normalize_compiled_lane_vector,
    fixed_point::{Fixed16, Vec3},
    legacy_math::sqrt_q16,
    static_world_collision::{
        StaticCollisionNodeId, StaticCollisionNodeIdError, StaticCollisionRecord,
        StaticCollisionScene, StaticCollisionSceneError, StaticNarrowObb, StaticObbBuildInput,
        build_static_obb_geometry,
    },
};

const NORMAL_COLLISION_CLASS_BIT: u32 = 1 << 29;
const REJECT_COLLISION_CLASS_BITS: u32 = (1 << 16) | (1 << 17);
const ALTERNATE_COLLISION_FLAG: u8 = 0x80;

/// Parsed, immutable collision data retained for one loaded stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedStaticCollisionScene {
    pub grid: CellGrid,
    pub scene: StaticCollisionScene<StaticNarrowObb>,
}

/// Safe reconstruction failures while translating parsed C3D records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParsedStaticCollisionBuildError {
    Grid(CellGridDeriveError),
    ObjectGraph(SceneAssemblyError),
    Group1OrdinalOutOfBounds {
        root_index: usize,
        mesh_node_index: usize,
        ordinal: u16,
        record_count: usize,
    },
    CollisionOrdinalOverflow {
        record_count: usize,
    },
    CellNode(StaticCollisionNodeIdError),
    Scene(StaticCollisionSceneError),
}

impl fmt::Display for ParsedStaticCollisionBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Grid(error) => write!(f, "could not derive main-C3D collision grid: {error:?}"),
            Self::ObjectGraph(error) => write!(f, "could not assemble OBJ graph: {error}"),
            Self::Group1OrdinalOutOfBounds {
                root_index,
                mesh_node_index,
                ordinal,
                record_count,
            } => write!(
                f,
                "OBJ root {root_index} mesh node {mesh_node_index} references group-1 ordinal {ordinal}, but only {record_count} records exist"
            ),
            Self::CollisionOrdinalOverflow { record_count } => write!(
                f,
                "static collision record count {record_count} exceeds the u16 ordinal arena"
            ),
            Self::CellNode(error) => error.fmt(f),
            Self::Scene(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ParsedStaticCollisionBuildError {}

/// Build the recovered static OBJ collision scene from already-parsed files.
///
/// `main_c3d` supplies only the cell-grid translations. `objects` supplies the
/// admitted roots, shallow mesh traversal, local basis/scale, immediate-parent
/// collision translation, authored group-1 bounds, and material flags.
pub fn build_parsed_static_collision_scene(
    main_c3d: &C3dFile,
    objects: &C3dFile,
) -> Result<ParsedStaticCollisionScene, ParsedStaticCollisionBuildError> {
    let main_translations: Vec<[i32; 3]> = main_c3d
        .group0_records()
        .map(|record| {
            let matrix = record.affine_matrix_q16();
            [matrix[12], matrix[13], matrix[14]]
        })
        .collect();
    let grid = derive_cell_grid_from_main_translations(&main_translations)
        .map_err(ParsedStaticCollisionBuildError::Grid)?;

    let object_records: Vec<_> = objects.group0_records().collect();
    let object_graph = SceneGraph::from_c3d_group0(object_records.iter().copied())
        .map_err(ParsedStaticCollisionBuildError::ObjectGraph)?;
    let group1_records: Vec<_> = objects.group1_records().collect();
    let mut records = Vec::new();
    let mut memberships = Vec::new();

    for (root_index, root) in object_records.iter().copied().enumerate() {
        let Some(alternate_flag) = admitted_material_flag(root.node_class()) else {
            continue;
        };
        let local = root.affine_matrix_q16();
        let collision_matrix = match root.parent_index() {
            Some(parent_index) => {
                let parent = object_records[parent_index].affine_matrix_q16();
                matmul_q16(&parent, &local)
            }
            None => local,
        };
        let rows = [
            [local[0], local[1], local[2]],
            [local[4], local[5], local[6]],
            [local[8], local[9], local[10]],
        ];
        let root_scale = rows.map(row_length_q16);
        let normalized_root_local_basis = rows.map(normalize_compiled_lane_vector);

        let root_node = object_graph.node(root_index);
        if root_node.has_mesh {
            emit_candidate(
                root_index,
                root_index,
                alternate_flag,
                &object_records,
                &group1_records,
                &grid,
                collision_matrix,
                root_scale,
                normalized_root_local_basis,
                &mut records,
                &mut memberships,
            )?;
        } else {
            let mut candidate = root_node.first_child;
            while let Some(mesh_node_index) = candidate {
                emit_candidate(
                    root_index,
                    mesh_node_index,
                    alternate_flag,
                    &object_records,
                    &group1_records,
                    &grid,
                    collision_matrix,
                    root_scale,
                    normalized_root_local_basis,
                    &mut records,
                    &mut memberships,
                )?;
                candidate = object_graph.node(mesh_node_index).next_sibling;
            }
        }
    }

    let scene = StaticCollisionScene::new(records, memberships)
        .map_err(ParsedStaticCollisionBuildError::Scene)?;
    Ok(ParsedStaticCollisionScene { grid, scene })
}

fn admitted_material_flag(class: u32) -> Option<u8> {
    if class & NORMAL_COLLISION_CLASS_BIT != 0 {
        Some(class as u8 & 0x7f)
    } else if class & REJECT_COLLISION_CLASS_BITS != 0 {
        None
    } else {
        Some((class as u8 & 0x7f) | ALTERNATE_COLLISION_FLAG)
    }
}

fn qmul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}

fn row_length_q16(row: [i32; 3]) -> i32 {
    let square_sum = row.into_iter().fold(0_i32, |sum, component| {
        sum.wrapping_add(qmul(component, component))
    });
    sqrt_q16(square_sum as u32) as i32
}

const fn vec3_from_raw(raw: [i32; 3]) -> Vec3 {
    Vec3::new(
        Fixed16::from_raw(raw[0]),
        Fixed16::from_raw(raw[1]),
        Fixed16::from_raw(raw[2]),
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_candidate(
    root_index: usize,
    mesh_node_index: usize,
    alternate_flag: u8,
    object_records: &[cmr_formats::c3d::C3dGroup0Record<'_>],
    group1_records: &[cmr_formats::c3d::C3dGroup1Record<'_>],
    grid: &CellGrid,
    collision_matrix: [i32; 16],
    root_scale: [i32; 3],
    normalized_root_local_basis: [[i32; 3]; 3],
    records: &mut Vec<StaticCollisionRecord<StaticNarrowObb>>,
    memberships: &mut Vec<(StaticCollisionNodeId, usize)>,
) -> Result<(), ParsedStaticCollisionBuildError> {
    let Some(ordinal) = object_records[mesh_node_index].values().next() else {
        return Ok(());
    };
    let group1 = group1_records.get(usize::from(ordinal)).ok_or(
        ParsedStaticCollisionBuildError::Group1OrdinalOutOfBounds {
            root_index,
            mesh_node_index,
            ordinal,
            record_count: group1_records.len(),
        },
    )?;
    let geometry = match build_static_obb_geometry(StaticObbBuildInput {
        collision_translation: vec3_from_raw([
            collision_matrix[12],
            collision_matrix[13],
            collision_matrix[14],
        ]),
        normalized_root_local_basis: normalized_root_local_basis.map(vec3_from_raw),
        root_scale: vec3_from_raw(root_scale),
        mesh_bound: vec3_from_raw(group1.collision_bounds_q16()),
    }) {
        Ok(geometry) => geometry,
        Err(_) => return Ok(()),
    };
    let collision_ordinal = u16::try_from(records.len()).map_err(|_| {
        ParsedStaticCollisionBuildError::CollisionOrdinalOverflow {
            record_count: records.len(),
        }
    })?;
    let record_index = records.len();
    let cell = grid.cell_of_point(geometry.center.x.raw(), geometry.center.z.raw());
    let node =
        StaticCollisionNodeId::new(cell).map_err(ParsedStaticCollisionBuildError::CellNode)?;
    records.push(StaticCollisionRecord {
        collision_ordinal,
        raw_material_flags: alternate_flag,
        geometry: StaticNarrowObb::from(geometry),
    });
    memberships.push((node, record_index));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_formats::c3d::{
        C3D_FORMAT_VERSION, C3D_GROUP0_AFFINE_MATRIX_OFFSET, C3D_GROUP0_FIXED_LEN,
        C3D_GROUP0_NODE_CLASS_OFFSET, C3D_GROUP0_VALUE_COUNT_OFFSET, C3D_GROUP0_VALUES_OFFSET,
        C3D_GROUP1_COLLISION_BOUNDS_OFFSET, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN, C3dLimits,
        parse_c3d,
    };

    const ONE: i32 = 1 << 16;

    #[derive(Clone)]
    struct Group0Spec {
        parent: i16,
        class: u32,
        matrix: [i32; 16],
        values: Vec<u16>,
    }

    fn matrix(translation: [i32; 3], scale: [i32; 3]) -> [i32; 16] {
        let mut value = [0; 16];
        value[0] = scale[0];
        value[5] = scale[1];
        value[10] = scale[2];
        value[15] = ONE;
        value[12..15].copy_from_slice(&translation);
        value
    }

    fn parsed(group0: &[Group0Spec], bounds: &[[i32; 3]]) -> C3dFile {
        let group0_len: usize = group0
            .iter()
            .map(|record| C3D_GROUP0_FIXED_LEN + record.values.len() * 2)
            .sum();
        let group0_start = C3D_HEADER_LEN;
        let group1_start = group0_start + group0_len;
        let end = group1_start + bounds.len() * C3D_GROUP1_FIXED_LEN;
        let mut bytes = vec![0_u8; end];
        bytes[0..4].copy_from_slice(b"CM3D");
        bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        bytes[12..14].copy_from_slice(&(group0.len() as u16).to_le_bytes());
        bytes[14..16].copy_from_slice(&(bounds.len() as u16).to_le_bytes());
        bytes[20..24].copy_from_slice(&(group0_start as u32).to_le_bytes());
        bytes[24..28].copy_from_slice(&(group1_start as u32).to_le_bytes());
        bytes[28..32].copy_from_slice(&(end as u32).to_le_bytes());

        let mut offset = group0_start;
        for record in group0 {
            let len = C3D_GROUP0_FIXED_LEN + record.values.len() * 2;
            bytes[offset..offset + 4].copy_from_slice(&(len as u32).to_le_bytes());
            bytes[offset + 4..offset + 6].copy_from_slice(&record.parent.to_le_bytes());
            for (index, component) in record.matrix.into_iter().enumerate() {
                let start = offset + C3D_GROUP0_AFFINE_MATRIX_OFFSET + index * 4;
                bytes[start..start + 4].copy_from_slice(&component.to_le_bytes());
            }
            bytes[offset + C3D_GROUP0_NODE_CLASS_OFFSET..offset + C3D_GROUP0_NODE_CLASS_OFFSET + 4]
                .copy_from_slice(&record.class.to_le_bytes());
            bytes[offset + C3D_GROUP0_VALUE_COUNT_OFFSET] = record.values.len() as u8;
            for (index, value) in record.values.iter().copied().enumerate() {
                let start = offset + C3D_GROUP0_VALUES_OFFSET + index * 2;
                bytes[start..start + 2].copy_from_slice(&value.to_le_bytes());
            }
            offset += len;
        }
        for bound in bounds {
            bytes[offset + 12..offset + 16]
                .copy_from_slice(&(C3D_GROUP1_FIXED_LEN as u32).to_le_bytes());
            for (index, component) in bound.iter().copied().enumerate() {
                let start = offset + C3D_GROUP1_COLLISION_BOUNDS_OFFSET + index * 4;
                bytes[start..start + 4].copy_from_slice(&component.to_le_bytes());
            }
            offset += C3D_GROUP1_FIXED_LEN;
        }
        parse_c3d(&bytes, C3dLimits::default()).unwrap()
    }

    fn main_grid() -> C3dFile {
        parsed(
            &[
                Group0Spec {
                    parent: -1,
                    class: 0,
                    matrix: matrix([0, 0, 0], [ONE; 3]),
                    values: vec![],
                },
                Group0Spec {
                    parent: -1,
                    class: 0,
                    matrix: matrix([0, 0, ONE], [ONE; 3]),
                    values: vec![],
                },
                Group0Spec {
                    parent: -1,
                    class: 0,
                    matrix: matrix([4 * ONE, 0, ONE], [ONE; 3]),
                    values: vec![],
                },
            ],
            &[],
        )
    }

    fn spec(parent: i16, class: u32, translation: [i32; 3], values: &[u16]) -> Group0Spec {
        Group0Spec {
            parent,
            class,
            matrix: matrix(translation, [ONE; 3]),
            values: values.to_vec(),
        }
    }

    #[test]
    fn admission_precedence_and_material_flags_match_recovered_bits() {
        let objects = parsed(
            &[
                spec(-1, (1 << 29) | (1 << 16) | 3, [0; 3], &[0]),
                spec(-1, (1 << 16) | 4, [0; 3], &[0]),
                spec(-1, 5, [0; 3], &[0]),
            ],
            &[[2 * ONE; 3]],
        );
        let built = build_parsed_static_collision_scene(&main_grid(), &objects).unwrap();
        let records = built.scene.records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].raw_material_flags, 3);
        assert_eq!(records[1].raw_material_flags, 0x85);
        assert_eq!(records[0].collision_ordinal, 0);
        assert_eq!(records[1].collision_ordinal, 1);
    }

    #[test]
    fn row_length_and_normalization_cover_zero_and_wrapping_square_sums() {
        assert_eq!(row_length_q16([0; 3]), 0);
        assert_eq!(normalize_compiled_lane_vector([0; 3]), [0; 3]);

        let wrapping_row = [i32::MAX; 3];
        assert_eq!(row_length_q16(wrapping_row), sqrt_q16(0xfffd_0000) as i32);
        assert_eq!(
            normalize_compiled_lane_vector(wrapping_row),
            wrapping_row.map(|component| {
                qmul(
                    component,
                    ((1_u64 << 32) / u64::from(sqrt_q16(0xfffd_0000))) as i32,
                )
            })
        );
    }

    #[test]
    fn meshless_root_walks_direct_children_only_and_continues_after_skips() {
        let objects = parsed(
            &[
                spec(-1, 1 << 29, [0; 3], &[]),
                spec(0, 1 << 29, [0; 3], &[]),
                spec(1, 1 << 29, [0; 3], &[0]),
                spec(0, 1 << 29, [0; 3], &[1]),
                spec(0, 1 << 29, [0; 3], &[2]),
            ],
            &[[2 * ONE; 3], [0, 2 * ONE, 2 * ONE], [4 * ONE; 3]],
        );
        let built = build_parsed_static_collision_scene(&main_grid(), &objects).unwrap();

        // Root 0 skips child 1 (no mesh), never descends to grandchild 2,
        // skips zero-scale child 3, then emits child 4. Root 1 later emits its
        // direct child 2; nodes 2 and 4 also emit when visited as roots.
        let records = built.scene.records();
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].geometry.radius, records[3].geometry.radius);
        assert_eq!(records[1].geometry.radius, records[2].geometry.radius);
        assert_ne!(records[0].geometry.radius, records[1].geometry.radius);
    }

    #[test]
    fn root_local_basis_is_not_parent_rotated_but_translation_uses_one_parent() {
        let mut parent = matrix([ONE, 2 * ONE, 3 * ONE], [ONE; 3]);
        parent[0] = 0;
        parent[2] = ONE;
        parent[8] = -ONE;
        parent[10] = 0;
        let child = matrix([4 * ONE, 5 * ONE, 6 * ONE], [2 * ONE, 3 * ONE, 4 * ONE]);
        let objects = parsed(
            &[
                Group0Spec {
                    parent: -1,
                    class: 1 << 16,
                    matrix: parent,
                    values: vec![],
                },
                Group0Spec {
                    parent: 0,
                    class: 1 << 29,
                    matrix: child,
                    values: vec![0],
                },
            ],
            &[[2 * ONE; 3]],
        );
        let built = build_parsed_static_collision_scene(&main_grid(), &objects).unwrap();
        let geometry = built.scene.records()[0].geometry;

        let expected_matrix = matmul_q16(&parent, &child);
        let rows = [
            [child[0], child[1], child[2]],
            [child[4], child[5], child[6]],
            [child[8], child[9], child[10]],
        ];
        let expected = build_static_obb_geometry(StaticObbBuildInput {
            collision_translation: vec3_from_raw([
                expected_matrix[12],
                expected_matrix[13],
                expected_matrix[14],
            ]),
            normalized_root_local_basis: rows
                .map(normalize_compiled_lane_vector)
                .map(vec3_from_raw),
            root_scale: vec3_from_raw(rows.map(row_length_q16)),
            mesh_bound: vec3_from_raw([2 * ONE; 3]),
        })
        .unwrap();
        assert_eq!(geometry, StaticNarrowObb::from(expected));
    }

    #[test]
    fn first_group1_reference_wins_and_invalid_first_reference_is_an_error() {
        let objects = parsed(
            &[spec(-1, 1 << 29, [0; 3], &[1, 0])],
            &[[2 * ONE; 3], [4 * ONE; 3]],
        );
        let built = build_parsed_static_collision_scene(&main_grid(), &objects).unwrap();
        let expected = build_static_obb_geometry(StaticObbBuildInput {
            collision_translation: Vec3::ZERO,
            normalized_root_local_basis: [
                vec3_from_raw(normalize_compiled_lane_vector([ONE, 0, 0])),
                vec3_from_raw(normalize_compiled_lane_vector([0, ONE, 0])),
                vec3_from_raw(normalize_compiled_lane_vector([0, 0, ONE])),
            ],
            root_scale: vec3_from_raw([
                row_length_q16([ONE, 0, 0]),
                row_length_q16([0, ONE, 0]),
                row_length_q16([0, 0, ONE]),
            ]),
            mesh_bound: vec3_from_raw([4 * ONE; 3]),
        })
        .unwrap();
        assert_eq!(
            built.scene.records()[0].geometry.axis_radius,
            expected.projection.axis_radius
        );

        let invalid = parsed(&[spec(-1, 1 << 29, [0; 3], &[2, 0])], &[[2 * ONE; 3]]);
        assert_eq!(
            build_parsed_static_collision_scene(&main_grid(), &invalid),
            Err(ParsedStaticCollisionBuildError::Group1OrdinalOutOfBounds {
                root_index: 0,
                mesh_node_index: 0,
                ordinal: 2,
                record_count: 1,
            })
        );
    }

    #[test]
    fn zero_scaled_candidate_does_not_consume_an_ordinal_and_memberships_prepend() {
        let objects = parsed(
            &[
                spec(-1, 1 << 29, [0; 3], &[0]),
                spec(-1, 1 << 29, [0; 3], &[1]),
                spec(-1, 1 << 29, [0; 3], &[1]),
            ],
            &[[0, ONE, ONE], [2 * ONE; 3]],
        );
        let built = build_parsed_static_collision_scene(&main_grid(), &objects).unwrap();
        assert_eq!(
            built
                .scene
                .records()
                .iter()
                .map(|record| record.collision_ordinal)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        let cell = StaticCollisionNodeId::new(built.grid.cell_of_point(0, 0)).unwrap();
        assert_eq!(built.scene.node_records(cell), [1, 0]);
    }

    #[test]
    fn translated_geometry_center_selects_a_nonzero_grid_cell() {
        let translation = [3 * ONE, 0, ONE];
        let objects = parsed(&[spec(-1, 1 << 29, translation, &[0])], &[[2 * ONE; 3]]);
        let built = build_parsed_static_collision_scene(&main_grid(), &objects).unwrap();
        let geometry = built.scene.records()[0].geometry;
        let expected_cell = built
            .grid
            .cell_of_point(geometry.center.x.raw(), geometry.center.z.raw());

        assert_ne!(expected_cell, 0);
        assert_eq!(geometry.center.x.raw(), translation[0]);
        assert_eq!(geometry.center.z.raw(), translation[2]);
        assert_eq!(
            built
                .scene
                .node_records(StaticCollisionNodeId::new(expected_cell).unwrap()),
            [0]
        );
    }

    #[test]
    fn collision_ordinal_overflow_is_atomic() {
        let objects = parsed(&[spec(-1, 1 << 29, [0; 3], &[0])], &[[2 * ONE; 3]]);
        let object_records: Vec<_> = objects.group0_records().collect();
        let group1_records: Vec<_> = objects.group1_records().collect();
        let grid =
            derive_cell_grid_from_main_translations(&[[0, 0, 0], [0, 0, ONE], [4 * ONE, 0, ONE]])
                .unwrap();
        let rows = [[ONE, 0, 0], [0, ONE, 0], [0, 0, ONE]];
        let geometry = StaticNarrowObb::from(
            build_static_obb_geometry(StaticObbBuildInput {
                collision_translation: Vec3::ZERO,
                normalized_root_local_basis: rows.map(vec3_from_raw),
                root_scale: vec3_from_raw([ONE; 3]),
                mesh_bound: vec3_from_raw([2 * ONE; 3]),
            })
            .unwrap(),
        );
        let mut records = vec![
            StaticCollisionRecord {
                collision_ordinal: 0,
                raw_material_flags: 0,
                geometry,
            };
            usize::from(u16::MAX) + 1
        ];
        let mut memberships = vec![(StaticCollisionNodeId::new(0).unwrap(), 0)];
        let record_len = records.len();
        let membership_len = memberships.len();

        assert_eq!(
            emit_candidate(
                0,
                0,
                0,
                &object_records,
                &group1_records,
                &grid,
                matrix([0; 3], [ONE; 3]),
                [ONE; 3],
                rows,
                &mut records,
                &mut memberships,
            ),
            Err(ParsedStaticCollisionBuildError::CollisionOrdinalOverflow {
                record_count: record_len,
            })
        );
        assert_eq!(records.len(), record_len);
        assert_eq!(memberships.len(), membership_len);
    }

    #[test]
    fn malformed_forward_parent_fails_before_building_records() {
        let objects = parsed(&[spec(0, 1 << 29, [0; 3], &[0])], &[[2 * ONE; 3]]);
        assert_eq!(
            build_parsed_static_collision_scene(&main_grid(), &objects),
            Err(ParsedStaticCollisionBuildError::ObjectGraph(
                SceneAssemblyError::ForwardParent { node: 0, parent: 0 }
            ))
        );
    }
}
