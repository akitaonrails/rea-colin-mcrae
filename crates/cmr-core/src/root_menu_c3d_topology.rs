//! Combined resource preparation and borrowed root-menu C3D topology plans.
//!
//! Construction deliberately joins selectors to the exact parsed asset set
//! from which they were prepared. The result owns decoded resources while its
//! typed geometry views borrow the source C3D files. Runtime vertex conversion,
//! side-vector interpretation, palette policy, and backend upload remain later
//! boundaries.

use std::fmt;

use cmr_formats::bitmap::BitmapLimits;

use crate::{
    c3d_batch::{C3dBatchPlanError, C3dConsecutiveBatch, plan_consecutive_batches},
    root_menu_assets::{RootMenuObjectIdentity, VisualVariant},
    root_menu_c3d_assets::{ROOT_MENU_C3D_REQUEST_COUNT, RootMenuC3dSet, RootMenuC3dSlot},
    root_menu_c3d_resources::{
        C3dPreparedResource, C3dPreparedResourceAccessError, C3dPreparedResourceId,
        C3dPreparedSelector, C3dResourceArenaIdentityExhausted, RootMenuC3dResourceDiagnostic,
        RootMenuC3dResourceSet, prepare_root_menu_c3d_resources,
    },
};

/// One source group-1 record and its consecutive prepared-selector batches.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dGroup1Topology<'asset> {
    record_index: usize,
    record_offset: usize,
    batches: Vec<C3dConsecutiveBatch<'asset, C3dPreparedSelector>>,
}

impl<'asset> RootMenuC3dGroup1Topology<'asset> {
    #[must_use]
    pub const fn record_index(&self) -> usize {
        self.record_index
    }

    #[must_use]
    pub const fn record_offset(&self) -> usize {
        self.record_offset
    }

    #[must_use]
    pub fn batches(&self) -> &[C3dConsecutiveBatch<'asset, C3dPreparedSelector>] {
        &self.batches
    }
}

/// Ordered topology plans for one successfully prepared fixed-slot object.
#[derive(Debug, Eq, PartialEq)]
pub struct RootMenuC3dObjectTopology<'asset> {
    slot: RootMenuC3dSlot,
    identity: RootMenuObjectIdentity,
    path: &'static str,
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
        }
    }
}

impl std::error::Error for RootMenuC3dTopologyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ArenaIdentity(source) => Some(source),
            Self::Batch { source, .. } => Some(source),
            Self::Group1Correspondence { .. } | Self::Group1Metadata { .. } => None,
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
                record_offset: record.offset(),
                batches,
            });
        }
        slots[slot.index()] = Some(RootMenuC3dObjectTopology {
            slot,
            identity: prepared.identity(),
            path: prepared.path(),
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
        collections::BTreeMap,
        fs,
        path::PathBuf,
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
        root_menu_assets::VisualVariant,
        root_menu_c3d_assets::load_root_menu_c3d,
        root_menu_c3d_resources::{C3dPreparedResourceAccessError, C3dResourcePreparationError},
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

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
        assert_eq!(
            second.resource(first_id),
            Err(C3dPreparedResourceAccessError::Foreign)
        );
        assert!(first.resource(first_id).is_ok());
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

    #[derive(Clone)]
    struct Group {
        entry32_count: usize,
        corners: Vec<[u16; 3]>,
        selectors: Vec<i16>,
    }

    fn group(entry32_count: usize, corners: &[[u16; 3]], selectors: &[i16]) -> Group {
        assert_eq!(corners.len(), selectors.len());
        Group {
            entry32_count,
            corners: corners.to_vec(),
            selectors: selectors.to_vec(),
        }
    }

    fn c3d(groups: &[Group], blobs: &[([u8; 12], Vec<u8>)]) -> Vec<u8> {
        let group1_len = groups
            .iter()
            .map(|group| {
                C3D_GROUP1_FIXED_LEN
                    + group.entry32_count * C3D_GROUP1_ENTRY32_LEN
                    + group.selectors.len() * C3D_GROUP1_ENTRY24_LEN
            })
            .sum::<usize>();
        let blob_start = C3D_HEADER_LEN + group1_len;
        let total_len = blob_start + blobs.iter().map(|(_, blob)| 16 + blob.len()).sum::<usize>();
        let mut bytes = vec![0; total_len];
        bytes[0..4].copy_from_slice(&C3D_MAGIC);
        bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        bytes[14..16].copy_from_slice(&(groups.len() as u16).to_le_bytes());
        bytes[16..18].copy_from_slice(&(blobs.len() as u16).to_le_bytes());
        for (index, offset) in [
            C3D_HEADER_LEN as u32,
            C3D_HEADER_LEN as u32,
            blob_start as u32,
        ]
        .into_iter()
        .enumerate()
        {
            let start = 20 + index * 4;
            bytes[start..start + 4].copy_from_slice(&offset.to_le_bytes());
        }

        let mut offset = C3D_HEADER_LEN;
        for group in groups {
            let record_len = C3D_GROUP1_FIXED_LEN
                + group.entry32_count * C3D_GROUP1_ENTRY32_LEN
                + group.selectors.len() * C3D_GROUP1_ENTRY24_LEN;
            bytes[offset + 12..offset + 16].copy_from_slice(&(record_len as u32).to_le_bytes());
            bytes[offset + 16..offset + 18]
                .copy_from_slice(&(group.entry32_count as u16).to_le_bytes());
            bytes[offset + 18..offset + 20]
                .copy_from_slice(&(group.selectors.len() as u16).to_le_bytes());
            let entries24 =
                offset + C3D_GROUP1_FIXED_LEN + group.entry32_count * C3D_GROUP1_ENTRY32_LEN;
            for (index, (corners, selector)) in
                group.corners.iter().zip(&group.selectors).enumerate()
            {
                let entry = entries24 + index * C3D_GROUP1_ENTRY24_LEN;
                for (corner, ordinal) in corners.iter().enumerate() {
                    let field = entry + C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[corner];
                    bytes[field..field + 2].copy_from_slice(&ordinal.to_le_bytes());
                }
                bytes[entry + 8..entry + 10].copy_from_slice(&selector.to_le_bytes());
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
        let palette_len = 256 * 4;
        let pixel_offset = 14 + 40 + palette_len;
        let stride = usize::from(depth).div_ceil(32) * 4;
        let file_len = pixel_offset + stride;
        let mut bytes = vec![0; file_len];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&(file_len as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&1i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&depth.to_le_bytes());
        bytes[34..38].copy_from_slice(&(stride as u32).to_le_bytes());
        bytes[46..50].copy_from_slice(&256u32.to_le_bytes());
        for index in 0..256 {
            bytes[54 + index * 4] = palette_blue.wrapping_add(index as u8);
        }
        bytes[pixel_offset] = pixel;
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
