//! Backend-neutral draw-command planning for the exact root-menu C3D shape.
//!
//! The original root callback updates one selected actor, marks it visible for
//! one scene pass, and defers actual drawing until the frame tail. A plan owns
//! immutable snapshots of that pass. It deliberately does not bind materials,
//! cache selectors, mutate unresolved fog/color fields, or rasterize pixels.

use std::fmt;

use cmr_formats::c3d::C3D_GROUP0_VALUES_OFFSET;

use crate::{
    display::DisplayExtent,
    root_menu_assets::{RootMenuObjectIdentity, VisualVariant},
    root_menu_c3d_assets::{RootMenuC3dAngles, RootMenuC3dSlot},
    root_menu_c3d_resources::{
        C3dPreparedResourceAccessError, C3dPreparedResourceId, C3dPreparedResourceProvenance,
        C3dPreparedSelector,
    },
    root_menu_c3d_runtime::{
        C3dMode2ResourceScaledVertex, C3dOpaqueSideVector, RootMenuC3dMode2ResourceScaledRuntime,
    },
    root_menu_c3d_topology::RootMenuC3dTopology,
    root_menu_c3d_transform::{
        C3dMatrixWords, C3dViewportWords, ROOT_MENU_C3D_PROJECTION, ROOT_MENU_C3D_VIEW,
        RootMenuC3dViewportError, root_menu_c3d_viewport, root_menu_c3d_world,
    },
};

pub const ROOT_MENU_C3D_VERTEX_TYPE: u32 = 2;
pub const ROOT_MENU_C3D_PRIMITIVE_CODE: u32 = 4;
pub const ROOT_MENU_C3D_DRAW_FLAGS: u32 = 8;

/// One source-ordered batch ready for a later compatibility executor.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dDrawCommand {
    slot: RootMenuC3dSlot,
    group0_record_index: usize,
    value_index: usize,
    group1_ordinal: u16,
    batch_index: usize,
    selector: C3dPreparedSelector,
    source_entry32_ordinals: Box<[u16]>,
    vertices: Box<[C3dMode2ResourceScaledVertex]>,
    side_vectors: Box<[C3dOpaqueSideVector]>,
    indices: Box<[u16]>,
}

impl RootMenuC3dDrawCommand {
    #[must_use]
    pub const fn slot(&self) -> RootMenuC3dSlot {
        self.slot
    }

    #[must_use]
    pub const fn group0_record_index(&self) -> usize {
        self.group0_record_index
    }

    #[must_use]
    pub const fn value_index(&self) -> usize {
        self.value_index
    }

    #[must_use]
    pub const fn group1_ordinal(&self) -> u16 {
        self.group1_ordinal
    }

    #[must_use]
    pub const fn batch_index(&self) -> usize {
        self.batch_index
    }

    #[must_use]
    pub const fn selector(&self) -> C3dPreparedSelector {
        self.selector
    }

    #[must_use]
    pub fn source_entry32_ordinals(&self) -> &[u16] {
        &self.source_entry32_ordinals
    }

    #[must_use]
    pub fn vertices(&self) -> &[C3dMode2ResourceScaledVertex] {
        &self.vertices
    }

    #[must_use]
    pub fn side_vectors(&self) -> &[C3dOpaqueSideVector] {
        &self.side_vectors
    }

    #[must_use]
    pub fn indices(&self) -> &[u16] {
        &self.indices
    }

    #[must_use]
    pub const fn vertex_type(&self) -> u32 {
        ROOT_MENU_C3D_VERTEX_TYPE
    }

    #[must_use]
    pub const fn primitive_code(&self) -> u32 {
        ROOT_MENU_C3D_PRIMITIVE_CODE
    }

    #[must_use]
    pub const fn draw_flags(&self) -> u32 {
        ROOT_MENU_C3D_DRAW_FLAGS
    }
}

/// Owned description of one selected root object's deferred scene submission.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dFramePlan {
    provenance: C3dPreparedResourceProvenance,
    variant: VisualVariant,
    slot: RootMenuC3dSlot,
    identity: RootMenuObjectIdentity,
    previous_angle: u16,
    resulting_angle: u16,
    delta: u16,
    world: C3dMatrixWords,
    viewport: C3dViewportWords,
    commands: Box<[RootMenuC3dDrawCommand]>,
}

impl RootMenuC3dFramePlan {
    #[must_use]
    pub const fn provenance(&self) -> C3dPreparedResourceProvenance {
        self.provenance
    }

    #[must_use]
    pub const fn variant(&self) -> VisualVariant {
        self.variant
    }

