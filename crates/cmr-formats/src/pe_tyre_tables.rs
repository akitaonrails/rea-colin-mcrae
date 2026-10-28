//! Authenticated extraction and payload-free encoding of the three static
//! per-wheel tyre/surface tables read by the wheel-force producer.
//!
//! Observation: `DAT_004f6b50` and `DAT_004f6f10` are 120-entry, 8-byte-stride
//! tables of which only the signed little-endian dword at entry offset 0 is
//! read; `DAT_004f777c` is 8 variants x 120 surfaces of three unsigned 16-bit
//! words. The meaning of the values is not asserted here, so field names carry
//! the source address. No table payload is compiled into this crate.

use std::fmt;

use crate::pe32::{Pe32Error, Pe32Image};

pub const TYRE_SURFACE_COUNT: usize = 120;
pub const TYRE_VARIANT_COUNT: usize = 8;
pub const TYRE_WHEEL_TERM_WORDS: usize = 3;
pub const TYRE_TABLES_FORMAT_VERSION: u32 = 1;

const CANDIDATE_STRIDE: usize = 8;
const CANDIDATE_DWORD_BYTES: usize = 4;
const WHEEL_TERM_ENTRY_BYTES: usize = TYRE_WHEEL_TERM_WORDS * 2;
/// Bytes of the first table: every entry is complete.
const BASE_6B50_SPAN: usize = TYRE_SURFACE_COUNT * CANDIDATE_STRIDE;
/// Bytes of the second table: the last entry contributes only its read dword,
/// so the span stops where the existing surface-coefficient family begins.
const BASE_6F10_SPAN: usize = (TYRE_SURFACE_COUNT - 1) * CANDIDATE_STRIDE + CANDIDATE_DWORD_BYTES;
const WHEEL_TERMS_SPAN: usize = TYRE_VARIANT_COUNT * TYRE_SURFACE_COUNT * WHEEL_TERM_ENTRY_BYTES;

const MAGIC: &[u8; 8] = b"CMRTYRE\0";
const HEADER_LEN: usize = 20;
const CANDIDATE_ENCODED_BYTES: usize = TYRE_SURFACE_COUNT * 4;
pub const TYRE_TABLES_ENCODED_LEN: usize =
    HEADER_LEN + 2 * CANDIDATE_ENCODED_BYTES + WHEEL_TERMS_SPAN;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TyreTablesLayout {
    pub image_base: u32,
    pub candidate_base_6b50_va: u32,
    pub candidate_base_6f10_va: u32,
    pub wheel_terms_777c_va: u32,
}

pub const OFFICIAL_EU_TYRE_TABLES_LAYOUT: TyreTablesLayout = TyreTablesLayout {
    image_base: 0x0040_0000,
    candidate_base_6b50_va: 0x004f_6b50,
    candidate_base_6f10_va: 0x004f_6f10,
    wheel_terms_777c_va: 0x004f_777c,
};

/// The three neutral tables in source order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TyreSurfaceTables {
    /// Dword at offset 0 of each 8-byte entry of `DAT_004f6b50`.
    pub candidate_base_6b50: [i32; TYRE_SURFACE_COUNT],
    /// Dword at offset 0 of each 8-byte entry of `DAT_004f6f10`.
    pub candidate_base_6f10: [i32; TYRE_SURFACE_COUNT],
    /// `DAT_004f777c[variant][surface]` as three little-endian words.
    pub wheel_terms_777c: [[[u16; TYRE_WHEEL_TERM_WORDS]; TYRE_SURFACE_COUNT]; TYRE_VARIANT_COUNT],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TyreTablesError {
    Pe(String),
    ImageBase {
        expected: u32,
        actual: u32,
    },
    UnsupportedLayout,
    Misaligned {
        virtual_address: u32,
        alignment: u32,
    },
    WrongLength {
        expected: usize,
        actual: usize,
    },
    InvalidHeader,
}

impl fmt::Display for TyreTablesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pe(e) => write!(f, "invalid PE32 image: {e}"),
            Self::ImageBase { expected, actual } => write!(
                f,
                "image base mismatch: expected {expected:#x}, got {actual:#x}"
            ),
            Self::UnsupportedLayout => write!(f, "unsupported tyre-table layout"),
            Self::Misaligned {
                virtual_address,
                alignment,
            } => write!(
                f,
                "table address {virtual_address:#x} is not {alignment}-byte aligned"
            ),
            Self::WrongLength { expected, actual } => {
                write!(f, "wrong encoded length: expected {expected}, got {actual}")
            }
            Self::InvalidHeader => write!(f, "invalid tyre-table header"),
        }
    }
}
impl std::error::Error for TyreTablesError {}

