//! Backend-neutral preparation of the root menu's embedded C3D resources.
//!
//! The reviewed root objects contain static indexed BMP resources. This module
//! decodes that bounded subset, interns exact visual identities into one owned
//! arena, and resolves each group-1 entry's resource selector. It deliberately
//! does not convert palettes, upload surfaces, construct render batches, or
//! create backend handles.

use std::{
    fmt,
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

use cmr_formats::{
    bitmap::{BitmapError, BitmapLimits, IndexedBitmap, parse_indexed_bitmap},
    c3d::{
        C3D_BLOB_LABEL_LEN, C3D_BLOB_RECORD_HEADER_LEN, C3dReferenceError, C3dResourceError,
        C3dResourceView,
    },
};

use crate::{
    root_menu_assets::{RootMenuObjectIdentity, VisualVariant},
    root_menu_c3d_assets::{
        ROOT_MENU_C3D_REQUEST_COUNT, RootMenuC3dAsset, RootMenuC3dSet, RootMenuC3dSlot,
    },
};

pub const ROOT_MENU_C3D_RESOURCE_CAPACITY: usize = 2048;

static NEXT_RESOURCE_ARENA_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct C3dPreparedResourceArenaId(NonZeroU64);

/// Opaque identity shared by products derived from one prepared resource arena.
///
/// The private payload prevents callers from forging a relationship between
/// independently prepared C3D sets while allowing later backend catalogs to
/// compare and retain the identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct C3dPreparedResourceProvenance(C3dPreparedResourceArenaId);

/// Stable, arena-scoped index into one [`RootMenuC3dResourceSet`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct C3dPreparedResourceId {
    arena: C3dPreparedResourceArenaId,
    index: u16,
}

impl C3dPreparedResourceId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.index as usize
    }

    pub(crate) const fn provenance(self) -> C3dPreparedResourceProvenance {
        C3dPreparedResourceProvenance(self.arena)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dResourceArenaIdentityExhausted;

impl fmt::Display for C3dResourceArenaIdentityExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("prepared C3D resource arena identity space is exhausted")
    }
}

impl std::error::Error for C3dResourceArenaIdentityExhausted {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dPreparedResourceAccessError {
    Foreign,
    Unknown { index: usize, resource_count: usize },
}

impl fmt::Display for C3dPreparedResourceAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Foreign => {
                formatter.write_str("prepared C3D resource ID belongs to another arena")
            }
            Self::Unknown {
                index,
                resource_count,
            } => write!(
                formatter,
                "prepared C3D resource index {index} is outside arena length {resource_count}"
            ),
        }
    }
}

impl std::error::Error for C3dPreparedResourceAccessError {}

/// Prepared selector assigned to one source group-1 24-byte entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum C3dPreparedSelector {
    Resource(C3dPreparedResourceId),
    /// Exact source `-1` group-2 sentinel.
    Missing,
}

/// One interned, owned indexed resource.
///
/// When later resources alias this entry, the first resource's complete bitmap
/// and palette remain authoritative.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct C3dPreparedResource {
    first_slot: RootMenuC3dSlot,
    first_path: &'static str,
    first_resource_ordinal: u16,
    source_label: [u8; C3D_BLOB_LABEL_LEN],
    source_dib_extent_low16: [u16; 2],
    bitmap: IndexedBitmap,
}

impl C3dPreparedResource {
    #[must_use]
    pub const fn first_slot(&self) -> RootMenuC3dSlot {
        self.first_slot
    }

    #[must_use]
    pub const fn first_path(&self) -> &'static str {
        self.first_path
    }

    #[must_use]
    pub const fn first_resource_ordinal(&self) -> u16 {
        self.first_resource_ordinal
    }

    #[must_use]
    pub const fn source_label(&self) -> &[u8; C3D_BLOB_LABEL_LEN] {
        &self.source_label
    }

    /// Raw low words read by the legacy surface constructor from the embedded
    /// BMP's signed 32-bit DIB width and height fields.
    #[must_use]
    pub const fn source_dib_extent_low16(&self) -> [u16; 2] {
        self.source_dib_extent_low16
    }

    #[must_use]
    pub const fn bitmap(&self) -> &IndexedBitmap {
        &self.bitmap
    }

    /// The recovered root mode-2 path does not install a surface color key.
    #[must_use]
    pub const fn surface_color_key_index(&self) -> Option<u8> {
        None
    }
}

/// Selector sequence for one group-1 record, in its entry24 source order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct C3dPreparedGroup1Selectors {
    record_index: usize,
    record_offset: usize,
    selectors: Vec<C3dPreparedSelector>,
}

impl C3dPreparedGroup1Selectors {
    #[must_use]
    pub const fn record_index(&self) -> usize {
        self.record_index
    }

    #[must_use]
    pub const fn record_offset(&self) -> usize {
        self.record_offset
    }

    #[must_use]
    pub fn selectors(&self) -> &[C3dPreparedSelector] {
        &self.selectors
    }
}

/// Prepared resources and selectors for one root object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootMenuC3dPreparedObject {
    slot: RootMenuC3dSlot,
    identity: RootMenuObjectIdentity,
    path: &'static str,
    ordinal_resources: Vec<C3dPreparedResourceId>,
    group1_selectors: Vec<C3dPreparedGroup1Selectors>,
}

impl RootMenuC3dPreparedObject {
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

    /// Maps every source group-2 ordinal to its prepared arena resource.
    #[must_use]
    pub fn ordinal_resources(&self) -> &[C3dPreparedResourceId] {
        &self.ordinal_resources
    }

