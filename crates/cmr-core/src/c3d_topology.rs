//! Domain-neutral checked topology for one prepared C3D object.
//!
//! Resource preparation owns decoded bitmap data. This layer joins the
//! resulting selector stream back to the exact parsed geometry records without
//! assigning frontend-screen meaning to either side.

use std::fmt;

use cmr_formats::c3d::{C3D_GROUP0_VALUES_OFFSET, C3dFile, C3dGroup0Record, C3dGroup1Record};
use sha2::{Digest, Sha256};

use crate::{
    c3d_batch::{C3dBatchPlanError, C3dConsecutiveBatch, plan_consecutive_batches},
    root_menu_c3d_resources::{
        C3dLoadPolicy, C3dPreparedGroup1Selectors, C3dPreparedResourceProvenance,
        C3dPreparedSelector, C3dSourceIdentity,
    },
};

/// Minimal prepared-object view required to join selectors to source geometry.
pub(crate) trait C3dPreparedObjectView {
    fn source(&self) -> &C3dSourceIdentity;
    fn source_sha256(&self) -> [u8; 32];
    fn policy(&self) -> C3dLoadPolicy;
    fn group1_selector_groups(&self) -> &[C3dPreparedGroup1Selectors];
}

#[derive(Debug, Eq, PartialEq)]
pub struct C3dGroup1Topology<'asset> {
    record_index: usize,
    record: C3dGroup1Record<'asset>,
    batches: Vec<C3dConsecutiveBatch<'asset, C3dPreparedSelector>>,
}

impl<'asset> C3dGroup1Topology<'asset> {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dGroup1TopologyReference {
    value_index: usize,
    source_offset: usize,
    group1_ordinal: u16,
}

impl C3dGroup1TopologyReference {
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

#[derive(Debug, Eq, PartialEq)]
pub struct C3dGroup0Topology<'asset> {
    record_index: usize,
    record: C3dGroup0Record<'asset>,
    references: Vec<C3dGroup1TopologyReference>,
}

impl<'asset> C3dGroup0Topology<'asset> {
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
    pub fn references(&self) -> impl ExactSizeIterator<Item = C3dGroup1TopologyReference> + '_ {
        self.references.iter().copied()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct C3dObjectTopology<'asset> {
    provenance: C3dPreparedResourceProvenance,
    source: C3dSourceIdentity,
    source_sha256: [u8; 32],
    policy: C3dLoadPolicy,
    group0_records: Vec<C3dGroup0Topology<'asset>>,
    groups: Vec<C3dGroup1Topology<'asset>>,
}

impl<'asset> C3dObjectTopology<'asset> {
    #[must_use]
    pub const fn provenance(&self) -> C3dPreparedResourceProvenance {
        self.provenance
    }
    #[must_use]
    pub const fn source(&self) -> &C3dSourceIdentity {
        &self.source
    }
    #[must_use]
    pub const fn source_sha256(&self) -> [u8; 32] {
        self.source_sha256
    }
    #[must_use]
    /// Policy captured during resource preparation, not implicit live runtime state.
    pub const fn policy(&self) -> C3dLoadPolicy {
        self.policy
    }
    pub fn group0_records(&self) -> impl ExactSizeIterator<Item = &C3dGroup0Topology<'asset>> {
        self.group0_records.iter()
    }
    #[must_use]
    pub fn group0_record(&self, index: usize) -> Option<&C3dGroup0Topology<'asset>> {
        self.group0_records.get(index)
    }
    pub fn groups(&self) -> impl ExactSizeIterator<Item = &C3dGroup1Topology<'asset>> {
        self.groups.iter()
    }
    #[must_use]
    pub fn group(&self, index: usize) -> Option<&C3dGroup1Topology<'asset>> {
        self.groups.get(index)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum C3dObjectTopologyError {
    SourceMismatch {
        prepared_sha256: [u8; 32],
        supplied_sha256: [u8; 32],
    },
    Group1Correspondence {
        source_count: usize,
        selector_count: usize,
    },
    Group1Metadata {
        expected_record_index: usize,
        selector_record_index: usize,
        source_record_offset: usize,
        selector_record_offset: usize,
    },
    ForeignSelector {
        record_index: usize,
        entry24_index: usize,
        resource_id: crate::root_menu_c3d_resources::C3dPreparedResourceId,
    },
    Group1ReferenceOutOfBounds {
        group0_record_index: usize,
        group0_record_offset: usize,
        value_index: usize,
        source_offset: usize,
        ordinal: u16,
        group1_count: usize,
    },
    Batch {
        record_index: usize,
        record_offset: usize,
        source: C3dBatchPlanError,
    },
}

