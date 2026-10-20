//! Runtime vertices for root-menu mode-2 C3D objects.
//!
//! The original loader converts one independent vertex array for every
//! group-0 value that references a group-1 record. This module preserves that
//! ownership, captures the opaque three-word side vector before mode-specific
//! mutation, and only then compacts vertices through the checked batch plan.
//! The resource-scaled output remains backend-neutral: palette attachment,
//! upload, rendering, and presentation are later boundaries.

use std::fmt;

use cmr_formats::c3d::{C3dEntry32, C3dGroup0Record};

use crate::{
    root_menu_assets::{RootMenuObjectIdentity, VisualVariant},
    root_menu_c3d_assets::{ROOT_MENU_C3D_REQUEST_COUNT, RootMenuC3dSlot},
    root_menu_c3d_resources::{
        C3dEntryMaterialPolicy, C3dLoadPolicy, C3dPreparedResourceAccessError,
        C3dPreparedResourceId, C3dPreparedResourceProvenance, C3dPreparedSelector,
        C3dResourceAllocationPolicy, C3dVertexMutationBranch,
    },
    root_menu_c3d_topology::{
        RootMenuC3dGroup0Topology, RootMenuC3dGroup1Topology, RootMenuC3dObjectTopology,
        RootMenuC3dTopology,
    },
};

const Q16_SCALE: f32 = f32::from_bits(0x3780_0000);
const MODE2_OPAQUE_WORD: u32 = 0xff00_0000;
const ORDINARY_MODE2_CORNER: [u8; 4] = [0, 0, 0, 0x96];

/// Selects the recovered gate controlling mode-2 corner tuple provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dMode2CornerSource {
    /// Use the ordinary mode-2 fallback tuple `[0, 0, 0, 0x96]`.
    OrdinaryDefaults,
    /// Use each byte-exact tuple stored in the source 24-byte entry.
    SourceEntryTuples,
}

pub use crate::root_menu_c3d_resources::C3dSurfaceLayoutGates;

/// State supplied independently to one root-menu C3D loader call.
///
/// `prepared_resource_count` is the count sampled at that loader entry. It is
/// deliberately not derived from the reconstructed preparation arena.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dMode2LoadState {
    pub prepared_resource_count: u32,
    pub corner_source: C3dMode2CornerSource,
    pub surface_layout: C3dSurfaceLayoutGates,
}

impl C3dLoadPolicy {
    /// Adapts the existing root mode-2 state without changing its behavior.
    #[must_use]
    pub const fn root_mode2(load_state: C3dMode2LoadState) -> Self {
        Self {
            mode: 2,
            caller_option: 0,
            caller_flags: 0x11,
            surface_constructor_flags: 0x35,
            entry_resource_count: load_state.prepared_resource_count,
            allocation: C3dResourceAllocationPolicy::InternExact,
            surface_layout: load_state.surface_layout,
            material: match load_state.corner_source {
                C3dMode2CornerSource::OrdinaryDefaults => C3dEntryMaterialPolicy::Mode2Ordinary,
                C3dMode2CornerSource::SourceEntryTuples => {
                    C3dEntryMaterialPolicy::Mode2SourceTuples
                }
            },
        }
    }
}

/// Fixed per-slot loader-entry states for the six independent root calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2LoadStates {
    slots: [C3dMode2LoadState; ROOT_MENU_C3D_REQUEST_COUNT],
}

impl RootMenuC3dMode2LoadStates {
    #[must_use]
    pub const fn new(slots: [C3dMode2LoadState; ROOT_MENU_C3D_REQUEST_COUNT]) -> Self {
        Self { slots }
    }

    #[must_use]
    pub const fn splat(state: C3dMode2LoadState) -> Self {
        Self {
            slots: [state; ROOT_MENU_C3D_REQUEST_COUNT],
        }
    }

    /// Recovered loader-entry state for a fresh official-EU frontend using the
    /// software renderer and the default mutable configuration.
    ///
    /// This is deliberately narrower than a general root-menu default: an
    /// accepted save, prior C3D allocations, a later reload, or a hardware
    /// renderer can change one or more of these values.
    #[must_use]
    pub const fn official_eu_initial_software_default_config() -> Self {
        const SURFACE_LAYOUT: C3dSurfaceLayoutGates = C3dSurfaceLayoutGates {
            quarter_if_large: false,
            force_quarter: false,
            half_if_large: false,
            expand_width_dominant: true,
        };
        const fn state(prepared_resource_count: u32) -> C3dMode2LoadState {
            C3dMode2LoadState {
                prepared_resource_count,
                corner_source: C3dMode2CornerSource::OrdinaryDefaults,
                surface_layout: SURFACE_LAYOUT,
            }
        }

        Self::new([
            state(55),
            state(63),
            state(63),
            state(69),
            state(73),
            state(79),
        ])
    }

    #[must_use]
    pub const fn get(self, slot: RootMenuC3dSlot) -> C3dMode2LoadState {
        self.slots[slot.index()]
    }
}

/// Exact eight-word runtime vertex before recovered resource-driven UV scaling.
///
/// Words 4 and 5 are layout-dependent: type 1 preserves source values for
/// present selectors, while type 2 interprets them as packed colors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dMode2PreScaleVertex {
    words: [u32; 8],
}

impl C3dMode2PreScaleVertex {
    #[must_use]
    pub const fn words(self) -> [u32; 8] {
        self.words
    }

    #[must_use]
    pub fn first_triplet(self) -> [f32; 3] {
        std::array::from_fn(|index| f32::from_bits(self.words[index]))
    }

    #[must_use]
    pub const fn reserved_word(self) -> u32 {
        self.words[3]
    }

    #[must_use]
    pub const fn opaque_triplet_words(self) -> [u32; 3] {
        [self.words[3], self.words[4], self.words[5]]
    }

    #[must_use]
    pub fn adjusted_pair(self) -> [f32; 2] {
        [f32::from_bits(self.words[6]), f32::from_bits(self.words[7])]
    }
}

/// The three runtime f32 words captured before mode-2 mutation.
///
/// Their higher-level rendering meaning remains unresolved, so only exact bits
/// are exposed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dOpaqueSideVector([u32; 3]);

