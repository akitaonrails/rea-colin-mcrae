//! Backend-neutral C3D batch planning and owned draw commands.
//!
//! Resource preparation is deliberately outside this module. Callers provide
//! one already-resolved selector per 24-byte entry, in source order. This
//! planner only preserves consecutive runs, resolves the three local 32-byte
//! entry ordinals, and compacts them in first-use order. The result still
//! contains borrowed source-format views. Later materialization produces the
//! owned draw command defined here without attaching a backend binding or
//! inventing unresolved raster behavior.

use std::{collections::HashMap, fmt};

use cmr_formats::c3d::{
    C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS, C3dEntry24, C3dEntry32, C3dGroup1Record,
};

use crate::{
    root_menu_c3d_resources::{C3dPreparedSelector, C3dVertexMutationBranch},
    root_menu_c3d_runtime::{C3dMode2ResourceScaledVertex, C3dOpaqueSideVector},
};

pub const C3D_PRIMITIVE_CODE: u32 = 4;
pub const C3D_DRAW_FLAGS: u32 = 8;

/// Source location of one compact, consecutive C3D draw batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dDrawOrdinal {
    group0_record_index: usize,
    value_index: usize,
    group1_ordinal: u16,
    batch_index: usize,
}

impl C3dDrawOrdinal {
    #[must_use]
    pub(crate) const fn new(
        group0_record_index: usize,
        value_index: usize,
        group1_ordinal: u16,
        batch_index: usize,
    ) -> Self {
        Self {
            group0_record_index,
            value_index,
            group1_ordinal,
            batch_index,
        }
    }

    #[must_use]
    pub const fn group0_record_index(self) -> usize {
        self.group0_record_index
    }

    #[must_use]
    pub const fn value_index(self) -> usize {
        self.value_index
    }

    #[must_use]
    pub const fn group1_ordinal(self) -> u16 {
        self.group1_ordinal
    }

    #[must_use]
    pub const fn batch_index(self) -> usize {
        self.batch_index
    }
}

/// Backend-neutral payload for one recovered C3D draw submission.
///
/// A missing selector is retained exactly. Resolving it to the recovered
/// default material/texture-zero operation belongs to a later backend
/// contract; this command never substitutes an arena resource for it.
#[derive(Debug, Eq, PartialEq)]
pub struct C3dDrawCommand {
    ordinal: C3dDrawOrdinal,
    selector: C3dPreparedSelector,
    vertex_mutation_branch: C3dVertexMutationBranch,
    source_entry32_ordinals: Box<[u16]>,
    vertices: Box<[C3dMode2ResourceScaledVertex]>,
    side_vectors: Box<[C3dOpaqueSideVector]>,
    indices: Box<[u16]>,
}

impl C3dDrawCommand {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub(crate) fn new(
        ordinal: C3dDrawOrdinal,
        selector: C3dPreparedSelector,
        vertex_mutation_branch: C3dVertexMutationBranch,
        source_entry32_ordinals: impl Into<Box<[u16]>>,
        vertices: impl Into<Box<[C3dMode2ResourceScaledVertex]>>,
        side_vectors: impl Into<Box<[C3dOpaqueSideVector]>>,
        indices: impl Into<Box<[u16]>>,
    ) -> Self {
        Self {
            ordinal,
            selector,
            vertex_mutation_branch,
            source_entry32_ordinals: source_entry32_ordinals.into(),
            vertices: vertices.into(),
            side_vectors: side_vectors.into(),
            indices: indices.into(),
        }
    }

    #[must_use]
    pub const fn ordinal(&self) -> C3dDrawOrdinal {
        self.ordinal
    }

    #[must_use]
    pub const fn group0_record_index(&self) -> usize {
        self.ordinal.group0_record_index()
    }

    #[must_use]
    pub const fn value_index(&self) -> usize {
        self.ordinal.value_index()
    }

    #[must_use]
    pub const fn group1_ordinal(&self) -> u16 {
        self.ordinal.group1_ordinal()
    }

    #[must_use]
    pub const fn batch_index(&self) -> usize {
        self.ordinal.batch_index()
    }

    #[must_use]
    pub const fn selector(&self) -> C3dPreparedSelector {
        self.selector
    }

    #[must_use]
    pub const fn vertex_mutation_branch(&self) -> C3dVertexMutationBranch {
        self.vertex_mutation_branch
    }

