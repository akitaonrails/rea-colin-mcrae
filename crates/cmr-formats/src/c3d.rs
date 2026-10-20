//! Bounded parsing for the observed `CM3D` object header.
//!
//! The legacy loader reads three counts and three payload-relative offsets from
//! a 32-byte prefix. This module reconstructs their proven record framing and
//! counted subarrays under neutral names, preserves all unknown and trailing
//! bytes, and retains the complete payload unchanged.

use std::fmt;

pub const C3D_HEADER_LEN: usize = 32;
pub const C3D_MAGIC: [u8; 4] = *b"CM3D";
pub const C3D_FORMAT_VERSION: u32 = 100;
pub const C3D_SECTION_COUNT: usize = 3;
pub const C3D_GROUP0_FIXED_LEN: usize = 80;
pub const C3D_GROUP0_VALUE_COUNT_OFFSET: usize = 76;
pub const C3D_GROUP0_VALUES_OFFSET: usize = 80;
pub const C3D_GROUP1_FIXED_LEN: usize = 48;
pub const C3D_GROUP1_PREFIX_LEN: usize = 12;
pub const C3D_GROUP1_UNKNOWN_LEN: usize = 28;
pub const C3D_GROUP1_ENTRY32_LEN: usize = 32;
pub const C3D_GROUP1_ENTRY24_LEN: usize = 24;
pub const C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS: [usize; 3] = [2, 4, 6];
pub const C3D_BLOB_RECORD_HEADER_LEN: usize = 16;
pub const C3D_BLOB_LABEL_LEN: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dLimits {
    pub max_payload_bytes: usize,
    pub max_group0_records: usize,
    pub max_group1_records: usize,
    pub max_blob_records: usize,
}

impl Default for C3dLimits {
    fn default() -> Self {
        Self {
            max_payload_bytes: 64 * 1024 * 1024,
            max_group0_records: u16::MAX as usize,
            max_group1_records: u16::MAX as usize,
            max_blob_records: u16::MAX as usize,
        }
    }
}

/// Fields read by the recovered loader, plus the still-unknown header bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dHeader {
    pub format_version: u32,
    /// Bytes at offset 8. Every reviewed root-menu object contains LE `32`,
    /// but the recovered loader does not consult this field.
    pub unknown_08: [u8; 4],
    pub section_counts: [u16; C3D_SECTION_COUNT],
    /// Bytes at offset 18. They are zero in the reviewed root-menu corpus.
    pub unknown_18: [u8; 2],
    pub section_offsets: [u32; C3D_SECTION_COUNT],
}

/// An admitted C3D payload. Unknown bytes remain exact for later slices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct C3dFile {
    header: C3dHeader,
    bytes: Vec<u8>,
    group0_records: Vec<C3dGroup0RecordSpan>,
    group1_records: Vec<C3dGroup1RecordSpan>,
    blob_records: Vec<C3dBlobRecordSpan>,
}

impl C3dFile {
    #[must_use]
    pub const fn header(&self) -> &C3dHeader {
        &self.header
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn group0_records(&self) -> impl ExactSizeIterator<Item = C3dGroup0Record<'_>> {
        self.group0_records.iter().map(|span| C3dGroup0Record {
            offset: span.offset,
            bytes: &self.bytes[span.offset..span.end],
            values: &self.bytes[span.values_start..span.values_end],
            trailing: &self.bytes[span.values_end..span.end],
        })
    }

    pub fn group1_records(&self) -> impl ExactSizeIterator<Item = C3dGroup1Record<'_>> {
        self.group1_records
            .iter()
            .map(|span| self.group1_record_view(span))
    }

    /// Resolves every counted group-0 value as a group-1 record ordinal.
    ///
    /// Resolution is deliberately separate from structural parsing. Each
    /// source value yields one result, so a bad ordinal does not hide later
    /// references.
    pub fn resolve_group1_references(
        &self,
    ) -> impl Iterator<Item = Result<C3dGroup1Reference<'_>, C3dReferenceError>> {
        self.group0_records
            .iter()
            .enumerate()
            .flat_map(move |(group0_record_index, span)| {
                let (values, remainder) =
                    self.bytes[span.values_start..span.values_end].as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                values
                    .iter()
                    .copied()
                    .map(u16::from_le_bytes)
                    .enumerate()
                    .map(move |(value_index, ordinal)| {
                        let source_offset = span.values_start + value_index * 2;
                        let target = self.group1_records.get(usize::from(ordinal)).ok_or(
                            C3dReferenceError::Group1OrdinalOutOfBounds {
                                group0_record_index,
                                value_index,
                                source_offset,
                                ordinal,
                                record_count: self.group1_records.len(),
                            },
                        )?;
                        Ok(C3dGroup1Reference {
                            group0_record_index,
                            value_index,
                            source_offset,
                            ordinal,
                            target: self.group1_record_view(target),
                        })
                    })
            })
    }

    fn group1_record_view(&self, span: &C3dGroup1RecordSpan) -> C3dGroup1Record<'_> {
        C3dGroup1Record {
            offset: span.offset,
            bytes: &self.bytes[span.offset..span.end],
            prefix: self.bytes[span.offset..span.offset + C3D_GROUP1_PREFIX_LEN]
                .try_into()
                .expect("validated C3D group-1 prefix"),
            unknown: self.bytes[span.offset + 20..span.offset + C3D_GROUP1_FIXED_LEN]
                .try_into()
                .expect("validated C3D group-1 unknown region"),
            entries_32: &self.bytes[span.entries_32_start..span.entries_32_end],
            entries_24: &self.bytes[span.entries_32_end..span.entries_24_end],
            trailing: &self.bytes[span.entries_24_end..span.end],
        }
    }

    /// Resolves the three unsigned little-endian ordinals at `+2`, `+4`, and
    /// `+6` in every group-1 24-byte entry against the 32-byte entries in the
    /// same group-1 record.
    ///
    /// Resolution is deliberately separate from structural parsing. Each
    /// source ordinal yields one result, so a bad ordinal does not hide later
    /// references.
    pub fn resolve_entry32_references(
        &self,
    ) -> impl Iterator<Item = Result<C3dEntry32Reference<'_>, C3dReferenceError>> {
        self.group1_records
            .iter()
            .enumerate()
            .flat_map(move |(group1_record_index, span)| {
                let (entries_32, entries_32_remainder) = self.bytes
                    [span.entries_32_start..span.entries_32_end]
                    .as_chunks::<C3D_GROUP1_ENTRY32_LEN>();
                debug_assert!(entries_32_remainder.is_empty());
                let (entries_24, entries_24_remainder) = self.bytes
                    [span.entries_32_end..span.entries_24_end]
                    .as_chunks::<C3D_GROUP1_ENTRY24_LEN>();
                debug_assert!(entries_24_remainder.is_empty());
                entries_24
                    .iter()
                    .enumerate()
                    .flat_map(move |(entry24_index, entry)| {
                        C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS
                            .into_iter()
                            .enumerate()
                            .map(move |(ordinal_index, ordinal_offset)| {
                                let source_offset = span.entries_32_end
                                    + entry24_index * C3D_GROUP1_ENTRY24_LEN
                                    + ordinal_offset;
                                let ordinal = read_u16(entry, ordinal_offset);
                                let target_index = usize::from(ordinal);
                                let target = entries_32.get(target_index).ok_or(
                                    C3dReferenceError::Entry32OrdinalOutOfBounds {
                                        group1_record_index,
                                        entry24_index,
                                        ordinal_index,
                                        entry24_field_offset: ordinal_offset,
                                        source_offset,
                                        ordinal,
                                        entry32_count: entries_32.len(),
                                    },
                                )?;
                                Ok(C3dEntry32Reference {
                                    group1_record_index,
                                    entry24_index,
                                    ordinal_index,
                                    entry24_field_offset: ordinal_offset,
                                    source_offset,
                                    ordinal,
                                    target_offset: span.entries_32_start
                                        + target_index * C3D_GROUP1_ENTRY32_LEN,
                                    target,
                                })
                            })
                    })
            })
    }

    pub fn blob_records(&self) -> impl ExactSizeIterator<Item = C3dBlobRecord<'_>> {
        self.blob_records
            .iter()
            .map(|span| self.blob_record_view(span))
    }

    /// Resolves the signed little-endian ordinal at `+8` in every group-1
    /// 24-byte entry. `-1` is the observed no-record sentinel.
    ///
    /// Resolution is deliberately separate from structural parsing. Each
    /// source entry yields one result, so a bad ordinal does not hide later
    /// references.
    pub fn resolve_group2_references(
        &self,
    ) -> impl Iterator<Item = Result<C3dGroup2Reference<'_>, C3dReferenceError>> {
        self.group1_records
            .iter()
            .enumerate()
            .flat_map(move |(group1_record_index, span)| {
                let (entries, remainder) = self.bytes[span.entries_32_end..span.entries_24_end]
                    .as_chunks::<C3D_GROUP1_ENTRY24_LEN>();
                debug_assert!(remainder.is_empty());
                entries
                    .iter()
                    .enumerate()
                    .map(move |(entry24_index, entry)| {
                        let source_offset =
                            span.entries_32_end + entry24_index * C3D_GROUP1_ENTRY24_LEN + 8;
                        let ordinal = read_i16(entry, 8);
                        let target = match ordinal {
                            -1 => None,
                            ..=-2 => {
                                return Err(C3dReferenceError::InvalidNegativeGroup2Ordinal {
                                    group1_record_index,
                                    entry24_index,
                                    source_offset,
                                    ordinal,
                                });
                            }
                            0.. => {
                                let index = usize::try_from(ordinal)
                                    .expect("non-negative i16 always fits usize");
                                let target = self.blob_records.get(index).ok_or(
                                    C3dReferenceError::Group2OrdinalOutOfBounds {
                                        group1_record_index,
                                        entry24_index,
                                        source_offset,
                                        ordinal,
                                        record_count: self.blob_records.len(),
                                    },
                                )?;
                                Some(self.blob_record_view(target))
                            }
                        };
                        Ok(C3dGroup2Reference {
                            group1_record_index,
                            entry24_index,
                            source_offset,
                            ordinal,
                            target,
                        })
                    })
            })
    }

    fn blob_record_view(&self, span: &C3dBlobRecordSpan) -> C3dBlobRecord<'_> {
        C3dBlobRecord {
            offset: span.offset,
            label: self.bytes[span.offset..span.offset + C3D_BLOB_LABEL_LEN]
                .try_into()
                .expect("validated C3D blob label"),
            blob: &self.bytes[span.blob_start..span.blob_end],
        }
    }
}

