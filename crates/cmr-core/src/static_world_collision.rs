//! Static-scenery collision orchestration from the official-EU mechanics path.
//!
//! `FUN_0043afd0` runs `FUN_0043d5d0` before per-car mechanics. That wrapper
//! queries four cells from a 4,096-cell index, tests the current eight-point car
//! hull, accumulates linear/angular impulses, updates a distinct angular-state
//! vector, and rotates a five-ID contact history. Native geometry construction
//! and response arithmetic are still under recovery, so callers supply an
//! explicit kernel. Previous X/Z is depenetration input after current overlap;
//! this is deliberately not a swept/TOI collision API.

use std::fmt;

use crate::c3d_terrain_cull::MAX_CELLS;
use crate::fixed_point::{Fixed16, Vec3};

pub const STATIC_CONTACT_CAPACITY: usize = 5;
/// Literal identity passed by the static-scenery wrapper to its response path.
pub const STATIC_SCENERY_CONTACT_ID: u16 = 0;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StaticCollisionNodeId(u16);

impl StaticCollisionNodeId {
    pub fn new(value: i32) -> Result<Self, StaticCollisionNodeIdError> {
        if value < 0 || value as usize >= MAX_CELLS {
            Err(StaticCollisionNodeIdError { value })
        } else {
            Ok(Self(value as u16))
        }
    }

    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCollisionNodeIdError {
    pub value: i32,
}

impl fmt::Display for StaticCollisionNodeIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "collision cell {} is outside 0..{MAX_CELLS}", self.value)
    }
}

impl std::error::Error for StaticCollisionNodeIdError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollisionRecord<G> {
    /// Sequential collision-record ordinal (`record+0x58`). This is not the
    /// static contact-history identity.
    pub collision_ordinal: u16,
    pub raw_material_flags: u8,
    /// Recovered collision geometry, never renderer-derived bounds.
    pub geometry: G,
}

impl<G> StaticCollisionRecord<G> {
    #[must_use]
    pub const fn raw_material_id(&self) -> u8 {
        self.raw_material_flags & 0x7f
    }

    #[must_use]
    pub const fn alternate_collision_path(&self) -> bool {
        self.raw_material_flags & 0x80 != 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollisionScene<G> {
    records: Vec<StaticCollisionRecord<G>>,
    nodes: Box<[Vec<usize>; MAX_CELLS]>,
}

impl<G> StaticCollisionScene<G> {
    /// Memberships arrive in builder traversal order. Each is prepended to its
    /// cell, reproducing the native linked-list reversal. Invalid references
    /// fail before a scene is published.
    pub fn new(
        records: Vec<StaticCollisionRecord<G>>,
        memberships: impl IntoIterator<Item = (StaticCollisionNodeId, usize)>,
    ) -> Result<Self, StaticCollisionSceneError> {
        let mut nodes = Box::new(std::array::from_fn(|_| Vec::new()));
        for (node, record_index) in memberships {
            if record_index >= records.len() {
                return Err(StaticCollisionSceneError::RecordIndex {
                    node,
                    record_index,
                    record_count: records.len(),
                });
            }
            nodes[node.index()].insert(0, record_index);
        }
        Ok(Self { records, nodes })
    }

    #[must_use]
    pub fn records(&self) -> &[StaticCollisionRecord<G>] {
        &self.records
    }

    #[must_use]
    pub fn node_records(&self, node: StaticCollisionNodeId) -> &[usize] {
        &self.nodes[node.index()]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticCollisionSceneError {
    RecordIndex {
        node: StaticCollisionNodeId,
        record_index: usize,
        record_count: usize,
    },
}

impl fmt::Display for StaticCollisionSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::RecordIndex {
                node,
                record_index,
                record_count,
            } => write!(
                f,
                "collision cell {} references record {record_index}, arena length is {record_count}",
                node.index()
            ),
        }
    }
}

impl std::error::Error for StaticCollisionSceneError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCarCollisionFrame {
    pub center: Vec3,
    /// Response/depenetration input only, not swept admission.
    pub previous_center_xz: [Fixed16; 2],
    pub world_hull_points: [Vec3; 8],
    pub active_nodes: [Option<StaticCollisionNodeId>; 4],
    pub linear_impulse_3cc: Vec3,
    pub angular_impulse_3d8: Vec3,
    /// Persistent `car+0x3f0..+0x3f8`, distinct from contact normals.
    pub angular_state_3f0: Vec3,
    pub angular_limit_2c: Vec3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticContactHistory {
    pub previous_ids: [u16; STATIC_CONTACT_CAPACITY],
    pub previous_len: u8,
    pub current_ids: [u16; STATIC_CONTACT_CAPACITY],
    pub current_len: u8,
    /// Reconstruction diagnostic only; the shipped path silently stops storing
    /// IDs at five and has no corresponding field.
    pub current_overflow: bool,
}

impl Default for StaticContactHistory {
    fn default() -> Self {
        Self {
            previous_ids: [0; STATIC_CONTACT_CAPACITY],
            previous_len: 0,
            current_ids: [0; STATIC_CONTACT_CAPACITY],
            current_len: 0,
            current_overflow: false,
        }
    }
}

impl StaticContactHistory {
    fn begin_tick(self) -> Self {
        Self {
            previous_ids: self.current_ids,
            previous_len: self.current_len.min(STATIC_CONTACT_CAPACITY as u8),
            current_ids: [0; STATIC_CONTACT_CAPACITY],
            current_len: 0,
            current_overflow: false,
        }
    }

