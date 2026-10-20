//! Combined resource preparation and borrowed root-menu C3D topology plans.
//!
//! Construction deliberately joins selectors to the exact parsed asset set
//! from which they were prepared. The result owns decoded resources while its
//! typed geometry views borrow the source C3D files. Runtime vertex conversion,
//! side-vector interpretation, palette policy, and backend upload remain later
//! boundaries.

use std::fmt;

use cmr_formats::{
    bitmap::BitmapLimits,
    c3d::{C3D_GROUP0_VALUES_OFFSET, C3dGroup0Record, C3dGroup1Record},
};

use crate::{
    c3d_batch::{C3dBatchPlanError, C3dConsecutiveBatch, plan_consecutive_batches},
    root_menu_assets::{RootMenuObjectIdentity, VisualVariant},
    root_menu_c3d_assets::{ROOT_MENU_C3D_REQUEST_COUNT, RootMenuC3dSet, RootMenuC3dSlot},
    root_menu_c3d_resources::{
        C3dPreparedResource, C3dPreparedResourceAccessError, C3dPreparedResourceId,
        C3dPreparedResourceProvenance, C3dPreparedSelector, C3dResourceArenaIdentityExhausted,
        RootMenuC3dResourceDiagnostic, RootMenuC3dResourceSet, prepare_root_menu_c3d_resources,
    },
};

/// One source group-1 record and its consecutive prepared-selector batches.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dGroup1Topology<'asset> {
    record_index: usize,
    record: C3dGroup1Record<'asset>,
    batches: Vec<C3dConsecutiveBatch<'asset, C3dPreparedSelector>>,
}

impl<'asset> RootMenuC3dGroup1Topology<'asset> {
    #[must_use]
    pub const fn record_index(&self) -> usize {
        self.record_index
    }

    #[must_use]
    pub const fn record_offset(&self) -> usize {
        self.record.offset()
    }

    #[must_use]
    pub const fn record(&self) -> C3dGroup1Record<'asset> {
        self.record
    }

    #[must_use]
    pub fn batches(&self) -> &[C3dConsecutiveBatch<'asset, C3dPreparedSelector>] {
        &self.batches
    }
}

/// One checked group-0 value that selects a group-1 topology template.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootMenuC3dGroup1TopologyReference {
    value_index: usize,
    source_offset: usize,
    group1_ordinal: u16,
}

impl RootMenuC3dGroup1TopologyReference {
    #[must_use]
    pub const fn value_index(self) -> usize {
        self.value_index
    }

    #[must_use]
    pub const fn source_offset(self) -> usize {
        self.source_offset
    }

    #[must_use]
    pub const fn group1_ordinal(self) -> u16 {
        self.group1_ordinal
    }
}

/// One source group-0 record and its ordered, checked group-1 references.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dGroup0Topology<'asset> {
    record_index: usize,
    record: C3dGroup0Record<'asset>,
    references: Vec<RootMenuC3dGroup1TopologyReference>,
}

impl<'asset> RootMenuC3dGroup0Topology<'asset> {
    #[must_use]
    pub const fn record_index(&self) -> usize {
        self.record_index
    }

    #[must_use]
    pub const fn record_offset(&self) -> usize {
        self.record.offset()
    }

    #[must_use]
    pub const fn record(&self) -> C3dGroup0Record<'asset> {
        self.record
    }

    pub fn references(
        &self,
    ) -> impl ExactSizeIterator<Item = RootMenuC3dGroup1TopologyReference> + '_ {
        self.references.iter().copied()
    }
}

/// Ordered topology plans for one successfully prepared fixed-slot object.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dObjectTopology<'asset> {
    slot: RootMenuC3dSlot,
    identity: RootMenuObjectIdentity,
    path: &'static str,
    group0_records: Vec<RootMenuC3dGroup0Topology<'asset>>,
    groups: Vec<RootMenuC3dGroup1Topology<'asset>>,
}

impl<'asset> RootMenuC3dObjectTopology<'asset> {
    #[must_use]
    pub const fn slot(&self) -> RootMenuC3dSlot {
        self.slot
    }

    #[must_use]
    pub const fn identity(&self) -> RootMenuObjectIdentity {
        self.identity
    }

    #[must_use]
    pub const fn path(&self) -> &'static str {
        self.path
    }

    pub fn group0_records(
        &self,
    ) -> impl ExactSizeIterator<Item = &RootMenuC3dGroup0Topology<'asset>> {
        self.group0_records.iter()
    }

    #[must_use]
    pub fn group0_record(&self, record_index: usize) -> Option<&RootMenuC3dGroup0Topology<'asset>> {
        self.group0_records.get(record_index)
    }

    pub fn groups(&self) -> impl ExactSizeIterator<Item = &RootMenuC3dGroup1Topology<'asset>> {
        self.groups.iter()
    }

    #[must_use]
    pub fn group(&self, record_index: usize) -> Option<&RootMenuC3dGroup1Topology<'asset>> {
        self.groups.get(record_index)
    }
}

/// Owned prepared resources paired with borrowed, fixed-slot topology plans.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dTopology<'asset> {
    variant: VisualVariant,
    resources: RootMenuC3dResourceSet,
    slots: [Option<RootMenuC3dObjectTopology<'asset>>; ROOT_MENU_C3D_REQUEST_COUNT],
}

impl<'asset> RootMenuC3dTopology<'asset> {
    #[must_use]
    pub const fn variant(&self) -> VisualVariant {
        self.variant
    }

    #[must_use]
    pub const fn provenance(&self) -> C3dPreparedResourceProvenance {
        self.resources.provenance()
    }

    pub fn slots(
        &self,
    ) -> impl ExactSizeIterator<Item = (RootMenuC3dSlot, Option<&RootMenuC3dObjectTopology<'asset>>)>
    {
        RootMenuC3dSlot::ALL
            .into_iter()
            .map(|slot| (slot, self.get(slot)))
    }

    #[must_use]
    pub fn get(&self, slot: RootMenuC3dSlot) -> Option<&RootMenuC3dObjectTopology<'asset>> {
        self.slots[slot.index()].as_ref()
    }

    #[must_use]
    pub fn loaded_len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    pub fn diagnostics(&self) -> impl ExactSizeIterator<Item = &RootMenuC3dResourceDiagnostic> {
        self.resources.diagnostics()
    }

    #[must_use]
    pub fn resource_len(&self) -> usize {
        self.resources.resource_len()
    }

    pub fn resources(
        &self,
    ) -> impl ExactSizeIterator<Item = (C3dPreparedResourceId, &C3dPreparedResource)> {
        self.resources.resources()
    }

    pub fn resource(
        &self,
        id: C3dPreparedResourceId,
    ) -> Result<&C3dPreparedResource, C3dPreparedResourceAccessError> {
        self.resources.resource(id)
    }

    /// Resolves a prepared selector without assigning a resource to `Missing`.
    pub fn resolve_selector(
        &self,
        selector: C3dPreparedSelector,
    ) -> Result<Option<&C3dPreparedResource>, C3dPreparedResourceAccessError> {
        match selector {
            C3dPreparedSelector::Resource(id) => self.resource(id).map(Some),
            C3dPreparedSelector::Missing => Ok(None),
        }
    }
}

/// Failure to construct the combined resource/topology value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RootMenuC3dTopologyError {
    ArenaIdentity(C3dResourceArenaIdentityExhausted),
    Group1Correspondence {
        slot: RootMenuC3dSlot,
        identity: RootMenuObjectIdentity,
        path: &'static str,
        source_count: usize,
        selector_count: usize,
    },
    Group1Metadata {
        slot: RootMenuC3dSlot,
        identity: RootMenuObjectIdentity,
        path: &'static str,
        expected_record_index: usize,
        selector_record_index: usize,
        source_record_offset: usize,
        selector_record_offset: usize,
    },
    Group1ReferenceOutOfBounds {
        slot: RootMenuC3dSlot,
        identity: RootMenuObjectIdentity,
        path: &'static str,
        group0_record_index: usize,
        group0_record_offset: usize,
        value_index: usize,
        source_offset: usize,
        ordinal: u16,
        group1_count: usize,
    },
    Batch {
        slot: RootMenuC3dSlot,
        identity: RootMenuObjectIdentity,
        path: &'static str,
        record_index: usize,
        record_offset: usize,
        source: C3dBatchPlanError,
    },
}

impl fmt::Display for RootMenuC3dTopologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ArenaIdentity(source) => source.fmt(formatter),
            Self::Group1Correspondence {
                slot,
                identity,
                path,
                source_count,
                selector_count,
            } => write!(
                formatter,
                "root-menu C3D slot {slot:?} ({identity:?}) at {path} has {source_count} source group-1 records but {selector_count} prepared selector groups"
            ),
            Self::Group1Metadata {
                slot,
                identity,
                path,
                expected_record_index,
                selector_record_index,
                source_record_offset,
                selector_record_offset,
            } => write!(
                formatter,
                "root-menu C3D slot {slot:?} ({identity:?}) at {path} source group-1 record {expected_record_index} at offset {source_record_offset} does not match prepared selector group {selector_record_index} at offset {selector_record_offset}"
            ),
            Self::Batch {
                slot,
                identity,
                path,
                record_index,
                record_offset,
                source,
            } => write!(
                formatter,
                "cannot plan root-menu C3D slot {slot:?} ({identity:?}) at {path} group-1 record {record_index} at offset {record_offset}: {source}"
            ),
            Self::Group1ReferenceOutOfBounds {
                slot,
                identity,
                path,
                group0_record_index,
                group0_record_offset,
                value_index,
                source_offset,
                ordinal,
                group1_count,
            } => write!(
                formatter,
                "root-menu C3D slot {slot:?} ({identity:?}) at {path} group-0 record {group0_record_index} at offset {group0_record_offset} value {value_index} at offset {source_offset} has group-1 ordinal {ordinal}, outside {group1_count} records"
            ),
        }
    }
}

impl std::error::Error for RootMenuC3dTopologyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ArenaIdentity(source) => Some(source),
            Self::Batch { source, .. } => Some(source),
            Self::Group1Correspondence { .. }
            | Self::Group1Metadata { .. }
            | Self::Group1ReferenceOutOfBounds { .. } => None,
        }
    }
}