/// One structurally recovered record from header group 0.
///
/// Field meanings remain unknown. The executable uses byte `+76` as the count
/// of little-endian `u16` values beginning at `+80`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dGroup0Record<'a> {
    offset: usize,
    bytes: &'a [u8],
    values: &'a [u8],
    trailing: &'a [u8],
}

impl<'a> C3dGroup0Record<'a> {
    #[must_use]
    pub const fn offset(self) -> usize {
        self.offset
    }

    #[must_use]
    pub fn declared_len(self) -> u32 {
        read_u32(self.bytes, 0)
    }

    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.bytes
    }

    #[must_use]
    pub const fn value_count(self) -> u8 {
        self.bytes[C3D_GROUP0_VALUE_COUNT_OFFSET]
    }

    pub fn values(self) -> impl ExactSizeIterator<Item = u16> + 'a {
        let (values, remainder) = self.values.as_chunks::<2>();
        debug_assert!(remainder.is_empty());
        values.iter().copied().map(u16::from_le_bytes)
    }

    #[must_use]
    pub const fn trailing_bytes(self) -> &'a [u8] {
        self.trailing
    }
}

/// One structurally recovered record from header group 1.
///
/// The two nested arrays remain opaque fixed-size entries; their contents and
/// the other fixed-prefix bytes intentionally have no inferred semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dGroup1Record<'a> {
    offset: usize,
    bytes: &'a [u8],
    prefix: &'a [u8; C3D_GROUP1_PREFIX_LEN],
    unknown: &'a [u8; C3D_GROUP1_UNKNOWN_LEN],
    entries_32: &'a [u8],
    entries_24: &'a [u8],
    trailing: &'a [u8],
}

impl<'a> C3dGroup1Record<'a> {
    #[must_use]
    pub const fn offset(self) -> usize {
        self.offset
    }

    #[must_use]
    pub fn declared_len(self) -> u32 {
        read_u32(self.bytes, 12)
    }

    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.bytes
    }

    #[must_use]
    pub const fn prefix(self) -> &'a [u8; C3D_GROUP1_PREFIX_LEN] {
        self.prefix
    }

    #[must_use]
    pub fn entry32_count(self) -> u16 {
        read_u16(self.bytes, 16)
    }

    #[must_use]
    pub fn entry24_count(self) -> u16 {
        read_u16(self.bytes, 18)
    }

    #[must_use]
    pub const fn unknown_bytes(self) -> &'a [u8; C3D_GROUP1_UNKNOWN_LEN] {
        self.unknown
    }

    pub fn entries_32(self) -> impl ExactSizeIterator<Item = &'a [u8; C3D_GROUP1_ENTRY32_LEN]> {
        let (entries, remainder) = self.entries_32.as_chunks::<C3D_GROUP1_ENTRY32_LEN>();
        debug_assert!(remainder.is_empty());
        entries.iter()
    }

    pub fn entries_24(self) -> impl ExactSizeIterator<Item = &'a [u8; C3D_GROUP1_ENTRY24_LEN]> {
        let (entries, remainder) = self.entries_24.as_chunks::<C3D_GROUP1_ENTRY24_LEN>();
        debug_assert!(remainder.is_empty());
        entries.iter()
    }

    #[must_use]
    pub const fn trailing_bytes(self) -> &'a [u8] {
        self.trailing
    }
}

/// One neutral record from the count-at-16, position-at-28 sequence.
///
/// The loader passes the first 12 bytes to a bounded string copy and the
/// following blob to format-dependent code. The blob is not proven to be
/// universally a bitmap, so neither field receives payload semantics here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dBlobRecord<'a> {
    offset: usize,
    label: &'a [u8; C3D_BLOB_LABEL_LEN],
    blob: &'a [u8],
}

/// One successfully resolved group-0 value and its borrowed group-1 target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dGroup1Reference<'a> {
    group0_record_index: usize,
    value_index: usize,
    source_offset: usize,
    ordinal: u16,
    target: C3dGroup1Record<'a>,
}

impl<'a> C3dGroup1Reference<'a> {
    #[must_use]
    pub const fn group0_record_index(self) -> usize {
        self.group0_record_index
    }

    #[must_use]
    pub const fn value_index(self) -> usize {
        self.value_index
    }

    #[must_use]
    pub const fn source_offset(self) -> usize {
        self.source_offset
    }

    #[must_use]
    pub const fn ordinal(self) -> u16 {
        self.ordinal
    }

    #[must_use]
    pub const fn target(self) -> C3dGroup1Record<'a> {
        self.target
    }
}

/// One successfully resolved group-1 24-byte entry and its borrowed group-2
/// target. The target is absent for the `-1` sentinel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dGroup2Reference<'a> {
    group1_record_index: usize,
    entry24_index: usize,
    source_offset: usize,
    ordinal: i16,
    target: Option<C3dBlobRecord<'a>>,
}

/// One successfully resolved group-1 24-byte-entry ordinal and its borrowed
/// 32-byte-entry target from the same group-1 record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dEntry32Reference<'a> {
    group1_record_index: usize,
    entry24_index: usize,
    ordinal_index: usize,
    entry24_field_offset: usize,
    source_offset: usize,
    ordinal: u16,
    target_offset: usize,
    target: &'a [u8; C3D_GROUP1_ENTRY32_LEN],
}

impl<'a> C3dEntry32Reference<'a> {
    #[must_use]
    pub const fn group1_record_index(self) -> usize {
        self.group1_record_index
    }

    #[must_use]
    pub const fn entry24_index(self) -> usize {
        self.entry24_index
    }

    #[must_use]
    pub const fn ordinal_index(self) -> usize {
        self.ordinal_index
    }

    #[must_use]
    pub const fn entry24_field_offset(self) -> usize {
        self.entry24_field_offset
    }

    #[must_use]
    pub const fn source_offset(self) -> usize {
        self.source_offset
    }

    #[must_use]
    pub const fn ordinal(self) -> u16 {
        self.ordinal
    }

    #[must_use]
    pub const fn target_offset(self) -> usize {
        self.target_offset
    }

    #[must_use]
    pub const fn target(self) -> &'a [u8; C3D_GROUP1_ENTRY32_LEN] {
        self.target
    }
}

impl<'a> C3dGroup2Reference<'a> {
    #[must_use]
    pub const fn group1_record_index(self) -> usize {
        self.group1_record_index
    }

    #[must_use]
    pub const fn entry24_index(self) -> usize {
        self.entry24_index
    }

    #[must_use]
    pub const fn source_offset(self) -> usize {
        self.source_offset
    }

    #[must_use]
    pub const fn ordinal(self) -> i16 {
        self.ordinal
    }

    #[must_use]
    pub const fn target(self) -> Option<C3dBlobRecord<'a>> {
        self.target
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dReferenceError {
    Group1OrdinalOutOfBounds {
        group0_record_index: usize,
        value_index: usize,
        source_offset: usize,
        ordinal: u16,
        record_count: usize,
    },
    InvalidNegativeGroup2Ordinal {
        group1_record_index: usize,
        entry24_index: usize,
        source_offset: usize,
        ordinal: i16,
    },
    Group2OrdinalOutOfBounds {
        group1_record_index: usize,
        entry24_index: usize,
        source_offset: usize,
        ordinal: i16,
        record_count: usize,
    },
    Entry32OrdinalOutOfBounds {
        group1_record_index: usize,
        entry24_index: usize,
        ordinal_index: usize,
        entry24_field_offset: usize,
        source_offset: usize,
        ordinal: u16,
        entry32_count: usize,
    },
}

impl fmt::Display for C3dReferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Group1OrdinalOutOfBounds {
                group0_record_index,
                value_index,
                source_offset,
                ordinal,
                record_count,
            } => write!(
                formatter,
                "C3D group-0 record {group0_record_index} value {value_index} at offset {source_offset} has group-1 ordinal {ordinal}, outside {record_count} records"
            ),
            Self::InvalidNegativeGroup2Ordinal {
                group1_record_index,
                entry24_index,
                source_offset,
                ordinal,
            } => write!(
                formatter,
                "C3D group-1 record {group1_record_index} 24-byte entry {entry24_index} at offset {source_offset} has group-2 ordinal {ordinal}, which is negative and is not the -1 sentinel"
            ),
            Self::Group2OrdinalOutOfBounds {
                group1_record_index,
                entry24_index,
                source_offset,
                ordinal,
                record_count,
            } => write!(
                formatter,
                "C3D group-1 record {group1_record_index} 24-byte entry {entry24_index} at offset {source_offset} has group-2 ordinal {ordinal}, outside {record_count} records"
            ),
            Self::Entry32OrdinalOutOfBounds {
                group1_record_index,
                entry24_index,
                ordinal_index,
                entry24_field_offset,
                source_offset,
                ordinal,
                entry32_count,
            } => write!(
                formatter,
                "C3D group-1 record {group1_record_index} 24-byte entry {entry24_index} ordinal {ordinal_index} at entry offset +{entry24_field_offset} (file offset {source_offset}) has 32-byte-entry ordinal {ordinal}, outside {entry32_count} entries"
            ),
        }
    }
}