    #[must_use]
    pub const fn vertex_type(&self) -> u32 {
        self.vertex_mutation_branch.submitted_vertex_type()
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
    pub const fn primitive_code(&self) -> u32 {
        C3D_PRIMITIVE_CODE
    }

    #[must_use]
    pub const fn draw_flags(&self) -> u32 {
        C3D_DRAW_FLAGS
    }
}

/// One consecutive run of 24-byte entries with the same resolved selector.
///
/// `indices` contains three compact indices per `entries_24` element. Each
/// index addresses the parallel `unique_entries_32` and
/// `unique_entry32_ordinals` arrays.
#[derive(Debug, Eq, PartialEq)]
pub struct C3dConsecutiveBatch<'a, S> {
    selector: S,
    entries_24: Vec<C3dEntry24<'a>>,
    unique_entry32_ordinals: Vec<u16>,
    unique_entries_32: Vec<C3dEntry32<'a>>,
    indices: Vec<u16>,
}

impl<'a, S: Copy> C3dConsecutiveBatch<'a, S> {
    #[must_use]
    pub const fn selector(&self) -> S {
        self.selector
    }

    #[must_use]
    pub fn entries_24(&self) -> &[C3dEntry24<'a>] {
        &self.entries_24
    }

    #[must_use]
    pub fn unique_entry32_ordinals(&self) -> &[u16] {
        &self.unique_entry32_ordinals
    }

    #[must_use]
    pub fn unique_entries_32(&self) -> &[C3dEntry32<'a>] {
        &self.unique_entries_32
    }

    #[must_use]
    pub fn indices(&self) -> &[u16] {
        &self.indices
    }
}

/// Failure to build a checked topology plan from one group-1 record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dBatchPlanError {
    SelectorCountMismatch {
        entry24_count: usize,
        selector_count: usize,
    },
    Entry32OrdinalOutOfBounds {
        record_offset: usize,
        entry24_index: usize,
        corner_index: usize,
        source_offset: usize,
        ordinal: u16,
        entry32_count: usize,
    },
}

impl fmt::Display for C3dBatchPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelectorCountMismatch {
                entry24_count,
                selector_count,
            } => write!(
                formatter,
                "C3D group-1 record has {entry24_count} 24-byte entries but {selector_count} resolved resource selectors were supplied"
            ),
            Self::Entry32OrdinalOutOfBounds {
                record_offset,
                entry24_index,
                corner_index,
                source_offset,
                ordinal,
                entry32_count,
            } => write!(
                formatter,
                "C3D group-1 record at offset {record_offset} 24-byte entry {entry24_index} corner {corner_index} at offset {source_offset} has 32-byte-entry ordinal {ordinal}, outside {entry32_count} entries"
            ),
        }
    }
}

impl std::error::Error for C3dBatchPlanError {}