    #[must_use]
    pub fn resource_for_ordinal(&self, ordinal: u16) -> Option<C3dPreparedResourceId> {
        self.ordinal_resources.get(usize::from(ordinal)).copied()
    }

    pub fn group1_selectors(&self) -> impl ExactSizeIterator<Item = &C3dPreparedGroup1Selectors> {
        self.group1_selectors.iter()
    }

    #[must_use]
    pub fn selectors_for_group1(&self, record_index: usize) -> Option<&C3dPreparedGroup1Selectors> {
        self.group1_selectors.get(record_index)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedC3dResourceKind {
    Animation,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum C3dResourcePreparationError {
    MalformedResource {
        resource_ordinal: u16,
        resource_offset: usize,
        source: C3dResourceError,
    },
    UnsupportedResource {
        resource_ordinal: u16,
        resource_offset: usize,
        kind: UnsupportedC3dResourceKind,
    },
    Bitmap {
        resource_ordinal: u16,
        resource_offset: usize,
        source: BitmapError,
    },
    Capacity {
        resource_ordinal: u16,
        resource_offset: usize,
        limit: usize,
    },
    Reference(C3dReferenceError),
}

impl fmt::Display for C3dResourcePreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedResource {
                resource_ordinal,
                resource_offset,
                source,
            } => write!(
                formatter,
                "C3D group-2 resource {resource_ordinal} at offset {resource_offset} is malformed: {source}"
            ),
            Self::UnsupportedResource {
                resource_ordinal,
                resource_offset,
                kind,
            } => write!(
                formatter,
                "C3D group-2 resource {resource_ordinal} at offset {resource_offset} has unsupported root-menu kind {kind:?}"
            ),
            Self::Bitmap {
                resource_ordinal,
                resource_offset,
                source,
            } => write!(
                formatter,
                "cannot decode C3D group-2 bitmap {resource_ordinal} at offset {resource_offset}: {source}"
            ),
            Self::Capacity {
                resource_ordinal,
                resource_offset,
                limit,
            } => write!(
                formatter,
                "C3D group-2 resource {resource_ordinal} at offset {resource_offset} exceeds prepared arena capacity {limit}"
            ),
            Self::Reference(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for C3dResourcePreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MalformedResource { source, .. } => Some(source),
            Self::Bitmap { source, .. } => Some(source),
            Self::Reference(source) => Some(source),
            Self::UnsupportedResource { .. } | Self::Capacity { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootMenuC3dResourceDiagnostic {
    slot: RootMenuC3dSlot,
    identity: RootMenuObjectIdentity,
    path: &'static str,
    source: C3dResourcePreparationError,
}

impl RootMenuC3dResourceDiagnostic {
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

    #[must_use]
    pub const fn error(&self) -> &C3dResourcePreparationError {
        &self.source
    }
}

impl fmt::Display for RootMenuC3dResourceDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot prepare root-menu C3D slot {:?} ({:?}) at {}: {}",
            self.slot, self.identity, self.path, self.source
        )
    }
}

impl std::error::Error for RootMenuC3dResourceDiagnostic {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Owned resource arena plus six fixed optional prepared-object slots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootMenuC3dResourceSet {
    arena: C3dPreparedResourceArenaId,
    variant: VisualVariant,
    slots: [Option<RootMenuC3dPreparedObject>; ROOT_MENU_C3D_REQUEST_COUNT],
    diagnostics: Vec<RootMenuC3dResourceDiagnostic>,
    resources: Vec<C3dPreparedResource>,
}

impl RootMenuC3dResourceSet {
    #[must_use]
    pub const fn variant(&self) -> VisualVariant {
        self.variant
    }

    #[must_use]
    pub const fn provenance(&self) -> C3dPreparedResourceProvenance {
        C3dPreparedResourceProvenance(self.arena)
    }

    pub fn slots(
        &self,
    ) -> impl ExactSizeIterator<Item = (RootMenuC3dSlot, Option<&RootMenuC3dPreparedObject>)> {
        RootMenuC3dSlot::ALL
            .into_iter()
            .map(|slot| (slot, self.get(slot)))
    }

    #[must_use]
    pub fn get(&self, slot: RootMenuC3dSlot) -> Option<&RootMenuC3dPreparedObject> {
        self.slots[slot.index()].as_ref()
    }

    #[must_use]
    pub fn loaded_len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    pub fn diagnostics(&self) -> impl ExactSizeIterator<Item = &RootMenuC3dResourceDiagnostic> {
        self.diagnostics.iter()
    }

    pub fn resource(
        &self,
        id: C3dPreparedResourceId,
    ) -> Result<&C3dPreparedResource, C3dPreparedResourceAccessError> {
        if id.arena != self.arena {
            return Err(C3dPreparedResourceAccessError::Foreign);
        }
        self.resources
            .get(id.index())
            .ok_or(C3dPreparedResourceAccessError::Unknown {
                index: id.index(),
                resource_count: self.resources.len(),
            })
    }

    pub fn resources(
        &self,
    ) -> impl ExactSizeIterator<Item = (C3dPreparedResourceId, &C3dPreparedResource)> {
        self.resources.iter().enumerate().map(|(index, resource)| {
            let id = self.resource_id(index);
            (id, resource)
        })
    }

    #[must_use]
    pub fn resource_len(&self) -> usize {
        self.resources.len()
    }

    fn resource_id(&self, index: usize) -> C3dPreparedResourceId {
        resource_id(self.arena, index)
    }
}

/// Prepares every currently loaded root object in fixed slot order.
///
/// Each object is transactional. A failure removes every arena resource first
/// introduced by that object, leaves its slot empty, records one diagnostic,
/// and continues with later objects. Existing loader holes remain holes.
pub fn prepare_root_menu_c3d_resources(
    assets: &RootMenuC3dSet,
    bitmap_limits: BitmapLimits,
) -> Result<RootMenuC3dResourceSet, C3dResourceArenaIdentityExhausted> {
    let source_slots = RootMenuC3dSlot::ALL.map(|slot| assets.get(slot));
    prepare_slots(assets.variant(), source_slots, bitmap_limits)
}

fn prepare_slots(
    variant: VisualVariant,
    source_slots: [Option<&RootMenuC3dAsset>; ROOT_MENU_C3D_REQUEST_COUNT],
    bitmap_limits: BitmapLimits,
) -> Result<RootMenuC3dResourceSet, C3dResourceArenaIdentityExhausted> {
    let arena = allocate_resource_arena_id()?;
    let mut resources = Vec::new();
    let mut slots = std::array::from_fn(|_| None);
    let mut diagnostics = Vec::new();

    for slot in RootMenuC3dSlot::ALL {
        let Some(asset) = source_slots[slot.index()] else {
            continue;
        };
        let transaction_start = resources.len();
        match prepare_object(asset, bitmap_limits, arena, &mut resources) {
            Ok(prepared) => slots[slot.index()] = Some(prepared),
            Err(source) => {
                resources.truncate(transaction_start);
                diagnostics.push(RootMenuC3dResourceDiagnostic {
                    slot,
                    identity: asset.identity,
                    path: asset.path,
                    source,
                });
            }
        }
    }

    Ok(RootMenuC3dResourceSet {
        arena,
        variant,
        slots,
        diagnostics,
        resources,
    })
}

fn prepare_object(
    asset: &RootMenuC3dAsset,
    bitmap_limits: BitmapLimits,
    arena: C3dPreparedResourceArenaId,
    resources: &mut Vec<C3dPreparedResource>,
) -> Result<RootMenuC3dPreparedObject, C3dResourcePreparationError> {
    let mut ordinal_resources = Vec::with_capacity(asset.file.blob_records().len());
    for (resource_index, record) in asset.file.blob_records().enumerate() {
        let resource_ordinal = u16::try_from(resource_index)
            .expect("C3D group-2 count is represented by a u16 header field");
        let resource_offset = record.offset() + C3D_BLOB_RECORD_HEADER_LEN;
        let bitmap_bytes = match record.classify_resource() {
            Ok(C3dResourceView::BitmapBytes(bytes)) => bytes,
            Ok(C3dResourceView::Animation(_)) => {
                return Err(C3dResourcePreparationError::UnsupportedResource {
                    resource_ordinal,
                    resource_offset,
                    kind: UnsupportedC3dResourceKind::Animation,
                });
            }
            Ok(C3dResourceView::Unknown(_)) => {
                return Err(C3dResourcePreparationError::UnsupportedResource {
                    resource_ordinal,
                    resource_offset,
                    kind: UnsupportedC3dResourceKind::Unknown,
                });
            }
            Err(source) => {
                return Err(C3dResourcePreparationError::MalformedResource {
                    resource_ordinal,
                    resource_offset,
                    source,
                });
            }
        };
        let bitmap = parse_indexed_bitmap(bitmap_bytes, bitmap_limits).map_err(|source| {
            C3dResourcePreparationError::Bitmap {
                resource_ordinal,
                resource_offset,
                source,
            }
        })?;
        let source_dib_extent_low16 = [
            u16::from_le_bytes([bitmap_bytes[18], bitmap_bytes[19]]),
            u16::from_le_bytes([bitmap_bytes[22], bitmap_bytes[23]]),
        ];
        let source = DecodedC3dResource {
            label: *record.label(),
            dib_extent_low16: source_dib_extent_low16,
            bitmap,
        };
        let id = intern_resource(
            resources,
            arena,
            asset,
            resource_ordinal,
            resource_offset,
            source,
        )?;
        ordinal_resources.push(id);
    }

    let mut group1_selectors = asset
        .file
        .group1_records()
        .enumerate()
        .map(|(record_index, record)| C3dPreparedGroup1Selectors {
            record_index,
            record_offset: record.offset(),
            selectors: Vec::with_capacity(record.entries_24().len()),
        })
        .collect::<Vec<_>>();
    for reference in asset.file.resolve_group2_references() {
        let reference = reference.map_err(C3dResourcePreparationError::Reference)?;
        let selector = match reference.target() {
            None => C3dPreparedSelector::Missing,
            Some(_) => {
                let ordinal = u16::try_from(reference.ordinal())
                    .expect("resolved non-negative i16 ordinal fits u16");
                C3dPreparedSelector::Resource(ordinal_resources[usize::from(ordinal)])
            }
        };
        group1_selectors[reference.group1_record_index()]
            .selectors
            .push(selector);
    }

    Ok(RootMenuC3dPreparedObject {
        slot: asset.slot,
        identity: asset.identity,
        path: asset.path,
        ordinal_resources,
        group1_selectors,
    })
}

struct DecodedC3dResource {
    label: [u8; C3D_BLOB_LABEL_LEN],
    dib_extent_low16: [u16; 2],
    bitmap: IndexedBitmap,
}

fn intern_resource(
    resources: &mut Vec<C3dPreparedResource>,
    arena: C3dPreparedResourceArenaId,
    asset: &RootMenuC3dAsset,
    resource_ordinal: u16,
    resource_offset: usize,
    source: DecodedC3dResource,
) -> Result<C3dPreparedResourceId, C3dResourcePreparationError> {
    if let Some(index) = resources.iter().position(|resource| {
        bounded_c_label_eq(&resource.source_label, &source.label)
            && resource.bitmap.width == source.bitmap.width
            && resource.bitmap.height == source.bitmap.height
            && resource.bitmap.pixels == source.bitmap.pixels
    }) {
        return Ok(resource_id(arena, index));
    }
    if resources.len() == ROOT_MENU_C3D_RESOURCE_CAPACITY {
        return Err(C3dResourcePreparationError::Capacity {
            resource_ordinal,
            resource_offset,
            limit: ROOT_MENU_C3D_RESOURCE_CAPACITY,
        });
    }
    let id = resource_id(arena, resources.len());
    resources.push(C3dPreparedResource {
        first_slot: asset.slot,
        first_path: asset.path,
        first_resource_ordinal: resource_ordinal,
        source_label: source.label,
        source_dib_extent_low16: source.dib_extent_low16,
        bitmap: source.bitmap,
    });
    Ok(id)
}

fn allocate_resource_arena_id()
-> Result<C3dPreparedResourceArenaId, C3dResourceArenaIdentityExhausted> {
    allocate_resource_arena_id_from(&NEXT_RESOURCE_ARENA_ID)
}

fn allocate_resource_arena_id_from(
    next_id: &AtomicU64,
) -> Result<C3dPreparedResourceArenaId, C3dResourceArenaIdentityExhausted> {
    let mut raw = next_id.load(Ordering::Relaxed);
    loop {
        let next = raw
            .checked_add(1)
            .ok_or(C3dResourceArenaIdentityExhausted)?;
        match next_id.compare_exchange_weak(raw, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => raw = observed,
        }
    }
    NonZeroU64::new(raw)
        .map(C3dPreparedResourceArenaId)
        .ok_or(C3dResourceArenaIdentityExhausted)
}

fn resource_id(arena: C3dPreparedResourceArenaId, index: usize) -> C3dPreparedResourceId {
    C3dPreparedResourceId {
        arena,
        index: u16::try_from(index).expect("prepared C3D arena capacity fits u16"),
    }
}

fn bounded_c_label_eq(left: &[u8; C3D_BLOB_LABEL_LEN], right: &[u8; C3D_BLOB_LABEL_LEN]) -> bool {
    for (&left_byte, &right_byte) in left.iter().zip(right) {
        if left_byte != right_byte {
            return false;
        }
        if left_byte == 0 {
            return true;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmr_formats::{
        bitmap::{BitmapMetadata, BitmapPaletteEntry},
        c3d::{C3D_FORMAT_VERSION, C3D_MAGIC, C3dLimits, parse_c3d},
    };

    #[test]
    fn interns_only_bounded_equal_labels_dimensions_and_pixels_and_keeps_first_palette() {
        let label_a = *b"SAME-LABEL!!";
        let label_b = *b"OTHER-LABEL!";
        let first = object(
            RootMenuC3dSlot::Championship,
            &[vec![], vec![], vec![], vec![]],
            &[
                (label_a, indexed_bmp(1, 10, 1, 8)),
                (label_a, indexed_bmp(1, 99, 1, 8)),
                (label_b, indexed_bmp(1, 10, 1, 8)),
                (label_a, indexed_bmp(2, 10, 1, 8)),
            ],
        );
        let second = object(
            RootMenuC3dSlot::Rally,
            &[vec![]],
            &[(label_a, indexed_bmp(1, 77, 1, 8))],
        );
        let prepared = prepare_slots(
            VisualVariant::Normal,
            slot_refs([Some(&first), Some(&second), None, None, None, None]),
            BitmapLimits::default(),
        )
        .unwrap();

        assert_eq!(prepared.loaded_len(), 2);
        assert_eq!(prepared.resource_len(), 3);
        let first_ids = prepared
            .get(RootMenuC3dSlot::Championship)
            .unwrap()
            .ordinal_resources();
        assert_eq!(
            first_ids.iter().map(|id| id.index()).collect::<Vec<_>>(),
            [0, 0, 1, 2]
        );
        assert_eq!(
            prepared
                .get(RootMenuC3dSlot::Rally)
                .unwrap()
                .ordinal_resources()[0],
            first_ids[0]
        );
        let retained = prepared.resource(first_ids[0]).unwrap();
        assert_eq!(retained.first_slot(), RootMenuC3dSlot::Championship);
        assert_eq!(retained.first_resource_ordinal(), 0);
        assert_eq!(retained.source_label(), &label_a);
        assert_eq!(retained.source_dib_extent_low16(), [1, 1]);
        assert_eq!(retained.bitmap().palette[0].blue, 10);
        assert_eq!(retained.bitmap().pixels, [1]);
        assert_eq!(retained.surface_color_key_index(), None);
        assert_eq!(prepared.resources().len(), 3);
    }

    #[test]
    fn retains_raw_dib_extent_words_before_bitmap_normalization() {
        let mut bitmap = indexed_bmp(1, 10, 1, 8);
        bitmap[22..26].copy_from_slice(&(-1i32).to_le_bytes());
        let asset = object(
            RootMenuC3dSlot::Championship,
            &[vec![0]],
            &[(*b"RAW-DIB-WORD", bitmap)],
        );
        let prepared = prepare_slots(
            VisualVariant::Normal,
            slot_refs([Some(&asset), None, None, None, None, None]),
            BitmapLimits::default(),
        )
        .unwrap();
        let resource = prepared.resources().next().unwrap().1;

        assert_eq!(resource.source_dib_extent_low16(), [1, u16::MAX]);
        assert_eq!(resource.bitmap().height, 1);
        assert!(resource.bitmap().source_was_top_down);
    }

    #[test]
    fn bounded_labels_ignore_bytes_after_equal_nul_but_not_differences_before_it() {
        let first_label = *b"EQ\0firsttail";
        let alias_label = *b"EQ\0othertail";
        let distinct_label = *b"ER\0firsttail";
        let asset = object(
            RootMenuC3dSlot::Championship,
            &[vec![]],
            &[
                (first_label, indexed_bmp(1, 10, 1, 8)),
                (alias_label, indexed_bmp(1, 20, 1, 8)),
                (distinct_label, indexed_bmp(1, 30, 1, 8)),
            ],
        );
        let prepared = prepare_slots(
            VisualVariant::Normal,
            slot_refs([Some(&asset), None, None, None, None, None]),
            BitmapLimits::default(),
        )
        .unwrap();

        let ids = prepared
            .get(RootMenuC3dSlot::Championship)
            .unwrap()
            .ordinal_resources();
        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[2]);
        assert_eq!(prepared.resource_len(), 2);
        assert_eq!(
            prepared.resource(ids[0]).unwrap().source_label(),
            &first_label
        );
        assert_eq!(
            prepared.resource(ids[0]).unwrap().bitmap().palette[0].blue,
            10
        );
    }

    #[test]
    fn resource_ids_are_scoped_to_their_arena_and_reject_unknown_indices() {
        let asset = object(
            RootMenuC3dSlot::Championship,
            &[vec![0]],
            &[(*b"SCOPED-ID!!!", indexed_bmp(1, 10, 1, 8))],
        );
        let slots = slot_refs([Some(&asset), None, None, None, None, None]);
        let first = prepare_slots(VisualVariant::Normal, slots, BitmapLimits::default()).unwrap();
        let second = prepare_slots(VisualVariant::Normal, slots, BitmapLimits::default()).unwrap();
        let first_id = first.resources().next().unwrap().0;

        assert_eq!(first.resource(first_id).unwrap().bitmap().pixels, [1]);
        assert_eq!(
            second.resource(first_id),
            Err(C3dPreparedResourceAccessError::Foreign)
        );

        let unknown = C3dPreparedResourceId {
            arena: first.arena,
            index: 1,
        };
        assert_eq!(
            first.resource(unknown),
            Err(C3dPreparedResourceAccessError::Unknown {
                index: 1,
                resource_count: 1,
            })
        );
    }

    #[test]
    fn arena_identity_exhaustion_is_a_typed_error() {
        let exhausted = AtomicU64::new(u64::MAX);

        assert_eq!(
            allocate_resource_arena_id_from(&exhausted),
            Err(C3dResourceArenaIdentityExhausted)
        );
        assert_eq!(exhausted.load(Ordering::Relaxed), u64::MAX);
    }

    #[test]
    fn equal_labels_and_pixels_do_not_alias_different_dimensions() {
        let asset = object(RootMenuC3dSlot::Championship, &[vec![]], &[]);
        let arena = test_arena();
        let mut tall = decoded_bitmap(1, 10);
        tall.height = 2;
        tall.pixels = vec![1, 2];
        let mut wide = decoded_bitmap(1, 20);
        wide.width = 2;
        wide.pixels = vec![1, 2];
        let mut resources = Vec::new();

        let tall_id = intern_resource(
            &mut resources,
            arena,
            &asset,
            0,
            100,
            DecodedC3dResource {
                label: *b"DIMENSIONS!!",
                dib_extent_low16: [1, 2],
                bitmap: tall,
            },
        )
        .unwrap();
        let wide_id = intern_resource(
            &mut resources,
            arena,
            &asset,
            1,
            200,
            DecodedC3dResource {
                label: *b"DIMENSIONS!!",
                dib_extent_low16: [2, 1],
                bitmap: wide,
            },
        )
        .unwrap();

        assert_ne!(tall_id, wide_id);
        assert_eq!(resources.len(), 2);
    }

    #[test]
    fn builds_selector_sequences_for_multiple_records_with_repeats_and_missing() {
        let asset = object(
            RootMenuC3dSlot::Options,
            &[vec![1, -1, 1, 0], vec![-1, 0]],
            &[
                (*b"RESOURCE-A!!", indexed_bmp(1, 1, 1, 8)),
                (*b"RESOURCE-B!!", indexed_bmp(2, 2, 1, 8)),
            ],
        );
        let prepared = prepare_slots(
            VisualVariant::Normal,
            slot_refs([None, None, None, None, Some(&asset), None]),
            BitmapLimits::default(),
        )
        .unwrap();
        let object = prepared.get(RootMenuC3dSlot::Options).unwrap();
        let ids = object.ordinal_resources();
        let groups = object.group1_selectors().collect::<Vec<_>>();

        assert_eq!(groups.len(), 2);
        assert_eq!((groups[0].record_index(), groups[1].record_index()), (0, 1));
        assert!(groups[0].record_offset() < groups[1].record_offset());
        assert_eq!(
            groups[0].selectors(),
            [
                C3dPreparedSelector::Resource(ids[1]),
                C3dPreparedSelector::Missing,
                C3dPreparedSelector::Resource(ids[1]),
                C3dPreparedSelector::Resource(ids[0]),
            ]
        );
        assert_eq!(
            groups[1].selectors(),
            [
                C3dPreparedSelector::Missing,
                C3dPreparedSelector::Resource(ids[0]),
            ]
        );
        assert_eq!(object.resource_for_ordinal(0), Some(ids[0]));
        assert_eq!(object.resource_for_ordinal(2), None);
        assert!(std::ptr::eq(
            object.selectors_for_group1(1).unwrap(),
            groups[1]
        ));
    }

    #[test]
    fn reports_bitmap_resource_kind_and_reference_failures_with_context() {
        let cases = [
            (
                vec![],
                vec![(*b"SHORT-BITMAP", b"BM".to_vec())],
                ExpectedError::Bitmap(BitmapError::Truncated),
            ),
            (
                vec![],
                vec![(*b"FOUR-BIT-BMP", indexed_bmp(1, 0, 1, 4))],
                ExpectedError::Bitmap(BitmapError::UnsupportedBitDepth(4)),
            ),
            (
                vec![],
                vec![(*b"ANIMATION!!!", b"ANIM\0\0\0\0".to_vec())],
                ExpectedError::Kind(UnsupportedC3dResourceKind::Animation),
            ),
            (
                vec![],
                vec![(*b"UNKNOWN!!!!!", b"opaque".to_vec())],
                ExpectedError::Kind(UnsupportedC3dResourceKind::Unknown),
            ),
            (
                vec![],
                vec![(*b"BAD-ANIM!!!!", b"ANIM".to_vec())],
                ExpectedError::MalformedAnimation,
            ),
            (
                vec![-2],
                vec![(*b"GOOD-BITMAP!", indexed_bmp(1, 0, 1, 8))],
                ExpectedError::InvalidNegativeReference,
            ),
            (
                vec![1],
                vec![(*b"GOOD-BITMAP!", indexed_bmp(1, 0, 1, 8))],
                ExpectedError::OutOfBoundsReference,
            ),
        ];

        for (index, (ordinals, blobs, expected)) in cases.into_iter().enumerate() {
            let asset = object(RootMenuC3dSlot::Load, &[ordinals], &blobs);
            let mut arena = Vec::new();
            let error = prepare_object(&asset, BitmapLimits::default(), test_arena(), &mut arena)
                .unwrap_err();
            assert_expected_error(index, error, expected);
        }
    }

    #[test]
    fn failed_object_rolls_back_only_its_new_resources_and_later_slots_continue() {
        let shared = (*b"SHARED-RES!!", indexed_bmp(1, 10, 1, 8));
        let rolled_back = (*b"ROLLED-BACK!", indexed_bmp(2, 20, 1, 8));
        let championship = object(
            RootMenuC3dSlot::Championship,
            &[vec![0]],
            std::slice::from_ref(&shared),
        );
        let rally = object(
            RootMenuC3dSlot::Rally,
            &[vec![0]],
            &[
                shared.clone(),
                rolled_back.clone(),
                (*b"BAD-RESOURCE", b"unknown".to_vec()),
            ],
        );
        let time_trial = object(RootMenuC3dSlot::TimeTrial, &[vec![0]], &[rolled_back]);

        let prepared = prepare_slots(
            VisualVariant::GPrefixed,
            slot_refs([
                Some(&championship),
                Some(&rally),
                Some(&time_trial),
                None,
                None,
                None,
            ]),
            BitmapLimits::default(),
        )
        .unwrap();

        assert_eq!(prepared.variant(), VisualVariant::GPrefixed);
        assert_eq!(prepared.loaded_len(), 2);
        assert!(prepared.get(RootMenuC3dSlot::Championship).is_some());
        assert!(prepared.get(RootMenuC3dSlot::Rally).is_none());
        assert!(prepared.get(RootMenuC3dSlot::TimeTrial).is_some());
        assert_eq!(prepared.resource_len(), 2);
        assert_eq!(prepared.diagnostics().len(), 1);
        let diagnostic = prepared.diagnostics().next().unwrap();
        assert_eq!(diagnostic.slot(), RootMenuC3dSlot::Rally);
        assert_eq!(diagnostic.identity(), RootMenuObjectIdentity::Rally);
        assert_eq!(diagnostic.path(), rally.path);
        assert!(matches!(
            diagnostic.error(),
            C3dResourcePreparationError::UnsupportedResource {
                resource_ordinal: 2,
                kind: UnsupportedC3dResourceKind::Unknown,
                ..
            }
        ));
        let second_resource = prepared
            .resources()
            .nth(1)
            .map(|(_, resource)| resource)
            .unwrap();
        assert_eq!(second_resource.first_slot(), RootMenuC3dSlot::TimeTrial);
        assert_eq!(
            prepared
                .slots()
                .map(|(slot, object)| (slot, object.is_some()))
                .collect::<Vec<_>>(),
            [
                (RootMenuC3dSlot::Championship, true),
                (RootMenuC3dSlot::Rally, false),
                (RootMenuC3dSlot::TimeTrial, true),
                (RootMenuC3dSlot::RallySchool, false),
                (RootMenuC3dSlot::Options, false),
                (RootMenuC3dSlot::Load, false),
            ]
        );
    }

    #[test]
    fn reference_failure_rolls_back_resources_and_later_slot_reuses_vacated_index() {
        let retained = (*b"RETAINED!!!!", indexed_bmp(1, 10, 1, 8));
        let retried = (*b"RETRIED!!!!!", indexed_bmp(2, 20, 1, 8));
        let championship = object(
            RootMenuC3dSlot::Championship,
            &[vec![0]],
            std::slice::from_ref(&retained),
        );
        let rally = object(
            RootMenuC3dSlot::Rally,
            &[vec![1]],
            std::slice::from_ref(&retried),
        );
        let time_trial = object(
            RootMenuC3dSlot::TimeTrial,
            &[vec![0]],
            std::slice::from_ref(&retried),
        );

        let prepared = prepare_slots(
            VisualVariant::Normal,
            slot_refs([
                Some(&championship),
                Some(&rally),
                Some(&time_trial),
                None,
                None,
                None,
            ]),
            BitmapLimits::default(),
        )
        .unwrap();

        let retained_id = prepared
            .get(RootMenuC3dSlot::Championship)
            .unwrap()
            .ordinal_resources()[0];
        let retried_id = prepared
            .get(RootMenuC3dSlot::TimeTrial)
            .unwrap()
            .ordinal_resources()[0];
        assert_eq!(retained_id.index(), 0);
        assert_eq!(retried_id.index(), 1);
        assert_eq!(prepared.resource(retained_id).unwrap().bitmap().pixels, [1]);
        assert_eq!(prepared.resource(retried_id).unwrap().bitmap().pixels, [2]);
        assert!(prepared.get(RootMenuC3dSlot::Rally).is_none());
        assert!(matches!(
            prepared.diagnostics().next().unwrap().error(),
            C3dResourcePreparationError::Reference(C3dReferenceError::Group2OrdinalOutOfBounds {
                ordinal: 1,
                record_count: 1,
                ..
            })
        ));
    }

    #[test]
    fn capacity_accepts_exact_limit_and_rejects_the_next_distinct_resource() {
        let asset = object(RootMenuC3dSlot::Load, &[vec![]], &[]);
        let arena = test_arena();
        let bitmap = decoded_bitmap(7, 9);
        let mut resources = Vec::with_capacity(ROOT_MENU_C3D_RESOURCE_CAPACITY);
        for index in 0..ROOT_MENU_C3D_RESOURCE_CAPACITY {
            let mut label = [1; 12];
            let mut value = index;
            for byte in &mut label {
                *byte = (value % 255 + 1) as u8;
                value /= 255;
            }
            intern_resource(
                &mut resources,
                arena,
                &asset,
                index as u16,
                100 + index,
                DecodedC3dResource {
                    label,
                    dib_extent_low16: [1, 1],
                    bitmap: bitmap.clone(),
                },
            )
            .unwrap();
        }
        assert_eq!(resources.len(), ROOT_MENU_C3D_RESOURCE_CAPACITY);

        let error = intern_resource(
            &mut resources,
            arena,
            &asset,
            ROOT_MENU_C3D_RESOURCE_CAPACITY as u16,
            9000,
            DecodedC3dResource {
                label: [0xff; 12],
                dib_extent_low16: [1, 1],
                bitmap,
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            C3dResourcePreparationError::Capacity {
                resource_ordinal: ROOT_MENU_C3D_RESOURCE_CAPACITY as u16,
                resource_offset: 9000,
                limit: ROOT_MENU_C3D_RESOURCE_CAPACITY,
            }
        );
        assert_eq!(resources.len(), ROOT_MENU_C3D_RESOURCE_CAPACITY);
    }

    #[test]
    fn prepared_output_owns_bitmaps_after_source_assets_are_dropped() {
        let prepared = {
            let asset = object(
                RootMenuC3dSlot::RallySchool,
                &[vec![0]],
                &[(*b"OWNED-BITMAP", indexed_bmp(42, 33, 1, 8))],
            );
            prepare_slots(
                VisualVariant::Normal,
                slot_refs([None, None, None, Some(&asset), None, None]),
                BitmapLimits::default(),
            )
            .unwrap()
        };

        assert_eq!(prepared.resource_len(), 1);
        let (id, resource) = prepared.resources().next().unwrap();
        assert_eq!(id.index(), 0);
        assert_eq!(resource.bitmap().pixels, [42]);
        assert_eq!(resource.bitmap().palette[0].blue, 33);
    }

    #[test]
    fn public_preparation_preserves_existing_loader_holes() {
        let assets = crate::root_menu_c3d_assets::synthetic_root_menu_c3d_set(
            [false; ROOT_MENU_C3D_REQUEST_COUNT],
            VisualVariant::GPrefixed,
        );
        let prepared = prepare_root_menu_c3d_resources(&assets, BitmapLimits::default()).unwrap();

        assert_eq!(prepared.variant(), VisualVariant::GPrefixed);
        assert_eq!(prepared.loaded_len(), 0);
        assert_eq!(prepared.resource_len(), 0);
        assert_eq!(prepared.diagnostics().len(), 0);
        assert!(prepared.slots().all(|(_, object)| object.is_none()));
    }

    enum ExpectedError {
        Bitmap(BitmapError),
        Kind(UnsupportedC3dResourceKind),
        MalformedAnimation,
        InvalidNegativeReference,
        OutOfBoundsReference,
    }

    fn assert_expected_error(
        case_index: usize,
        actual: C3dResourcePreparationError,
        expected: ExpectedError,
    ) {
        let actual_debug = format!("{actual:?}");
        let matches = match expected {
            ExpectedError::Bitmap(expected) => matches!(
                actual,
                C3dResourcePreparationError::Bitmap {
                    resource_ordinal: 0,
                    source,
                    ..
                } if source == expected
            ),
            ExpectedError::Kind(expected) => matches!(
                actual,
                C3dResourcePreparationError::UnsupportedResource {
                    resource_ordinal: 0,
                    kind,
                    ..
                } if kind == expected
            ),
            ExpectedError::MalformedAnimation => matches!(
                actual,
                C3dResourcePreparationError::MalformedResource {
                    resource_ordinal: 0,
                    source: C3dResourceError::TruncatedAnimationHeader { .. },
                    ..
                }
            ),
            ExpectedError::InvalidNegativeReference => matches!(
                actual,
                C3dResourcePreparationError::Reference(
                    C3dReferenceError::InvalidNegativeGroup2Ordinal { ordinal: -2, .. }
                )
            ),
            ExpectedError::OutOfBoundsReference => matches!(
                actual,
                C3dResourcePreparationError::Reference(
                    C3dReferenceError::Group2OrdinalOutOfBounds {
                        ordinal: 1,
                        record_count: 1,
                        ..
                    }
                )
            ),
        };
        assert!(matches, "case {case_index} returned {actual_debug}");
    }

    fn slot_refs(
        slots: [Option<&RootMenuC3dAsset>; ROOT_MENU_C3D_REQUEST_COUNT],
    ) -> [Option<&RootMenuC3dAsset>; ROOT_MENU_C3D_REQUEST_COUNT] {
        slots
    }

    fn test_arena() -> C3dPreparedResourceArenaId {
        allocate_resource_arena_id().unwrap()
    }

    fn object(
        slot: RootMenuC3dSlot,
        groups: &[Vec<i16>],
        blobs: &[([u8; 12], Vec<u8>)],
    ) -> RootMenuC3dAsset {
        RootMenuC3dAsset {
            slot,
            identity: slot.identity(),
            path: slot.path(VisualVariant::Normal),
            file: parse_c3d(&c3d_bytes(groups, blobs), C3dLimits::default()).unwrap(),
        }
    }

    fn c3d_bytes(groups: &[Vec<i16>], blobs: &[([u8; 12], Vec<u8>)]) -> Vec<u8> {
        let group1_len = groups
            .iter()
            .map(|ordinals| 48 + ordinals.len() * 24)
            .sum::<usize>();
        let blob_start = 32 + group1_len;
        let total_len = blob_start + blobs.iter().map(|(_, blob)| 16 + blob.len()).sum::<usize>();
        let mut bytes = vec![0; total_len];
        bytes[0..4].copy_from_slice(&C3D_MAGIC);
        bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        bytes[12..14].copy_from_slice(&0u16.to_le_bytes());
        bytes[14..16].copy_from_slice(&(groups.len() as u16).to_le_bytes());
        bytes[16..18].copy_from_slice(&(blobs.len() as u16).to_le_bytes());
        for (index, offset) in [32u32, 32, blob_start as u32].into_iter().enumerate() {
            let start = 20 + index * 4;
            bytes[start..start + 4].copy_from_slice(&offset.to_le_bytes());
        }

        let mut offset = 32;
        for ordinals in groups {
            let record_len = 48 + ordinals.len() * 24;
            bytes[offset + 12..offset + 16].copy_from_slice(&(record_len as u32).to_le_bytes());
            bytes[offset + 18..offset + 20].copy_from_slice(&(ordinals.len() as u16).to_le_bytes());
            for (index, ordinal) in ordinals.iter().enumerate() {
                let entry = offset + 48 + index * 24;
                bytes[entry + 8..entry + 10].copy_from_slice(&ordinal.to_le_bytes());
            }
            offset += record_len;
        }
        debug_assert_eq!(offset, blob_start);
        for (label, blob) in blobs {
            bytes[offset..offset + 12].copy_from_slice(label);
            bytes[offset + 12..offset + 16].copy_from_slice(&(blob.len() as u32).to_le_bytes());
            bytes[offset + 16..offset + 16 + blob.len()].copy_from_slice(blob);
            offset += 16 + blob.len();
        }
        debug_assert_eq!(offset, total_len);
        bytes
    }

    fn indexed_bmp(pixel: u8, palette_blue: u8, width: u32, depth: u16) -> Vec<u8> {
        let palette_len = 256 * 4;
        let pixel_offset = 14 + 40 + palette_len;
        let stride = (u64::from(width) * u64::from(depth)).div_ceil(32) as usize * 4;
        let file_len = pixel_offset + stride;
        let mut bytes = vec![0; file_len];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&(file_len as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&(width as i32).to_le_bytes());
        bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&depth.to_le_bytes());
        bytes[34..38].copy_from_slice(&(stride as u32).to_le_bytes());
        bytes[46..50].copy_from_slice(&256u32.to_le_bytes());
        for index in 0..256 {
            let start = 54 + index * 4;
            bytes[start] = palette_blue.wrapping_add(index as u8);
            bytes[start + 1] = index as u8;
            bytes[start + 2] = 255 - index as u8;
        }
        bytes[pixel_offset] = pixel;
        bytes
    }

    fn decoded_bitmap(pixel: u8, palette_blue: u8) -> IndexedBitmap {
        IndexedBitmap {
            width: 1,
            height: 1,
            source_was_top_down: false,
            palette: vec![
                BitmapPaletteEntry {
                    red: 0,
                    green: 0,
                    blue: palette_blue,
                    reserved: 0,
                };
                256
            ],
            pixels: vec![pixel],
            metadata: BitmapMetadata {
                reserved_1: 0,
                reserved_2: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                important_colors: 0,
            },
        }
    }
}