fn pe_error(error: Pe32Error) -> TyreTablesError {
    TyreTablesError::Pe(format!("{error:?}"))
}

pub fn extract_tyre_surface_tables(
    input: &[u8],
    layout: &TyreTablesLayout,
) -> Result<TyreSurfaceTables, TyreTablesError> {
    let second_start = u64::from(layout.candidate_base_6b50_va) + BASE_6B50_SPAN as u64;
    if u64::from(layout.candidate_base_6f10_va) != second_start {
        return Err(TyreTablesError::UnsupportedLayout);
    }
    for (virtual_address, alignment) in [
        (layout.candidate_base_6b50_va, 4),
        (layout.candidate_base_6f10_va, 4),
        (layout.wheel_terms_777c_va, 2),
    ] {
        if virtual_address % alignment != 0 {
            return Err(TyreTablesError::Misaligned {
                virtual_address,
                alignment,
            });
        }
    }
    let image = Pe32Image::parse(input).map_err(pe_error)?;
    if image.image_base() != layout.image_base {
        return Err(TyreTablesError::ImageBase {
            expected: layout.image_base,
            actual: image.image_base(),
        });
    }
    let first = image
        .read_va_range(layout.candidate_base_6b50_va, BASE_6B50_SPAN)
        .map_err(pe_error)?;
    let second = image
        .read_va_range(layout.candidate_base_6f10_va, BASE_6F10_SPAN)
        .map_err(pe_error)?;
    let terms = image
        .read_va_range(layout.wheel_terms_777c_va, WHEEL_TERMS_SPAN)
        .map_err(pe_error)?;
    let candidates = |bytes: &[u8]| {
        std::array::from_fn(|i| {
            let at = i * CANDIDATE_STRIDE;
            i32::from_le_bytes(bytes[at..at + 4].try_into().expect("bounded span"))
        })
    };
    Ok(TyreSurfaceTables {
        candidate_base_6b50: candidates(first),
        candidate_base_6f10: candidates(second),
        wheel_terms_777c: std::array::from_fn(|v| {
            std::array::from_fn(|s| {
                let at = (s + TYRE_SURFACE_COUNT * v) * WHEEL_TERM_ENTRY_BYTES;
                std::array::from_fn(|w| {
                    u16::from_le_bytes(terms[at + w * 2..at + w * 2 + 2].try_into().unwrap())
                })
            })
        }),
    })
}

pub fn encode_tyre_surface_tables(tables: &TyreSurfaceTables) -> Vec<u8> {
    let mut out = Vec::with_capacity(TYRE_TABLES_ENCODED_LEN);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&TYRE_TABLES_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(TYRE_SURFACE_COUNT as u32).to_le_bytes());
    out.extend_from_slice(&(TYRE_VARIANT_COUNT as u32).to_le_bytes());
    for value in tables
        .candidate_base_6b50
        .iter()
        .chain(&tables.candidate_base_6f10)
    {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for word in tables.wheel_terms_777c.iter().flatten().flatten() {
        out.extend_from_slice(&word.to_le_bytes());
    }
    debug_assert_eq!(out.len(), TYRE_TABLES_ENCODED_LEN);
    out
}