impl std::error::Error for C3dReferenceError {}

impl<'a> C3dBlobRecord<'a> {
    #[must_use]
    pub const fn offset(self) -> usize {
        self.offset
    }

    #[must_use]
    pub const fn label(self) -> &'a [u8; C3D_BLOB_LABEL_LEN] {
        self.label
    }

    #[must_use]
    pub const fn blob(self) -> &'a [u8] {
        self.blob
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct C3dBlobRecordSpan {
    offset: usize,
    blob_start: usize,
    blob_end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct C3dGroup0RecordSpan {
    offset: usize,
    end: usize,
    values_start: usize,
    values_end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct C3dGroup1RecordSpan {
    offset: usize,
    end: usize,
    entries_32_start: usize,
    entries_32_end: usize,
    entries_24_end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum C3dError {
    PayloadTooLarge {
        actual: usize,
        limit: usize,
    },
    TruncatedHeader {
        actual: usize,
        required: usize,
    },
    InvalidMagic([u8; 4]),
    UnsupportedVersion(u32),
    TooManyGroupRecords {
        group: usize,
        actual: usize,
        limit: usize,
    },
    TruncatedGroupRecordHeader {
        group: usize,
        record: usize,
        offset: usize,
        available: usize,
        required: usize,
    },
    GroupRecordTooShort {
        group: usize,
        record: usize,
        offset: usize,
        declared_record_len: u32,
        minimum: usize,
    },
    GroupRecordOutOfBounds {
        group: usize,
        record: usize,
        offset: usize,
        declared_record_len: u32,
        available_record_bytes: usize,
    },
    GroupRecordLayoutOutOfBounds {
        group: usize,
        record: usize,
        offset: usize,
        declared_record_len: u32,
        required_record_len: usize,
    },
    TooManyBlobRecords {
        actual: usize,
        limit: usize,
    },
    TruncatedBlobRecordHeader {
        record: usize,
        offset: usize,
        available: usize,
        required: usize,
    },
    BlobRecordOutOfBounds {
        record: usize,
        offset: usize,
        declared_blob_len: u32,
        available_blob_bytes: usize,
    },
    PositionOutOfBounds {
        index: usize,
        position: u32,
        payload_len: usize,
    },
}

impl fmt::Display for C3dError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooLarge { actual, limit } => {
                write!(formatter, "C3D payload is {actual} bytes; limit is {limit}")
            }
            Self::TruncatedHeader { actual, required } => write!(
                formatter,
                "C3D header needs {required} bytes but payload contains {actual}"
            ),
            Self::InvalidMagic(actual) => {
                write!(formatter, "invalid C3D magic {actual:02x?}")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported C3D format version {version}")
            }
            Self::TooManyGroupRecords {
                group,
                actual,
                limit,
            } => write!(
                formatter,
                "C3D group {group} declares {actual} records; limit is {limit}"
            ),
            Self::TruncatedGroupRecordHeader {
                group,
                record,
                offset,
                available,
                required,
            } => write!(
                formatter,
                "C3D group {group} record {record} at {offset} needs {required} header bytes but {available} remain"
            ),
            Self::GroupRecordTooShort {
                group,
                record,
                offset,
                declared_record_len,
                minimum,
            } => write!(
                formatter,
                "C3D group {group} record {record} at {offset} declares {declared_record_len} bytes; minimum is {minimum}"
            ),
            Self::GroupRecordOutOfBounds {
                group,
                record,
                offset,
                declared_record_len,
                available_record_bytes,
            } => write!(
                formatter,
                "C3D group {group} record {record} at {offset} declares {declared_record_len} bytes but {available_record_bytes} remain"
            ),
            Self::GroupRecordLayoutOutOfBounds {
                group,
                record,
                offset,
                declared_record_len,
                required_record_len,
            } => write!(
                formatter,
                "C3D group {group} record {record} at {offset} declares {declared_record_len} bytes but its counted layout needs {required_record_len}"
            ),
            Self::TooManyBlobRecords { actual, limit } => {
                write!(
                    formatter,
                    "C3D declares {actual} blob records; limit is {limit}"
                )
            }
            Self::TruncatedBlobRecordHeader {
                record,
                offset,
                available,
                required,
            } => write!(
                formatter,
                "C3D blob record {record} at {offset} needs {required} header bytes but {available} remain"
            ),
            Self::BlobRecordOutOfBounds {
                record,
                offset,
                declared_blob_len,
                available_blob_bytes,
            } => write!(
                formatter,
                "C3D blob record {record} at {offset} declares {declared_blob_len} blob bytes but {available_blob_bytes} remain"
            ),
            Self::PositionOutOfBounds {
                index,
                position,
                payload_len,
            } => write!(
                formatter,
                "C3D position {index} at {position} is outside {payload_len} payload bytes"
            ),
        }
    }
}

impl std::error::Error for C3dError {}

/// Parses the fields that the recovered C3D loader reads.
///
/// Requiring `CM3D` and validating offsets are reconstruction safety policies:
/// the original comparison and pointer arithmetic do not safely reject every
/// malformed payload.
pub fn parse_c3d(input: &[u8], limits: C3dLimits) -> Result<C3dFile, C3dError> {
    if input.len() > limits.max_payload_bytes {
        return Err(C3dError::PayloadTooLarge {
            actual: input.len(),
            limit: limits.max_payload_bytes,
        });
    }
    if input.len() < C3D_HEADER_LEN {
        return Err(C3dError::TruncatedHeader {
            actual: input.len(),
            required: C3D_HEADER_LEN,
        });
    }

    let magic = input[0..4].try_into().expect("fixed-size magic");
    if magic != C3D_MAGIC {
        return Err(C3dError::InvalidMagic(magic));
    }
    let format_version = read_u32(input, 4);
    if format_version != C3D_FORMAT_VERSION {
        return Err(C3dError::UnsupportedVersion(format_version));
    }

    let section_counts = [
        read_u16(input, 12),
        read_u16(input, 14),
        read_u16(input, 16),
    ];
    let section_offsets = [
        read_u32(input, 20),
        read_u32(input, 24),
        read_u32(input, 28),
    ];
    validate_positions(input.len(), section_offsets)?;
    let blob_records = parse_blob_record_spans(
        input,
        usize::from(section_counts[2]),
        usize::try_from(section_offsets[2]).expect("validated C3D position fits usize"),
        limits.max_blob_records,
    )?;
    let group1_records = parse_group1_record_spans(
        input,
        usize::from(section_counts[1]),
        usize::try_from(section_offsets[1]).expect("validated C3D position fits usize"),
        limits.max_group1_records,
    )?;
    let group0_records = parse_group0_record_spans(
        input,
        usize::from(section_counts[0]),
        usize::try_from(section_offsets[0]).expect("validated C3D position fits usize"),
        limits.max_group0_records,
    )?;

    Ok(C3dFile {
        header: C3dHeader {
            format_version,
            unknown_08: input[8..12].try_into().expect("fixed-size field"),
            section_counts,
            unknown_18: input[18..20].try_into().expect("fixed-size field"),
            section_offsets,
        },
        bytes: input.to_vec(),
        group0_records,
        group1_records,
        blob_records,
    })
}