impl C3dOpaqueSideVector {
    #[must_use]
    pub const fn words(self) -> [u32; 3] {
        self.0
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2PreScaleBatch {
    selector: C3dPreparedSelector,
    source_entry32_ordinals: Vec<u16>,
    vertices: Vec<C3dMode2PreScaleVertex>,
    side_vectors: Vec<C3dOpaqueSideVector>,
    indices: Vec<u16>,
}

impl RootMenuC3dMode2PreScaleBatch {
    #[must_use]
    pub const fn selector(&self) -> C3dPreparedSelector {
        self.selector
    }

    /// Source group-1 entry32 ordinals parallel to `vertices` and
    /// `side_vectors`.
    #[must_use]
    pub fn source_entry32_ordinals(&self) -> &[u16] {
        &self.source_entry32_ordinals
    }

    #[must_use]
    pub fn vertices(&self) -> &[C3dMode2PreScaleVertex] {
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
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2PreScaleReference {
    value_index: usize,
    source_offset: usize,
    group1_ordinal: u16,
    batches: Vec<RootMenuC3dMode2PreScaleBatch>,
}

impl RootMenuC3dMode2PreScaleReference {
    #[must_use]
    pub const fn value_index(&self) -> usize {
        self.value_index
    }

    #[must_use]
    pub const fn source_offset(&self) -> usize {
        self.source_offset
    }

    #[must_use]
    pub const fn group1_ordinal(&self) -> u16 {
        self.group1_ordinal
    }

    pub fn batches(&self) -> impl ExactSizeIterator<Item = &RootMenuC3dMode2PreScaleBatch> {
        self.batches.iter()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2PreScaleGroup0 {
    record_index: usize,
    record_offset: usize,
    references: Vec<RootMenuC3dMode2PreScaleReference>,
}

impl RootMenuC3dMode2PreScaleGroup0 {
    #[must_use]
    pub const fn record_index(&self) -> usize {
        self.record_index
    }

    #[must_use]
    pub const fn record_offset(&self) -> usize {
        self.record_offset
    }

    pub fn references(&self) -> impl ExactSizeIterator<Item = &RootMenuC3dMode2PreScaleReference> {
        self.references.iter()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2PreScaleObject {
    slot: RootMenuC3dSlot,
    identity: RootMenuObjectIdentity,
    path: &'static str,
    group0_records: Vec<RootMenuC3dMode2PreScaleGroup0>,
}

impl RootMenuC3dMode2PreScaleObject {
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

    pub fn group0_records(&self) -> impl ExactSizeIterator<Item = &RootMenuC3dMode2PreScaleGroup0> {
        self.group0_records.iter()
    }
}

/// Fixed-slot mode-2 runtime data before resource-driven coordinate scaling.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2PreScaleRuntime {
    provenance: C3dPreparedResourceProvenance,
    variant: VisualVariant,
    slots: [Option<RootMenuC3dMode2PreScaleObject>; ROOT_MENU_C3D_REQUEST_COUNT],
}

/// Exact eight-word runtime vertex after resource-derived UV scaling.
///
/// The submitted vertex type determines the meaning of words 3 through 5.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dMode2ResourceScaledVertex {
    words: [u32; 8],
}

impl C3dMode2ResourceScaledVertex {
    #[must_use]
    pub const fn words(self) -> [u32; 8] {
        self.words
    }

    #[must_use]
    pub fn first_triplet(self) -> [f32; 3] {
        std::array::from_fn(|index| f32::from_bits(self.words[index]))
    }

    #[must_use]
    pub const fn reserved_word(self) -> u32 {
        self.words[3]
    }

    #[must_use]
    pub const fn opaque_triplet_words(self) -> [u32; 3] {
        [self.words[3], self.words[4], self.words[5]]
    }

    #[must_use]
    pub fn adjusted_pair(self) -> [f32; 2] {
        [f32::from_bits(self.words[6]), f32::from_bits(self.words[7])]
    }
}

/// Type-2-only view exposing the packed diffuse/specular layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dType2ResourceScaledVertex(C3dMode2ResourceScaledVertex);

/// Type-1-only view exposing position, normal, and texture coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dType1ResourceScaledVertex(C3dMode2ResourceScaledVertex);

impl C3dType1ResourceScaledVertex {
    #[must_use]
    pub const fn new(
        vertex: C3dMode2ResourceScaledVertex,
        branch: C3dVertexMutationBranch,
    ) -> Option<Self> {
        match branch {
            C3dVertexMutationBranch::Type1 => Some(Self(vertex)),
            C3dVertexMutationBranch::Type2 => None,
        }
    }

    #[must_use]
    pub fn position(self) -> [f32; 3] {
        self.0.first_triplet()
    }

    #[must_use]
    pub fn normal(self) -> [f32; 3] {
        self.0.opaque_triplet_words().map(f32::from_bits)
    }

    #[must_use]
    pub fn uv(self) -> [f32; 2] {
        self.0.adjusted_pair()
    }
}

impl C3dType2ResourceScaledVertex {
    #[must_use]
    pub const fn new(
        vertex: C3dMode2ResourceScaledVertex,
        branch: C3dVertexMutationBranch,
    ) -> Option<Self> {
        match branch {
            C3dVertexMutationBranch::Type1 => None,
            C3dVertexMutationBranch::Type2 => Some(Self(vertex)),
        }
    }

    #[must_use]
    pub const fn vertex(self) -> C3dMode2ResourceScaledVertex {
        self.0
    }

    #[must_use]
    pub const fn diffuse(self) -> u32 {
        self.0.words[4]
    }

    #[must_use]
    pub const fn specular(self) -> u32 {
        self.0.words[5]
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2ResourceScaledBatch {
    selector: C3dPreparedSelector,
    source_entry32_ordinals: Vec<u16>,
    vertices: Vec<C3dMode2ResourceScaledVertex>,
    side_vectors: Vec<C3dOpaqueSideVector>,
    indices: Vec<u16>,
}

impl RootMenuC3dMode2ResourceScaledBatch {
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
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2ResourceScaledReference {
    value_index: usize,
    source_offset: usize,
    group1_ordinal: u16,
    batches: Vec<RootMenuC3dMode2ResourceScaledBatch>,
}

impl RootMenuC3dMode2ResourceScaledReference {
    #[must_use]
    pub const fn value_index(&self) -> usize {
        self.value_index
    }

    #[must_use]
    pub const fn source_offset(&self) -> usize {
        self.source_offset
    }

    #[must_use]
    pub const fn group1_ordinal(&self) -> u16 {
        self.group1_ordinal
    }

    pub fn batches(&self) -> impl ExactSizeIterator<Item = &RootMenuC3dMode2ResourceScaledBatch> {
        self.batches.iter()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2ResourceScaledGroup0 {
    record_index: usize,
    record_offset: usize,
    references: Vec<RootMenuC3dMode2ResourceScaledReference>,
}

impl RootMenuC3dMode2ResourceScaledGroup0 {
    #[must_use]
    pub const fn record_index(&self) -> usize {
        self.record_index
    }

    #[must_use]
    pub const fn record_offset(&self) -> usize {
        self.record_offset
    }

    pub fn references(
        &self,
    ) -> impl ExactSizeIterator<Item = &RootMenuC3dMode2ResourceScaledReference> {
        self.references.iter()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2ResourceScaledObject {
    slot: RootMenuC3dSlot,
    identity: RootMenuObjectIdentity,
    path: &'static str,
    group0_records: Vec<RootMenuC3dMode2ResourceScaledGroup0>,
}

impl RootMenuC3dMode2ResourceScaledObject {
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
    ) -> impl ExactSizeIterator<Item = &RootMenuC3dMode2ResourceScaledGroup0> {
        self.group0_records.iter()
    }
}

/// Fixed-slot, resource-scaled mode-2 CPU runtime data.
///
/// Resource selectors remain backend-neutral. This value is therefore not
/// render-ready.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dMode2ResourceScaledRuntime {
    provenance: C3dPreparedResourceProvenance,
    variant: VisualVariant,
    slots: [Option<RootMenuC3dMode2ResourceScaledObject>; ROOT_MENU_C3D_REQUEST_COUNT],
}

impl RootMenuC3dMode2ResourceScaledRuntime {
    #[must_use]
    pub const fn provenance(&self) -> C3dPreparedResourceProvenance {
        self.provenance
    }

    #[must_use]
    pub const fn variant(&self) -> VisualVariant {
        self.variant
    }

    #[must_use]
    pub fn get(&self, slot: RootMenuC3dSlot) -> Option<&RootMenuC3dMode2ResourceScaledObject> {
        self.slots[slot.index()].as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dSurfaceLayoutAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootMenuC3dMode2MaterializationError {
    MissingGroup1Topology {
        slot: RootMenuC3dSlot,
        group0_record_index: usize,
        value_index: usize,
        group1_ordinal: u16,
        group1_count: usize,
    },
    ResourceAccess {
        slot: RootMenuC3dSlot,
        group0_record_index: usize,
        value_index: usize,
        group1_ordinal: u16,
        entry24_index: usize,
        entry24_offset: usize,
        resource_id: C3dPreparedResourceId,
        source: C3dPreparedResourceAccessError,
    },
    UnusableSurfaceExtent {
        slot: RootMenuC3dSlot,
        group0_record_index: usize,
        value_index: usize,
        group1_ordinal: u16,
        entry24_index: usize,
        entry24_offset: usize,
        resource_id: C3dPreparedResourceId,
        first_slot: RootMenuC3dSlot,
        axis: C3dSurfaceLayoutAxis,
        extent_bits: u16,
    },
}

impl fmt::Display for RootMenuC3dMode2MaterializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingGroup1Topology {
                slot,
                group0_record_index,
                value_index,
                group1_ordinal,
                group1_count,
            } => write!(
                formatter,
                "root-menu C3D slot {slot:?} group-0 record {group0_record_index} value {value_index} references missing group-1 topology {group1_ordinal} of {group1_count}"
            ),
            Self::ResourceAccess {
                slot,
                group0_record_index,
                value_index,
                group1_ordinal,
                entry24_index,
                entry24_offset,
                resource_id,
                source,
            } => write!(
                formatter,
                "root-menu C3D slot {slot:?} group-0 record {group0_record_index} value {value_index} group-1 {group1_ordinal} entry24 {entry24_index} at offset {entry24_offset} cannot resolve prepared resource {resource_id:?}: {source}"
            ),
            Self::UnusableSurfaceExtent {
                slot,
                group0_record_index,
                value_index,
                group1_ordinal,
                entry24_index,
                entry24_offset,
                resource_id,
                first_slot,
                axis,
                extent_bits,
            } => write!(
                formatter,
                "root-menu C3D slot {slot:?} group-0 record {group0_record_index} value {value_index} group-1 {group1_ordinal} entry24 {entry24_index} at offset {entry24_offset} resource {resource_id:?} first prepared in {first_slot:?} has unusable {axis:?} surface extent bits {extent_bits:#06x}"
            ),
        }
    }
}

impl std::error::Error for RootMenuC3dMode2MaterializationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ResourceAccess { source, .. } => Some(source),
            Self::MissingGroup1Topology { .. } | Self::UnusableSurfaceExtent { .. } => None,
        }
    }
}

impl RootMenuC3dMode2PreScaleRuntime {
    #[must_use]
    pub const fn provenance(&self) -> C3dPreparedResourceProvenance {
        self.provenance
    }

    #[must_use]
    pub const fn variant(&self) -> VisualVariant {
        self.variant
    }

    #[must_use]
    pub fn get(&self, slot: RootMenuC3dSlot) -> Option<&RootMenuC3dMode2PreScaleObject> {
        self.slots[slot.index()].as_ref()
    }
}

/// Materializes each checked group-0 -> group-1 reference independently.
pub fn materialize_root_menu_mode2_pre_scale(
    topology: &RootMenuC3dTopology<'_>,
    load_states: RootMenuC3dMode2LoadStates,
) -> Result<RootMenuC3dMode2PreScaleRuntime, RootMenuC3dMode2MaterializationError> {
    let mut slots = std::array::from_fn(|_| None);
    for (slot, object) in topology.slots() {
        let Some(object) = object else { continue };
        slots[slot.index()] = Some(materialize_object(object, load_states.get(slot))?);
    }
    Ok(RootMenuC3dMode2PreScaleRuntime {
        provenance: topology.provenance(),
        variant: topology.variant(),
        slots,
    })
}

/// Materializes mode-2 CPU geometry and applies recovered resource-derived
/// coordinate scaling before compacting selector runs.
pub fn materialize_root_menu_mode2_resource_scaled(
    topology: &RootMenuC3dTopology<'_>,
    load_states: RootMenuC3dMode2LoadStates,
) -> Result<RootMenuC3dMode2ResourceScaledRuntime, RootMenuC3dMode2MaterializationError> {
    let mut slots = std::array::from_fn(|_| None);
    for (slot, object) in topology.slots() {
        let Some(object) = object else { continue };
        slots[slot.index()] = Some(materialize_resource_scaled_object(
            topology,
            object,
            load_states,
        )?);
    }
    Ok(RootMenuC3dMode2ResourceScaledRuntime {
        provenance: topology.provenance(),
        variant: topology.variant(),
        slots,
    })
}

fn materialize_resource_scaled_object(
    topology: &RootMenuC3dTopology<'_>,
    object: &RootMenuC3dObjectTopology<'_>,
    load_states: RootMenuC3dMode2LoadStates,
) -> Result<RootMenuC3dMode2ResourceScaledObject, RootMenuC3dMode2MaterializationError> {
    let load_state = load_states.get(object.slot());
    let mut pass_through = load_state.prepared_resource_count != 0;
    let mut group0_records = Vec::new();
    for group0 in object.group0_records() {
        if group0.record().field_04_i16() == -1 {
            pass_through = true;
        }
        let transform = !pass_through;
        let group1_count = object.groups().len();
        let references = group0
            .references()
            .map(|reference| {
                let group1 = object
                    .group(usize::from(reference.group1_ordinal()))
                    .ok_or(
                        RootMenuC3dMode2MaterializationError::MissingGroup1Topology {
                            slot: object.slot(),
                            group0_record_index: group0.record_index(),
                            value_index: reference.value_index(),
                            group1_ordinal: reference.group1_ordinal(),
                            group1_count,
                        },
                    )?;
                let context = ResourceScaleContext {
                    slot: object.slot(),
                    group0_record_index: group0.record_index(),
                    value_index: reference.value_index(),
                    group1_ordinal: reference.group1_ordinal(),
                };
                let batches = materialize_resource_scaled_reference(
                    topology,
                    load_states,
                    group0.record(),
                    group1,
                    transform,
                    context,
                )?;
                Ok(RootMenuC3dMode2ResourceScaledReference {
                    value_index: reference.value_index(),
                    source_offset: reference.source_offset(),
                    group1_ordinal: reference.group1_ordinal(),
                    batches,
                })
            })
            .collect::<Result<_, _>>()?;
        group0_records.push(RootMenuC3dMode2ResourceScaledGroup0 {
            record_index: group0.record_index(),
            record_offset: group0.record_offset(),
            references,
        });
    }
    Ok(RootMenuC3dMode2ResourceScaledObject {
        slot: object.slot(),
        identity: object.identity(),
        path: object.path(),
        group0_records,
    })
}

#[derive(Clone, Copy)]
struct ResourceScaleContext {
    slot: RootMenuC3dSlot,
    group0_record_index: usize,
    value_index: usize,
    group1_ordinal: u16,
}

fn materialize_resource_scaled_reference(
    topology: &RootMenuC3dTopology<'_>,
    load_states: RootMenuC3dMode2LoadStates,
    group0: C3dGroup0Record<'_>,
    group1: &RootMenuC3dGroup1Topology<'_>,
    transform: bool,
    context: ResourceScaleContext,
) -> Result<Vec<RootMenuC3dMode2ResourceScaledBatch>, RootMenuC3dMode2MaterializationError> {
    let matrix = group0.affine_matrix_q16();
    let mut vertices = group1
        .record()
        .entries_32()
        .map(|entry| convert_entry(entry, transform.then_some(&matrix)))
        .collect::<Vec<_>>();
    let side_vectors = vertices
        .iter()
        .map(|vertex| C3dOpaqueSideVector(vertex.opaque_triplet_words()))
        .collect::<Vec<_>>();

    let load_state = load_states.get(context.slot);
    apply_vertex_mutation(
        &mut vertices,
        group1,
        C3dLoadPolicy::root_mode2(load_state).vertex_mutation_branch(),
        load_state.corner_source,
    );
    apply_resource_scaling(topology, load_states, &mut vertices, group1, context)?;

    Ok(group1
        .batches()
        .iter()
        .map(|batch| RootMenuC3dMode2ResourceScaledBatch {
            selector: batch.selector(),
            source_entry32_ordinals: batch.unique_entry32_ordinals().to_vec(),
            vertices: batch
                .unique_entry32_ordinals()
                .iter()
                .map(|ordinal| C3dMode2ResourceScaledVertex {
                    words: vertices[usize::from(*ordinal)].words,
                })
                .collect(),
            side_vectors: batch
                .unique_entry32_ordinals()
                .iter()
                .map(|ordinal| side_vectors[usize::from(*ordinal)])
                .collect(),
            indices: batch.indices().to_vec(),
        })
        .collect())
}

fn apply_vertex_mutation(
    vertices: &mut [C3dMode2PreScaleVertex],
    group1: &RootMenuC3dGroup1Topology<'_>,
    branch: C3dVertexMutationBranch,
    corner_source: C3dMode2CornerSource,
) {
    for batch in group1.batches() {
        for entry in batch.entries_24() {
            match branch {
                C3dVertexMutationBranch::Type1 => {
                    apply_type1_entry_mutation(vertices, entry.entry32_ordinals(), batch.selector())
                }
                C3dVertexMutationBranch::Type2 => {
                    let tuples = match corner_source {
                        C3dMode2CornerSource::OrdinaryDefaults => [ORDINARY_MODE2_CORNER; 3],
                        C3dMode2CornerSource::SourceEntryTuples => entry.corner_tuples(),
                    };
                    apply_type2_entry_mutation(
                        vertices,
                        entry.entry32_ordinals(),
                        tuples,
                        batch.selector(),
                    );
                }
            }
        }
    }
}

fn apply_type1_entry_mutation(
    vertices: &mut [C3dMode2PreScaleVertex],
    ordinals: [u16; 3],
    selector: C3dPreparedSelector,
) {
    if selector == C3dPreparedSelector::Missing {
        for ordinal in ordinals {
            vertices[usize::from(ordinal)].words[4] = MODE2_OPAQUE_WORD;
        }
    }
}

fn apply_type2_entry_mutation(
    vertices: &mut [C3dMode2PreScaleVertex],
    ordinals: [u16; 3],
    tuples: [[u8; 4]; 3],
    selector: C3dPreparedSelector,
) {
    for (ordinal, tuple) in ordinals.into_iter().zip(tuples) {
        let vertex = &mut vertices[usize::from(ordinal)];
        match selector {
            C3dPreparedSelector::Resource(_) => {
                vertex.words[4] = pack_mode2_tuple(tuple);
                vertex.words[5] = MODE2_OPAQUE_WORD;
            }
            C3dPreparedSelector::Missing => {
                vertex.words[4] = MODE2_OPAQUE_WORD;
                vertex.words[5] = MODE2_OPAQUE_WORD;
            }
        }
    }
}

fn apply_resource_scaling(
    topology: &RootMenuC3dTopology<'_>,
    load_states: RootMenuC3dMode2LoadStates,
    vertices: &mut [C3dMode2PreScaleVertex],
    group1: &RootMenuC3dGroup1Topology<'_>,
    context: ResourceScaleContext,
) -> Result<(), RootMenuC3dMode2MaterializationError> {
    let mut visited = vec![false; vertices.len()];
    let mut entry24_index = 0;
    for batch in group1.batches() {
        for entry in batch.entries_24() {
            let current_index = entry24_index;
            entry24_index += 1;
            let C3dPreparedSelector::Resource(resource_id) = batch.selector() else {
                continue;
            };
            let resource = topology.resource(resource_id).map_err(|source| {
                RootMenuC3dMode2MaterializationError::ResourceAccess {
                    slot: context.slot,
                    group0_record_index: context.group0_record_index,
                    value_index: context.value_index,
                    group1_ordinal: context.group1_ordinal,
                    entry24_index: current_index,
                    entry24_offset: entry.offset(),
                    resource_id,
                    source,
                }
            })?;
            let first_slot = resource
                .first_root_slot()
                .expect("root topology contains only root-owned C3D resources");
            let [source_width, source_height] = resource.source_dib_extent_low16();
            let layout = derive_surface_layout(
                source_width,
                source_height,
                load_states.get(first_slot).surface_layout,
            )
            .map_err(|issue| {
                RootMenuC3dMode2MaterializationError::UnusableSurfaceExtent {
                    slot: context.slot,
                    group0_record_index: context.group0_record_index,
                    value_index: context.value_index,
                    group1_ordinal: context.group1_ordinal,
                    entry24_index: current_index,
                    entry24_offset: entry.offset(),
                    resource_id,
                    first_slot,
                    axis: issue.axis,
                    extent_bits: issue.extent_bits,
                }
            })?;
            let ordinals = entry.entry32_ordinals();
            if layout.vertical_padding != 0 {
                for ordinal in ordinals {
                    if !visited[usize::from(ordinal)] {
                        scale_runtime_word(
                            &mut vertices[usize::from(ordinal)].words[7],
                            layout.height,
                            layout.vertical_padding,
                        );
                    }
                }
            }
            if layout.horizontal_padding != 0 {
                for ordinal in ordinals {
                    if !visited[usize::from(ordinal)] {
                        scale_runtime_word(
                            &mut vertices[usize::from(ordinal)].words[6],
                            layout.width,
                            layout.horizontal_padding,
                        );
                    }
                }
            }
            // The original marks after both three-corner loops. Repeated
            // ordinals within this entry therefore receive repeated products.
            for ordinal in ordinals {
                visited[usize::from(ordinal)] = true;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dSurfaceReduction {
    None,
    Half,
    Quarter,
}

impl C3dSurfaceReduction {
    #[must_use]
    pub const fn divisor(self) -> u16 {
        match self {
            Self::None => 1,
            Self::Half => 2,
            Self::Quarter => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedC3dSurfaceLayout {
    width: i16,
    height: i16,
    horizontal_padding: i32,
    vertical_padding: i32,
    reduction: C3dSurfaceReduction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dSurfaceLayoutError {
    axis: C3dSurfaceLayoutAxis,
    extent_bits: u16,
}

impl ResolvedC3dSurfaceLayout {
    #[must_use]
    pub const fn extent_bits(self) -> [u16; 2] {
        [self.width as u16, self.height as u16]
    }

    #[must_use]
    pub const fn horizontal_padding(self) -> i32 {
        self.horizontal_padding
    }

    #[must_use]
    pub const fn vertical_padding(self) -> i32 {
        self.vertical_padding
    }

    #[must_use]
    pub const fn reduction(self) -> C3dSurfaceReduction {
        self.reduction
    }
}

impl C3dSurfaceLayoutError {
    #[must_use]
    pub const fn axis(self) -> C3dSurfaceLayoutAxis {
        self.axis
    }

    #[must_use]
    pub const fn extent_bits(self) -> u16 {
        self.extent_bits
    }
}

impl fmt::Display for C3dSurfaceLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unusable {:?} C3D surface extent bits {:#06x}",
            self.axis, self.extent_bits
        )
    }
}

impl std::error::Error for C3dSurfaceLayoutError {}

/// Resolves the recovered legacy surface dimensions without allocating pixels.
pub fn resolve_c3d_surface_layout(
    [original_width, original_height]: [u16; 2],
    gates: C3dSurfaceLayoutGates,
) -> Result<ResolvedC3dSurfaceLayout, C3dSurfaceLayoutError> {
    derive_surface_layout(original_width, original_height, gates)
}

fn derive_surface_layout(
    original_width: u16,
    original_height: u16,
    gates: C3dSurfaceLayoutGates,
) -> Result<ResolvedC3dSurfaceLayout, C3dSurfaceLayoutError> {
    let mut width = original_width.wrapping_add(3) & !3;
    let mut height = original_height;
    let large = width >= 64 && height >= 64;
    let reduction = if (gates.quarter_if_large && large) || gates.force_quarter {
        width /= 4;
        height /= 4;
        C3dSurfaceReduction::Quarter
    } else if gates.half_if_large && large {
        width /= 2;
        height /= 2;
        C3dSurfaceReduction::Half
    } else {
        C3dSurfaceReduction::None
    };

    let mut horizontal_padding = 0;
    let mut vertical_padding = 0;
    if height > width {
        horizontal_padding = i32::from(height) - i32::from(width) + 1;
        width = height;
    }
    if gates.expand_width_dominant && width > height {
        vertical_padding = i32::from(width) - i32::from(height) + 1;
        height = width;
    }

    let signed_width = width as i16;
    if horizontal_padding != 0 && signed_width <= 0 {
        return Err(C3dSurfaceLayoutError {
            axis: C3dSurfaceLayoutAxis::Horizontal,
            extent_bits: width,
        });
    }
    let signed_height = height as i16;
    if vertical_padding != 0 && signed_height <= 0 {
        return Err(C3dSurfaceLayoutError {
            axis: C3dSurfaceLayoutAxis::Vertical,
            extent_bits: height,
        });
    }
    Ok(ResolvedC3dSurfaceLayout {
        width: signed_width,
        height: signed_height,
        horizontal_padding,
        vertical_padding,
        reduction,
    })
}

fn scale_runtime_word(word: &mut u32, extent: i16, padding: i32) {
    let extent = f64::from(extent);
    let factor = (extent - f64::from(padding)) / extent;
    let scaled = factor * f64::from(f32::from_bits(*word));
    *word = (scaled as f32).to_bits();
}

fn materialize_object(
    object: &RootMenuC3dObjectTopology<'_>,
    load_state: C3dMode2LoadState,
) -> Result<RootMenuC3dMode2PreScaleObject, RootMenuC3dMode2MaterializationError> {
    let mut pass_through = load_state.prepared_resource_count != 0;
    let mutation_branch = C3dLoadPolicy::root_mode2(load_state).vertex_mutation_branch();
    let mut group0_records = Vec::new();
    for group0 in object.group0_records() {
        if group0.record().field_04_i16() == -1 {
            pass_through = true;
        }
        group0_records.push(materialize_group0(
            object,
            group0,
            !pass_through,
            mutation_branch,
            load_state.corner_source,
        )?);
    }
    Ok(RootMenuC3dMode2PreScaleObject {
        slot: object.slot(),
        identity: object.identity(),
        path: object.path(),
        group0_records,
    })
}

fn materialize_group0(
    object: &RootMenuC3dObjectTopology<'_>,
    group0: &RootMenuC3dGroup0Topology<'_>,
    transform: bool,
    mutation_branch: C3dVertexMutationBranch,
    corner_source: C3dMode2CornerSource,
) -> Result<RootMenuC3dMode2PreScaleGroup0, RootMenuC3dMode2MaterializationError> {
    let group1_count = object.groups().len();
    let references = group0
        .references()
        .map(|reference| {
            let group1 = object
                .group(usize::from(reference.group1_ordinal()))
                .ok_or(
                    RootMenuC3dMode2MaterializationError::MissingGroup1Topology {
                        slot: object.slot(),
                        group0_record_index: group0.record_index(),
                        value_index: reference.value_index(),
                        group1_ordinal: reference.group1_ordinal(),
                        group1_count,
                    },
                )?;
            let batches = materialize_reference(
                group0.record(),
                group1,
                transform,
                mutation_branch,
                corner_source,
            );
            Ok(RootMenuC3dMode2PreScaleReference {
                value_index: reference.value_index(),
                source_offset: reference.source_offset(),
                group1_ordinal: reference.group1_ordinal(),
                batches,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(RootMenuC3dMode2PreScaleGroup0 {
        record_index: group0.record_index(),
        record_offset: group0.record_offset(),
        references,
    })
}

fn materialize_reference(
    group0: C3dGroup0Record<'_>,
    group1: &RootMenuC3dGroup1Topology<'_>,
    transform: bool,
    mutation_branch: C3dVertexMutationBranch,
    corner_source: C3dMode2CornerSource,
) -> Vec<RootMenuC3dMode2PreScaleBatch> {
    let matrix = group0.affine_matrix_q16();
    let mut vertices = group1
        .record()
        .entries_32()
        .map(|entry| convert_entry(entry, transform.then_some(&matrix)))
        .collect::<Vec<_>>();
    let side_vectors = vertices
        .iter()
        .map(|vertex| C3dOpaqueSideVector(vertex.opaque_triplet_words()))
        .collect::<Vec<_>>();

    apply_vertex_mutation(&mut vertices, group1, mutation_branch, corner_source);

    group1
        .batches()
        .iter()
        .map(|batch| RootMenuC3dMode2PreScaleBatch {
            selector: batch.selector(),
            source_entry32_ordinals: batch.unique_entry32_ordinals().to_vec(),
            vertices: batch
                .unique_entry32_ordinals()
                .iter()
                .map(|ordinal| vertices[usize::from(*ordinal)])
                .collect(),
            side_vectors: batch
                .unique_entry32_ordinals()
                .iter()
                .map(|ordinal| side_vectors[usize::from(*ordinal)])
                .collect(),
            indices: batch.indices().to_vec(),
        })
        .collect()
}

fn convert_entry(entry: C3dEntry32<'_>, matrix: Option<&[i32; 16]>) -> C3dMode2PreScaleVertex {
    let components = entry.components_q16();
    let first = matrix.map_or([components[0], components[1], components[2]], |matrix| {
        affine_q16(matrix, [components[0], components[1], components[2]])
    });
    let adjusted_24 = adjust_first_mapping_component(components[3]);
    // The original x87 sequence does not store to f32 between scaling and
    // adding one. Folding the exact Q16 one into the integer avoids a second
    // rounding while preserving that single final f32 store.
    let adjusted_28 = ((i64::from(components[4]) + 65_536) as f32) * Q16_SCALE;
    C3dMode2PreScaleVertex {
        words: [
            q16_to_f32(first[0]).to_bits(),
            q16_to_f32(first[1]).to_bits(),
            q16_to_f32(first[2]).to_bits(),
            q16_to_f32(components[5]).to_bits(),
            q16_to_f32(components[6]).to_bits(),
            q16_to_f32(components[7]).to_bits(),
            adjusted_24.to_bits(),
            adjusted_28.to_bits(),
        ],
    }
}

fn q16_to_f32(value: i32) -> f32 {
    (value as f32) * Q16_SCALE
}

fn adjust_first_mapping_component(value: i32) -> f32 {
    let mut adjusted = q16_to_f32(value);
    if adjusted > 1.0 {
        adjusted -= 1.0;
    }
    adjusted
}

fn q16_multiply_low32(left: i32, right: i32) -> i32 {
    ((i64::from(left) * i64::from(right)) >> 16) as i32
}

fn affine_q16(matrix: &[i32; 16], input: [i32; 3]) -> [i32; 3] {
    std::array::from_fn(|component| {
        matrix[12 + component]
            .wrapping_add(q16_multiply_low32(matrix[component], input[0]))
            .wrapping_add(q16_multiply_low32(matrix[4 + component], input[1]))
            .wrapping_add(q16_multiply_low32(matrix[8 + component], input[2]))
    })
}

fn pack_mode2_tuple([t0, t1, t2, t3]: [u8; 4]) -> u32 {
    u32::from_le_bytes([t2, t1, t0, t3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_eu_initial_software_default_config_maps_exact_root_slots() {
        let states = RootMenuC3dMode2LoadStates::official_eu_initial_software_default_config();
        let expected = [
            (RootMenuC3dSlot::Championship, 55),
            (RootMenuC3dSlot::Rally, 63),
            (RootMenuC3dSlot::TimeTrial, 63),
            (RootMenuC3dSlot::RallySchool, 69),
            (RootMenuC3dSlot::Options, 73),
            (RootMenuC3dSlot::Load, 79),
        ];
        let expected_layout = C3dSurfaceLayoutGates {
            quarter_if_large: false,
            force_quarter: false,
            half_if_large: false,
            expand_width_dominant: true,
        };

        for (slot, prepared_resource_count) in expected {
            assert_eq!(
                states.get(slot),
                C3dMode2LoadState {
                    prepared_resource_count,
                    corner_source: C3dMode2CornerSource::OrdinaryDefaults,
                    surface_layout: expected_layout,
                },
                "unexpected recovered state for {slot:?}",
            );
        }
    }

    #[test]
    fn layout_neutral_vertex_accessors_preserve_the_exact_eight_words() {
        let words = [
            1.25_f32.to_bits(),
            (-2.5_f32).to_bits(),
            3.75_f32.to_bits(),
            0x1234_5678,
            0x96aa_bbcc,
            0xff11_2233,
            0.25_f32.to_bits(),
            0.75_f32.to_bits(),
        ];
        let pre_scale = C3dMode2PreScaleVertex { words };
        let scaled = C3dMode2ResourceScaledVertex { words };

        for vertex in [pre_scale.words(), scaled.words()] {
            assert_eq!(vertex, words);
        }
        assert_eq!(pre_scale.first_triplet(), [1.25, -2.5, 3.75]);
        assert_eq!(scaled.first_triplet(), [1.25, -2.5, 3.75]);
        assert_eq!(pre_scale.reserved_word(), 0x1234_5678);
        assert_eq!(scaled.reserved_word(), 0x1234_5678);
        assert_eq!(
            pre_scale.opaque_triplet_words(),
            [0x1234_5678, 0x96aa_bbcc, 0xff11_2233]
        );
        assert_eq!(
            scaled.opaque_triplet_words(),
            [0x1234_5678, 0x96aa_bbcc, 0xff11_2233]
        );
        assert_eq!(
            C3dType2ResourceScaledVertex::new(scaled, C3dVertexMutationBranch::Type1),
            None
        );
        let type2 =
            C3dType2ResourceScaledVertex::new(scaled, C3dVertexMutationBranch::Type2).unwrap();
        assert_eq!(type2.diffuse(), 0x96aa_bbcc);
        assert_eq!(type2.specular(), 0xff11_2233);
        assert_eq!(pre_scale.adjusted_pair(), [0.25, 0.75]);
        assert_eq!(scaled.adjusted_pair(), [0.25, 0.75]);

        let type1 =
            C3dType1ResourceScaledVertex::new(scaled, C3dVertexMutationBranch::Type1).unwrap();
        assert_eq!(type1.position(), [1.25, -2.5, 3.75]);
        assert_eq!(type1.normal().map(f32::to_bits), words[3..6]);
        assert_eq!(type1.uv(), [0.25, 0.75]);
        assert_eq!(
            C3dType1ResourceScaledVertex::new(scaled, C3dVertexMutationBranch::Type2),
            None
        );
    }

    #[test]
    fn type1_preserves_present_words_and_missing_writes_only_word4() {
        let original = C3dMode2PreScaleVertex {
            words: [0, 0, 0, 0x1111_1111, 0x2222_2222, 0x3333_3333, 0, 0],
        };
        let mut vertices = [original];
        apply_type1_entry_mutation(
            &mut vertices,
            [0, 0, 0],
            C3dPreparedSelector::Resource(C3dPreparedResourceId::new_for_test(0)),
        );
        assert_eq!(vertices, [original]);

        apply_type1_entry_mutation(&mut vertices, [0, 0, 0], C3dPreparedSelector::Missing);
        assert_eq!(
            vertices[0].words,
            [0, 0, 0, 0x1111_1111, MODE2_OPAQUE_WORD, 0x3333_3333, 0, 0,]
        );
        apply_type1_entry_mutation(
            &mut vertices,
            [0, 0, 0],
            C3dPreparedSelector::Resource(C3dPreparedResourceId::new_for_test(0)),
        );
        assert_eq!(
            vertices[0].words,
            [0, 0, 0, 0x1111_1111, MODE2_OPAQUE_WORD, 0x3333_3333, 0, 0,]
        );
    }

    #[test]
    fn type2_retains_present_missing_and_fixed_uniform_mutation() {
        let original = C3dMode2PreScaleVertex {
            words: [0, 0, 0, 0x1111_1111, 0x2222_2222, 0x3333_3333, 0, 0],
        };
        let mut vertices = [original, original];
        apply_type2_entry_mutation(
            &mut vertices,
            [0, 0, 0],
            [[0x96; 4]; 3],
            C3dPreparedSelector::Resource(C3dPreparedResourceId::new_for_test(0)),
        );
        assert_eq!(vertices[0].words[4], 0x9696_9696);
        assert_eq!(vertices[0].words[5], MODE2_OPAQUE_WORD);

        apply_type2_entry_mutation(
            &mut vertices,
            [1, 1, 1],
            [[1, 2, 3, 4]; 3],
            C3dPreparedSelector::Missing,
        );
        assert_eq!(vertices[1].words[4], MODE2_OPAQUE_WORD);
        assert_eq!(vertices[1].words[5], MODE2_OPAQUE_WORD);
    }

    #[test]
    fn q16_multiply_uses_signed_shift_and_retains_low_32_bits() {
        assert_eq!(q16_multiply_low32(-1, 1), -1);
        assert_eq!(q16_multiply_low32(i32::MAX, i32::MAX), -65_536);
        assert_eq!(q16_multiply_low32(i32::MIN, i32::MIN), 0);
    }

    #[test]
    fn affine_uses_recovered_columns_translation_and_wrapping_sums() {
        let mut matrix = [0; 16];
        matrix[0] = 1 << 16;
        matrix[5] = 2 << 16;
        matrix[10] = 3 << 16;
        matrix[12] = 10 << 16;
        matrix[13] = 20 << 16;
        matrix[14] = 30 << 16;
        assert_eq!(
            affine_q16(&matrix, [4 << 16, 5 << 16, 6 << 16]),
            [14 << 16, 30 << 16, 48 << 16]
        );

        matrix[12] = i32::MAX;
        matrix[0] = 1 << 16;
        assert_eq!(affine_q16(&matrix, [1, 0, 0])[0], i32::MIN);
    }

    #[test]
    fn q16_multiply_matches_independent_modulo_reference() {
        let mut state = 0x4d59_5df4_d0f3_3173_u64;
        for _ in 0..10_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let left = (state >> 32) as i32;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let right = (state >> 32) as i32;
            let shifted = (i128::from(left) * i128::from(right)) >> 16;
            let expected = (shifted as u128 & u128::from(u32::MAX)) as u32 as i32;
            assert_eq!(q16_multiply_low32(left, right), expected);
        }
    }

    #[test]
    fn mode2_tuple_pack_preserves_the_recovered_byte_order() {
        assert_eq!(pack_mode2_tuple([0xaa, 0xbb, 0xcc, 0xdd]), 0xddaa_bbcc);
        assert_eq!(pack_mode2_tuple(ORDINARY_MODE2_CORNER), 0x9600_0000);
    }

    #[test]
    fn second_adjusted_component_is_single_rounded() {
        let source = 67_108_565_i32;
        let single = ((i64::from(source) + 65_536) as f32) * Q16_SCALE;
        let double = q16_to_f32(source) + 1.0;
        assert_eq!(single.to_bits(), 0x4480_1fdb);
        assert_eq!(double.to_bits(), 0x4480_1fda);
    }

    #[test]
    fn q16_f32_conversion_pins_signed_boundaries() {
        assert_eq!(q16_to_f32(0).to_bits(), 0);
        assert_eq!(q16_to_f32(65_536).to_bits(), 1.0f32.to_bits());
        assert_eq!(q16_to_f32(-65_536).to_bits(), (-1.0f32).to_bits());
        assert_eq!(q16_to_f32(i32::MAX).to_bits(), 32_768.0f32.to_bits());
        assert_eq!(q16_to_f32(i32::MIN).to_bits(), (-32_768.0f32).to_bits());
    }

    #[test]
    fn first_mapping_component_subtracts_one_once_only_when_strictly_above_one() {
        assert_eq!(
            adjust_first_mapping_component(0).to_bits(),
            0.0f32.to_bits()
        );
        assert_eq!(
            adjust_first_mapping_component(-65_536).to_bits(),
            (-1.0f32).to_bits()
        );
        assert_eq!(
            adjust_first_mapping_component(65_536).to_bits(),
            1.0f32.to_bits()
        );
        assert_eq!(
            adjust_first_mapping_component(65_537).to_bits(),
            Q16_SCALE.to_bits()
        );
        assert_eq!(
            adjust_first_mapping_component(3 * 65_536).to_bits(),
            2.0f32.to_bits()
        );
    }

    #[test]
    fn surface_layout_aligns_scales_and_expands_in_recovered_order() {
        assert_eq!(
            derive_surface_layout(5, 3, C3dSurfaceLayoutGates::default()),
            Ok(ResolvedC3dSurfaceLayout {
                width: 8,
                height: 3,
                horizontal_padding: 0,
                vertical_padding: 0,
                reduction: C3dSurfaceReduction::None,
            })
        );
        assert_eq!(
            derive_surface_layout(5, 10, C3dSurfaceLayoutGates::default()),
            Ok(ResolvedC3dSurfaceLayout {
                width: 10,
                height: 10,
                horizontal_padding: 3,
                vertical_padding: 0,
                reduction: C3dSurfaceReduction::None,
            })
        );
        assert_eq!(
            derive_surface_layout(
                5,
                3,
                C3dSurfaceLayoutGates {
                    expand_width_dominant: true,
                    ..C3dSurfaceLayoutGates::default()
                },
            ),
            Ok(ResolvedC3dSurfaceLayout {
                width: 8,
                height: 8,
                horizontal_padding: 0,
                vertical_padding: 6,
                reduction: C3dSurfaceReduction::None,
            })
        );
    }

    #[test]
    fn quarter_precedes_half_and_force_quarter_ignores_large_gate() {
        let both = C3dSurfaceLayoutGates {
            quarter_if_large: true,
            half_if_large: true,
            ..C3dSurfaceLayoutGates::default()
        };
        assert_eq!(
            derive_surface_layout(128, 64, both),
            Ok(ResolvedC3dSurfaceLayout {
                width: 32,
                height: 16,
                horizontal_padding: 0,
                vertical_padding: 0,
                reduction: C3dSurfaceReduction::Quarter,
            })
        );
        assert_eq!(
            resolve_c3d_surface_layout([61, 64], both)
                .unwrap()
                .reduction(),
            C3dSurfaceReduction::Quarter
        );
        assert_eq!(
            resolve_c3d_surface_layout([60, 64], both)
                .unwrap()
                .reduction(),
            C3dSurfaceReduction::None
        );
        assert_eq!(
            derive_surface_layout(
                128,
                64,
                C3dSurfaceLayoutGates {
                    half_if_large: true,
                    ..C3dSurfaceLayoutGates::default()
                },
            ),
            Ok(ResolvedC3dSurfaceLayout {
                width: 64,
                height: 32,
                horizontal_padding: 0,
                vertical_padding: 0,
                reduction: C3dSurfaceReduction::Half,
            })
        );
        assert_eq!(
            derive_surface_layout(
                8,
                8,
                C3dSurfaceLayoutGates {
                    force_quarter: true,
                    ..C3dSurfaceLayoutGates::default()
                },
            ),
            Ok(ResolvedC3dSurfaceLayout {
                width: 2,
                height: 2,
                horizontal_padding: 0,
                vertical_padding: 0,
                reduction: C3dSurfaceReduction::Quarter,
            })
        );
    }

    #[test]
    fn surface_layout_preserves_wrapping_and_rejects_only_used_negative_extents() {
        assert_eq!(
            derive_surface_layout(65_535, 1, C3dSurfaceLayoutGates::default()),
            Ok(ResolvedC3dSurfaceLayout {
                width: 1,
                height: 1,
                horizontal_padding: 2,
                vertical_padding: 0,
                reduction: C3dSurfaceReduction::None,
            })
        );
        assert_eq!(
            derive_surface_layout(65_535, 0, C3dSurfaceLayoutGates::default()),
            Ok(ResolvedC3dSurfaceLayout {
                width: 0,
                height: 0,
                horizontal_padding: 0,
                vertical_padding: 0,
                reduction: C3dSurfaceReduction::None,
            })
        );
        assert_eq!(
            derive_surface_layout(
                1,
                1,
                C3dSurfaceLayoutGates {
                    force_quarter: true,
                    ..C3dSurfaceLayoutGates::default()
                },
            ),
            Ok(ResolvedC3dSurfaceLayout {
                width: 1,
                height: 0,
                horizontal_padding: 0,
                vertical_padding: 0,
                reduction: C3dSurfaceReduction::Quarter,
            })
        );
        assert_eq!(
            derive_surface_layout(
                40_000,
                1,
                C3dSurfaceLayoutGates {
                    expand_width_dominant: true,
                    ..C3dSurfaceLayoutGates::default()
                },
            ),
            Err(C3dSurfaceLayoutError {
                axis: C3dSurfaceLayoutAxis::Vertical,
                extent_bits: 40_000,
            })
        );
    }

    #[test]
    fn resource_scaling_uses_f64_intermediate_and_one_f32_store() {
        let mut word = 1.25f32.to_bits();
        scale_runtime_word(&mut word, 10, 3);
        assert_eq!(word, 0.875f32.to_bits());

        let mut discriminator = 0x03a6_fc54;
        scale_runtime_word(&mut discriminator, 3, 2);
        assert_eq!(discriminator, 0x02de_a5c5);
        assert_eq!(
            ((1.0f32 / 3.0) * f32::from_bits(0x03a6_fc54)).to_bits(),
            0x02de_a5c6
        );
    }
}