    fn contained_last_tick(&self, id: u16) -> bool {
        self.previous_ids[..usize::from(self.previous_len)].contains(&id)
    }

    fn push(&mut self, id: u16) -> bool {
        let index = usize::from(self.current_len);
        if index < STATIC_CONTACT_CAPACITY {
            self.current_ids[index] = id;
            self.current_len += 1;
            true
        } else {
            self.current_overflow = true;
            false
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCollisionContribution {
    pub contact_normal: Vec3,
    pub linear_impulse_3cc: Vec3,
    pub angular_impulse_3d8: Vec3,
    pub angular_state_3f0: Vec3,
}

/// Sequential state visible to each native response invocation. The response
/// reads and rewrites these values per contact, including component clamps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCollisionRunningState {
    pub linear_impulse_3cc: Vec3,
    pub angular_impulse_3d8: Vec3,
    pub angular_state_3f0: Vec3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticResolvedContact {
    pub collision_ordinal: u16,
    pub contact_history_id: u16,
    pub raw_material_id: u8,
    pub alternate_collision_path: bool,
    pub contact_normal: Vec3,
    pub repeated_from_previous_tick: bool,
    pub stored_in_current_history: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollisionEffects {
    pub linear_impulse_3cc: Vec3,
    pub angular_impulse_3d8: Vec3,
    pub angular_state_3f0: Vec3,
    pub history: StaticContactHistory,
    pub contacts: Vec<StaticResolvedContact>,
}

pub trait StaticCollisionKernel<G> {
    type Error;

    /// `None` means no overlap at the current pose.
    fn resolve(
        &mut self,
        frame: &StaticCarCollisionFrame,
        record: &StaticCollisionRecord<G>,
        running: StaticCollisionRunningState,
    ) -> Result<Option<StaticCollisionContribution>, Self::Error>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollisionExecutionError<E> {
    pub node: StaticCollisionNodeId,
    pub record_index: usize,
    pub source: E,
}

/// Resolve ordered static-object candidates. Kernel errors return no aggregate,
/// an explicit safe reconstruction policy.
pub fn resolve_static_world_collisions<G, K: StaticCollisionKernel<G>>(
    scene: &StaticCollisionScene<G>,
    frame: &StaticCarCollisionFrame,
    prior_history: StaticContactHistory,
    kernel: &mut K,
) -> Result<StaticCollisionEffects, StaticCollisionExecutionError<K::Error>> {
    let mut effects = StaticCollisionEffects {
        linear_impulse_3cc: frame.linear_impulse_3cc,
        angular_impulse_3d8: frame.angular_impulse_3d8,
        angular_state_3f0: frame.angular_state_3f0,
        history: prior_history.begin_tick(),
        contacts: Vec::new(),
    };
    for node in frame.active_nodes.into_iter().flatten() {
        for &record_index in scene.node_records(node) {
            let record = &scene.records[record_index];
            let contribution = kernel
                .resolve(
                    frame,
                    record,
                    StaticCollisionRunningState {
                        linear_impulse_3cc: effects.linear_impulse_3cc,
                        angular_impulse_3d8: effects.angular_impulse_3d8,
                        angular_state_3f0: effects.angular_state_3f0,
                    },
                )
                .map_err(|source| StaticCollisionExecutionError {
                    node,
                    record_index,
                    source,
                })?;
            let Some(contribution) = contribution else {
                continue;
            };
            effects.linear_impulse_3cc = contribution.linear_impulse_3cc;
            effects.angular_impulse_3d8 = contribution.angular_impulse_3d8;
            effects.angular_state_3f0 = contribution.angular_state_3f0;
            let repeated = effects
                .history
                .contained_last_tick(STATIC_SCENERY_CONTACT_ID);
            let stored = effects.history.push(STATIC_SCENERY_CONTACT_ID);
            effects.contacts.push(StaticResolvedContact {
                collision_ordinal: record.collision_ordinal,
                contact_history_id: STATIC_SCENERY_CONTACT_ID,
                raw_material_id: record.raw_material_id(),
                alternate_collision_path: record.alternate_collision_path(),
                contact_normal: contribution.contact_normal,
                repeated_from_previous_tick: repeated,
                stored_in_current_history: stored,
            });
        }
    }
    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(value: i32) -> StaticCollisionNodeId {
        StaticCollisionNodeId::new(value).unwrap()
    }
    fn vec3(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(
            Fixed16::from_raw(x),
            Fixed16::from_raw(y),
            Fixed16::from_raw(z),
        )
    }
    fn record(id: u16, material: u8, geometry: u8) -> StaticCollisionRecord<u8> {
        StaticCollisionRecord {
            collision_ordinal: id,
            raw_material_flags: material,
            geometry,
        }
    }
    fn frame(nodes: [Option<StaticCollisionNodeId>; 4]) -> StaticCarCollisionFrame {
        StaticCarCollisionFrame {
            center: Vec3::ZERO,
            previous_center_xz: [Fixed16::ZERO; 2],
            world_hull_points: [Vec3::ZERO; 8],
            active_nodes: nodes,
            linear_impulse_3cc: Vec3::ZERO,
            angular_impulse_3d8: Vec3::ZERO,
            angular_state_3f0: vec3(7, 8, 9),
            angular_limit_2c: vec3(10, 11, 12),
        }
    }

    #[derive(Default)]
    struct Script {
        calls: Vec<(u16, [i32; 2], StaticCollisionRunningState)>,
        fail: Option<u16>,
    }
    impl StaticCollisionKernel<u8> for Script {
        type Error = u16;
        fn resolve(
            &mut self,
            frame: &StaticCarCollisionFrame,
            record: &StaticCollisionRecord<u8>,
            running: StaticCollisionRunningState,
        ) -> Result<Option<StaticCollisionContribution>, Self::Error> {
            self.calls.push((
                record.collision_ordinal,
                frame.previous_center_xz.map(Fixed16::raw),
                running,
            ));
            if self.fail == Some(record.collision_ordinal) {
                return Err(record.collision_ordinal);
            }
            if record.geometry == 0 {
                return Ok(None);
            }
            Ok(Some(StaticCollisionContribution {
                contact_normal: vec3(i32::from(record.geometry), 0, 0),
                linear_impulse_3cc: running.linear_impulse_3cc.add(vec3(
                    i32::MAX,
                    i32::from(record.geometry),
                    0,
                )),
                angular_impulse_3d8: running.angular_impulse_3d8.add(vec3(
                    0,
                    0,
                    i32::from(record.geometry),
                )),
                angular_state_3f0: running.angular_state_3f0.add(vec3(1, 0, 0)),
            }))
        }
    }

    #[test]
    fn node_domain_and_membership_are_checked_and_ordered() {
        assert_eq!(node(0).index(), 0);
        assert_eq!(node(4095).index(), 4095);
        assert!(StaticCollisionNodeId::new(-1).is_err());
        assert!(StaticCollisionNodeId::new(4096).is_err());
        assert!(StaticCollisionScene::new(vec![record(1, 0, 1)], [(node(4), 1)]).is_err());
        let scene = StaticCollisionScene::new(
            vec![record(10, 0, 1), record(11, 0, 1)],
            [(node(7), 1), (node(7), 0)],
        )
        .unwrap();
        assert_eq!(scene.node_records(node(7)), &[0, 1]);
    }

    #[test]
    fn ordered_current_contacts_accumulate_and_rotate_history() {
        let scene = StaticCollisionScene::new(
            vec![record(10, 0x85, 1), record(11, 0x7f, 0), record(12, 3, 2)],
            [(node(2), 0), (node(2), 1), (node(3), 2)],
        )
        .unwrap();
        let mut input = frame([Some(node(2)), Some(node(3)), None, None]);
        input.previous_center_xz = [Fixed16::from_raw(123), Fixed16::from_raw(-456)];
        input.linear_impulse_3cc = vec3(5, 6, 7);
        input.angular_impulse_3d8 = vec3(1, 2, 3);
        let history = StaticContactHistory {
            current_ids: [0, 99, 0, 0, 0],
            current_len: 2,
            ..Default::default()
        };
        let mut kernel = Script::default();
        let effects =
            resolve_static_world_collisions(&scene, &input, history, &mut kernel).unwrap();
        assert_eq!(
            kernel.calls.iter().map(|v| v.0).collect::<Vec<_>>(),
            [11, 10, 12]
        );
        assert!(kernel.calls.iter().all(|v| v.1 == [123, -456]));
        assert_eq!(effects.linear_impulse_3cc, vec3(3, 9, 7));
        assert_eq!(effects.angular_impulse_3d8, vec3(1, 2, 6));
        assert_eq!(
            kernel.calls[2].2.linear_impulse_3cc,
            vec3(i32::MIN + 4, 7, 7)
        );
        assert_eq!(kernel.calls[2].2.angular_impulse_3d8, vec3(1, 2, 4));
        assert_eq!(effects.angular_state_3f0, vec3(9, 8, 9));
        assert_eq!(effects.history.previous_ids[..2], [0, 99]);
        assert_eq!(effects.history.current_ids[..2], [0, 0]);
        assert_eq!(effects.contacts[0].collision_ordinal, 10);
        assert_eq!(effects.contacts[0].contact_history_id, 0);
        assert_eq!(effects.contacts[0].raw_material_id, 5);
        assert!(effects.contacts[0].alternate_collision_path);
        assert!(effects.contacts[0].repeated_from_previous_tick);
        assert!(effects.contacts[1].repeated_from_previous_tick);
    }

    #[test]
    fn path_crossing_without_current_overlap_is_not_swept_collision() {
        let scene = StaticCollisionScene::new(vec![record(10, 0, 0)], [(node(1), 0)]).unwrap();
        let mut input = frame([Some(node(1)), None, None, None]);
        input.previous_center_xz = [Fixed16::from_int(-100), Fixed16::from_int(100)];
        input.center = vec3(100 << 16, 0, -(100 << 16));
        let effects = resolve_static_world_collisions(
            &scene,
            &input,
            Default::default(),
            &mut Script::default(),
        )
        .unwrap();
        assert!(effects.contacts.is_empty());
        assert_eq!(effects.linear_impulse_3cc, Vec3::ZERO);
    }

    #[test]
    fn history_saturates_but_all_contacts_still_contribute() {
        let records = (0..7).map(|id| record(id, id as u8, 1)).collect();
        let scene = StaticCollisionScene::new(records, (0..7).map(|i| (node(8), i))).unwrap();
        let effects = resolve_static_world_collisions(
            &scene,
            &frame([Some(node(8)), None, None, None]),
            Default::default(),
            &mut Script::default(),
        )
        .unwrap();
        assert_eq!(effects.contacts.len(), 7);
        assert_eq!(effects.history.current_ids, [0; 5]);
        assert_eq!(effects.history.current_len, 5);
        assert!(effects.history.current_overflow);
        assert_eq!(effects.linear_impulse_3cc.x.raw(), i32::MAX - 6);
    }

    #[test]
    fn kernel_failure_reports_ordered_record_without_an_aggregate() {
        let scene = StaticCollisionScene::new(
            vec![record(20, 0, 1), record(21, 0, 1)],
            [(node(9), 0), (node(9), 1)],
        )
        .unwrap();
        let mut kernel = Script {
            fail: Some(21),
            ..Default::default()
        };
        let error = resolve_static_world_collisions(
            &scene,
            &frame([Some(node(9)), None, None, None]),
            Default::default(),
            &mut kernel,
        )
        .unwrap_err();
        assert_eq!(
            error,
            StaticCollisionExecutionError {
                node: node(9),
                record_index: 1,
                source: 21
            }
        );
    }
}