fn parse_group0_record_spans(
    input: &[u8],
    count: usize,
    mut offset: usize,
    limit: usize,
) -> Result<Vec<C3dGroup0RecordSpan>, C3dError> {
    const GROUP: usize = 0;
    const LENGTH_FIELD_END: usize = 4;

    if count > limit {
        return Err(C3dError::TooManyGroupRecords {
            group: GROUP,
            actual: count,
            limit,
        });
    }

    let mut records = Vec::with_capacity(count);
    for record in 0..count {
        let available = input.len().saturating_sub(offset);
        if available < LENGTH_FIELD_END {
            return Err(C3dError::TruncatedGroupRecordHeader {
                group: GROUP,
                record,
                offset,
                available,
                required: LENGTH_FIELD_END,
            });
        }
        let declared_record_len = read_u32(input, offset);
        if declared_record_len < C3D_GROUP0_FIXED_LEN as u32 {
            return Err(C3dError::GroupRecordTooShort {
                group: GROUP,
                record,
                offset,
                declared_record_len,
                minimum: C3D_GROUP0_FIXED_LEN,
            });
        }
        let record_len = usize::try_from(declared_record_len).unwrap_or(usize::MAX);
        if record_len > available {
            return Err(C3dError::GroupRecordOutOfBounds {
                group: GROUP,
                record,
                offset,
                declared_record_len,
                available_record_bytes: available,
            });
        }
        let end = offset + record_len;
        let value_count = usize::from(input[offset + C3D_GROUP0_VALUE_COUNT_OFFSET]);
        let required_record_len = C3D_GROUP0_VALUES_OFFSET + value_count * 2;
        if required_record_len > record_len {
            return Err(C3dError::GroupRecordLayoutOutOfBounds {
                group: GROUP,
                record,
                offset,
                declared_record_len,
                required_record_len,
            });
        }
        let values_start = offset + C3D_GROUP0_VALUES_OFFSET;
        let values_end = values_start + value_count * 2;
        records.push(C3dGroup0RecordSpan {
            offset,
            end,
            values_start,
            values_end,
        });
        offset = end;
    }
    Ok(records)
}

fn parse_group1_record_spans(
    input: &[u8],
    count: usize,
    mut offset: usize,
    limit: usize,
) -> Result<Vec<C3dGroup1RecordSpan>, C3dError> {
    const GROUP: usize = 1;
    const LENGTH_FIELD_END: usize = 16;

    if count > limit {
        return Err(C3dError::TooManyGroupRecords {
            group: GROUP,
            actual: count,
            limit,
        });
    }

    let mut records = Vec::with_capacity(count);
    for record in 0..count {
        let available = input.len().saturating_sub(offset);
        if available < LENGTH_FIELD_END {
            return Err(C3dError::TruncatedGroupRecordHeader {
                group: GROUP,
                record,
                offset,
                available,
                required: LENGTH_FIELD_END,
            });
        }
        let declared_record_len = read_u32(input, offset + 12);
        if declared_record_len < C3D_GROUP1_FIXED_LEN as u32 {
            return Err(C3dError::GroupRecordTooShort {
                group: GROUP,
                record,
                offset,
                declared_record_len,
                minimum: C3D_GROUP1_FIXED_LEN,
            });
        }
        let record_len = usize::try_from(declared_record_len).unwrap_or(usize::MAX);
        if record_len > available {
            return Err(C3dError::GroupRecordOutOfBounds {
                group: GROUP,
                record,
                offset,
                declared_record_len,
                available_record_bytes: available,
            });
        }
        let end = offset + record_len;
        let entry32_count = usize::from(read_u16(input, offset + 16));
        let entry24_count = usize::from(read_u16(input, offset + 18));
        let entries_32_bytes = entry32_count * C3D_GROUP1_ENTRY32_LEN;
        let entries_24_bytes = entry24_count * C3D_GROUP1_ENTRY24_LEN;
        let required_record_len = C3D_GROUP1_FIXED_LEN + entries_32_bytes + entries_24_bytes;
        if required_record_len > record_len {
            return Err(C3dError::GroupRecordLayoutOutOfBounds {
                group: GROUP,
                record,
                offset,
                declared_record_len,
                required_record_len,
            });
        }
        let entries_32_start = offset + C3D_GROUP1_FIXED_LEN;
        let entries_32_end = entries_32_start + entries_32_bytes;
        let entries_24_end = entries_32_end + entries_24_bytes;
        records.push(C3dGroup1RecordSpan {
            offset,
            end,
            entries_32_start,
            entries_32_end,
            entries_24_end,
        });
        offset = end;
    }
    Ok(records)
}

fn parse_blob_record_spans(
    input: &[u8],
    count: usize,
    mut offset: usize,
    limit: usize,
) -> Result<Vec<C3dBlobRecordSpan>, C3dError> {
    if count > limit {
        return Err(C3dError::TooManyBlobRecords {
            actual: count,
            limit,
        });
    }

    let mut records = Vec::with_capacity(count);
    for record in 0..count {
        let available = input.len().saturating_sub(offset);
        if available < C3D_BLOB_RECORD_HEADER_LEN {
            return Err(C3dError::TruncatedBlobRecordHeader {
                record,
                offset,
                available,
                required: C3D_BLOB_RECORD_HEADER_LEN,
            });
        }
        let blob_start = offset + C3D_BLOB_RECORD_HEADER_LEN;
        let declared_blob_len = read_u32(input, offset + C3D_BLOB_LABEL_LEN);
        let available_blob_bytes = input.len() - blob_start;
        let blob_len = declared_blob_len as usize;
        if blob_len > available_blob_bytes {
            return Err(C3dError::BlobRecordOutOfBounds {
                record,
                offset,
                declared_blob_len,
                available_blob_bytes,
            });
        }
        let blob_end = blob_start + blob_len;
        records.push(C3dBlobRecordSpan {
            offset,
            blob_start,
            blob_end,
        });
        offset = blob_end;
    }
    Ok(records)
}

fn validate_positions(
    payload_len: usize,
    positions: [u32; C3D_SECTION_COUNT],
) -> Result<(), C3dError> {
    for (index, position) in positions.into_iter().enumerate() {
        let position_usize = usize::try_from(position).unwrap_or(usize::MAX);
        if position_usize > payload_len {
            return Err(C3dError::PositionOutOfBounds {
                index,
                position,
                payload_len,
            });
        }
    }
    Ok(())
}

fn read_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        input[offset..offset + 2]
            .try_into()
            .expect("validated header"),
    )
}

fn read_i16(input: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(
        input[offset..offset + 2]
            .try_into()
            .expect("validated record"),
    )
}