    #[must_use]
    pub const fn slot(&self) -> RootMenuC3dSlot {
        self.slot
    }

    #[must_use]
    pub const fn identity(&self) -> RootMenuObjectIdentity {
        self.identity
    }

    #[must_use]
    pub const fn previous_angle(&self) -> u16 {
        self.previous_angle
    }

    #[must_use]
    pub const fn resulting_angle(&self) -> u16 {
        self.resulting_angle
    }

    #[must_use]
    pub const fn delta(&self) -> u16 {
        self.delta
    }

    #[must_use]
    pub const fn effective_y_phase(&self) -> u16 {
        self.resulting_angle & 0x0fff
    }

    #[must_use]
    pub const fn world(&self) -> C3dMatrixWords {
        self.world
    }

    #[must_use]
    pub const fn view(&self) -> C3dMatrixWords {
        ROOT_MENU_C3D_VIEW
    }

    #[must_use]
    pub const fn projection(&self) -> C3dMatrixWords {
        ROOT_MENU_C3D_PROJECTION
    }

    #[must_use]
    pub const fn viewport(&self) -> C3dViewportWords {
        self.viewport
    }

    #[must_use]
    pub fn commands(&self) -> &[RootMenuC3dDrawCommand] {
        &self.commands
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootMenuC3dRootShapeIssue {
    ObjectMetadata,
    TopologyGroup0Count {
        actual: usize,
    },
    RuntimeGroup0Count {
        actual: usize,
    },
    TopologyGroup1Count {
        actual: usize,
    },
    Group0Metadata,
    TopologyReferenceCount {
        actual: usize,
    },
    RuntimeReferenceCount {
        actual: usize,
    },
    ReferenceMetadata,
    Group1Metadata,
    BatchCount {
        topology: usize,
        runtime: usize,
    },
    BatchMetadata {
        batch_index: usize,
    },
    BatchPayload {
        batch_index: usize,
    },
    MissingSelector {
        batch_index: usize,
    },
    ResourceAccess {
        batch_index: usize,
        resource_id: C3dPreparedResourceId,
        source: C3dPreparedResourceAccessError,
    },
}

impl fmt::Display for RootMenuC3dRootShapeIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectMetadata => formatter.write_str("object metadata differs"),
            Self::TopologyGroup0Count { actual } => {
                write!(
                    formatter,
                    "topology has {actual} group-0 records instead of 1"
                )
            }
            Self::RuntimeGroup0Count { actual } => {
                write!(
                    formatter,
                    "runtime has {actual} group-0 records instead of 1"
                )
            }
            Self::TopologyGroup1Count { actual } => {
                write!(
                    formatter,
                    "topology has {actual} group-1 records instead of 1"
                )
            }
            Self::Group0Metadata => formatter.write_str("group-0 metadata is not root level zero"),
            Self::TopologyReferenceCount { actual } => {
                write!(
                    formatter,
                    "topology has {actual} level references instead of 1"
                )
            }
            Self::RuntimeReferenceCount { actual } => {
                write!(
                    formatter,
                    "runtime has {actual} level references instead of 1"
                )
            }
            Self::ReferenceMetadata => {
                formatter.write_str("level reference metadata does not select group-1 zero")
            }
            Self::Group1Metadata => {
                formatter.write_str("group-1 zero is empty or has nonzero exact-root metadata")
            }
            Self::BatchCount { topology, runtime } => write!(
                formatter,
                "topology/runtime batch counts differ ({topology} versus {runtime})"
            ),
            Self::BatchMetadata { batch_index } => {
                write!(formatter, "batch {batch_index} metadata differs")
            }
            Self::BatchPayload { batch_index } => {
                write!(
                    formatter,
                    "batch {batch_index} compact payload is inconsistent"
                )
            }
            Self::MissingSelector { batch_index } => {
                write!(
                    formatter,
                    "batch {batch_index} has a missing root resource selector"
                )
            }
            Self::ResourceAccess {
                batch_index,
                resource_id,
                source,
            } => write!(
                formatter,
                "batch {batch_index} cannot resolve prepared resource {resource_id:?}: {source}"
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootMenuC3dFramePlanError {
    VariantMismatch {
        topology: VisualVariant,
        runtime: VisualVariant,
    },
    ProvenanceMismatch,
    SlotPresenceMismatch {
        slot: RootMenuC3dSlot,
        topology_present: bool,
        runtime_present: bool,
    },
    UnexpectedRootShape {
        slot: RootMenuC3dSlot,
        issue: RootMenuC3dRootShapeIssue,
    },
    Viewport(RootMenuC3dViewportError),
}

impl fmt::Display for RootMenuC3dFramePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VariantMismatch { topology, runtime } => write!(
                formatter,
                "root-menu C3D topology variant {topology:?} differs from runtime variant {runtime:?}"
            ),
            Self::ProvenanceMismatch => formatter.write_str(
                "root-menu C3D topology and runtime belong to different prepared arenas",
            ),
            Self::SlotPresenceMismatch {
                slot,
                topology_present,
                runtime_present,
            } => write!(
                formatter,
                "root-menu C3D slot {slot:?} presence differs between topology ({topology_present}) and runtime ({runtime_present})"
            ),
            Self::UnexpectedRootShape { slot, issue } => {
                write!(
                    formatter,
                    "root-menu C3D slot {slot:?} is not the exact root shape: {issue}"
                )
            }
            Self::Viewport(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for RootMenuC3dFramePlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Viewport(source) => Some(source),
            _ => None,
        }
    }
}

/// Plans one selected root object after complete preflight validation.
///
/// `Ok(None)` means the optional slot is absent from both matching products.
/// Every typed error and absent slot leaves all persistent angles unchanged.
pub fn plan_root_menu_c3d_frame(
    topology: &RootMenuC3dTopology<'_>,
    runtime: &RootMenuC3dMode2ResourceScaledRuntime,
    angles: &mut RootMenuC3dAngles,
    slot: RootMenuC3dSlot,
    delta: u16,
    extent: DisplayExtent,
) -> Result<Option<RootMenuC3dFramePlan>, RootMenuC3dFramePlanError> {
    if topology.variant() != runtime.variant() {
        return Err(RootMenuC3dFramePlanError::VariantMismatch {
            topology: topology.variant(),
            runtime: runtime.variant(),
        });
    }
    if topology.provenance() != runtime.provenance() {
        return Err(RootMenuC3dFramePlanError::ProvenanceMismatch);
    }
    let viewport = root_menu_c3d_viewport(extent).map_err(RootMenuC3dFramePlanError::Viewport)?;
    let topology_object = topology.get(slot);
    let runtime_object = runtime.get(slot);
    match (topology_object, runtime_object) {
        (None, None) => return Ok(None),
        (Some(_), None) | (None, Some(_)) => {
            return Err(RootMenuC3dFramePlanError::SlotPresenceMismatch {
                slot,
                topology_present: topology_object.is_some(),
                runtime_present: runtime_object.is_some(),
            });
        }
        (Some(_), Some(_)) => {}
    }
    let topology_object = topology_object.expect("matched present topology slot");
    let runtime_object = runtime_object.expect("matched present runtime slot");
    let commands = preflight_root_commands(topology, slot, topology_object, runtime_object)?;

    let (previous_angle, resulting_angle) = angles.advance_loaded_slot(slot, delta);
    Ok(Some(RootMenuC3dFramePlan {
        provenance: topology.provenance(),
        variant: topology.variant(),
        slot,
        identity: topology_object.identity(),
        previous_angle,
        resulting_angle,
        delta,
        world: root_menu_c3d_world(resulting_angle),
        viewport,
        commands: commands.into_boxed_slice(),
    }))
}

fn preflight_root_commands(
    topology: &RootMenuC3dTopology<'_>,
    slot: RootMenuC3dSlot,
    topology_object: &crate::root_menu_c3d_topology::RootMenuC3dObjectTopology<'_>,
    runtime_object: &crate::root_menu_c3d_runtime::RootMenuC3dMode2ResourceScaledObject,
) -> Result<Vec<RootMenuC3dDrawCommand>, RootMenuC3dFramePlanError> {
    let fail = |issue| RootMenuC3dFramePlanError::UnexpectedRootShape { slot, issue };
    if topology_object.slot() != slot
        || runtime_object.slot() != slot
        || topology_object.identity() != slot.identity()
        || runtime_object.identity() != slot.identity()
        || topology_object.identity() != runtime_object.identity()
        || topology_object.path() != runtime_object.path()
        || topology_object.path() != slot.path(topology.variant())
    {
        return Err(fail(RootMenuC3dRootShapeIssue::ObjectMetadata));
    }

    let topology_group0 = topology_object.group0_records().collect::<Vec<_>>();
    if topology_group0.len() != 1 {
        return Err(fail(RootMenuC3dRootShapeIssue::TopologyGroup0Count {
            actual: topology_group0.len(),
        }));
    }
    let runtime_group0 = runtime_object.group0_records().collect::<Vec<_>>();
    if runtime_group0.len() != 1 {
        return Err(fail(RootMenuC3dRootShapeIssue::RuntimeGroup0Count {
            actual: runtime_group0.len(),
        }));
    }
    let topology_groups = topology_object.groups().collect::<Vec<_>>();
    if topology_groups.len() != 1 {
        return Err(fail(RootMenuC3dRootShapeIssue::TopologyGroup1Count {
            actual: topology_groups.len(),
        }));
    }

    let topology_group0 = topology_group0[0];
    let runtime_group0 = runtime_group0[0];
    if topology_group0.record_index() != 0
        || runtime_group0.record_index() != 0
        || topology_group0.record_offset() != runtime_group0.record_offset()
        || topology_group0.record().field_04_i16() != -1
        || topology_group0.record().values().collect::<Vec<_>>() != [0]
    {
        return Err(fail(RootMenuC3dRootShapeIssue::Group0Metadata));
    }

    let topology_references = topology_group0.references().collect::<Vec<_>>();
    if topology_references.len() != 1 {
        return Err(fail(RootMenuC3dRootShapeIssue::TopologyReferenceCount {
            actual: topology_references.len(),
        }));
    }
    let runtime_references = runtime_group0.references().collect::<Vec<_>>();
    if runtime_references.len() != 1 {
        return Err(fail(RootMenuC3dRootShapeIssue::RuntimeReferenceCount {
            actual: runtime_references.len(),
        }));
    }
    let topology_reference = topology_references[0];
    let runtime_reference = runtime_references[0];
    if topology_reference.value_index() != 0
        || topology_reference.group1_ordinal() != 0
        || runtime_reference.value_index() != 0
        || runtime_reference.group1_ordinal() != 0
        || topology_reference.source_offset() != runtime_reference.source_offset()
        || topology_reference.source_offset()
            != topology_group0.record_offset() + C3D_GROUP0_VALUES_OFFSET
    {
        return Err(fail(RootMenuC3dRootShapeIssue::ReferenceMetadata));
    }

    let group1 = topology_groups[0];
    let group1_record = group1.record();
    if group1.record_index() != 0
        || group1_record.entry32_count() == 0
        || group1_record.entry24_count() == 0
        || group1_record.unknown_bytes().iter().any(|byte| *byte != 0)
    {
        return Err(fail(RootMenuC3dRootShapeIssue::Group1Metadata));
    }

    let topology_batches = group1.batches();
    let runtime_batches = runtime_reference.batches().collect::<Vec<_>>();
    if topology_batches.len() != runtime_batches.len() || topology_batches.is_empty() {
        return Err(fail(RootMenuC3dRootShapeIssue::BatchCount {
            topology: topology_batches.len(),
            runtime: runtime_batches.len(),
        }));
    }

    let mut commands = Vec::with_capacity(topology_batches.len());
    for (batch_index, (topology_batch, runtime_batch)) in
        topology_batches.iter().zip(runtime_batches).enumerate()
    {
        if topology_batch.selector() != runtime_batch.selector()
            || topology_batch.unique_entry32_ordinals() != runtime_batch.source_entry32_ordinals()
            || topology_batch.indices() != runtime_batch.indices()
        {
            return Err(fail(RootMenuC3dRootShapeIssue::BatchMetadata {
                batch_index,
            }));
        }
        let C3dPreparedSelector::Resource(resource_id) = runtime_batch.selector() else {
            return Err(fail(RootMenuC3dRootShapeIssue::MissingSelector {
                batch_index,
            }));
        };
        topology.resource(resource_id).map_err(|source| {
            fail(RootMenuC3dRootShapeIssue::ResourceAccess {
                batch_index,
                resource_id,
                source,
            })
        })?;
        let vertex_count = runtime_batch.vertices().len();
        if vertex_count != runtime_batch.source_entry32_ordinals().len()
            || vertex_count != runtime_batch.side_vectors().len()
            || runtime_batch.indices().len() != topology_batch.entries_24().len().saturating_mul(3)
            || runtime_batch
                .indices()
                .iter()
                .any(|index| usize::from(*index) >= vertex_count)
        {
            return Err(fail(RootMenuC3dRootShapeIssue::BatchPayload {
                batch_index,
            }));
        }
        commands.push(RootMenuC3dDrawCommand {
            slot,
            group0_record_index: 0,
            value_index: 0,
            group1_ordinal: 0,
            batch_index,
            selector: runtime_batch.selector(),
            source_entry32_ordinals: runtime_batch.source_entry32_ordinals().into(),
            vertices: runtime_batch.vertices().into(),
            side_vectors: runtime_batch.side_vectors().into(),
            indices: runtime_batch.indices().into(),
        });
    }
    Ok(commands)
}