impl fmt::Display for C3dObjectTopologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot build C3D object topology: {self:?}")
    }
}

impl std::error::Error for C3dObjectTopologyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Batch { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Joins one parsed C3D file to selectors prepared from that same file.
pub(crate) fn build_c3d_object_topology<'asset>(
    file: &'asset C3dFile,
    prepared: &impl C3dPreparedObjectView,
    provenance: C3dPreparedResourceProvenance,
) -> Result<C3dObjectTopology<'asset>, C3dObjectTopologyError> {
    let supplied_sha256 = Sha256::digest(file.bytes()).into();
    if supplied_sha256 != prepared.source_sha256() {
        return Err(C3dObjectTopologyError::SourceMismatch {
            prepared_sha256: prepared.source_sha256(),
            supplied_sha256,
        });
    }
    let source_records = file.group1_records().collect::<Vec<_>>();
    let selector_groups = prepared.group1_selector_groups();
    if source_records.len() != selector_groups.len() {
        return Err(C3dObjectTopologyError::Group1Correspondence {
            source_count: source_records.len(),
            selector_count: selector_groups.len(),
        });
    }

    let mut groups = Vec::with_capacity(source_records.len());
    for (record_index, (record, selectors)) in
        source_records.into_iter().zip(selector_groups).enumerate()
    {
        if selectors.record_index() != record_index || selectors.record_offset() != record.offset()
        {
            return Err(C3dObjectTopologyError::Group1Metadata {
                expected_record_index: record_index,
                selector_record_index: selectors.record_index(),
                source_record_offset: record.offset(),
                selector_record_offset: selectors.record_offset(),
            });
        }
        for (entry24_index, selector) in selectors.selectors().iter().copied().enumerate() {
            if let C3dPreparedSelector::Resource(resource_id) = selector
                && resource_id.provenance() != provenance
            {
                return Err(C3dObjectTopologyError::ForeignSelector {
                    record_index,
                    entry24_index,
                    resource_id,
                });
            }
        }
        let batches =
            plan_consecutive_batches(record, selectors.selectors()).map_err(|source| {
                C3dObjectTopologyError::Batch {
                    record_index,
                    record_offset: record.offset(),
                    source,
                }
            })?;
        groups.push(C3dGroup1Topology {
            record_index,
            record,
            batches,
        });
    }

    let group1_count = groups.len();
    let mut group0_records = Vec::new();
    for (record_index, record) in file.group0_records().enumerate() {
        let mut references = Vec::with_capacity(record.values().len());
        for (value_index, ordinal) in record.values().enumerate() {
            let source_offset = record.offset() + C3D_GROUP0_VALUES_OFFSET + value_index * 2;
            if usize::from(ordinal) >= group1_count {
                return Err(C3dObjectTopologyError::Group1ReferenceOutOfBounds {
                    group0_record_index: record_index,
                    group0_record_offset: record.offset(),
                    value_index,
                    source_offset,
                    ordinal,
                    group1_count,
                });
            }
            references.push(C3dGroup1TopologyReference {
                value_index,
                source_offset,
                group1_ordinal: ordinal,
            });
        }
        group0_records.push(C3dGroup0Topology {
            record_index,
            record,
            references,
        });
    }

    Ok(C3dObjectTopology {
        provenance,
        source: prepared.source().clone(),
        source_sha256: supplied_sha256,
        policy: prepared.policy(),
        group0_records,
        groups,
    })
}