/// Prepares root-menu resources and joins their selectors to exact source records.
///
/// Resource-preparation failures retain the existing fixed-slot diagnostic and
/// continuation policy. A topology failure returns no partial combined value.
pub fn prepare_root_menu_c3d_topology<'asset>(
    assets: &'asset RootMenuC3dSet,
    bitmap_limits: BitmapLimits,
) -> Result<RootMenuC3dTopology<'asset>, RootMenuC3dTopologyError> {
    let resources = prepare_root_menu_c3d_resources(assets, bitmap_limits)
        .map_err(RootMenuC3dTopologyError::ArenaIdentity)?;
    let mut slots = std::array::from_fn(|_| None);

    for slot in RootMenuC3dSlot::ALL {
        let (Some(asset), Some(prepared)) = (assets.get(slot), resources.get(slot)) else {
            continue;
        };
        let source_records = asset.file.group1_records().collect::<Vec<_>>();
        let selector_groups = prepared.group1_selectors().collect::<Vec<_>>();
        if source_records.len() != selector_groups.len() {
            return Err(RootMenuC3dTopologyError::Group1Correspondence {
                slot,
                identity: prepared.identity(),
                path: prepared.path(),
                source_count: source_records.len(),
                selector_count: selector_groups.len(),
            });
        }

        let mut groups = Vec::with_capacity(source_records.len());
        for (record_index, (record, selectors)) in
            source_records.into_iter().zip(selector_groups).enumerate()
        {
            if selectors.record_index() != record_index
                || selectors.record_offset() != record.offset()
            {
                return Err(RootMenuC3dTopologyError::Group1Metadata {
                    slot,
                    identity: prepared.identity(),
                    path: prepared.path(),
                    expected_record_index: record_index,
                    selector_record_index: selectors.record_index(),
                    source_record_offset: record.offset(),
                    selector_record_offset: selectors.record_offset(),
                });
            }
            let batches =
                plan_consecutive_batches(record, selectors.selectors()).map_err(|source| {
                    RootMenuC3dTopologyError::Batch {
                        slot,
                        identity: prepared.identity(),
                        path: prepared.path(),
                        record_index,
                        record_offset: record.offset(),
                        source,
                    }
                })?;
            groups.push(RootMenuC3dGroup1Topology {
                record_index,
                record,
                batches,
            });
        }

        let group1_count = groups.len();
        let mut group0_records = Vec::new();
        for (record_index, record) in asset.file.group0_records().enumerate() {
            let mut references = Vec::with_capacity(record.values().len());
            for (value_index, ordinal) in record.values().enumerate() {
                let source_offset = record.offset() + C3D_GROUP0_VALUES_OFFSET + value_index * 2;
                if usize::from(ordinal) >= group1_count {
                    return Err(RootMenuC3dTopologyError::Group1ReferenceOutOfBounds {
                        slot,
                        identity: prepared.identity(),
                        path: prepared.path(),
                        group0_record_index: record_index,
                        group0_record_offset: record.offset(),
                        value_index,
                        source_offset,
                        ordinal,
                        group1_count,
                    });
                }
                references.push(RootMenuC3dGroup1TopologyReference {
                    value_index,
                    source_offset,
                    group1_ordinal: ordinal,
                });
            }
            group0_records.push(RootMenuC3dGroup0Topology {
                record_index,
                record,
                references,
            });
        }
        slots[slot.index()] = Some(RootMenuC3dObjectTopology {
            slot,
            identity: prepared.identity(),
            path: prepared.path(),
            group0_records,
            groups,
        });
    }

    Ok(RootMenuC3dTopology {
        variant: assets.variant(),
        resources,
        slots,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        convert::Infallible,
        fs,
        path::PathBuf,
        rc::Rc,
        sync::atomic::{AtomicU64, Ordering},
    };

    use cmr_assets::{
        AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
    };
    use cmr_formats::{
        bitmap::{BitmapError, BitmapLimits},
        c3d::{
            C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3D_FORMAT_VERSION, C3D_GROUP1_ENTRY24_LEN,
            C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN, C3D_HEADER_LEN, C3D_MAGIC, C3dLimits,
        },
        stored_asset::DecodeLimits,
    };
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::{
        display::{
            CooperativeMode, DisplayExtent, DisplaySurfaces, IndexedMode, IndexedVideoBackend,
            SurfaceTopology,
        },
        root_menu_assets::VisualVariant,
        root_menu_c3d_assets::{RootMenuC3dAngles, load_root_menu_c3d},
        root_menu_c3d_execute::{
            RootMenuC3dBatchBackend, RootMenuC3dBatchExecutor, RootMenuC3dBatchStage,
            RootMenuC3dExecuteError, bind_root_menu_c3d_resources,
        },
        root_menu_c3d_frame_plan::{
            ROOT_MENU_C3D_DRAW_FLAGS, ROOT_MENU_C3D_PRIMITIVE_CODE, ROOT_MENU_C3D_VERTEX_TYPE,
            RootMenuC3dFramePlanError, RootMenuC3dRootShapeIssue, plan_root_menu_c3d_frame,
        },
        root_menu_c3d_resources::{C3dPreparedResourceAccessError, C3dResourcePreparationError},
        root_menu_c3d_runtime::{
            C3dMode2CornerSource, C3dMode2LoadState, C3dSurfaceLayoutAxis, C3dSurfaceLayoutGates,
            RootMenuC3dMode2LoadStates, RootMenuC3dMode2MaterializationError,
            materialize_root_menu_mode2_pre_scale, materialize_root_menu_mode2_resource_scaled,
        },
        root_menu_c3d_scene::{
            PreparedRootMenuC3dPipeline, PreparedRootMenuC3dPipelineError, RootMenuC3dBindingState,
            RootMenuC3dDepthClear, RootMenuC3dFramePipeline, RootMenuC3dSceneBackend,
            RootMenuC3dSceneStage,
        },
        root_menu_c3d_transform::{
            ROOT_MENU_C3D_PROJECTION, ROOT_MENU_C3D_VIEW, root_menu_c3d_viewport,
            root_menu_c3d_world,
        },
        root_menu_c3d_visibility::RootMenuC3dVisibilityLatches,
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BatchEvent {
        World(crate::root_menu_c3d_transform::C3dMatrixWords),
        Binding(u8),
        Draw {
            batch_index: usize,
            selector_index: usize,
            vertex_type: u32,
            primitive_code: u32,
            draw_flags: u32,
        },
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct BatchBackendError(RootMenuC3dBatchStage);

    impl std::fmt::Display for BatchBackendError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "injected {:?} failure", self.0)
        }
    }

    impl std::error::Error for BatchBackendError {}

    #[derive(Default)]
    struct RecordingBatchBackend {
        events: Vec<BatchEvent>,
        fail: Option<(RootMenuC3dBatchStage, Option<u8>)>,
    }

    impl RecordingBatchBackend {
        fn record(
            &mut self,
            stage: RootMenuC3dBatchStage,
            binding: Option<u8>,
            event: BatchEvent,
        ) -> Result<(), BatchBackendError> {
            self.events.push(event);
            if self.fail == Some((stage, binding)) {
                Err(BatchBackendError(stage))
            } else {
                Ok(())
            }
        }
    }

    impl RootMenuC3dBatchBackend<u8> for RecordingBatchBackend {
        type Error = BatchBackendError;

        fn set_root_c3d_world(
            &mut self,
            world: crate::root_menu_c3d_transform::C3dMatrixWords,
        ) -> Result<(), Self::Error> {
            self.record(RootMenuC3dBatchStage::World, None, BatchEvent::World(world))
        }

        fn bind_root_c3d_resource(&mut self, binding: &u8) -> Result<(), Self::Error> {
            self.record(
                RootMenuC3dBatchStage::Binding,
                Some(*binding),
                BatchEvent::Binding(*binding),
            )
        }

        fn draw_root_c3d_batch(
            &mut self,
            command: &crate::root_menu_c3d_frame_plan::RootMenuC3dDrawCommand,
        ) -> Result<(), Self::Error> {
            let C3dPreparedSelector::Resource(resource_id) = command.selector() else {
                unreachable!("exact-root planner rejects missing selectors")
            };
            self.record(
                RootMenuC3dBatchStage::Draw,
                Some(resource_id.index() as u8),
                BatchEvent::Draw {
                    batch_index: command.batch_index(),
                    selector_index: resource_id.index(),
                    vertex_type: command.vertex_type(),
                    primitive_code: command.primitive_code(),
                    draw_flags: command.draw_flags(),
                },
            )
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct SceneBackendError(RootMenuC3dSceneStage);

    impl std::fmt::Display for SceneBackendError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "injected {:?} scene failure", self.0)
        }
    }

    impl std::error::Error for SceneBackendError {}

    struct RecordingSceneBatchBackend {
        batch: RecordingBatchBackend,
        scene_fail: Option<RootMenuC3dSceneStage>,
        tail_state: RootMenuC3dBindingState,
    }

    impl Default for RecordingSceneBatchBackend {
        fn default() -> Self {
            Self {
                batch: RecordingBatchBackend::default(),
                scene_fail: None,
                tail_state: RootMenuC3dBindingState::Preserved,
            }
        }
    }

    impl RecordingSceneBatchBackend {
        fn scene(&self, stage: RootMenuC3dSceneStage) -> Result<(), SceneBackendError> {
            if self.scene_fail == Some(stage) {
                Err(SceneBackendError(stage))
            } else {
                Ok(())
            }
        }

        fn binding_count(&self) -> usize {
            self.batch
                .events
                .iter()
                .filter(|event| matches!(event, BatchEvent::Binding(_)))
                .count()
        }
    }

    impl IndexedVideoBackend for RecordingSceneBatchBackend {
        type Error = SceneBackendError;
        type Surface = u8;
        type Palette = u8;

        fn create_device(&mut self) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn set_cooperative_mode(&mut self, _: CooperativeMode) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn set_display_mode(&mut self, _: IndexedMode) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn create_surfaces(
            &mut self,
            _: SurfaceTopology,
            _: IndexedMode,
        ) -> Result<DisplaySurfaces<u8>, Self::Error> {
            unreachable!()
        }
        fn create_palette(
            &mut self,
            _: &[cmr_formats::riff_palette::PaletteEntry; 256],
        ) -> Result<u8, Self::Error> {
            unreachable!()
        }
        fn attach_palette(&mut self, _: u8, _: u8) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn upload_palette(
            &mut self,
            _: u8,
            _: &[cmr_formats::riff_palette::PaletteEntry; 256],
        ) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn create_source(&mut self, _: &crate::surface::IndexedSurface) -> Result<u8, Self::Error> {
            unreachable!()
        }
        fn blit(&mut self, _: u8, _: u8, _: crate::blit::BlitPlan) -> Result<(), Self::Error> {
            unreachable!()
        }
        fn destroy_source(&mut self, _: u8) {
            unreachable!()
        }
    }

    impl RootMenuC3dBatchBackend<u8> for RecordingSceneBatchBackend {
        type Error = BatchBackendError;

        fn set_root_c3d_world(
            &mut self,
            world: crate::root_menu_c3d_transform::C3dMatrixWords,
        ) -> Result<(), Self::Error> {
            self.batch.set_root_c3d_world(world)
        }

        fn bind_root_c3d_resource(&mut self, binding: &u8) -> Result<(), Self::Error> {
            self.batch.bind_root_c3d_resource(binding)
        }

        fn draw_root_c3d_batch(
            &mut self,
            command: &crate::root_menu_c3d_frame_plan::RootMenuC3dDrawCommand,
        ) -> Result<(), Self::Error> {
            self.batch.draw_root_c3d_batch(command)
        }
    }

    impl RootMenuC3dSceneBackend for RecordingSceneBatchBackend {
        type SceneError = SceneBackendError;

        fn clear_root_c3d_depth(
            &mut self,
            _: u8,
            _: DisplayExtent,
        ) -> Result<(), Self::SceneError> {
            self.scene(RootMenuC3dSceneStage::ClearDepth)
        }
        fn set_root_c3d_view(
            &mut self,
            _: crate::root_menu_c3d_transform::C3dMatrixWords,
        ) -> Result<(), Self::SceneError> {
            self.scene(RootMenuC3dSceneStage::View)
        }
        fn set_root_c3d_viewport(
            &mut self,
            _: crate::root_menu_c3d_transform::C3dViewportWords,
        ) -> Result<(), Self::SceneError> {
            self.scene(RootMenuC3dSceneStage::Viewport)
        }
        fn begin_root_c3d_scene(&mut self, _: u8) -> Result<(), Self::SceneError> {
            self.scene(RootMenuC3dSceneStage::Begin)
        }
        fn reset_root_c3d_world(&mut self) -> Result<(), Self::SceneError> {
            self.scene(RootMenuC3dSceneStage::ResetWorld)
        }
        fn finish_root_c3d_scene_tail(
            &mut self,
        ) -> Result<RootMenuC3dBindingState, Self::SceneError> {
            self.scene(RootMenuC3dSceneStage::Tail)?;
            Ok(self.tail_state)
        }
        fn end_root_c3d_scene(&mut self) -> Result<(), Self::SceneError> {
            self.scene(RootMenuC3dSceneStage::End)
        }
    }

    #[test]
    fn aliases_and_missing_values_form_only_consecutive_equal_selector_runs() {
        let groups = [group(1, &[[0, 0, 0]; 5], &[0, 1, -1, -1, 0])];
        let blobs = [
            (*b"ALIAS\0first!", indexed_bmp(7, 10, 8)),
            (*b"ALIAS\0other!", indexed_bmp(7, 99, 8)),
        ];
        let fixture = fixture_for(&[(RootMenuC3dSlot::Championship, c3d(&groups, &blobs))]);
        let assets = load(&fixture);

        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let object = topology.get(RootMenuC3dSlot::Championship).unwrap();
        let batches = object.group(0).unwrap().batches();

        assert_eq!(batches.len(), 3);
        let first = batches[0].selector();
        assert!(matches!(first, C3dPreparedSelector::Resource(_)));
        assert_eq!(batches[0].entries_24().len(), 2);
        assert_eq!(batches[1].selector(), C3dPreparedSelector::Missing);
        assert_eq!(batches[1].entries_24().len(), 2);
        assert_eq!(batches[2].selector(), first);
        assert_eq!(
            topology.resolve_selector(C3dPreparedSelector::Missing),
            Ok(None)
        );
        assert_eq!(topology.resource_len(), 1);
    }

    #[test]
    fn preserves_group_order_offsets_source_order_and_borrowed_views() {
        let groups = [
            group(2, &[[1, 0, 1], [0, 1, 0]], &[0, 0]),
            group(1, &[[0, 0, 0]], &[0]),
        ];
        let blobs = [(*b"ORDERED!!!!!", indexed_bmp(3, 4, 8))];
        let fixture = fixture_for(&[(RootMenuC3dSlot::Rally, c3d(&groups, &blobs))]);
        let assets = load(&fixture);
        let source = assets.get(RootMenuC3dSlot::Rally).unwrap();
        let source_records = source.file.group1_records().collect::<Vec<_>>();
        let source_first_entry = source_records[0].entries_24().next().unwrap();

        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let object = topology.get(RootMenuC3dSlot::Rally).unwrap();
        let planned = object.groups().collect::<Vec<_>>();

        assert_eq!(planned.len(), 2);
        for (index, group) in planned.iter().enumerate() {
            assert_eq!(group.record_index(), index);
            assert_eq!(group.record_offset(), source_records[index].offset());
        }
        assert_eq!(planned[0].batches()[0].entries_24().len(), 2);
        assert!(std::ptr::eq(
            planned[0].batches()[0].entries_24()[0].bytes(),
            source_first_entry.bytes()
        ));
    }

    #[test]
    fn separate_topologies_keep_resource_ids_arena_scoped() {
        let groups = [group(1, &[[0, 0, 0]], &[0])];
        let blobs = [(*b"SCOPED-ID!!!", indexed_bmp(1, 2, 8))];
        let fixture = fixture_for(&[(RootMenuC3dSlot::TimeTrial, c3d(&groups, &blobs))]);
        let assets = load(&fixture);

        let first = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let second = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let first_id = first.resources().next().unwrap().0;
        let second_id = second.resources().next().unwrap().0;

        assert_ne!(first_id, second_id);
        assert_ne!(first.provenance(), second.provenance());
        assert_eq!(
            second.resource(first_id),
            Err(C3dPreparedResourceAccessError::Foreign)
        );
        assert!(first.resource(first_id).is_ok());
    }

    #[test]
    fn runtime_products_retain_their_topology_provenance() {
        let groups = [group(1, &[[0, 0, 0]], &[0])];
        let blobs = [(*b"PROVENANCE!!", indexed_bmp(1, 2, 8))];
        let fixture = fixture_for(&[(RootMenuC3dSlot::Rally, c3d(&groups, &blobs))]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let states = uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults);

        let pre_scale = materialize_root_menu_mode2_pre_scale(&topology, states).unwrap();
        let scaled = materialize_root_menu_mode2_resource_scaled(&topology, states).unwrap();

        assert_eq!(pre_scale.provenance(), topology.provenance());
        assert_eq!(scaled.provenance(), topology.provenance());
        assert_eq!(pre_scale.variant(), topology.variant());
        assert_eq!(scaled.variant(), topology.variant());
    }

    #[test]
    fn all_missing_runtimes_still_retain_distinct_arena_provenance() {
        let groups = [group(1, &[[0, 0, 0]], &[-1])];
        let bytes = c3d_with_group0(
            &[Group0 {
                field_04: -1,
                matrix: identity_matrix(),
                references: vec![0],
            }],
            &groups,
            &[],
        );
        let fixture = fixture_for(&[(RootMenuC3dSlot::Load, bytes)]);
        let assets = load(&fixture);
        let first = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let second = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let states = uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults);
        let first_runtime = materialize_root_menu_mode2_resource_scaled(&first, states).unwrap();
        let second_runtime = materialize_root_menu_mode2_resource_scaled(&second, states).unwrap();

        assert_eq!(first.resource_len(), 0);
        assert_eq!(second.resource_len(), 0);
        assert_eq!(first_runtime.provenance(), first.provenance());
        assert_eq!(second_runtime.provenance(), second.provenance());
        assert_ne!(first.provenance(), second.provenance());
        assert_ne!(first_runtime.provenance(), second_runtime.provenance());

        let object = first_runtime.get(RootMenuC3dSlot::Load).unwrap();
        let batch = object
            .group0_records()
            .next()
            .unwrap()
            .references()
            .next()
            .unwrap()
            .batches()
            .next()
            .unwrap();
        assert_eq!(batch.selector(), C3dPreparedSelector::Missing);
    }

    #[test]
    fn exact_root_frame_plan_owns_source_ordered_submission_snapshots() {
        let slot = RootMenuC3dSlot::Rally;
        let plan = {
            let group = group(3, &[[2, 0, 2], [1, 1, 0], [0, 2, 1]], &[0, 1, 0]);
            let blobs = [
                (*b"PLAN-FIRST!!", indexed_bmp(1, 2, 8)),
                (*b"PLAN-SECOND!", indexed_bmp(3, 4, 8)),
            ];
            let fixture = fixture_for(&[(slot, exact_root_c3d(&group, &blobs))]);
            let assets = load(&fixture);
            let topology =
                prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
            let runtime = materialize_root_menu_mode2_resource_scaled(
                &topology,
                uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
            )
            .unwrap();
            let expected = runtime
                .get(slot)
                .unwrap()
                .group0_records()
                .next()
                .unwrap()
                .references()
                .next()
                .unwrap()
                .batches()
                .map(|batch| {
                    (
                        batch.selector(),
                        batch.source_entry32_ordinals().to_vec(),
                        batch.vertices().to_vec(),
                        batch.side_vectors().to_vec(),
                        batch.indices().to_vec(),
                    )
                })
                .collect::<Vec<_>>();
            let extent = DisplayExtent {
                width: 640,
                height: 480,
            };
            let mut angles = RootMenuC3dAngles::new();
            let plan = plan_root_menu_c3d_frame(&topology, &runtime, &mut angles, slot, 8, extent)
                .unwrap()
                .unwrap();

            assert_eq!(plan.provenance(), topology.provenance());
            assert_eq!(plan.variant(), VisualVariant::Normal);
            assert_eq!(plan.slot(), slot);
            assert_eq!(plan.identity(), slot.identity());
            assert_eq!((plan.previous_angle(), plan.resulting_angle()), (0, 8));
            assert_eq!(plan.delta(), 8);
            assert_eq!(plan.effective_y_phase(), 8);
            assert_eq!(plan.world(), root_menu_c3d_world(8));
            assert_eq!(plan.view(), ROOT_MENU_C3D_VIEW);
            assert_eq!(plan.projection(), ROOT_MENU_C3D_PROJECTION);
            assert_eq!(plan.viewport(), root_menu_c3d_viewport(extent).unwrap());
            assert_eq!(plan.commands().len(), expected.len());
            for (index, (command, expected)) in plan.commands().iter().zip(expected).enumerate() {
                assert_eq!(command.slot(), slot);
                assert_eq!(command.group0_record_index(), 0);
                assert_eq!(command.value_index(), 0);
                assert_eq!(command.group1_ordinal(), 0);
                assert_eq!(command.batch_index(), index);
                assert_eq!(command.selector(), expected.0);
                assert_eq!(command.source_entry32_ordinals(), expected.1);
                assert_eq!(command.vertices(), expected.2);
                assert_eq!(command.side_vectors(), expected.3);
                assert_eq!(command.indices(), expected.4);
                assert_eq!(command.vertex_type(), ROOT_MENU_C3D_VERTEX_TYPE);
                assert_eq!(command.primitive_code(), ROOT_MENU_C3D_PRIMITIVE_CODE);
                assert_eq!(command.draw_flags(), ROOT_MENU_C3D_DRAW_FLAGS);
            }
            assert_eq!(angles.angle(slot), 8);
            plan
        };

        assert_eq!(plan.commands().len(), 3);
        assert_eq!(plan.commands()[0].source_entry32_ordinals(), &[2, 0]);
        assert_eq!(plan.commands()[0].indices(), &[0, 1, 0]);
        assert_eq!(plan.commands()[1].source_entry32_ordinals(), &[1, 0]);
        assert_eq!(plan.commands()[1].indices(), &[0, 0, 1]);
        assert_eq!(plan.commands()[2].source_entry32_ordinals(), &[0, 2, 1]);
        assert_eq!(plan.commands()[2].indices(), &[0, 1, 2]);
    }

    #[test]
    fn prepared_scene_pipeline_rejects_cross_arena_runtime_and_bindings() {
        let slot = RootMenuC3dSlot::Rally;
        let group = group(3, &[[0, 1, 2]], &[0]);
        let blobs = [(*b"PIPELINE-ONE", indexed_bmp(2, 2, 8))];
        let fixture = fixture_for(&[(slot, exact_root_c3d(&group, &blobs))]);
        let assets = load(&fixture);
        let first = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let second = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let first_runtime = materialize_root_menu_mode2_resource_scaled(
            &first,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        let second_runtime = materialize_root_menu_mode2_resource_scaled(
            &second,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        let first_catalog = bind_root_menu_c3d_resources(&first, |id, _| {
            Ok::<_, std::convert::Infallible>(id.index() as u8)
        })
        .unwrap();
        let second_catalog = bind_root_menu_c3d_resources(&second, |id, _| {
            Ok::<_, std::convert::Infallible>(id.index() as u8)
        })
        .unwrap();
        let mut first_executor = RootMenuC3dBatchExecutor::new(&first_catalog);
        let mut second_executor = RootMenuC3dBatchExecutor::new(&second_catalog);
        let mut angles = RootMenuC3dAngles::new();
        let mut visibility = RootMenuC3dVisibilityLatches::new();

        assert!(
            PreparedRootMenuC3dPipeline::new(
                &first,
                &first_runtime,
                &mut angles,
                &mut visibility,
                &mut first_executor,
                RootMenuC3dDepthClear::Clear,
            )
            .is_ok()
        );
        assert!(matches!(
            PreparedRootMenuC3dPipeline::new(
                &first,
                &second_runtime,
                &mut angles,
                &mut visibility,
                &mut first_executor,
                RootMenuC3dDepthClear::Clear,
            ),
            Err(PreparedRootMenuC3dPipelineError::RuntimeProvenanceMismatch)
        ));
        assert!(matches!(
            PreparedRootMenuC3dPipeline::new(
                &first,
                &first_runtime,
                &mut angles,
                &mut visibility,
                &mut second_executor,
                RootMenuC3dDepthClear::Clear,
            ),
            Err(PreparedRootMenuC3dPipelineError::BindingProvenanceMismatch)
        ));
    }

    #[test]
    fn prepared_scene_pipeline_executes_real_plans_and_invalidates_uncertain_bindings() {
        let slot = RootMenuC3dSlot::Rally;
        let root_group = group(1, &[[0, 0, 0]], &[0]);
        let blobs = [(*b"PIPE-EXECUTE", indexed_bmp(2, 2, 8))];
        let fixture = fixture_for(&[(slot, exact_root_c3d(&root_group, &blobs))]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let runtime = materialize_root_menu_mode2_resource_scaled(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        let catalog = bind_root_menu_c3d_resources(&topology, |id, _| {
            Ok::<_, std::convert::Infallible>(id.index() as u8)
        })
        .unwrap();
        let mut executor = RootMenuC3dBatchExecutor::new(&catalog);
        let mut angles = RootMenuC3dAngles::new();
        let mut visibility = RootMenuC3dVisibilityLatches::new();
        let mut pipeline = PreparedRootMenuC3dPipeline::new(
            &topology,
            &runtime,
            &mut angles,
            &mut visibility,
            &mut executor,
            RootMenuC3dDepthClear::Clear,
        )
        .unwrap();
        let mut backend = RecordingSceneBatchBackend::default();
        let extent = DisplayExtent {
            width: 640,
            height: 480,
        };

        let plan = pipeline.plan_frame(slot, 8, extent).unwrap();
        assert_eq!(pipeline.visibility_mask(slot).raw(), 0xff);
        assert!(
            pipeline
                .plan_frame(
                    slot,
                    8,
                    DisplayExtent {
                        width: 0,
                        height: 480,
                    },
                )
                .is_err()
        );
        assert_eq!(
            pipeline.visibility_mask(slot).raw(),
            0xff,
            "a transactional planning error leaves existing latches unchanged"
        );
        pipeline
            .execute_terminal_scene(&mut backend, 7, extent, plan)
            .unwrap();
        assert_eq!(pipeline.visibility_mask(slot).raw(), 0x00);
        assert!(
            pipeline
                .plan_frame(RootMenuC3dSlot::Options, 8, extent)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            pipeline.visibility_mask(RootMenuC3dSlot::Options).raw(),
            0x00
        );
        assert_eq!(backend.binding_count(), 1);

        backend.batch.events.clear();
        backend.tail_state = RootMenuC3dBindingState::Changed;
        let plan = pipeline.plan_frame(slot, 8, extent).unwrap();
        assert_eq!(pipeline.visibility_mask(slot).raw(), 0xff);
        pipeline
            .execute_terminal_scene(&mut backend, 7, extent, plan)
            .unwrap();
        assert_eq!(pipeline.visibility_mask(slot).raw(), 0x00);
        assert_eq!(backend.binding_count(), 0);

        backend.batch.events.clear();
        backend.tail_state = RootMenuC3dBindingState::Preserved;
        let plan = pipeline.plan_frame(slot, 8, extent).unwrap();
        pipeline
            .execute_terminal_scene(&mut backend, 7, extent, plan)
            .unwrap();
        assert_eq!(pipeline.visibility_mask(slot).raw(), 0x00);
        assert_eq!(backend.binding_count(), 1);

        backend.batch.events.clear();
        backend.batch.fail = Some((RootMenuC3dBatchStage::World, None));
        let plan = pipeline.plan_frame(slot, 8, extent).unwrap();
        assert!(
            pipeline
                .execute_terminal_scene(&mut backend, 7, extent, plan)
                .is_err()
        );
        assert_eq!(pipeline.visibility_mask(slot).raw(), 0x00);
        backend.batch.fail = None;
        backend.batch.events.clear();
        let plan = pipeline.plan_frame(slot, 8, extent).unwrap();
        pipeline
            .execute_terminal_scene(&mut backend, 7, extent, plan)
            .unwrap();
        assert_eq!(backend.binding_count(), 1);

        for failure in [
            RootMenuC3dSceneStage::ClearDepth,
            RootMenuC3dSceneStage::View,
            RootMenuC3dSceneStage::Viewport,
            RootMenuC3dSceneStage::Begin,
            RootMenuC3dSceneStage::ResetWorld,
            RootMenuC3dSceneStage::Tail,
            RootMenuC3dSceneStage::End,
        ] {
            backend.batch.events.clear();
            backend.scene_fail = Some(failure);
            let plan = pipeline.plan_frame(slot, 8, extent).unwrap();
            assert!(
                pipeline
                    .execute_terminal_scene(&mut backend, 7, extent, plan)
                    .is_err()
            );
            assert_eq!(pipeline.visibility_mask(slot).raw(), 0x00);
            backend.scene_fail = None;
            backend.batch.events.clear();
            let plan = pipeline.plan_frame(slot, 8, extent).unwrap();
            pipeline
                .execute_terminal_scene(&mut backend, 7, extent, plan)
                .unwrap();
            let expected_rebinds = usize::from(matches!(
                failure,
                RootMenuC3dSceneStage::Tail | RootMenuC3dSceneStage::End
            ));
            assert_eq!(
                backend.binding_count(),
                expected_rebinds,
                "failure stage {failure:?}"
            );
        }
    }

    #[test]
    fn frame_plan_errors_and_absent_slots_are_transactional() {
        let slot = RootMenuC3dSlot::Options;
        let group = group(1, &[[0, 0, 0]], &[0]);
        let blobs = [(*b"TRANSACTION!", indexed_bmp(1, 2, 8))];
        let fixture = fixture_for(&[(slot, exact_root_c3d(&group, &blobs))]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let runtime = materialize_root_menu_mode2_resource_scaled(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        let mut angles = RootMenuC3dAngles::new();
        let original = angles.clone();

        assert!(matches!(
            plan_root_menu_c3d_frame(
                &topology,
                &runtime,
                &mut angles,
                slot,
                8,
                DisplayExtent {
                    width: 0,
                    height: 480
                },
            ),
            Err(RootMenuC3dFramePlanError::Viewport(_))
        ));
        assert_eq!(angles, original);
        assert_eq!(
            plan_root_menu_c3d_frame(
                &topology,
                &runtime,
                &mut angles,
                RootMenuC3dSlot::Load,
                8,
                DisplayExtent {
                    width: 640,
                    height: 480
                },
            )
            .unwrap(),
            None
        );
        assert_eq!(angles, original);
    }

    #[test]
    fn frame_plan_rejects_foreign_arenas_and_missing_root_resources_before_mutation() {
        let slot = RootMenuC3dSlot::Load;
        let group = group(1, &[[0, 0, 0]], &[-1]);
        let fixture = fixture_for(&[(slot, exact_root_c3d(&group, &[]))]);
        let assets = load(&fixture);
        let first = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let second = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let states = uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults);
        let first_runtime = materialize_root_menu_mode2_resource_scaled(&first, states).unwrap();
        let second_runtime = materialize_root_menu_mode2_resource_scaled(&second, states).unwrap();
        let extent = DisplayExtent {
            width: 640,
            height: 480,
        };
        let mut angles = RootMenuC3dAngles::new();

        assert_eq!(
            plan_root_menu_c3d_frame(&first, &second_runtime, &mut angles, slot, 8, extent,),
            Err(RootMenuC3dFramePlanError::ProvenanceMismatch)
        );
        assert_eq!(angles.angle(slot), 0);
        assert_eq!(
            plan_root_menu_c3d_frame(&first, &first_runtime, &mut angles, slot, 8, extent,),
            Err(RootMenuC3dFramePlanError::UnexpectedRootShape {
                slot,
                issue: RootMenuC3dRootShapeIssue::MissingSelector { batch_index: 0 },
            })
        );
        assert_eq!(angles.angle(slot), 0);
    }

    #[test]
    fn frame_plan_preserves_raw_wrapping_angle_state() {
        let slot = RootMenuC3dSlot::Championship;
        let group = group(1, &[[0, 0, 0]], &[0]);
        let blobs = [(*b"ANGLE-WRAP!!", indexed_bmp(1, 2, 8))];
        let fixture = fixture_for(&[(slot, exact_root_c3d(&group, &blobs))]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let runtime = materialize_root_menu_mode2_resource_scaled(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        let extent = DisplayExtent {
            width: 640,
            height: 480,
        };
        let mut angles = RootMenuC3dAngles::new();

        let near_wrap =
            plan_root_menu_c3d_frame(&topology, &runtime, &mut angles, slot, 0xfff8, extent)
                .unwrap()
                .unwrap();
        assert_eq!(near_wrap.resulting_angle(), 0xfff8);
        assert_eq!(near_wrap.effective_y_phase(), 0x0ff8);
        let wrapped = plan_root_menu_c3d_frame(&topology, &runtime, &mut angles, slot, 8, extent)
            .unwrap()
            .unwrap();
        assert_eq!(
            (wrapped.previous_angle(), wrapped.resulting_angle()),
            (0xfff8, 0)
        );
        assert_eq!(wrapped.world(), root_menu_c3d_world(0));

        let mut full_turn_angles = RootMenuC3dAngles::new();
        let full_turn = plan_root_menu_c3d_frame(
            &topology,
            &runtime,
            &mut full_turn_angles,
            slot,
            0x1000,
            extent,
        )
        .unwrap()
        .unwrap();
        assert_eq!(full_turn.resulting_angle(), 0x1000);
        assert_eq!(full_turn.effective_y_phase(), 0);
        assert_eq!(full_turn.world(), root_menu_c3d_world(0x1000));
    }

    #[test]
    fn frame_plan_rejects_reachable_non_root_shapes_without_mutating_angles() {
        let slot = RootMenuC3dSlot::TimeTrial;
        let root_group = group(1, &[[0, 0, 0]], &[0]);
        let blobs = [(*b"SHAPE-CHECK!", indexed_bmp(1, 2, 8))];
        let group0 = || Group0 {
            field_04: -1,
            matrix: identity_matrix(),
            references: vec![0],
        };
        let mut nonzero_metadata = exact_root_c3d(&root_group, &blobs);
        let group1_start = C3D_HEADER_LEN + C3D_GROUP0_VALUES_OFFSET + 2;
        nonzero_metadata[group1_start + 20] = 1;
        let cases = [
            (
                c3d(std::slice::from_ref(&root_group), &blobs),
                RootMenuC3dRootShapeIssue::TopologyGroup0Count { actual: 0 },
            ),
            (
                c3d_with_group0(
                    &[group0(), group0()],
                    std::slice::from_ref(&root_group),
                    &blobs,
                ),
                RootMenuC3dRootShapeIssue::TopologyGroup0Count { actual: 2 },
            ),
            (
                c3d_with_group0(
                    &[Group0 {
                        field_04: 0,
                        ..group0()
                    }],
                    std::slice::from_ref(&root_group),
                    &blobs,
                ),
                RootMenuC3dRootShapeIssue::Group0Metadata,
            ),
            (
                c3d_with_group0(
                    &[Group0 {
                        references: vec![0, 0],
                        ..group0()
                    }],
                    std::slice::from_ref(&root_group),
                    &blobs,
                ),
                RootMenuC3dRootShapeIssue::Group0Metadata,
            ),
            (
                c3d_with_group0(
                    &[group0()],
                    &[root_group.clone(), root_group.clone()],
                    &blobs,
                ),
                RootMenuC3dRootShapeIssue::TopologyGroup1Count { actual: 2 },
            ),
            (
                c3d_with_group0(&[group0()], &[group(0, &[], &[])], &[]),
                RootMenuC3dRootShapeIssue::Group1Metadata,
            ),
            (nonzero_metadata, RootMenuC3dRootShapeIssue::Group1Metadata),
        ];

        for (bytes, expected_issue) in cases {
            let fixture = fixture_for(&[(slot, bytes)]);
            let assets = load(&fixture);
            let topology =
                prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
            let runtime = materialize_root_menu_mode2_resource_scaled(
                &topology,
                uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
            )
            .unwrap();
            let mut angles = RootMenuC3dAngles::new();

            assert_eq!(
                plan_root_menu_c3d_frame(
                    &topology,
                    &runtime,
                    &mut angles,
                    slot,
                    8,
                    DisplayExtent {
                        width: 640,
                        height: 480
                    },
                ),
                Err(RootMenuC3dFramePlanError::UnexpectedRootShape {
                    slot,
                    issue: expected_issue,
                })
            );
            assert_eq!(angles.angle(slot), 0);
        }
    }

    #[test]
    fn binding_catalog_maps_once_checks_arena_and_releases_in_reverse_order() {
        let slot = RootMenuC3dSlot::Rally;
        let root_group = group(3, &[[0, 1, 2], [2, 1, 0], [1, 0, 2]], &[0, 1, 2]);
        let blobs = [
            (*b"BIND-FIRST!!", indexed_bmp(1, 2, 8)),
            (*b"BIND-SECOND!", indexed_bmp(3, 4, 8)),
            (*b"BIND-THIRD!!", indexed_bmp(5, 6, 8)),
        ];
        let fixture = fixture_for(&[(slot, exact_root_c3d(&root_group, &blobs))]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let independent = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let mut mapped = Vec::new();
        let catalog = bind_root_menu_c3d_resources(&topology, |id, resource| {
            mapped.push((
                id.index(),
                resource.first_slot(),
                resource.first_resource_ordinal(),
                resource.surface_color_key_index(),
            ));
            Ok::<_, Infallible>(id.index() as u8)
        })
        .unwrap();

        assert_eq!(catalog.provenance(), topology.provenance());
        assert_eq!(catalog.len(), 3);
        assert_eq!(
            mapped,
            [(0, slot, 0, None), (1, slot, 1, None), (2, slot, 2, None),]
        );
        for (id, _) in topology.resources() {
            assert_eq!(*catalog.binding(id).unwrap(), id.index() as u8);
        }
        let foreign_id = independent.resources().next().unwrap().0;
        assert_eq!(
            catalog.binding(foreign_id),
            Err(C3dPreparedResourceAccessError::Foreign)
        );

        #[derive(Debug)]
        struct DropBinding {
            index: u8,
            drops: Rc<RefCell<Vec<u8>>>,
        }
        impl Drop for DropBinding {
            fn drop(&mut self) {
                self.drops.borrow_mut().push(self.index);
            }
        }
        let failed_drops = Rc::new(RefCell::new(Vec::new()));
        let error = bind_root_menu_c3d_resources(&topology, |id, _| {
            if id.index() == 2 {
                Err("create")
            } else {
                Ok(DropBinding {
                    index: id.index() as u8,
                    drops: Rc::clone(&failed_drops),
                })
            }
        })
        .unwrap_err();
        assert_eq!(error.resource_id.index(), 2);
        assert_eq!(error.first_slot, slot);
        assert_eq!(error.first_resource_ordinal, 2);
        assert_eq!(error.source, "create");
        assert_eq!(failed_drops.borrow().as_slice(), [1, 0]);

        let unwind_drops = Rc::new(RefCell::new(Vec::new()));
        let unwind_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = bind_root_menu_c3d_resources(&topology, |id, _| {
                if id.index() == 2 {
                    panic!("injected binding panic");
                }
                Ok::<_, Infallible>(DropBinding {
                    index: id.index() as u8,
                    drops: Rc::clone(&unwind_drops),
                })
            });
        }));
        assert!(unwind_result.is_err());
        assert_eq!(unwind_drops.borrow().as_slice(), [1, 0]);

        let committed_drops = Rc::new(RefCell::new(Vec::new()));
        let committed = bind_root_menu_c3d_resources(&topology, |id, _| {
            Ok::<_, Infallible>(DropBinding {
                index: id.index() as u8,
                drops: Rc::clone(&committed_drops),
            })
        })
        .unwrap();
        drop(committed);
        assert_eq!(committed_drops.borrow().as_slice(), [2, 1, 0]);
    }

    #[test]
    fn batch_executor_binds_changed_selectors_in_order_and_caches_across_frames() {
        let slot = RootMenuC3dSlot::Championship;
        let root_group = group(3, &[[2, 0, 2], [1, 1, 0], [0, 2, 1]], &[0, 1, 0]);
        let blobs = [
            (*b"EXEC-FIRST!!", indexed_bmp(1, 2, 8)),
            (*b"EXEC-SECOND!", indexed_bmp(3, 4, 8)),
        ];
        let fixture = fixture_for(&[(slot, exact_root_c3d(&root_group, &blobs))]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let runtime = materialize_root_menu_mode2_resource_scaled(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        let catalog =
            bind_root_menu_c3d_resources(&topology, |id, _| Ok::<_, Infallible>(id.index() as u8))
                .unwrap();
        let extent = DisplayExtent {
            width: 640,
            height: 480,
        };
        let mut angles = RootMenuC3dAngles::new();
        let first = plan_root_menu_c3d_frame(&topology, &runtime, &mut angles, slot, 8, extent)
            .unwrap()
            .unwrap();
        let second = plan_root_menu_c3d_frame(&topology, &runtime, &mut angles, slot, 8, extent)
            .unwrap()
            .unwrap();
        let third = plan_root_menu_c3d_frame(&topology, &runtime, &mut angles, slot, 8, extent)
            .unwrap()
            .unwrap();
        let mut backend = RecordingBatchBackend::default();
        let mut executor = RootMenuC3dBatchExecutor::new(&catalog);

        assert_eq!(
            executor.execute(&mut backend, first).unwrap(),
            crate::root_menu_c3d_execute::RootMenuC3dBatchOutcome {
                bindings: 3,
                draws: 3,
            }
        );
        assert_eq!(
            backend.events,
            [
                BatchEvent::World(root_menu_c3d_world(8)),
                BatchEvent::Binding(0),
                BatchEvent::Draw {
                    batch_index: 0,
                    selector_index: 0,
                    vertex_type: 2,
                    primitive_code: 4,
                    draw_flags: 8,
                },
                BatchEvent::Binding(1),
                BatchEvent::Draw {
                    batch_index: 1,
                    selector_index: 1,
                    vertex_type: 2,
                    primitive_code: 4,
                    draw_flags: 8,
                },
                BatchEvent::Binding(0),
                BatchEvent::Draw {
                    batch_index: 2,
                    selector_index: 0,
                    vertex_type: 2,
                    primitive_code: 4,
                    draw_flags: 8,
                },
            ]
        );

        backend.events.clear();
        assert_eq!(
            executor.execute(&mut backend, second).unwrap(),
            crate::root_menu_c3d_execute::RootMenuC3dBatchOutcome {
                bindings: 2,
                draws: 3,
            }
        );
        assert_eq!(
            backend.events[0],
            BatchEvent::World(root_menu_c3d_world(16))
        );
        assert!(matches!(
            backend.events[1],
            BatchEvent::Draw { batch_index: 0, .. }
        ));
        assert_eq!(backend.events[2], BatchEvent::Binding(1));

        backend.events.clear();
        executor.invalidate_bindings();
        executor.execute(&mut backend, third).unwrap();
        assert_eq!(backend.events[1], BatchEvent::Binding(0));
    }

    #[test]
    fn batch_executor_preflights_arena_and_uses_safe_failure_cache_boundaries() {
        let slot = RootMenuC3dSlot::Options;
        let root_group = group(1, &[[0, 0, 0]], &[0]);
        let blobs = [(*b"EXEC-FAIL!!!", indexed_bmp(1, 2, 8))];
        let fixture = fixture_for(&[(slot, exact_root_c3d(&root_group, &blobs))]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let foreign = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let runtime = materialize_root_menu_mode2_resource_scaled(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        let catalog =
            bind_root_menu_c3d_resources(&topology, |id, _| Ok::<_, Infallible>(id.index() as u8))
                .unwrap();
        let foreign_catalog =
            bind_root_menu_c3d_resources(&foreign, |id, _| Ok::<_, Infallible>(id.index() as u8))
                .unwrap();
        let extent = DisplayExtent {
            width: 640,
            height: 480,
        };
        let mut angles = RootMenuC3dAngles::new();
        let mut make_plan = || {
            plan_root_menu_c3d_frame(&topology, &runtime, &mut angles, slot, 8, extent)
                .unwrap()
                .unwrap()
        };
        let mut backend = RecordingBatchBackend::default();
        let mut foreign_executor = RootMenuC3dBatchExecutor::new(&foreign_catalog);

        assert_eq!(
            foreign_executor.execute(&mut backend, make_plan()),
            Err(RootMenuC3dExecuteError::Preflight(
                crate::root_menu_c3d_execute::RootMenuC3dBatchPreflightError::CatalogProvenanceMismatch
            ))
        );
        assert!(backend.events.is_empty());

        let mut world_executor = RootMenuC3dBatchExecutor::new(&catalog);
        backend.fail = Some((RootMenuC3dBatchStage::World, None));
        let world_error = world_executor
            .execute(&mut backend, make_plan())
            .unwrap_err();
        assert!(matches!(
            world_error,
            RootMenuC3dExecuteError::Backend(ref error)
                if error.stage == RootMenuC3dBatchStage::World
                    && error.batch_index.is_none()
                    && error.resource_id.is_none()
        ));
        assert!(matches!(backend.events.as_slice(), [BatchEvent::World(_)]));
        backend.events.clear();
        backend.fail = None;
        world_executor.execute(&mut backend, make_plan()).unwrap();
        assert_eq!(backend.events[1], BatchEvent::Binding(0));

        backend.events.clear();
        backend.fail = Some((RootMenuC3dBatchStage::World, None));
        assert!(world_executor.execute(&mut backend, make_plan()).is_err());
        backend.events.clear();
        backend.fail = None;
        world_executor.execute(&mut backend, make_plan()).unwrap();
        assert!(matches!(backend.events[1], BatchEvent::Draw { .. }));

        let mut executor = RootMenuC3dBatchExecutor::new(&catalog);
        backend.fail = Some((RootMenuC3dBatchStage::Binding, Some(0)));
        assert!(matches!(
            executor.execute(&mut backend, make_plan()),
            Err(RootMenuC3dExecuteError::Backend(error))
                if error.stage == RootMenuC3dBatchStage::Binding
        ));
        backend.events.clear();
        backend.fail = None;
        executor.execute(&mut backend, make_plan()).unwrap();
        assert_eq!(backend.events[1], BatchEvent::Binding(0));

        backend.events.clear();
        backend.fail = Some((RootMenuC3dBatchStage::Draw, Some(0)));
        let mut draw_failure_executor = RootMenuC3dBatchExecutor::new(&catalog);
        assert!(matches!(
            draw_failure_executor.execute(&mut backend, make_plan()),
            Err(RootMenuC3dExecuteError::Backend(error))
                if error.stage == RootMenuC3dBatchStage::Draw
        ));
        assert_eq!(backend.events[1], BatchEvent::Binding(0));
        backend.events.clear();
        backend.fail = None;
        draw_failure_executor
            .execute(&mut backend, make_plan())
            .unwrap();
        assert!(matches!(backend.events[1], BatchEvent::Draw { .. }));
    }

    #[test]
    fn batch_failure_has_object_and_record_context_and_returns_no_value() {
        let groups = [group(1, &[[1, 0, 0]], &[0])];
        let blobs = [(*b"BAD-ORDINAL!", indexed_bmp(1, 2, 8))];
        let fixture = fixture_for(&[(RootMenuC3dSlot::RallySchool, c3d(&groups, &blobs))]);
        let assets = load(&fixture);
        let source_record = assets
            .get(RootMenuC3dSlot::RallySchool)
            .unwrap()
            .file
            .group1_records()
            .next()
            .unwrap();
        let source_entry = source_record.entries_24().next().unwrap();

        let error = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap_err();

        assert_eq!(
            error,
            RootMenuC3dTopologyError::Batch {
                slot: RootMenuC3dSlot::RallySchool,
                identity: RootMenuC3dSlot::RallySchool.identity(),
                path: RootMenuC3dSlot::RallySchool.path(VisualVariant::Normal),
                record_index: 0,
                record_offset: source_record.offset(),
                source: C3dBatchPlanError::Entry32OrdinalOutOfBounds {
                    record_offset: source_record.offset(),
                    entry24_index: 0,
                    corner_index: 0,
                    source_offset: source_entry.offset() + C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[0],
                    ordinal: 1,
                    entry32_count: 1,
                },
            }
        );
    }

    #[test]
    fn resource_failure_leaves_a_hole_and_later_slot_topology_survives() {
        let groups = [group(1, &[[0, 0, 0]], &[0])];
        let bad = [(*b"BAD-BITMAP!!", indexed_bmp(1, 2, 4))];
        let good = [(*b"GOOD-BITMAP!", indexed_bmp(1, 2, 8))];
        let fixture = fixture_for(&[
            (RootMenuC3dSlot::Championship, c3d(&groups, &bad)),
            (RootMenuC3dSlot::Load, c3d(&groups, &good)),
        ]);
        let assets = load(&fixture);

        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();

        assert!(topology.get(RootMenuC3dSlot::Championship).is_none());
        assert!(topology.get(RootMenuC3dSlot::Load).is_some());
        assert!(matches!(
            topology.diagnostics().next().unwrap().error(),
            C3dResourcePreparationError::Bitmap {
                source: BitmapError::UnsupportedBitDepth(4),
                ..
            }
        ));
    }

    #[test]
    fn loaded_holes_remain_in_their_fixed_slots() {
        let groups = [group(1, &[[0, 0, 0]], &[0])];
        let blobs = [(*b"ONLY-OPTIONS", indexed_bmp(1, 2, 8))];
        let fixture = fixture_for(&[(RootMenuC3dSlot::Options, c3d(&groups, &blobs))]);
        let assets = load(&fixture);

        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();

        assert_eq!(topology.loaded_len(), 1);
        assert_eq!(
            topology
                .slots()
                .map(|(slot, value)| (slot, value.is_some()))
                .collect::<Vec<_>>(),
            RootMenuC3dSlot::ALL
                .map(|slot| (slot, slot == RootMenuC3dSlot::Options))
                .to_vec()
        );
    }

    #[test]
    fn retains_empty_group1_record_with_no_batches() {
        let fixture = fixture_for(&[(RootMenuC3dSlot::Load, c3d(&[group(0, &[], &[])], &[]))]);
        let assets = load(&fixture);

        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let group = topology
            .get(RootMenuC3dSlot::Load)
            .unwrap()
            .group(0)
            .unwrap();

        assert_eq!(group.record_index(), 0);
        assert!(group.batches().is_empty());
    }

    #[test]
    fn catalogs_group0_reference_ownership_and_rejects_bad_ordinals_transactionally() {
        let matrix = identity_matrix();
        let groups = [group(1, &[[0, 0, 0]], &[-1])];
        let valid = c3d_with_group0(
            &[Group0 {
                field_04: -1,
                matrix,
                references: vec![0, 0],
            }],
            &groups,
            &[],
        );
        let fixture = fixture_for(&[(RootMenuC3dSlot::Load, valid)]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let object = topology.get(RootMenuC3dSlot::Load).unwrap();
        let group0 = object.group0_record(0).unwrap();
        let references = group0.references().collect::<Vec<_>>();

        assert_eq!(group0.record_index(), 0);
        assert_eq!(group0.record().field_04_i16(), -1);
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].value_index(), 0);
        assert_eq!(references[1].value_index(), 1);
        assert_eq!(references[0].group1_ordinal(), 0);
        assert_eq!(references[1].group1_ordinal(), 0);
        assert_eq!(references[0].source_offset(), group0.record_offset() + 80);
        assert_eq!(references[1].source_offset(), group0.record_offset() + 82);

        let invalid = c3d_with_group0(
            &[Group0 {
                field_04: 0,
                matrix,
                references: vec![1],
            }],
            &groups,
            &[],
        );
        let fixture = fixture_for(&[(RootMenuC3dSlot::Rally, invalid)]);
        let assets = load(&fixture);
        let source_group0 = assets
            .get(RootMenuC3dSlot::Rally)
            .unwrap()
            .file
            .group0_records()
            .next()
            .unwrap();

        assert_eq!(
            prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()),
            Err(RootMenuC3dTopologyError::Group1ReferenceOutOfBounds {
                slot: RootMenuC3dSlot::Rally,
                identity: RootMenuC3dSlot::Rally.identity(),
                path: RootMenuC3dSlot::Rally.path(VisualVariant::Normal),
                group0_record_index: 0,
                group0_record_offset: source_group0.offset(),
                value_index: 0,
                source_offset: source_group0.offset() + 80,
                ordinal: 1,
                group1_count: 1,
            })
        );
    }

    #[test]
    fn runtime_instances_use_per_load_snapshot_and_latch_pass_through_after_minus_one() {
        let entries = [[1 << 16, 2 << 16, 3 << 16, 0, 0, 0, 0, 0]];
        let groups = [runtime_group(&entries, &[[0, 0, 0]], &[0], &[[[0; 4]; 3]])];
        let mut translated = identity_matrix();
        translated[12] = 10 << 16;
        translated[13] = 20 << 16;
        translated[14] = 30 << 16;
        let fixture = fixture_for(&[(
            RootMenuC3dSlot::Championship,
            c3d_with_group0(
                &[
                    Group0 {
                        field_04: 0,
                        matrix: translated,
                        references: vec![0],
                    },
                    Group0 {
                        field_04: -1,
                        matrix: translated,
                        references: vec![0],
                    },
                    Group0 {
                        field_04: 0,
                        matrix: translated,
                        references: vec![0],
                    },
                ],
                &groups,
                &[(*b"RUNTIME!!!!!", indexed_bmp(1, 2, 8))],
            ),
        )]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();

        let transformed = materialize_root_menu_mode2_pre_scale(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        let object = transformed.get(RootMenuC3dSlot::Championship).unwrap();
        let records = object.group0_records().collect::<Vec<_>>();
        let first_vertex = |record_index: usize| {
            records[record_index]
                .references()
                .next()
                .unwrap()
                .batches()
                .next()
                .unwrap()
                .vertices()[0]
        };
        assert_eq!(first_vertex(0).first_triplet(), [11.0, 22.0, 33.0]);
        assert_eq!(first_vertex(1).first_triplet(), [1.0, 2.0, 3.0]);
        assert_eq!(first_vertex(2).first_triplet(), [1.0, 2.0, 3.0]);

        let pass_through = materialize_root_menu_mode2_pre_scale(
            &topology,
            uniform_load_states(u32::MAX, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        for record in pass_through
            .get(RootMenuC3dSlot::Championship)
            .unwrap()
            .group0_records()
        {
            assert_eq!(
                record
                    .references()
                    .next()
                    .unwrap()
                    .batches()
                    .next()
                    .unwrap()
                    .vertices()[0]
                    .first_triplet(),
                [1.0, 2.0, 3.0]
            );
        }
    }

    #[test]
    fn independent_slots_use_their_own_loader_count_and_corner_gate() {
        let groups = [runtime_group(
            &[[1 << 16, 2 << 16, 3 << 16, 0, 0, 0, 0, 0]],
            &[[0, 0, 0]],
            &[0],
            &[[[0xaa, 0xbb, 0xcc, 0xdd]; 3]],
        )];
        let mut translated = identity_matrix();
        translated[12] = 10 << 16;
        let object = |label| {
            c3d_with_group0(
                &[Group0 {
                    field_04: 0,
                    matrix: translated,
                    references: vec![0],
                }],
                &groups,
                &[(label, indexed_bmp(1, 2, 8))],
            )
        };
        let fixture = fixture_for(&[
            (RootMenuC3dSlot::Championship, object(*b"SLOT-STATE-A")),
            (RootMenuC3dSlot::Rally, object(*b"SLOT-STATE-B")),
        ]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let mut states = [C3dMode2LoadState {
            prepared_resource_count: 1,
            corner_source: C3dMode2CornerSource::OrdinaryDefaults,
            surface_layout: Default::default(),
        }; ROOT_MENU_C3D_REQUEST_COUNT];
        states[RootMenuC3dSlot::Championship.index()] = C3dMode2LoadState {
            prepared_resource_count: 0,
            corner_source: C3dMode2CornerSource::SourceEntryTuples,
            surface_layout: Default::default(),
        };

        let runtime = materialize_root_menu_mode2_pre_scale(
            &topology,
            RootMenuC3dMode2LoadStates::new(states),
        )
        .unwrap();
        let vertex = |slot| {
            runtime
                .get(slot)
                .unwrap()
                .group0_records()
                .next()
                .unwrap()
                .references()
                .next()
                .unwrap()
                .batches()
                .next()
                .unwrap()
                .vertices()[0]
        };

        assert_eq!(
            vertex(RootMenuC3dSlot::Championship).first_triplet(),
            [11.0, 2.0, 3.0]
        );
        assert_eq!(
            vertex(RootMenuC3dSlot::Championship).opaque_triplet_words()[1],
            0xddaa_bbcc
        );
        assert_eq!(
            vertex(RootMenuC3dSlot::Rally).first_triplet(),
            [1.0, 2.0, 3.0]
        );
        assert_eq!(
            vertex(RootMenuC3dSlot::Rally).opaque_triplet_words()[1],
            0x9600_0000
        );
    }

    #[test]
    fn mode2_mutation_is_global_last_writer_and_side_vectors_precede_it() {
        let entries = [[
            0,
            0,
            0,
            2 * 65_536 + 16_384,
            67_108_565,
            1 << 16,
            2 << 16,
            3 << 16,
        ]];
        let final_tuple = [0xaa, 0xbb, 0xcc, 0xdd];
        let groups = [runtime_group(
            &entries,
            &[[0, 0, 0], [0, 0, 0], [0, 0, 0]],
            &[0, -1, 0],
            &[[[1, 2, 3, 4]; 3], [[5, 6, 7, 8]; 3], [final_tuple; 3]],
        )];
        let fixture = fixture_for(&[(
            RootMenuC3dSlot::Options,
            c3d_with_group0(
                &[Group0 {
                    field_04: -1,
                    matrix: identity_matrix(),
                    references: vec![0],
                }],
                &groups,
                &[(*b"LAST-WRITER!", indexed_bmp(1, 2, 8))],
            ),
        )]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let runtime = materialize_root_menu_mode2_pre_scale(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::SourceEntryTuples),
        )
        .unwrap();
        let reference = runtime
            .get(RootMenuC3dSlot::Options)
            .unwrap()
            .group0_records()
            .next()
            .unwrap()
            .references()
            .next()
            .unwrap();
        let batches = reference.batches().collect::<Vec<_>>();

        assert_eq!(batches.len(), 3);
        for batch in &batches {
            assert_eq!(batch.indices(), &[0, 0, 0]);
            assert_eq!(batch.source_entry32_ordinals(), &[0]);
            assert_eq!(batch.vertices().len(), 1);
            assert_eq!(batch.vertices()[0].opaque_triplet_words()[1], 0xddaa_bbcc);
            assert_eq!(batch.vertices()[0].opaque_triplet_words()[2], 0xff00_0000);
            assert_eq!(
                batch.vertices()[0].opaque_triplet_words()[0],
                1.0f32.to_bits()
            );
            assert_eq!(
                batch.side_vectors()[0].words(),
                [1.0f32.to_bits(), 2.0f32.to_bits(), 3.0f32.to_bits()]
            );
            assert_eq!(batch.vertices()[0].adjusted_pair()[0], 1.25);
            assert_eq!(
                batch.vertices()[0].adjusted_pair()[1].to_bits(),
                0x4480_1fdb
            );
        }

        let defaults = materialize_root_menu_mode2_pre_scale(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        for batch in defaults
            .get(RootMenuC3dSlot::Options)
            .unwrap()
            .group0_records()
            .next()
            .unwrap()
            .references()
            .next()
            .unwrap()
            .batches()
        {
            assert_eq!(batch.vertices()[0].opaque_triplet_words()[1], 0x9600_0000);
        }
    }

    #[test]
    fn missing_last_writer_overrides_an_earlier_present_tuple() {
        let groups = [runtime_group(
            &[[0; 8]],
            &[[0, 0, 0], [0, 0, 0]],
            &[0, -1],
            &[[[1, 2, 3, 4]; 3], [[5, 6, 7, 8]; 3]],
        )];
        let fixture = fixture_for(&[(
            RootMenuC3dSlot::Load,
            c3d_with_group0(
                &[Group0 {
                    field_04: -1,
                    matrix: identity_matrix(),
                    references: vec![0],
                }],
                &groups,
                &[(*b"MISS-LAST!!!", indexed_bmp(1, 2, 8))],
            ),
        )]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let runtime = materialize_root_menu_mode2_pre_scale(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::SourceEntryTuples),
        )
        .unwrap();
        for batch in runtime
            .get(RootMenuC3dSlot::Load)
            .unwrap()
            .group0_records()
            .next()
            .unwrap()
            .references()
            .next()
            .unwrap()
            .batches()
        {
            assert_eq!(
                batch.vertices()[0].opaque_triplet_words()[1..],
                [0xff00_0000, 0xff00_0000]
            );
        }
    }

    #[test]
    fn resource_scaling_is_source_ordinal_global_across_runs_and_missing() {
        let entry = [
            1 << 16,
            2 << 16,
            3 << 16,
            32_768,
            0,
            4 << 16,
            5 << 16,
            6 << 16,
        ];
        let entries = [entry; 3];
        let corners = [0, 1, 2];
        let groups = [
            runtime_group(
                &entries,
                &[corners, corners, corners],
                &[1, 0, 1],
                &[[[0; 4]; 3]; 3],
            ),
            runtime_group(&entries, &[corners, corners], &[-1, 1], &[[[0; 4]; 3]; 2]),
            runtime_group(&entries, &[corners, corners], &[0, 1], &[[[0; 4]; 3]; 2]),
        ];
        let fixture = fixture_for(&[(
            RootMenuC3dSlot::Championship,
            c3d_with_group0(
                &[Group0 {
                    field_04: -1,
                    matrix: identity_matrix(),
                    references: vec![0, 1, 2],
                }],
                &groups,
                &[
                    (*b"SQUARE!!!!!!", indexed_bmp_dimensions(1, 2, 8, 4, 4)),
                    (*b"TALL!!!!!!!!", indexed_bmp_dimensions(1, 2, 8, 4, 8)),
                ],
            ),
        )]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let states = uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults);
        let pre_scale = materialize_root_menu_mode2_pre_scale(&topology, states).unwrap();
        let scaled = materialize_root_menu_mode2_resource_scaled(&topology, states).unwrap();
        let pre_reference = pre_scale
            .get(RootMenuC3dSlot::Championship)
            .unwrap()
            .group0_records()
            .next()
            .unwrap()
            .references()
            .next()
            .unwrap();
        let references = scaled
            .get(RootMenuC3dSlot::Championship)
            .unwrap()
            .group0_records()
            .next()
            .unwrap()
            .references()
            .collect::<Vec<_>>();

        for batch in references[0].batches() {
            assert_eq!(batch.vertices()[0].adjusted_pair()[0], 0.1875);
        }
        for batch in references[1].batches() {
            assert_eq!(batch.vertices()[0].adjusted_pair()[0], 0.1875);
        }
        for batch in references[2].batches() {
            assert_eq!(batch.vertices()[0].adjusted_pair()[0], 0.5);
        }

        let before = pre_reference.batches().next().unwrap();
        let after = references[0].batches().next().unwrap();
        assert_eq!(after.selector(), before.selector());
        assert_eq!(
            after.source_entry32_ordinals(),
            before.source_entry32_ordinals()
        );
        assert_eq!(after.side_vectors(), before.side_vectors());
        assert_eq!(after.indices(), before.indices());
        let before_words = before.vertices()[0].words();
        let after_words = after.vertices()[0].words();
        assert_eq!(&after_words[..6], &before_words[..6]);
        assert_eq!(after_words[7], before_words[7]);
    }

    #[test]
    fn scaling_repeats_duplicate_corners_but_resets_for_each_reference() {
        let groups = [runtime_group(
            &[[0, 0, 0, 32_768, 0, 0, 0, 0]],
            &[[0, 0, 0]],
            &[0],
            &[[[0; 4]; 3]],
        )];
        let fixture = fixture_for(&[(
            RootMenuC3dSlot::Rally,
            c3d_with_group0(
                &[Group0 {
                    field_04: -1,
                    matrix: identity_matrix(),
                    references: vec![0, 0],
                }],
                &groups,
                &[(*b"DUPLICATE!!!", indexed_bmp_dimensions(1, 2, 8, 4, 8))],
            ),
        )]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let runtime = materialize_root_menu_mode2_resource_scaled(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        let references = runtime
            .get(RootMenuC3dSlot::Rally)
            .unwrap()
            .group0_records()
            .next()
            .unwrap()
            .references()
            .collect::<Vec<_>>();

        assert_eq!(references.len(), 2);
        for reference in references {
            assert_eq!(
                reference.batches().next().unwrap().vertices()[0].adjusted_pair()[0],
                27.0 / 1024.0
            );
        }
    }

    #[test]
    fn scaling_visited_storage_is_not_limited_to_legacy_1024_entries() {
        let mut entries = vec![[0; 8]; 1_025];
        entries[1_024][3] = 32_768;
        let groups = [runtime_group(
            &entries,
            &[[1_024, 1_024, 1_024]],
            &[0],
            &[[[0; 4]; 3]],
        )];
        let fixture = fixture_for(&[(
            RootMenuC3dSlot::TimeTrial,
            c3d_with_group0(
                &[Group0 {
                    field_04: -1,
                    matrix: identity_matrix(),
                    references: vec![0],
                }],
                &groups,
                &[(*b"OVER-1024!!!", indexed_bmp_dimensions(1, 2, 8, 4, 8))],
            ),
        )]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let runtime = materialize_root_menu_mode2_resource_scaled(
            &topology,
            uniform_load_states(0, C3dMode2CornerSource::OrdinaryDefaults),
        )
        .unwrap();
        let batch = runtime
            .get(RootMenuC3dSlot::TimeTrial)
            .unwrap()
            .group0_records()
            .next()
            .unwrap()
            .references()
            .next()
            .unwrap()
            .batches()
            .next()
            .unwrap();
        assert_eq!(batch.source_entry32_ordinals(), &[1_024]);
        assert_eq!(batch.vertices()[0].adjusted_pair()[0], 27.0 / 1024.0);
    }

    #[test]
    fn aliased_resource_uses_the_first_creating_slots_layout_policy() {
        let entries = [[0, 0, 0, 0, 0, 0, 0, 0]; 3];
        let groups = [runtime_group(&entries, &[[0, 1, 2]], &[0], &[[[0; 4]; 3]])];
        let object = || {
            c3d_with_group0(
                &[Group0 {
                    field_04: -1,
                    matrix: identity_matrix(),
                    references: vec![0],
                }],
                &groups,
                &[(*b"ALIAS-SCALE!", indexed_bmp_dimensions(1, 2, 8, 8, 4))],
            )
        };
        let fixture = fixture_for(&[
            (RootMenuC3dSlot::Championship, object()),
            (RootMenuC3dSlot::Rally, object()),
        ]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        assert_eq!(topology.resource_len(), 1);
        let mut states = [C3dMode2LoadState {
            prepared_resource_count: 0,
            corner_source: C3dMode2CornerSource::OrdinaryDefaults,
            surface_layout: C3dSurfaceLayoutGates::default(),
        }; ROOT_MENU_C3D_REQUEST_COUNT];
        states[RootMenuC3dSlot::Championship.index()]
            .surface_layout
            .expand_width_dominant = true;
        let runtime = materialize_root_menu_mode2_resource_scaled(
            &topology,
            RootMenuC3dMode2LoadStates::new(states),
        )
        .unwrap();

        for slot in [RootMenuC3dSlot::Championship, RootMenuC3dSlot::Rally] {
            assert_eq!(
                runtime
                    .get(slot)
                    .unwrap()
                    .group0_records()
                    .next()
                    .unwrap()
                    .references()
                    .next()
                    .unwrap()
                    .batches()
                    .next()
                    .unwrap()
                    .vertices()[0]
                    .adjusted_pair()[1],
                0.375
            );
        }
    }

    #[test]
    fn unusable_surface_extent_is_contextual_and_transactional() {
        let groups = [runtime_group(&[[0; 8]], &[[0, 0, 0]], &[0], &[[[0; 4]; 3]])];
        let mut bitmap = indexed_bmp(1, 2, 8);
        bitmap[22..26].copy_from_slice(&(-1i32).to_le_bytes());
        let fixture = fixture_for(&[(
            RootMenuC3dSlot::Options,
            c3d_with_group0(
                &[Group0 {
                    field_04: -1,
                    matrix: identity_matrix(),
                    references: vec![0],
                }],
                &groups,
                &[(*b"BAD-EXTENT!!", bitmap)],
            ),
        )]);
        let assets = load(&fixture);
        let topology = prepare_root_menu_c3d_topology(&assets, BitmapLimits::default()).unwrap();
        let resource_id = topology.resources().next().unwrap().0;
        let object = topology.get(RootMenuC3dSlot::Options).unwrap();
        let entry = object.group(0).unwrap().batches()[0].entries_24()[0];
        let states = [C3dMode2LoadState {
            prepared_resource_count: 0,
            corner_source: C3dMode2CornerSource::OrdinaryDefaults,
            surface_layout: C3dSurfaceLayoutGates::default(),
        }; ROOT_MENU_C3D_REQUEST_COUNT];

        assert_eq!(
            materialize_root_menu_mode2_resource_scaled(
                &topology,
                RootMenuC3dMode2LoadStates::new(states),
            ),
            Err(
                RootMenuC3dMode2MaterializationError::UnusableSurfaceExtent {
                    slot: RootMenuC3dSlot::Options,
                    group0_record_index: 0,
                    value_index: 0,
                    group1_ordinal: 0,
                    entry24_index: 0,
                    entry24_offset: entry.offset(),
                    resource_id,
                    first_slot: RootMenuC3dSlot::Options,
                    axis: C3dSurfaceLayoutAxis::Horizontal,
                    extent_bits: u16::MAX,
                }
            )
        );
    }

    fn identity_matrix() -> [i32; 16] {
        let mut matrix = [0; 16];
        matrix[0] = 1 << 16;
        matrix[5] = 1 << 16;
        matrix[10] = 1 << 16;
        matrix[15] = 1 << 16;
        matrix
    }

    fn uniform_load_states(
        prepared_resource_count: u32,
        corner_source: C3dMode2CornerSource,
    ) -> RootMenuC3dMode2LoadStates {
        RootMenuC3dMode2LoadStates::splat(C3dMode2LoadState {
            prepared_resource_count,
            corner_source,
            surface_layout: Default::default(),
        })
    }

    #[derive(Clone)]
    struct Group {
        entries_32: Vec<[i32; 8]>,
        corners: Vec<[u16; 3]>,
        selectors: Vec<i16>,
        tuples: Vec<[[u8; 4]; 3]>,
    }

    fn group(entry32_count: usize, corners: &[[u16; 3]], selectors: &[i16]) -> Group {
        assert_eq!(corners.len(), selectors.len());
        Group {
            entries_32: vec![[0; 8]; entry32_count],
            corners: corners.to_vec(),
            selectors: selectors.to_vec(),
            tuples: vec![[[0; 4]; 3]; selectors.len()],
        }
    }

    fn runtime_group(
        entries_32: &[[i32; 8]],
        corners: &[[u16; 3]],
        selectors: &[i16],
        tuples: &[[[u8; 4]; 3]],
    ) -> Group {
        assert_eq!(corners.len(), selectors.len());
        assert_eq!(tuples.len(), selectors.len());
        Group {
            entries_32: entries_32.to_vec(),
            corners: corners.to_vec(),
            selectors: selectors.to_vec(),
            tuples: tuples.to_vec(),
        }
    }

    struct Group0 {
        field_04: i16,
        matrix: [i32; 16],
        references: Vec<u16>,
    }

    fn c3d(groups: &[Group], blobs: &[([u8; 12], Vec<u8>)]) -> Vec<u8> {
        c3d_with_group0(&[], groups, blobs)
    }

    fn exact_root_c3d(group: &Group, blobs: &[([u8; 12], Vec<u8>)]) -> Vec<u8> {
        c3d_with_group0(
            &[Group0 {
                field_04: -1,
                matrix: identity_matrix(),
                references: vec![0],
            }],
            std::slice::from_ref(group),
            blobs,
        )
    }

    fn c3d_with_group0(
        group0_records: &[Group0],
        groups: &[Group],
        blobs: &[([u8; 12], Vec<u8>)],
    ) -> Vec<u8> {
        let group0_len = group0_records
            .iter()
            .map(|record| C3D_GROUP0_VALUES_OFFSET + record.references.len() * 2)
            .sum::<usize>();
        let group1_len = groups
            .iter()
            .map(|group| {
                C3D_GROUP1_FIXED_LEN
                    + group.entries_32.len() * C3D_GROUP1_ENTRY32_LEN
                    + group.selectors.len() * C3D_GROUP1_ENTRY24_LEN
            })
            .sum::<usize>();
        let group1_start = C3D_HEADER_LEN + group0_len;
        let blob_start = group1_start + group1_len;
        let total_len = blob_start + blobs.iter().map(|(_, blob)| 16 + blob.len()).sum::<usize>();
        let mut bytes = vec![0; total_len];
        bytes[0..4].copy_from_slice(&C3D_MAGIC);
        bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        bytes[12..14].copy_from_slice(&(group0_records.len() as u16).to_le_bytes());
        bytes[14..16].copy_from_slice(&(groups.len() as u16).to_le_bytes());
        bytes[16..18].copy_from_slice(&(blobs.len() as u16).to_le_bytes());
        for (index, offset) in [
            C3D_HEADER_LEN as u32,
            group1_start as u32,
            blob_start as u32,
        ]
        .into_iter()
        .enumerate()
        {
            let start = 20 + index * 4;
            bytes[start..start + 4].copy_from_slice(&offset.to_le_bytes());
        }

        let mut offset = C3D_HEADER_LEN;
        for record in group0_records {
            let record_len = C3D_GROUP0_VALUES_OFFSET + record.references.len() * 2;
            bytes[offset..offset + 4].copy_from_slice(&(record_len as u32).to_le_bytes());
            bytes[offset + 4..offset + 6].copy_from_slice(&record.field_04.to_le_bytes());
            for (index, value) in record.matrix.iter().enumerate() {
                let field = offset + 8 + index * 4;
                bytes[field..field + 4].copy_from_slice(&value.to_le_bytes());
            }
            bytes[offset + 76] = record.references.len() as u8;
            for (index, ordinal) in record.references.iter().enumerate() {
                let field = offset + C3D_GROUP0_VALUES_OFFSET + index * 2;
                bytes[field..field + 2].copy_from_slice(&ordinal.to_le_bytes());
            }
            offset += record_len;
        }
        debug_assert_eq!(offset, group1_start);
        for group in groups {
            let record_len = C3D_GROUP1_FIXED_LEN
                + group.entries_32.len() * C3D_GROUP1_ENTRY32_LEN
                + group.selectors.len() * C3D_GROUP1_ENTRY24_LEN;
            bytes[offset + 12..offset + 16].copy_from_slice(&(record_len as u32).to_le_bytes());
            bytes[offset + 16..offset + 18]
                .copy_from_slice(&(group.entries_32.len() as u16).to_le_bytes());
            bytes[offset + 18..offset + 20]
                .copy_from_slice(&(group.selectors.len() as u16).to_le_bytes());
            let entries24 =
                offset + C3D_GROUP1_FIXED_LEN + group.entries_32.len() * C3D_GROUP1_ENTRY32_LEN;
            for (entry_index, components) in group.entries_32.iter().enumerate() {
                let entry = offset + C3D_GROUP1_FIXED_LEN + entry_index * C3D_GROUP1_ENTRY32_LEN;
                for (component_index, component) in components.iter().enumerate() {
                    let field = entry + component_index * 4;
                    bytes[field..field + 4].copy_from_slice(&component.to_le_bytes());
                }
            }
            for (index, (corners, selector)) in
                group.corners.iter().zip(&group.selectors).enumerate()
            {
                let entry = entries24 + index * C3D_GROUP1_ENTRY24_LEN;
                for (corner, ordinal) in corners.iter().enumerate() {
                    let field = entry + C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[corner];
                    bytes[field..field + 2].copy_from_slice(&ordinal.to_le_bytes());
                }
                bytes[entry + 8..entry + 10].copy_from_slice(&selector.to_le_bytes());
                for (corner_index, tuple) in group.tuples[index].iter().enumerate() {
                    let field = entry + 10 + corner_index * 4;
                    bytes[field..field + 4].copy_from_slice(tuple);
                }
            }
            offset += record_len;
        }
        for (label, blob) in blobs {
            bytes[offset..offset + 12].copy_from_slice(label);
            bytes[offset + 12..offset + 16].copy_from_slice(&(blob.len() as u32).to_le_bytes());
            bytes[offset + 16..offset + 16 + blob.len()].copy_from_slice(blob);
            offset += 16 + blob.len();
        }
        bytes
    }

    fn indexed_bmp(pixel: u8, palette_blue: u8, depth: u16) -> Vec<u8> {
        indexed_bmp_dimensions(pixel, palette_blue, depth, 1, 1)
    }

    fn indexed_bmp_dimensions(
        pixel: u8,
        palette_blue: u8,
        depth: u16,
        width: u32,
        height: u32,
    ) -> Vec<u8> {
        let palette_len = 256 * 4;
        let pixel_offset = 14 + 40 + palette_len;
        let stride = (width as usize * usize::from(depth)).div_ceil(32) * 4;
        let image_len = stride * height as usize;
        let file_len = pixel_offset + image_len;
        let mut bytes = vec![0; file_len];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&(file_len as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&(width as i32).to_le_bytes());
        bytes[22..26].copy_from_slice(&(height as i32).to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&depth.to_le_bytes());
        bytes[34..38].copy_from_slice(&(image_len as u32).to_le_bytes());
        bytes[46..50].copy_from_slice(&256u32.to_le_bytes());
        for index in 0..256 {
            bytes[54 + index * 4] = palette_blue.wrapping_add(index as u8);
        }
        if depth == 8 {
            for row in 0..height as usize {
                bytes[pixel_offset + row * stride..pixel_offset + row * stride + width as usize]
                    .fill(pixel);
            }
        } else {
            bytes[pixel_offset] = pixel;
        }
        bytes
    }

    struct Fixture {
        _temporary: TempDirectory,
        store: AssetStore,
    }

    struct TempDirectory(PathBuf);

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn fixture_for(files: &[(RootMenuC3dSlot, Vec<u8>)]) -> Fixture {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temporary = TempDirectory(
            std::env::temp_dir().join(format!("cmr-c3d-topology-{}-{id}", std::process::id())),
        );
        let source = temporary.0.join("source");
        fs::create_dir_all(source.join("New_Fe")).unwrap();
        let mut mapped = BTreeMap::new();
        for (slot, bytes) in files {
            mapped.insert(slot.path(VisualVariant::Normal), bytes.clone());
        }
        let mut profile_files = Vec::new();
        for (path, bytes) in &mapped {
            let destination = source.join(path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(destination, bytes).unwrap();
            profile_files.push(asset_file(path, bytes));
        }
        profile_files.sort_by(|left, right| left.path.cmp(&right.path));
        let profile = AssetProfile {
            schema_version: 1,
            id: format!("synthetic-c3d-topology-{id}"),
            roots: vec!["New_Fe".to_owned()],
            empty_directories: Vec::new(),
            sentinels: Vec::new(),
            files: profile_files,
        };
        let reviewed = ReviewedProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
        let imported = temporary.0.join("runtime-data");
        import_source(&source, &imported, &reviewed).unwrap();
        let store = AssetStore::open(
            &imported,
            &reviewed,
            DecodeLimits {
                max_stored_bytes: 128 * 1024,
                max_payload_bytes: 128 * 1024,
            },
        )
        .unwrap();
        Fixture {
            _temporary: temporary,
            store,
        }
    }

    fn load(fixture: &Fixture) -> RootMenuC3dSet {
        load_root_menu_c3d(&fixture.store, VisualVariant::Normal, C3dLimits::default())
    }

    fn asset_file(path: &str, bytes: &[u8]) -> AssetFile {
        AssetFile {
            path: path.to_owned(),
            size: bytes.len() as u64,
            sha256: hex_sha256(bytes),
            encoding: StorageEncoding::Raw,
        }
    }

    fn hex_sha256(bytes: &[u8]) -> String {
        use std::fmt::Write as _;

        let digest = Sha256::digest(bytes);
        let mut encoded = String::with_capacity(digest.len() * 2);
        for byte in digest {
            write!(encoded, "{byte:02x}").unwrap();
        }
        encoded
    }
}
