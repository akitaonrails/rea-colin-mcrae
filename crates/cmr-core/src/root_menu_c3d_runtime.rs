//! Pre-resource-scaling runtime vertices for root-menu mode-2 C3D objects.
//!
//! The original loader converts one independent vertex array for every
//! group-0 value that references a group-1 record. This module preserves that
//! ownership, captures the opaque three-word side vector before mode-specific
//! mutation, and only then compacts vertices through the checked batch plan.
//! Resource-driven mapping-coordinate scaling remains a later boundary.

use std::fmt;

use cmr_formats::c3d::{C3dEntry32, C3dGroup0Record};

use crate::{
    root_menu_assets::{RootMenuObjectIdentity, VisualVariant},
    root_menu_c3d_assets::{ROOT_MENU_C3D_REQUEST_COUNT, RootMenuC3dSlot},
    root_menu_c3d_resources::C3dPreparedSelector,
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

/// State supplied independently to one root-menu C3D loader call.
///
/// `prepared_resource_count` is the count sampled at that loader entry. It is
/// deliberately not derived from the reconstructed preparation arena.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dMode2LoadState {
    pub prepared_resource_count: u32,
    pub corner_source: C3dMode2CornerSource,
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

    #[must_use]
    pub const fn get(self, slot: RootMenuC3dSlot) -> C3dMode2LoadState {
        self.slots[slot.index()]
    }
}

/// Exact eight-word runtime vertex before unrecovered resource-driven scaling.
///
/// Words 0..=2 and 6..=7 contain f32 bits. Mode 2 leaves word 3 as f32 bits
/// but replaces words 4 and 5 with packed opaque values.
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
    variant: VisualVariant,
    slots: [Option<RootMenuC3dMode2PreScaleObject>; ROOT_MENU_C3D_REQUEST_COUNT],
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
        }
    }
}

impl std::error::Error for RootMenuC3dMode2MaterializationError {}

impl RootMenuC3dMode2PreScaleRuntime {
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
        variant: topology.variant(),
        slots,
    })
}

fn materialize_object(
    object: &RootMenuC3dObjectTopology<'_>,
    load_state: C3dMode2LoadState,
) -> Result<RootMenuC3dMode2PreScaleObject, RootMenuC3dMode2MaterializationError> {
    let mut pass_through = load_state.prepared_resource_count != 0;
    let mut group0_records = Vec::new();
    for group0 in object.group0_records() {
        if group0.record().field_04_i16() == -1 {
            pass_through = true;
        }
        group0_records.push(materialize_group0(
            object,
            group0,
            !pass_through,
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
            let batches = materialize_reference(group0.record(), group1, transform, corner_source);
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

    for batch in group1.batches() {
        for entry in batch.entries_24() {
            let tuples = match corner_source {
                C3dMode2CornerSource::OrdinaryDefaults => [ORDINARY_MODE2_CORNER; 3],
                C3dMode2CornerSource::SourceEntryTuples => entry.corner_tuples(),
            };
            for (ordinal, tuple) in entry.entry32_ordinals().into_iter().zip(tuples) {
                let vertex = &mut vertices[usize::from(ordinal)];
                match batch.selector() {
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
    }

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
}