/// Plans consecutive selector runs and their compact local entry indices.
///
/// Selectors must already reflect resource preparation, including any fallback
/// for the source format's `-1` group-2 sentinel. Equal selectors separated by
/// another selector produce separate batches. Within a batch, entry ordinals
/// are deduplicated by ordinal identity, never by payload equality.
pub fn plan_consecutive_batches<'a, S: Copy + Eq>(
    record: C3dGroup1Record<'a>,
    selectors: &[S],
) -> Result<Vec<C3dConsecutiveBatch<'a, S>>, C3dBatchPlanError> {
    let entries_24 = record.entries_24();
    let entry24_count = entries_24.len();
    if selectors.len() != entry24_count {
        return Err(C3dBatchPlanError::SelectorCountMismatch {
            entry24_count,
            selector_count: selectors.len(),
        });
    }

    let entries_32 = record.entries_32().collect::<Vec<_>>();
    let mut batches = Vec::<C3dConsecutiveBatch<'a, S>>::new();
    let mut ordinal_to_compact_index = HashMap::<u16, u16>::new();

    for (entry24_index, (entry, selector)) in entries_24.zip(selectors.iter().copied()).enumerate()
    {
        if batches
            .last()
            .is_none_or(|batch| batch.selector != selector)
        {
            batches.push(C3dConsecutiveBatch {
                selector,
                entries_24: Vec::new(),
                unique_entry32_ordinals: Vec::new(),
                unique_entries_32: Vec::new(),
                indices: Vec::new(),
            });
            ordinal_to_compact_index.clear();
        }

        let batch = batches.last_mut().expect("a batch was just ensured");
        batch.entries_24.push(entry);

        for (corner_index, ordinal) in entry.entry32_ordinals().into_iter().enumerate() {
            let compact_index = if let Some(index) = ordinal_to_compact_index.get(&ordinal) {
                *index
            } else {
                let source = entries_32.get(usize::from(ordinal)).copied().ok_or(
                    C3dBatchPlanError::Entry32OrdinalOutOfBounds {
                        record_offset: record.offset(),
                        entry24_index,
                        corner_index,
                        source_offset: entry.offset()
                            + C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[corner_index],
                        ordinal,
                        entry32_count: entries_32.len(),
                    },
                )?;
                // The source entry count is an admitted u16 field, so there
                // can be at most 65,535 distinct valid ordinals (indices
                // 0..=65,534) in one batch.
                let index = u16::try_from(batch.unique_entries_32.len())
                    .expect("valid C3D entry count keeps compact indices in u16 range");
                ordinal_to_compact_index.insert(ordinal, index);
                batch.unique_entry32_ordinals.push(ordinal);
                batch.unique_entries_32.push(source);
                index
            };
            batch.indices.push(compact_index);
        }
    }

    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_formats::c3d::{
        C3D_FORMAT_VERSION, C3D_GROUP1_ENTRY24_LEN, C3D_GROUP1_ENTRY32_LEN, C3D_GROUP1_FIXED_LEN,
        C3D_HEADER_LEN, C3dFile, C3dLimits, parse_c3d,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Selector {
        A,
        B,
    }

    #[test]
    fn draw_command_preserves_neutral_ordinal_branch_and_missing_selector() {
        let ordinal = C3dDrawOrdinal::new(3, 4, 5, 6);
        let command = C3dDrawCommand::new(
            ordinal,
            C3dPreparedSelector::Missing,
            C3dVertexMutationBranch::Type2,
            Box::<[u16]>::default(),
            Box::<[C3dMode2ResourceScaledVertex]>::default(),
            Box::<[C3dOpaqueSideVector]>::default(),
            Box::<[u16]>::default(),
        );

        assert_eq!(command.ordinal(), ordinal);
        assert_eq!(command.group0_record_index(), 3);
        assert_eq!(command.value_index(), 4);
        assert_eq!(command.group1_ordinal(), 5);
        assert_eq!(command.batch_index(), 6);
        assert_eq!(command.selector(), C3dPreparedSelector::Missing);
        assert_eq!(
            command.vertex_mutation_branch(),
            C3dVertexMutationBranch::Type2
        );
        assert_eq!(command.vertex_type(), 2);
        assert_eq!(command.primitive_code(), 4);
        assert_eq!(command.draw_flags(), 8);
        assert!(command.source_entry32_ordinals().is_empty());
        assert!(command.vertices().is_empty());
        assert!(command.side_vectors().is_empty());
        assert!(command.indices().is_empty());
    }

    #[test]
    fn preserves_consecutive_runs_and_first_use_ordinal_order() {
        let file = synthetic_file(
            &entry_bytes(3),
            &[[2, 0, 2], [0, 1, 2], [1, 1, 1], [2, 2, 2]],
        );
        let record = file.group1_records().next().unwrap();

        let batches = plan_consecutive_batches(
            record,
            &[Selector::A, Selector::A, Selector::B, Selector::A],
        )
        .unwrap();

        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].selector(), Selector::A);
        assert_eq!(batches[0].entries_24().len(), 2);
        assert_eq!(batches[0].unique_entry32_ordinals(), &[2, 0, 1]);
        assert_eq!(batches[0].indices(), &[0, 1, 0, 1, 2, 0]);
        assert_eq!(batches[1].selector(), Selector::B);
        assert_eq!(batches[1].unique_entry32_ordinals(), &[1]);
        assert_eq!(batches[1].indices(), &[0, 0, 0]);
        assert_eq!(batches[2].selector(), Selector::A);
        assert_eq!(batches[2].unique_entry32_ordinals(), &[2]);
        assert_eq!(batches[2].indices(), &[0, 0, 0]);
    }

    #[test]
    fn deduplicates_ordinals_not_equal_payload_bytes() {
        let identical = [[0x5a; C3D_GROUP1_ENTRY32_LEN]; 2];
        let file = synthetic_file(&identical, &[[0, 1, 0]]);
        let record = file.group1_records().next().unwrap();

        let batches = plan_consecutive_batches(record, &[Selector::A]).unwrap();
        let batch = &batches[0];

        assert_eq!(batch.unique_entry32_ordinals(), &[0, 1]);
        assert_eq!(batch.indices(), &[0, 1, 0]);
        assert_eq!(batch.unique_entries_32()[0].bytes(), &[0x5a; 32]);
        assert_eq!(
            batch.unique_entries_32()[0].bytes(),
            batch.unique_entries_32()[1].bytes()
        );
        assert_ne!(
            batch.unique_entries_32()[0].offset(),
            batch.unique_entries_32()[1].offset()
        );
    }

    #[test]
    fn rejects_short_and_long_selector_lists_before_planning() {
        let file = synthetic_file(&entry_bytes(1), &[[0, 0, 0]]);
        let record = file.group1_records().next().unwrap();

        assert_eq!(
            plan_consecutive_batches::<Selector>(record, &[]),
            Err(C3dBatchPlanError::SelectorCountMismatch {
                entry24_count: 1,
                selector_count: 0,
            })
        );
        assert_eq!(
            plan_consecutive_batches(record, &[Selector::A, Selector::B]),
            Err(C3dBatchPlanError::SelectorCountMismatch {
                entry24_count: 1,
                selector_count: 2,
            })
        );
    }

    #[test]
    fn reports_each_bad_corner_with_its_absolute_source_offset() {
        for corner_index in 0..3 {
            let mut ordinals = [0, 0, 0];
            ordinals[corner_index] = 1;
            let file = synthetic_file(&entry_bytes(1), &[ordinals]);
            let record = file.group1_records().next().unwrap();
            let entry = record.entries_24().next().unwrap();

            assert_eq!(
                plan_consecutive_batches(record, &[Selector::A]),
                Err(C3dBatchPlanError::Entry32OrdinalOutOfBounds {
                    record_offset: record.offset(),
                    entry24_index: 0,
                    corner_index,
                    source_offset: entry.offset()
                        + C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[corner_index],
                    ordinal: 1,
                    entry32_count: 1,
                })
            );
        }
    }

    #[test]
    fn empty_record_produces_no_batches() {
        let file = synthetic_file(&[], &[]);
        let record = file.group1_records().next().unwrap();

        assert!(
            plan_consecutive_batches::<Selector>(record, &[])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn planned_views_borrow_the_original_typed_entries() {
        let entries = entry_bytes(2);
        let file = synthetic_file(&entries, &[[1, 0, 1]]);
        let record = file.group1_records().next().unwrap();
        let source_entries = record.entries_32().collect::<Vec<_>>();
        let source_entry24 = record.entries_24().next().unwrap();

        let batches = plan_consecutive_batches(record, &[Selector::A]).unwrap();
        let batch = &batches[0];

        assert_eq!(batch.entries_24(), &[source_entry24]);
        assert_eq!(
            batch.unique_entries_32(),
            &[source_entries[1], source_entries[0]]
        );
        assert!(std::ptr::eq(
            batch.unique_entries_32()[0].bytes(),
            source_entries[1].bytes()
        ));
    }

    fn entry_bytes(count: usize) -> Vec<[u8; C3D_GROUP1_ENTRY32_LEN]> {
        (0..count)
            .map(|index| [u8::try_from(index + 1).unwrap(); C3D_GROUP1_ENTRY32_LEN])
            .collect()
    }

    fn synthetic_file(
        entries_32: &[[u8; C3D_GROUP1_ENTRY32_LEN]],
        ordinals: &[[u16; 3]],
    ) -> C3dFile {
        let record_len = C3D_GROUP1_FIXED_LEN
            + entries_32.len() * C3D_GROUP1_ENTRY32_LEN
            + ordinals.len() * C3D_GROUP1_ENTRY24_LEN;
        let payload_len = C3D_HEADER_LEN + record_len;
        let mut input = vec![0; payload_len];
        input[0..4].copy_from_slice(b"CM3D");
        input[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        input[8..12].copy_from_slice(&(C3D_HEADER_LEN as u32).to_le_bytes());
        input[14..16].copy_from_slice(&1_u16.to_le_bytes());
        input[20..24].copy_from_slice(&(C3D_HEADER_LEN as u32).to_le_bytes());
        input[24..28].copy_from_slice(&(C3D_HEADER_LEN as u32).to_le_bytes());
        input[28..32].copy_from_slice(&(payload_len as u32).to_le_bytes());

        let record_offset = C3D_HEADER_LEN;
        input[record_offset + 12..record_offset + 16]
            .copy_from_slice(&(record_len as u32).to_le_bytes());
        input[record_offset + 16..record_offset + 18]
            .copy_from_slice(&u16::try_from(entries_32.len()).unwrap().to_le_bytes());
        input[record_offset + 18..record_offset + 20]
            .copy_from_slice(&u16::try_from(ordinals.len()).unwrap().to_le_bytes());

        let entries32_offset = record_offset + C3D_GROUP1_FIXED_LEN;
        for (index, entry) in entries_32.iter().enumerate() {
            let start = entries32_offset + index * C3D_GROUP1_ENTRY32_LEN;
            input[start..start + C3D_GROUP1_ENTRY32_LEN].copy_from_slice(entry);
        }

        let entries24_offset = entries32_offset + entries_32.len() * C3D_GROUP1_ENTRY32_LEN;
        for (entry_index, entry_ordinals) in ordinals.iter().enumerate() {
            let start = entries24_offset + entry_index * C3D_GROUP1_ENTRY24_LEN;
            input[start] = u8::try_from(entry_index).unwrap();
            for (corner_index, ordinal) in entry_ordinals.iter().enumerate() {
                let field = start + C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[corner_index];
                input[field..field + 2].copy_from_slice(&ordinal.to_le_bytes());
            }
        }

        parse_c3d(&input, C3dLimits::default()).unwrap()
    }
}
