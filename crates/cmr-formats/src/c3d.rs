//! Bounded parsing for the observed `CM3D` object header.
//!
//! The legacy loader reads three counts and three payload-relative offsets from
//! a 32-byte prefix. Only the third count/position iterator is recovered so
//! far; this module exposes its length-delimited blobs under neutral names,
//! preserves the other groups, and retains the complete payload unchanged.

use std::fmt;

pub const C3D_HEADER_LEN: usize = 32;
pub const C3D_MAGIC: [u8; 4] = *b"CM3D";
pub const C3D_FORMAT_VERSION: u32 = 100;
pub const C3D_SECTION_COUNT: usize = 3;
pub const C3D_BLOB_RECORD_HEADER_LEN: usize = 16;
pub const C3D_BLOB_LABEL_LEN: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dLimits {
    pub max_payload_bytes: usize,
    pub max_blob_records: usize,
}

impl Default for C3dLimits {
    fn default() -> Self {
        Self {
            max_payload_bytes: 64 * 1024 * 1024,
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

    pub fn blob_records(&self) -> impl ExactSizeIterator<Item = C3dBlobRecord<'_>> {
        self.blob_records.iter().map(|span| C3dBlobRecord {
            offset: span.offset,
            label: self.bytes[span.offset..span.offset + C3D_BLOB_LABEL_LEN]
                .try_into()
                .expect("validated C3D blob label"),
            blob: &self.bytes[span.blob_start..span.blob_end],
        })
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

    Ok(C3dFile {
        header: C3dHeader {
            format_version,
            unknown_08: input[8..12].try_into().expect("fixed-size field"),
            section_counts,
            unknown_18: input[18..20].try_into().expect("fixed-size field"),
            section_offsets,
        },
        bytes: input.to_vec(),
        blob_records,
    })
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
        max_blob_records: 8,
    };

    #[test]
    fn parses_observed_fields_and_preserves_every_byte() {
        let mut input = fixture([1, 2, 0], [32, 40, 48], 64);
        input[8..12].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
        input[18..20].copy_from_slice(&[0x12, 0x34]);
        input[63] = 0xfe;

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        assert_eq!(
            parsed.header(),
            &C3dHeader {
                format_version: C3D_FORMAT_VERSION,
                unknown_08: [0xaa, 0xbb, 0xcc, 0xdd],
                section_counts: [1, 2, 0],
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
        let parsed = parse_c3d(&fixture([7, 8, 0], [31, 0, 12], 64), LIMITS).unwrap();
        assert_eq!(parsed.header().section_counts, [7, 8, 0]);
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
    fn parses_counted_blob_records_and_preserves_labels_blobs_and_tail() {
        let mut input = fixture([91, 73, 2], [7, 11, 40], 80);
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
        let mut input = fixture([u16::MAX, u16::MAX - 1, 1], [59, 58, 40], 60);
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
}