fn read_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        input[offset..offset + 4]
            .try_into()
            .expect("validated header"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: C3dLimits = C3dLimits {
        max_payload_bytes: 512,
        max_group0_records: 8,
        max_group1_records: 8,
        max_blob_records: 8,
    };

    #[test]
    fn parses_observed_fields_and_preserves_every_byte() {
        let mut input = fixture([0, 0, 0], [32, 40, 48], 64);
        input[8..12].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
        input[18..20].copy_from_slice(&[0x12, 0x34]);
        input[63] = 0xfe;

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        assert_eq!(
            parsed.header(),
            &C3dHeader {
                format_version: C3D_FORMAT_VERSION,
                unknown_08: [0xaa, 0xbb, 0xcc, 0xdd],
                section_counts: [0, 0, 0],
                unknown_18: [0x12, 0x34],
                section_offsets: [32, 40, 48],
            }
        );
        assert_eq!(parsed.bytes(), input);
        assert_eq!(parsed.into_bytes(), input);
    }

    #[test]
    fn accepts_the_exact_payload_limit_and_rejects_one_byte_more() {
        let limits = C3dLimits {
            max_payload_bytes: 64,
            ..LIMITS
        };
        assert!(parse_c3d(&fixture([0, 0, 0], [32, 40, 48], 64), limits).is_ok());
        assert_eq!(
            parse_c3d(&fixture([0, 0, 0], [32, 40, 48], 65), limits),
            Err(C3dError::PayloadTooLarge {
                actual: 65,
                limit: 64,
            })
        );
    }

    #[test]
    fn rejects_every_truncated_header_length() {
        let complete = fixture([0, 0, 0], [0, 0, 0], C3D_HEADER_LEN);
        for length in 0..C3D_HEADER_LEN {
            assert_eq!(
                parse_c3d(&complete[..length], LIMITS),
                Err(C3dError::TruncatedHeader {
                    actual: length,
                    required: C3D_HEADER_LEN,
                })
            );
        }
    }

    #[test]
    fn rejects_each_bad_magic_byte() {
        for index in 0..C3D_MAGIC.len() {
            let mut input = fixture([0, 0, 0], [0, 0, 0], C3D_HEADER_LEN);
            input[index] ^= 0xff;
            let actual = input[0..4].try_into().unwrap();
            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::InvalidMagic(actual))
            );
        }
    }

    #[test]
    fn accepts_only_the_observed_little_endian_version() {
        for version in [99u32, 101, 0x6400_0000] {
            let mut input = fixture([0, 0, 0], [0, 0, 0], C3D_HEADER_LEN);
            input[4..8].copy_from_slice(&version.to_le_bytes());
            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::UnsupportedVersion(version))
            );
        }
    }

    #[test]
    fn accepts_unordered_or_header_internal_positions_without_inventing_semantics() {
        let parsed = parse_c3d(&fixture([0, 0, 0], [31, 0, 12], 64), LIMITS).unwrap();
        assert_eq!(parsed.header().section_counts, [0, 0, 0]);
        assert_eq!(parsed.header().section_offsets, [31, 0, 12]);
    }

    #[test]
    fn validates_each_section_position_independently() {
        for index in 0..C3D_SECTION_COUNT {
            let counts = [0; C3D_SECTION_COUNT];
            let mut offsets = [0; C3D_SECTION_COUNT];
            offsets[index] = 64;
            assert!(parse_c3d(&fixture(counts, offsets, 64), LIMITS).is_ok());

            offsets[index] = 65;
            assert_eq!(
                parse_c3d(&fixture(counts, offsets, 64), LIMITS),
                Err(C3dError::PositionOutOfBounds {
                    index,
                    position: 65,
                    payload_len: 64,
                })
            );

            offsets[index] = u32::MAX;
            assert_eq!(
                parse_c3d(&fixture(counts, offsets, 64), LIMITS),
                Err(C3dError::PositionOutOfBounds {
                    index,
                    position: u32::MAX,
                    payload_len: 64,
                })
            );
        }
    }

    #[test]
    fn positions_accept_zero_or_payload_end_but_not_beyond() {
        assert!(parse_c3d(&fixture([0, 0, 0], [0, 64, 32], 64), LIMITS).is_ok());
        assert_eq!(
            parse_c3d(&fixture([0, 0, 0], [0, 65, 32], 64), LIMITS),
            Err(C3dError::PositionOutOfBounds {
                index: 1,
                position: 65,
                payload_len: 64,
            })
        );
    }

    #[test]
    fn parses_all_three_groups_without_assigning_payload_semantics() {
        let mut input = fixture([2, 2, 1], [32, 196, 351], 373);
        let group0_second = write_group0_record(&mut input, 32, &[0x1234], &[0xa1, 0xa2]);
        assert_eq!(group0_second, 116);
        let group1_start = write_group0_record(&mut input, group0_second, &[], &[]);
        assert_eq!(group1_start, 196);

        let first_prefix = *b"GROUP1-A\0XYZ";
        let group1_second = write_group1_record(&mut input, group1_start, first_prefix, 1, 1, 3);
        assert_eq!(group1_second, 303);
        let blob_start = write_group1_record(&mut input, group1_second, *b"GROUP1-B\0XYZ", 0, 0, 0);
        assert_eq!(blob_start, 351);
        let blob_end = write_blob_record(&mut input, blob_start, *b"BLOB-GROUP-2", &[7, 8, 9, 10]);
        input[blob_end..].copy_from_slice(&[0xe1, 0xe2]);

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        let group0 = parsed.group0_records().collect::<Vec<_>>();
        assert_eq!(group0.len(), 2);
        assert_eq!(group0[0].offset(), 32);
        assert_eq!(group0[0].declared_len(), 84);
        assert_eq!(group0[0].value_count(), 1);
        assert_eq!(group0[0].values().collect::<Vec<_>>(), [0x1234]);
        assert_eq!(group0[0].trailing_bytes(), [0xa1, 0xa2]);
        assert_eq!(group0[1].offset(), 116);
        assert_eq!(group0[1].bytes().len(), C3D_GROUP0_FIXED_LEN);

        let group1 = parsed.group1_records().collect::<Vec<_>>();
        assert_eq!(group1.len(), 2);
        assert_eq!(group1[0].offset(), 196);
        assert_eq!(group1[0].prefix(), &first_prefix);
        assert_eq!(group1[0].entry32_count(), 1);
        assert_eq!(group1[0].entry24_count(), 1);
        assert_eq!(group1[0].unknown_bytes()[0], 0x80);
        assert_eq!(group1[0].entries_32().next().unwrap()[0], 0x20);
        assert_eq!(group1[0].entries_24().next().unwrap()[0], 0x40);
        assert_eq!(group1[0].trailing_bytes(), [0xd0, 0xd1, 0xd2]);
        assert_eq!(group1[1].declared_len(), 48);
        assert_eq!(group1[1].entries_32().len(), 0);
        assert_eq!(group1[1].entries_24().len(), 0);
        assert!(group1[1].trailing_bytes().is_empty());

        let blob = parsed.blob_records().next().unwrap();
        assert_eq!(blob.offset(), 351);
        assert_eq!(blob.blob(), [7, 8, 9, 10]);
        assert_eq!(parsed.bytes(), input);
    }

    #[test]
    fn resolves_group0_values_as_borrowed_group1_records_and_continues_after_error() {
        let mut input = fixture([2, 2, 0], [32, 200, 296], 296);
        let second_group0 = write_group0_record(&mut input, 32, &[1, 2, 0], &[]);
        assert_eq!(second_group0, 118);
        assert_eq!(
            write_group0_record(&mut input, second_group0, &[1], &[]),
            200
        );
        let second_group1 = write_group1_record(&mut input, 200, *b"GROUP1-ZERO!", 0, 0, 0);
        assert_eq!(second_group1, 248);
        assert_eq!(
            write_group1_record(&mut input, second_group1, *b"GROUP1-ONE!!", 0, 0, 0,),
            296
        );

        // Structural admission remains independent from ordinal validity.
        let parsed = parse_c3d(&input, LIMITS).unwrap();
        let mut references = parsed.resolve_group1_references();

        let first = references.next().unwrap().unwrap();
        assert_eq!(first.group0_record_index(), 0);
        assert_eq!(first.value_index(), 0);
        assert_eq!(first.source_offset(), 112);
        assert_eq!(first.ordinal(), 1);
        assert!(std::ptr::eq(
            first.target().bytes(),
            &parsed.bytes()[248..296]
        ));

        assert_eq!(
            references.next().unwrap(),
            Err(C3dReferenceError::Group1OrdinalOutOfBounds {
                group0_record_index: 0,
                value_index: 1,
                source_offset: 114,
                ordinal: 2,
                record_count: 2,
            })
        );

        let after_error = references.next().unwrap().unwrap();
        assert_eq!(after_error.group0_record_index(), 0);
        assert_eq!(after_error.value_index(), 2);
        assert_eq!(after_error.source_offset(), 116);
        assert_eq!(after_error.ordinal(), 0);
        assert!(std::ptr::eq(
            after_error.target().bytes(),
            &parsed.bytes()[200..248]
        ));

        let next_record = references.next().unwrap().unwrap();
        assert_eq!(next_record.group0_record_index(), 1);
        assert_eq!(next_record.value_index(), 0);
        assert_eq!(next_record.source_offset(), 198);
        assert_eq!(next_record.ordinal(), 1);
        assert!(references.next().is_none());
    }

    #[test]
    fn resolves_group1_entry_offset_8_as_signed_little_endian_group2_ordinal() {
        let mut input = fixture([0, 1, 2], [32, 32, 200], 235);
        assert_eq!(
            write_group1_record(&mut input, 32, *b"GROUP1-REFS!", 0, 5, 0),
            200
        );
        let entries_start = 32 + C3D_GROUP1_FIXED_LEN;
        for (index, ordinal) in [1_i16, -1, -2, 2, 0].into_iter().enumerate() {
            let offset = entries_start + index * C3D_GROUP1_ENTRY24_LEN + 8;
            input[offset..offset + 2].copy_from_slice(&ordinal.to_le_bytes());
        }
        let second_blob = write_blob_record(&mut input, 200, *b"GROUP2-ZERO!", &[0xa0]);
        assert_eq!(second_blob, 217);
        assert_eq!(
            write_blob_record(&mut input, second_blob, *b"GROUP2-ONE!!", &[0xb0, 0xb1]),
            235
        );

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        let raw_first = parsed
            .group1_records()
            .next()
            .unwrap()
            .entries_24()
            .next()
            .unwrap();
        assert_eq!(&raw_first[8..10], &[1, 0]);

        let mut references = parsed.resolve_group2_references();
        let first = references.next().unwrap().unwrap();
        assert_eq!(first.group1_record_index(), 0);
        assert_eq!(first.entry24_index(), 0);
        assert_eq!(first.source_offset(), 88);
        assert_eq!(first.ordinal(), 1);
        let target = first.target().unwrap();
        assert!(std::ptr::eq(target.blob(), &parsed.bytes()[233..235]));

        let sentinel = references.next().unwrap().unwrap();
        assert_eq!(sentinel.entry24_index(), 1);
        assert_eq!(sentinel.source_offset(), 112);
        assert_eq!(sentinel.ordinal(), -1);
        assert_eq!(sentinel.target(), None);

        assert_eq!(
            references.next().unwrap(),
            Err(C3dReferenceError::InvalidNegativeGroup2Ordinal {
                group1_record_index: 0,
                entry24_index: 2,
                source_offset: 136,
                ordinal: -2,
            })
        );
        assert_eq!(
            references.next().unwrap(),
            Err(C3dReferenceError::Group2OrdinalOutOfBounds {
                group1_record_index: 0,
                entry24_index: 3,
                source_offset: 160,
                ordinal: 2,
                record_count: 2,
            })
        );

        let after_errors = references.next().unwrap().unwrap();
        assert_eq!(after_errors.entry24_index(), 4);
        assert_eq!(after_errors.source_offset(), 184);
        assert_eq!(after_errors.ordinal(), 0);
        assert!(std::ptr::eq(
            after_errors.target().unwrap().blob(),
            &parsed.bytes()[216..217]
        ));
        assert!(references.next().is_none());
    }

    #[test]
    fn resolves_entry24_unsigned_ordinals_within_their_own_group1_record() {
        let mut input = fixture([0, 1, 0], [32, 64, 256], 256);
        assert_eq!(
            write_group1_record(&mut input, 64, *b"GROUP1-ATTR!", 3, 2, 0),
            256
        );
        let entries32_start = 64 + C3D_GROUP1_FIXED_LEN;
        for index in 0..3 {
            input[entries32_start + index * C3D_GROUP1_ENTRY32_LEN] = 0xa0 + index as u8;
        }
        let entries24_start = entries32_start + 3 * C3D_GROUP1_ENTRY32_LEN;
        for (entry_index, ordinals) in [[2_u16, 1, 0], [3, u16::MAX, 1]].into_iter().enumerate() {
            for (ordinal_index, ordinal) in ordinals.into_iter().enumerate() {
                let offset = entries24_start
                    + entry_index * C3D_GROUP1_ENTRY24_LEN
                    + C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[ordinal_index];
                input[offset..offset + 2].copy_from_slice(&ordinal.to_le_bytes());
            }
        }

        // Structural admission and byte preservation remain independent from
        // the two intentionally invalid semantic references.
        let parsed = parse_c3d(&input, LIMITS).unwrap();
        assert_eq!(parsed.bytes(), input);
        let mut references = parsed.resolve_entry32_references();

        for (ordinal_index, (ordinal, source_offset, target_offset)) in
            [(2, 210, 176), (1, 212, 144), (0, 214, 112)]
                .into_iter()
                .enumerate()
        {
            let reference = references.next().unwrap().unwrap();
            assert_eq!(reference.group1_record_index(), 0);
            assert_eq!(reference.entry24_index(), 0);
            assert_eq!(reference.ordinal_index(), ordinal_index);
            assert_eq!(
                reference.entry24_field_offset(),
                C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[ordinal_index]
            );
            assert_eq!(reference.source_offset(), source_offset);
            assert_eq!(reference.ordinal(), ordinal);
            assert_eq!(reference.target_offset(), target_offset);
            assert_eq!(reference.target()[0], 0xa0 + ordinal as u8);
            assert!(std::ptr::eq(
                reference.target().as_ptr(),
                parsed.bytes()[target_offset..target_offset + C3D_GROUP1_ENTRY32_LEN].as_ptr()
            ));
        }

        assert_eq!(
            references.next().unwrap(),
            Err(C3dReferenceError::Entry32OrdinalOutOfBounds {
                group1_record_index: 0,
                entry24_index: 1,
                ordinal_index: 0,
                entry24_field_offset: 2,
                source_offset: 234,
                ordinal: 3,
                entry32_count: 3,
            })
        );
        assert_eq!(
            references.next().unwrap(),
            Err(C3dReferenceError::Entry32OrdinalOutOfBounds {
                group1_record_index: 0,
                entry24_index: 1,
                ordinal_index: 1,
                entry24_field_offset: 4,
                source_offset: 236,
                ordinal: u16::MAX,
                entry32_count: 3,
            })
        );
        let after_errors = references.next().unwrap().unwrap();
        assert_eq!(after_errors.entry24_index(), 1);
        assert_eq!(after_errors.ordinal_index(), 2);
        assert_eq!(after_errors.source_offset(), 238);
        assert_eq!(after_errors.ordinal(), 1);
        assert_eq!(after_errors.target_offset(), 144);
        assert!(references.next().is_none());
    }

    #[test]
    fn reports_relocated_entry32_references_when_the_target_array_is_empty() {
        let mut input = fixture([0, 1, 0], [32, 200, 272], 300);
        assert_eq!(
            write_group1_record(&mut input, 200, *b"GROUP1-EMPTY", 0, 1, 0),
            272
        );
        for (ordinal_index, ordinal) in [0_u16, 256, u16::MAX].into_iter().enumerate() {
            let source_offset = 248 + C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS[ordinal_index];
            input[source_offset..source_offset + 2].copy_from_slice(&ordinal.to_le_bytes());
        }

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        let errors = parsed
            .resolve_entry32_references()
            .collect::<Result<Vec<_>, _>>()
            .unwrap_err();
        assert_eq!(
            errors,
            C3dReferenceError::Entry32OrdinalOutOfBounds {
                group1_record_index: 0,
                entry24_index: 0,
                ordinal_index: 0,
                entry24_field_offset: 2,
                source_offset: 250,
                ordinal: 0,
                entry32_count: 0,
            }
        );

        let all = parsed
            .resolve_entry32_references()
            .collect::<Vec<Result<_, _>>>();
        assert_eq!(all.len(), 3);
        assert!(matches!(
            all[1],
            Err(C3dReferenceError::Entry32OrdinalOutOfBounds {
                ordinal_index: 1,
                source_offset: 252,
                ordinal: 256,
                entry32_count: 0,
                ..
            })
        ));
        assert!(matches!(
            all[2],
            Err(C3dReferenceError::Entry32OrdinalOutOfBounds {
                ordinal_index: 2,
                source_offset: 254,
                ordinal: u16::MAX,
                entry32_count: 0,
                ..
            })
        ));
    }

    #[test]
    fn entry32_ordinals_are_scoped_to_each_originating_group1_record() {
        let mut input = fixture([0, 2, 0], [32, 32, 240], 240);
        let second_record = write_group1_record(&mut input, 32, *b"GROUP1-FIRST", 1, 1, 0);
        assert_eq!(second_record, 136);
        assert_eq!(
            write_group1_record(&mut input, second_record, *b"GROUP1-SECON", 1, 1, 0),
            240
        );
        input[80] = 0xa1;
        input[184] = 0xb2;
        for entries24_start in [112, 216] {
            for ordinal_offset in C3D_ENTRY24_ENTRY32_ORDINAL_OFFSETS {
                input[entries24_start + ordinal_offset..entries24_start + ordinal_offset + 2]
                    .copy_from_slice(&0_u16.to_le_bytes());
            }
        }

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        let references = parsed
            .resolve_entry32_references()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(references.len(), 6);
        assert!(
            references[..3]
                .iter()
                .all(|reference| reference.group1_record_index() == 0
                    && reference.target_offset() == 80
                    && reference.target()[0] == 0xa1)
        );
        assert!(
            references[3..]
                .iter()
                .all(|reference| reference.group1_record_index() == 1
                    && reference.target_offset() == 184
                    && reference.target()[0] == 0xb2)
        );
    }

    #[test]
    fn reports_relocated_extreme_and_empty_target_references_without_rejecting_the_file() {
        let mut input = fixture([1, 1, 0], [64, 200, 400], 420);
        assert_eq!(
            write_group0_record(&mut input, 64, &[u16::MAX, 0], &[]),
            148
        );
        assert_eq!(
            write_group1_record(&mut input, 200, *b"GROUP1-EDGE!", 0, 3, 0),
            320
        );
        let entries_start = 200 + C3D_GROUP1_FIXED_LEN;
        for (index, ordinal) in [i16::MIN, 0, -1].into_iter().enumerate() {
            let source_offset = entries_start + index * C3D_GROUP1_ENTRY24_LEN + 8;
            input[source_offset..source_offset + 2].copy_from_slice(&ordinal.to_le_bytes());
        }

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        assert_eq!(parsed.bytes(), input);

        let mut group1_references = parsed.resolve_group1_references();
        assert_eq!(
            group1_references.next().unwrap(),
            Err(C3dReferenceError::Group1OrdinalOutOfBounds {
                group0_record_index: 0,
                value_index: 0,
                source_offset: 144,
                ordinal: u16::MAX,
                record_count: 1,
            })
        );
        let later_group1 = group1_references.next().unwrap().unwrap();
        assert_eq!(later_group1.source_offset(), 146);
        assert_eq!(later_group1.ordinal(), 0);
        assert!(group1_references.next().is_none());

        let mut group2_references = parsed.resolve_group2_references();
        assert_eq!(
            group2_references.next().unwrap(),
            Err(C3dReferenceError::InvalidNegativeGroup2Ordinal {
                group1_record_index: 0,
                entry24_index: 0,
                source_offset: 256,
                ordinal: i16::MIN,
            })
        );
        assert_eq!(
            group2_references.next().unwrap(),
            Err(C3dReferenceError::Group2OrdinalOutOfBounds {
                group1_record_index: 0,
                entry24_index: 1,
                source_offset: 280,
                ordinal: 0,
                record_count: 0,
            })
        );
        let sentinel_after_errors = group2_references.next().unwrap().unwrap();
        assert_eq!(sentinel_after_errors.source_offset(), 304);
        assert_eq!(sentinel_after_errors.ordinal(), -1);
        assert_eq!(sentinel_after_errors.target(), None);
        assert!(group2_references.next().is_none());
    }

    #[test]
    fn rejects_every_truncated_group_record_length_field() {
        for available in 0..4 {
            let offset = 40;
            let input = fixture([1, 0, 0], [offset as u32, 0, 0], offset + available);
            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::TruncatedGroupRecordHeader {
                    group: 0,
                    record: 0,
                    offset,
                    available,
                    required: 4,
                })
            );
        }
        for available in 0..16 {
            let offset = 40;
            let input = fixture([0, 1, 0], [0, offset as u32, 0], offset + available);
            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::TruncatedGroupRecordHeader {
                    group: 1,
                    record: 0,
                    offset,
                    available,
                    required: 16,
                })
            );
        }
    }

    #[test]
    fn rejects_every_declared_group_record_length_below_its_fixed_layout() {
        for declared_record_len in 0..C3D_GROUP0_FIXED_LEN as u32 {
            let mut input = fixture([1, 0, 0], [40, 0, 0], 128);
            input[40..44].copy_from_slice(&declared_record_len.to_le_bytes());
            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::GroupRecordTooShort {
                    group: 0,
                    record: 0,
                    offset: 40,
                    declared_record_len,
                    minimum: C3D_GROUP0_FIXED_LEN,
                })
            );
        }
        for declared_record_len in 0..C3D_GROUP1_FIXED_LEN as u32 {
            let mut input = fixture([0, 1, 0], [0, 40, 0], 96);
            input[52..56].copy_from_slice(&declared_record_len.to_le_bytes());
            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::GroupRecordTooShort {
                    group: 1,
                    record: 0,
                    offset: 40,
                    declared_record_len,
                    minimum: C3D_GROUP1_FIXED_LEN,
                })
            );
        }
    }

    #[test]
    fn rejects_every_physically_truncated_group_fixed_prefix_after_its_length_field() {
        for available_record_bytes in 4..C3D_GROUP0_FIXED_LEN {
            let offset = 40;
            let mut input = fixture(
                [1, 0, 0],
                [offset as u32, 0, 0],
                offset + available_record_bytes,
            );
            input[offset..offset + 4].copy_from_slice(&(C3D_GROUP0_FIXED_LEN as u32).to_le_bytes());
            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::GroupRecordOutOfBounds {
                    group: 0,
                    record: 0,
                    offset,
                    declared_record_len: C3D_GROUP0_FIXED_LEN as u32,
                    available_record_bytes,
                })
            );
        }

        for available_record_bytes in 16..C3D_GROUP1_FIXED_LEN {
            let offset = 40;
            let mut input = fixture(
                [0, 1, 0],
                [0, offset as u32, 0],
                offset + available_record_bytes,
            );
            input[offset + 12..offset + 16]
                .copy_from_slice(&(C3D_GROUP1_FIXED_LEN as u32).to_le_bytes());
            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::GroupRecordOutOfBounds {
                    group: 1,
                    record: 0,
                    offset,
                    declared_record_len: C3D_GROUP1_FIXED_LEN as u32,
                    available_record_bytes,
                })
            );
        }
    }

    #[test]
    fn rejects_group_records_beyond_payload_and_counted_layout_beyond_record() {
        for (group, length_offset, minimum) in [
            (0usize, 40usize, C3D_GROUP0_FIXED_LEN),
            (1, 52, C3D_GROUP1_FIXED_LEN),
        ] {
            for declared_record_len in [minimum as u32 + 1, u32::MAX] {
                let counts = if group == 0 { [1, 0, 0] } else { [0, 1, 0] };
                let offsets = if group == 0 { [40, 0, 0] } else { [0, 40, 0] };
                let mut input = fixture(counts, offsets, 40 + minimum);
                input[length_offset..length_offset + 4]
                    .copy_from_slice(&declared_record_len.to_le_bytes());
                assert_eq!(
                    parse_c3d(&input, LIMITS),
                    Err(C3dError::GroupRecordOutOfBounds {
                        group,
                        record: 0,
                        offset: 40,
                        declared_record_len,
                        available_record_bytes: minimum,
                    })
                );
            }
        }

        let mut group0 = fixture([1, 0, 0], [40, 0, 0], 120);
        group0[40..44].copy_from_slice(&80u32.to_le_bytes());
        group0[116] = 1;
        assert_eq!(
            parse_c3d(&group0, LIMITS),
            Err(C3dError::GroupRecordLayoutOutOfBounds {
                group: 0,
                record: 0,
                offset: 40,
                declared_record_len: 80,
                required_record_len: 82,
            })
        );

        let mut group1 = fixture([0, 1, 0], [0, 40, 0], 88);
        group1[52..56].copy_from_slice(&48u32.to_le_bytes());
        group1[56..58].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            parse_c3d(&group1, LIMITS),
            Err(C3dError::GroupRecordLayoutOutOfBounds {
                group: 1,
                record: 0,
                offset: 40,
                declared_record_len: 48,
                required_record_len: 80,
            })
        );
    }

    #[test]
    fn group_count_limits_precede_record_reads_and_accept_exact_limits() {
        let input = fixture([2, 2, 0], [128, 128, 0], 128);
        assert_eq!(
            parse_c3d(
                &input,
                C3dLimits {
                    max_group1_records: 1,
                    ..LIMITS
                }
            ),
            Err(C3dError::TooManyGroupRecords {
                group: 1,
                actual: 2,
                limit: 1,
            })
        );
        let group0_only = fixture([2, 0, 0], [128, 0, 0], 128);
        assert_eq!(
            parse_c3d(
                &group0_only,
                C3dLimits {
                    max_group0_records: 1,
                    ..LIMITS
                }
            ),
            Err(C3dError::TooManyGroupRecords {
                group: 0,
                actual: 2,
                limit: 1,
            })
        );

        let mut exact = fixture([1, 1, 0], [32, 112, 0], 160);
        write_group0_record(&mut exact, 32, &[], &[]);
        write_group1_record(&mut exact, 112, *b"EXACT-LIMIT!", 0, 0, 0);
        assert!(
            parse_c3d(
                &exact,
                C3dLimits {
                    max_group0_records: 1,
                    max_group1_records: 1,
                    ..LIMITS
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn reports_exact_second_group_record_truncation() {
        let mut group0 = fixture([2, 0, 0], [40, 0, 0], 123);
        write_group0_record(&mut group0, 40, &[], &[]);
        assert_eq!(
            parse_c3d(&group0, LIMITS),
            Err(C3dError::TruncatedGroupRecordHeader {
                group: 0,
                record: 1,
                offset: 120,
                available: 3,
                required: 4,
            })
        );

        let mut group1 = fixture([0, 2, 0], [0, 40, 0], 103);
        write_group1_record(&mut group1, 40, *b"FIRST-REC!!!", 0, 0, 0);
        assert_eq!(
            parse_c3d(&group1, LIMITS),
            Err(C3dError::TruncatedGroupRecordHeader {
                group: 1,
                record: 1,
                offset: 88,
                available: 15,
                required: 16,
            })
        );
    }

    #[test]
    fn extreme_nested_counts_return_layout_errors_without_allocation() {
        let mut group0 = fixture([1, 0, 0], [40, 0, 0], 120);
        group0[40..44].copy_from_slice(&80u32.to_le_bytes());
        group0[116] = u8::MAX;
        assert_eq!(
            parse_c3d(&group0, LIMITS),
            Err(C3dError::GroupRecordLayoutOutOfBounds {
                group: 0,
                record: 0,
                offset: 40,
                declared_record_len: 80,
                required_record_len: C3D_GROUP0_VALUES_OFFSET + usize::from(u8::MAX) * 2,
            })
        );

        let mut group1 = fixture([0, 1, 0], [0, 40, 0], 88);
        group1[52..56].copy_from_slice(&48u32.to_le_bytes());
        group1[56..58].copy_from_slice(&u16::MAX.to_le_bytes());
        group1[58..60].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(
            parse_c3d(&group1, LIMITS),
            Err(C3dError::GroupRecordLayoutOutOfBounds {
                group: 1,
                record: 0,
                offset: 40,
                declared_record_len: 48,
                required_record_len: C3D_GROUP1_FIXED_LEN
                    + usize::from(u16::MAX) * C3D_GROUP1_ENTRY32_LEN
                    + usize::from(u16::MAX) * C3D_GROUP1_ENTRY24_LEN,
            })
        );
    }

    #[test]
    fn blob_errors_precede_group_one_and_group_zero_errors() {
        let input = fixture([1, 1, 1], [32, 32, 32], 32);
        assert_eq!(
            parse_c3d(&input, LIMITS),
            Err(C3dError::TruncatedBlobRecordHeader {
                record: 0,
                offset: 32,
                available: 0,
                required: C3D_BLOB_RECORD_HEADER_LEN,
            })
        );
    }

    #[test]
    fn parses_counted_blob_records_and_preserves_labels_blobs_and_tail() {
        let mut input = fixture([0, 0, 2], [7, 11, 40], 80);
        let first_label = *b"FIRST\0\xaa\xbb\xcc\xdd\xee\xff";
        let second_label = *b"SECOND-LABEL";
        let second_offset = write_blob_record(&mut input, 40, first_label, &[0x10, 0x20, 0x30]);
        let consumed_end = write_blob_record(&mut input, second_offset, second_label, &[]);
        input[consumed_end..].copy_from_slice(&[0xd1, 0xd2, 0xd3, 0xd4, 0xd5]);

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        let records = parsed.blob_records().collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].offset(), 40);
        assert_eq!(records[0].label(), &first_label);
        assert_eq!(records[0].blob(), [0x10, 0x20, 0x30]);
        assert_eq!(records[1].offset(), second_offset);
        assert_eq!(records[1].label(), &second_label);
        assert_eq!(records[1].blob(), []);
        assert_eq!(parsed.bytes(), input);
    }

    #[test]
    fn blob_iterator_uses_only_the_third_count_and_position() {
        let mut input = fixture([0, 0, 1], [59, 58, 40], 60);
        let label = *b"THIRD-ONLY!!";
        write_blob_record(&mut input, 40, label, &[1, 2, 3, 4]);

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        let record = parsed.blob_records().next().unwrap();
        assert_eq!(record.offset(), 40);
        assert_eq!(record.label(), &label);
        assert_eq!(record.blob(), [1, 2, 3, 4]);
    }

    #[test]
    fn zero_blob_count_does_not_read_at_its_position() {
        let input = fixture([0, 0, 0], [0, 0, 64], 64);
        let parsed = parse_c3d(&input, LIMITS).unwrap();
        assert_eq!(parsed.blob_records().len(), 0);
    }

    #[test]
    fn rejects_every_truncated_blob_record_header_length() {
        for available in 0..C3D_BLOB_RECORD_HEADER_LEN {
            let offset = 40;
            let input = fixture([0, 0, 1], [0, 0, offset as u32], offset + available);
            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::TruncatedBlobRecordHeader {
                    record: 0,
                    offset,
                    available,
                    required: C3D_BLOB_RECORD_HEADER_LEN,
                })
            );
        }
    }

    #[test]
    fn rejects_blob_lengths_beyond_the_remaining_payload() {
        for declared_blob_len in [1, u32::MAX] {
            let mut input = fixture([0, 0, 1], [0, 0, 48], 64);
            input[60..64].copy_from_slice(&declared_blob_len.to_le_bytes());
            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::BlobRecordOutOfBounds {
                    record: 0,
                    offset: 48,
                    declared_blob_len,
                    available_blob_bytes: 0,
                })
            );
        }
    }

    #[test]
    fn accepts_a_zero_length_blob_record_at_the_exact_payload_end() {
        let mut input = fixture([0, 0, 1], [0, 0, 40], 56);
        let label = *b"EMPTY-BLOB!!";
        assert_eq!(write_blob_record(&mut input, 40, label, &[]), input.len());

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        let record = parsed.blob_records().next().unwrap();
        assert_eq!(record.label(), &label);
        assert!(record.blob().is_empty());
    }

    #[test]
    fn rejects_blobs_one_byte_short_of_their_declared_length() {
        for declared_blob_len in 1..=8u32 {
            let offset = 40;
            let available_blob_bytes = declared_blob_len as usize - 1;
            let mut input = fixture(
                [0, 0, 1],
                [0, 0, offset as u32],
                offset + C3D_BLOB_RECORD_HEADER_LEN + available_blob_bytes,
            );
            input[offset + C3D_BLOB_LABEL_LEN..offset + C3D_BLOB_RECORD_HEADER_LEN]
                .copy_from_slice(&declared_blob_len.to_le_bytes());

            assert_eq!(
                parse_c3d(&input, LIMITS),
                Err(C3dError::BlobRecordOutOfBounds {
                    record: 0,
                    offset,
                    declared_blob_len,
                    available_blob_bytes,
                })
            );
        }
    }

    #[test]
    fn rejects_the_blob_record_after_an_exact_first_record() {
        let mut input = fixture([0, 0, 2], [0, 0, 40], 60);
        let second_offset = write_blob_record(&mut input, 40, *b"ONE\0RAWTAIL!", &[1, 2, 3, 4]);
        assert_eq!(second_offset, input.len());
        assert_eq!(
            parse_c3d(&input, LIMITS),
            Err(C3dError::TruncatedBlobRecordHeader {
                record: 1,
                offset: 60,
                available: 0,
                required: C3D_BLOB_RECORD_HEADER_LEN,
            })
        );
    }

    #[test]
    fn enforces_blob_record_limit_before_reading_records() {
        let input = fixture([0, 0, 2], [0, 0, 64], 64);
        assert_eq!(
            parse_c3d(
                &input,
                C3dLimits {
                    max_blob_records: 1,
                    ..LIMITS
                }
            ),
            Err(C3dError::TooManyBlobRecords {
                actual: 2,
                limit: 1,
            })
        );
    }

    #[test]
    fn accepts_the_exact_blob_record_limit_and_zero_records_at_zero_limit() {
        let mut one = fixture([0, 0, 1], [0, 0, 40], 56);
        write_blob_record(&mut one, 40, *b"EXACT-LIMIT!", &[]);
        assert!(
            parse_c3d(
                &one,
                C3dLimits {
                    max_blob_records: 1,
                    ..LIMITS
                }
            )
            .is_ok()
        );

        let zero = fixture([0, 0, 0], [0, 0, 32], 32);
        assert!(
            parse_c3d(
                &zero,
                C3dLimits {
                    max_blob_records: 0,
                    ..LIMITS
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn maximum_count_on_a_tiny_payload_returns_a_typed_error() {
        let input = fixture([0, 0, u16::MAX], [0, 0, 32], 32);
        assert_eq!(
            parse_c3d(
                &input,
                C3dLimits {
                    max_blob_records: u16::MAX as usize,
                    ..LIMITS
                }
            ),
            Err(C3dError::TruncatedBlobRecordHeader {
                record: 0,
                offset: 32,
                available: 0,
                required: C3D_BLOB_RECORD_HEADER_LEN,
            })
        );
    }

    #[test]
    fn relocating_only_the_blob_position_preserves_record_contents() {
        let label = *b"RELOCATE\0XYZ";
        let blob = [9, 8, 7, 6, 5];
        let mut first = fixture([0, 0, 1], [0, 0, 32], 80);
        write_blob_record(&mut first, 32, label, &blob);
        let mut second = fixture([0, 0, 1], [0, 0, 48], 80);
        write_blob_record(&mut second, 48, label, &blob);

        let first = parse_c3d(&first, LIMITS).unwrap();
        let second = parse_c3d(&second, LIMITS).unwrap();
        let first_record = first.blob_records().next().unwrap();
        let second_record = second.blob_records().next().unwrap();
        assert_ne!(first_record.offset(), second_record.offset());
        assert_eq!(first_record.label(), second_record.label());
        assert_eq!(first_record.blob(), second_record.blob());
    }

    fn fixture(
        counts: [u16; C3D_SECTION_COUNT],
        offsets: [u32; C3D_SECTION_COUNT],
        len: usize,
    ) -> Vec<u8> {
        let mut input = vec![0; len];
        if len < C3D_HEADER_LEN {
            return input;
        }
        input[0..4].copy_from_slice(&C3D_MAGIC);
        input[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
        input[8..12].copy_from_slice(&(C3D_HEADER_LEN as u32).to_le_bytes());
        for (index, count) in counts.into_iter().enumerate() {
            let start = 12 + index * 2;
            input[start..start + 2].copy_from_slice(&count.to_le_bytes());
        }
        for (index, offset) in offsets.into_iter().enumerate() {
            let start = 20 + index * 4;
            input[start..start + 4].copy_from_slice(&offset.to_le_bytes());
        }
        input
    }

    fn write_blob_record(
        input: &mut [u8],
        offset: usize,
        label: [u8; C3D_BLOB_LABEL_LEN],
        blob: &[u8],
    ) -> usize {
        let blob_start = offset + C3D_BLOB_RECORD_HEADER_LEN;
        let end = blob_start + blob.len();
        input[offset..offset + C3D_BLOB_LABEL_LEN].copy_from_slice(&label);
        input[offset + C3D_BLOB_LABEL_LEN..blob_start]
            .copy_from_slice(&(blob.len() as u32).to_le_bytes());
        input[blob_start..end].copy_from_slice(blob);
        end
    }

    fn write_group0_record(
        input: &mut [u8],
        offset: usize,
        values: &[u16],
        trailing: &[u8],
    ) -> usize {
        let len = C3D_GROUP0_FIXED_LEN + values.len() * 2 + trailing.len();
        let end = offset + len;
        input[offset..end].fill(0x55);
        input[offset..offset + 4].copy_from_slice(&(len as u32).to_le_bytes());
        input[offset + C3D_GROUP0_VALUE_COUNT_OFFSET] = values.len() as u8;
        for (index, value) in values.iter().enumerate() {
            let start = offset + C3D_GROUP0_VALUES_OFFSET + index * 2;
            input[start..start + 2].copy_from_slice(&value.to_le_bytes());
        }
        input[end - trailing.len()..end].copy_from_slice(trailing);
        end
    }

    fn write_group1_record(
        input: &mut [u8],
        offset: usize,
        prefix: [u8; C3D_GROUP1_PREFIX_LEN],
        entry32_count: u16,
        entry24_count: u16,
        trailing_len: usize,
    ) -> usize {
        let entries_32_bytes = usize::from(entry32_count) * C3D_GROUP1_ENTRY32_LEN;
        let entries_24_bytes = usize::from(entry24_count) * C3D_GROUP1_ENTRY24_LEN;
        let len = C3D_GROUP1_FIXED_LEN + entries_32_bytes + entries_24_bytes + trailing_len;
        let end = offset + len;
        input[offset..end].fill(0);
        input[offset..offset + C3D_GROUP1_PREFIX_LEN].copy_from_slice(&prefix);
        input[offset + 12..offset + 16].copy_from_slice(&(len as u32).to_le_bytes());
        input[offset + 16..offset + 18].copy_from_slice(&entry32_count.to_le_bytes());
        input[offset + 18..offset + 20].copy_from_slice(&entry24_count.to_le_bytes());
        for (index, byte) in input[offset + 20..offset + C3D_GROUP1_FIXED_LEN]
            .iter_mut()
            .enumerate()
        {
            *byte = 0x80 + index as u8;
        }
        let entries_32_start = offset + C3D_GROUP1_FIXED_LEN;
        input[entries_32_start..entries_32_start + entries_32_bytes].fill(0x20);
        let entries_24_start = entries_32_start + entries_32_bytes;
        input[entries_24_start..entries_24_start + entries_24_bytes].fill(0x40);
        for (index, byte) in input[end - trailing_len..end].iter_mut().enumerate() {
            *byte = 0xd0 + index as u8;
        }
        end
    }
}
