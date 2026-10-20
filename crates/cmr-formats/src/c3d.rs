//! Bounded parsing for the observed `CM3D` object header.
//!
//! The legacy loader reads three counts and three payload-relative offsets from
//! a 32-byte prefix. The meaning and record layouts behind those sections are
//! not recovered yet, so this module deliberately preserves them under neutral
//! names and retains the complete payload unchanged.

use std::fmt;

pub const C3D_HEADER_LEN: usize = 32;
pub const C3D_MAGIC: [u8; 4] = *b"CM3D";
pub const C3D_FORMAT_VERSION: u32 = 100;
pub const C3D_SECTION_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dLimits {
    pub max_payload_bytes: usize,
}

impl Default for C3dLimits {
    fn default() -> Self {
        Self {
            max_payload_bytes: 64 * 1024 * 1024,
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

/// An admitted C3D payload. Unknown records remain byte-exact for later slices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct C3dFile {
    header: C3dHeader,
    bytes: Vec<u8>,
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

    Ok(C3dFile {
        header: C3dHeader {
            format_version,
            unknown_08: input[8..12].try_into().expect("fixed-size field"),
            section_counts,
            unknown_18: input[18..20].try_into().expect("fixed-size field"),
            section_offsets,
        },
        bytes: input.to_vec(),
    })
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
        max_payload_bytes: 64,
    };

    #[test]
    fn parses_observed_fields_and_preserves_every_byte() {
        let mut input = fixture([1, 2, 3], [32, 40, 48], 64);
        input[8..12].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
        input[18..20].copy_from_slice(&[0x12, 0x34]);
        input[63] = 0xfe;

        let parsed = parse_c3d(&input, LIMITS).unwrap();
        assert_eq!(
            parsed.header(),
            &C3dHeader {
                format_version: C3D_FORMAT_VERSION,
                unknown_08: [0xaa, 0xbb, 0xcc, 0xdd],
                section_counts: [1, 2, 3],
                unknown_18: [0x12, 0x34],
                section_offsets: [32, 40, 48],
            }
        );
        assert_eq!(parsed.bytes(), input);
        assert_eq!(parsed.into_bytes(), input);
    }

    #[test]
    fn accepts_the_exact_payload_limit_and_rejects_one_byte_more() {
        assert!(parse_c3d(&fixture([1, 1, 1], [32, 40, 48], 64), LIMITS).is_ok());
        assert_eq!(
            parse_c3d(&fixture([1, 1, 1], [32, 40, 48], 65), LIMITS),
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
        let parsed = parse_c3d(&fixture([7, 8, 9], [31, 0, 12], 64), LIMITS).unwrap();
        assert_eq!(parsed.header().section_counts, [7, 8, 9]);
        assert_eq!(parsed.header().section_offsets, [31, 0, 12]);
    }

    #[test]
    fn validates_each_section_position_independently() {
        for index in 0..C3D_SECTION_COUNT {
            let counts = [1; C3D_SECTION_COUNT];
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
}