pub fn decode_tyre_surface_tables(input: &[u8]) -> Result<TyreSurfaceTables, TyreTablesError> {
    if input.len() != TYRE_TABLES_ENCODED_LEN {
        return Err(TyreTablesError::WrongLength {
            expected: TYRE_TABLES_ENCODED_LEN,
            actual: input.len(),
        });
    }
    let word = |at: usize| u32::from_le_bytes(input[at..at + 4].try_into().unwrap());
    if &input[..8] != MAGIC
        || word(8) != TYRE_TABLES_FORMAT_VERSION
        || word(12) != TYRE_SURFACE_COUNT as u32
        || word(16) != TYRE_VARIANT_COUNT as u32
    {
        return Err(TyreTablesError::InvalidHeader);
    }
    let second_start = HEADER_LEN + CANDIDATE_ENCODED_BYTES;
    let terms_start = second_start + CANDIDATE_ENCODED_BYTES;
    Ok(TyreSurfaceTables {
        candidate_base_6b50: std::array::from_fn(|i| word(HEADER_LEN + i * 4) as i32),
        candidate_base_6f10: std::array::from_fn(|i| word(second_start + i * 4) as i32),
        wheel_terms_777c: std::array::from_fn(|v| {
            std::array::from_fn(|s| {
                let at = terms_start + (s + TYRE_SURFACE_COUNT * v) * WHEEL_TERM_ENTRY_BYTES;
                std::array::from_fn(|w| {
                    u16::from_le_bytes(input[at + w * 2..at + w * 2 + 2].try_into().unwrap())
                })
            })
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PE: usize = 0x80;
    const OPTIONAL: usize = PE + 24;
    const SECTIONS: usize = OPTIONAL + 0xe0;
    const RAW: usize = 0x400;
    const SECTION_RVA: u32 = 0x000f_6000;
    const SECTION_SIZE: usize = 0x3000;

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn file_offset(va: u32) -> usize {
        RAW + (va - 0x0040_0000 - SECTION_RVA) as usize
    }

    fn synthetic_tables() -> TyreSurfaceTables {
        TyreSurfaceTables {
            candidate_base_6b50: std::array::from_fn(|i| i as i32 - 60),
            candidate_base_6f10: std::array::from_fn(|i| i32::MIN + i as i32 * 7),
            wheel_terms_777c: std::array::from_fn(|v| {
                std::array::from_fn(|s| {
                    [
                        (v * 1000 + s) as u16,
                        u16::MAX - s as u16,
                        (v as u16) << 12 | s as u16,
                    ]
                })
            }),
        }
    }

    fn synthetic_pe() -> Vec<u8> {
        let layout = OFFICIAL_EU_TYRE_TABLES_LAYOUT;
        let mut bytes = vec![0_u8; RAW + SECTION_SIZE];
        bytes[..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, 0x3c, PE as u32);
        bytes[PE..PE + 4].copy_from_slice(b"PE\0\0");
        put_u16(&mut bytes, PE + 4, 0x14c);
        put_u16(&mut bytes, PE + 6, 1);
        put_u16(&mut bytes, PE + 20, 0xe0);
        put_u16(&mut bytes, OPTIONAL, 0x10b);
        put_u32(&mut bytes, OPTIONAL + 28, layout.image_base);
        put_u32(&mut bytes, SECTIONS + 8, SECTION_SIZE as u32);
        put_u32(&mut bytes, SECTIONS + 12, SECTION_RVA);
        put_u32(&mut bytes, SECTIONS + 16, SECTION_SIZE as u32);
        put_u32(&mut bytes, SECTIONS + 20, RAW as u32);

        let tables = synthetic_tables();
        for i in 0..TYRE_SURFACE_COUNT {
            let a = file_offset(layout.candidate_base_6b50_va) + i * 8;
            put_u32(&mut bytes, a, tables.candidate_base_6b50[i] as u32);
            // Unread second dword of each entry must not leak into the result.
            put_u32(&mut bytes, a + 4, 0xdead_beef);
            let b = file_offset(layout.candidate_base_6f10_va) + i * 8;
            put_u32(&mut bytes, b, tables.candidate_base_6f10[i] as u32);
            if i + 1 < TYRE_SURFACE_COUNT {
                put_u32(&mut bytes, b + 4, 0xdead_beef);
            }
        }
        for (v, variant) in tables.wheel_terms_777c.iter().enumerate() {
            for (s, entry) in variant.iter().enumerate() {
                let at = file_offset(layout.wheel_terms_777c_va)
                    + (s + TYRE_SURFACE_COUNT * v) * WHEEL_TERM_ENTRY_BYTES;
                for (w, value) in entry.iter().enumerate() {
                    put_u16(&mut bytes, at + w * 2, *value);
                }
            }
        }
        bytes
    }

    #[test]
    fn spans_match_the_reviewed_addresses() {
        let l = OFFICIAL_EU_TYRE_TABLES_LAYOUT;
        assert_eq!(l.candidate_base_6b50_va + BASE_6B50_SPAN as u32, 0x4f6f10);
        // Ends exactly where the existing surface family begins.
        assert_eq!(l.candidate_base_6f10_va + BASE_6F10_SPAN as u32, 0x4f72cc);
        assert_eq!(l.wheel_terms_777c_va + WHEEL_TERMS_SPAN as u32, 0x4f8dfc);
        assert_eq!(TYRE_TABLES_ENCODED_LEN, 20 + 960 + 5760);
    }

    #[test]
    fn extracts_read_dwords_and_indexes_variants_by_surface() {
        let bytes = synthetic_pe();
        let got = extract_tyre_surface_tables(&bytes, &OFFICIAL_EU_TYRE_TABLES_LAYOUT).unwrap();
        assert_eq!(got, synthetic_tables());
        assert_eq!(got.candidate_base_6f10[119], i32::MIN + 119 * 7);
        assert_eq!(got.wheel_terms_777c[7][119][0], (7 * 1000 + 119) as u16);
        assert_eq!(got.wheel_terms_777c[1][0][1], u16::MAX);
    }

    #[test]
    fn rejects_truncation_out_of_range_and_wrong_image() {
        let l = OFFICIAL_EU_TYRE_TABLES_LAYOUT;
        let bytes = synthetic_pe();
        let end = file_offset(l.wheel_terms_777c_va) + WHEEL_TERMS_SPAN;
        for len in [end - 1, end - 6, file_offset(l.candidate_base_6f10_va) + 4] {
            let err = extract_tyre_surface_tables(&bytes[..len], &l).unwrap_err();
            let TyreTablesError::Pe(text) = err else {
                panic!("expected PE error");
            };
            assert!(
                text.contains("Truncated") && text.contains("offset"),
                "{text}"
            );
        }
        extract_tyre_surface_tables(&bytes[..end], &l).unwrap();
        assert!(matches!(
            extract_tyre_surface_tables(&bytes[..1], &l),
            Err(TyreTablesError::Pe(_))
        ));
        let mut short_section = bytes.clone();
        put_u32(&mut short_section, SECTIONS + 8, 0x1000);
        let TyreTablesError::Pe(text) =
            extract_tyre_surface_tables(&short_section, &l).unwrap_err()
        else {
            panic!("expected PE error");
        };
        assert!(
            text.contains("UnmappedRange") && text.contains("virtual_address"),
            "{text}"
        );
        let mut wrong_base = l;
        wrong_base.image_base += 0x1000;
        assert_eq!(
            extract_tyre_surface_tables(&bytes, &wrong_base),
            Err(TyreTablesError::ImageBase {
                expected: 0x0040_1000,
                actual: 0x0040_0000
            })
        );
        let mut before_image = l;
        before_image.wheel_terms_777c_va = 0x1000;
        assert!(matches!(
            extract_tyre_surface_tables(&bytes, &before_image),
            Err(TyreTablesError::Pe(_))
        ));
    }

    #[test]
    fn rejects_misaligned_and_non_contiguous_layouts() {
        let l = OFFICIAL_EU_TYRE_TABLES_LAYOUT;
        let bytes = synthetic_pe();
        let mut odd_terms = l;
        odd_terms.wheel_terms_777c_va += 1;
        assert_eq!(
            extract_tyre_surface_tables(&bytes, &odd_terms),
            Err(TyreTablesError::Misaligned {
                virtual_address: 0x4f777d,
                alignment: 2
            })
        );
        let mut gap = l;
        gap.candidate_base_6f10_va += 4;
        assert_eq!(
            extract_tyre_surface_tables(&bytes, &gap),
            Err(TyreTablesError::UnsupportedLayout)
        );
        let mut misaligned_first = l;
        misaligned_first.candidate_base_6b50_va += 2;
        misaligned_first.candidate_base_6f10_va += 2;
        assert!(matches!(
            extract_tyre_surface_tables(&bytes, &misaligned_first),
            Err(TyreTablesError::Misaligned { alignment: 4, .. })
        ));
    }

    #[test]
    fn rejects_ambiguous_section_overlap() {
        let mut bytes = synthetic_pe();
        put_u16(&mut bytes, PE + 6, 2);
        bytes.resize(bytes.len().max(SECTIONS + 80), 0);
        bytes.copy_within(SECTIONS..SECTIONS + 40, SECTIONS + 40);
        assert!(matches!(
            extract_tyre_surface_tables(&bytes, &OFFICIAL_EU_TYRE_TABLES_LAYOUT),
            Err(TyreTablesError::Pe(text)) if text.contains("AmbiguousRange")
        ));
    }

    #[test]
    fn codec_round_trips_and_rejects_every_header_and_length_boundary() {
        let tables = synthetic_tables();
        let encoded = encode_tyre_surface_tables(&tables);
        assert_eq!(encoded.len(), TYRE_TABLES_ENCODED_LEN);
        assert_eq!(decode_tyre_surface_tables(&encoded).unwrap(), tables);
        assert!(matches!(
            decode_tyre_surface_tables(&encoded[..encoded.len() - 1]),
            Err(TyreTablesError::WrongLength { .. })
        ));
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(matches!(
            decode_tyre_surface_tables(&trailing),
            Err(TyreTablesError::WrongLength { .. })
        ));
        for offset in [0, 8, 12, 16] {
            let mut candidate = encoded.clone();
            candidate[offset] ^= 1;
            assert_eq!(
                decode_tyre_surface_tables(&candidate),
                Err(TyreTablesError::InvalidHeader)
            );
        }
    }

    #[test]
    fn codec_preserves_extreme_values_in_each_table() {
        let mut tables = synthetic_tables();
        tables.candidate_base_6b50[0] = i32::MIN;
        tables.candidate_base_6b50[119] = i32::MAX;
        tables.candidate_base_6f10[119] = -1;
        tables.wheel_terms_777c[7][119] = [0, u16::MAX, 0x8000];
        let decoded = decode_tyre_surface_tables(&encode_tyre_surface_tables(&tables)).unwrap();
        assert_eq!(decoded, tables);
    }
}
