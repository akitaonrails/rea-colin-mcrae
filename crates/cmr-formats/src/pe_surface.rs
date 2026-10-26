//! Bounded extraction of the two surface-coefficient families embedded in a
//! positively identified game executable.
//!
//! This module parses only the PE structure needed to translate the reviewed
//! official-EU virtual address into a file span. Artifact hashing, edition
//! selection, filesystem access, and persistence belong to higher layers. No
//! coefficient payload is compiled into this crate.

use std::{error::Error, fmt};

use crate::pe32::{Pe32Error, Pe32Image};

/// Number of signed little-endian dwords in each recovered family.
pub const SURFACE_COEFFICIENT_COUNT: usize = 120;
/// Number of per-effect contiguous capability tables in the reviewed PE span.
pub const SURFACE_CAPABILITY_COUNT: usize = 16;
/// Number of surface ids in each capability table.
pub const SURFACE_CAPABILITY_ENTRIES: usize = 120;

/// Virtual address of the first family in the reviewed official-EU profile.
pub const OFFICIAL_EU_SURFACE_FAMILY_VA: u32 = 0x004f_72cc;
/// Virtual address of the first capability table in the reviewed official-EU profile.
pub const OFFICIAL_EU_SURFACE_CAPABILITY_VA: u32 = 0x004e_c0f8;

const FAMILY_BYTES: usize = SURFACE_COEFFICIENT_COUNT * size_of::<i32>();
const BOTH_FAMILIES_BYTES: usize = FAMILY_BYTES * 2;
/// Byte length of the sixteen contiguous capability tables.
pub const SURFACE_CAPABILITY_BYTES: usize = SURFACE_CAPABILITY_COUNT * SURFACE_CAPABILITY_ENTRIES;

/// One neutral 120-entry coefficient family from the executable image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceCoefficientFamily {
    /// Raw signed little-endian words in source order.
    pub values: [i32; SURFACE_COEFFICIENT_COUNT],
}

/// The two adjacent families rooted at the reviewed official-EU address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceCoefficientFamilies {
    pub first: SurfaceCoefficientFamily,
    pub second: SurfaceCoefficientFamily,
}

/// Sixteen neutral capability tables in address order; fifteen are consumed by `FUN_00424a40`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceCapabilityTables {
    pub values: [[u8; SURFACE_CAPABILITY_ENTRIES]; SURFACE_CAPABILITY_COUNT],
}

/// Structural failures while locating or decoding the bounded PE span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeSurfaceError {
    Truncated {
        offset: usize,
        needed: usize,
        input_len: usize,
    },
    InvalidDosMagic {
        offset: usize,
    },
    InvalidPeSignature {
        offset: usize,
    },
    UnsupportedMachine {
        offset: usize,
        machine: u16,
    },
    NotPe32 {
        offset: usize,
        magic: u16,
    },
    OptionalHeaderTooShort {
        offset: usize,
        size: usize,
    },
    AddressBeforeImage {
        virtual_address: u32,
        image_base: u32,
    },
    UnmappedRange {
        virtual_address: u32,
        byte_len: usize,
    },
    AmbiguousRange {
        virtual_address: u32,
        first_section_offset: usize,
        second_section_offset: usize,
    },
    ArithmeticOverflow {
        offset: usize,
    },
}

impl fmt::Display for PeSurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated {
                offset,
                needed,
                input_len,
            } => write!(
                formatter,
                "PE data is truncated at file offset {offset:#x}: need {needed} bytes, input length is {input_len:#x}"
            ),
            Self::InvalidDosMagic { offset } => {
                write!(formatter, "invalid DOS magic at file offset {offset:#x}")
            }
            Self::InvalidPeSignature { offset } => {
                write!(formatter, "invalid PE signature at file offset {offset:#x}")
            }
            Self::UnsupportedMachine { offset, machine } => write!(
                formatter,
                "unsupported PE machine {machine:#06x} at file offset {offset:#x}"
            ),
            Self::NotPe32 { offset, magic } => write!(
                formatter,
                "optional-header magic {magic:#06x} at file offset {offset:#x} is not PE32"
            ),
            Self::OptionalHeaderTooShort { offset, size } => write!(
                formatter,
                "PE32 optional header at file offset {offset:#x} is only {size:#x} bytes"
            ),
            Self::AddressBeforeImage {
                virtual_address,
                image_base,
            } => write!(
                formatter,
                "virtual address {virtual_address:#010x} precedes image base {image_base:#010x}"
            ),
            Self::UnmappedRange {
                virtual_address,
                byte_len,
            } => write!(
                formatter,
                "PE sections do not map {byte_len:#x} bytes at virtual address {virtual_address:#010x}"
            ),
            Self::AmbiguousRange {
                virtual_address,
                first_section_offset,
                second_section_offset,
            } => write!(
                formatter,
                "overlapping PE sections at file offsets {first_section_offset:#x} and {second_section_offset:#x} both map virtual address {virtual_address:#010x}"
            ),
            Self::ArithmeticOverflow { offset } => {
                write!(formatter, "PE offset arithmetic overflow near {offset:#x}")
            }
        }
    }
}

impl Error for PeSurfaceError {}

impl From<Pe32Error> for PeSurfaceError {
    fn from(error: Pe32Error) -> Self {
        match error {
            Pe32Error::EmptyRange { virtual_address } => Self::UnmappedRange {
                virtual_address,
                byte_len: 0,
            },
            Pe32Error::Truncated {
                offset,
                needed,
                input_len,
            } => Self::Truncated {
                offset,
                needed,
                input_len,
            },
            Pe32Error::InvalidDosMagic { offset } => Self::InvalidDosMagic { offset },
            Pe32Error::InvalidPeSignature { offset } => Self::InvalidPeSignature { offset },
            Pe32Error::UnsupportedMachine { offset, machine } => {
                Self::UnsupportedMachine { offset, machine }
            }
            Pe32Error::NotPe32 { offset, magic } => Self::NotPe32 { offset, magic },
            Pe32Error::OptionalHeaderTooShort { offset, size } => {
                Self::OptionalHeaderTooShort { offset, size }
            }
            Pe32Error::AddressBeforeImage {
                virtual_address,
                image_base,
            } => Self::AddressBeforeImage {
                virtual_address,
                image_base,
            },
            Pe32Error::UnmappedRange {
                virtual_address,
                byte_len,
            } => Self::UnmappedRange {
                virtual_address,
                byte_len,
            },
            Pe32Error::AmbiguousRange {
                virtual_address,
                first_section_offset,
                second_section_offset,
            } => Self::AmbiguousRange {
                virtual_address,
                first_section_offset,
                second_section_offset,
            },
            Pe32Error::SectionNotExecutable {
                virtual_address, ..
            } => Self::UnmappedRange {
                virtual_address,
                byte_len: BOTH_FAMILIES_BYTES,
            },
            Pe32Error::ArithmeticOverflow { offset } => Self::ArithmeticOverflow { offset },
        }
    }
}

/// Extract the two reviewed surface-coefficient families from caller-owned PE
/// bytes.
///
/// The caller must first establish that `input` is the supported artifact.
/// This function intentionally performs no hash/profile selection.
pub fn extract_official_eu_surface_coefficients(
    input: &[u8],
) -> Result<SurfaceCoefficientFamilies, PeSurfaceError> {
    let image = Pe32Image::parse(input)?;
    let encoded = image.read_va_range(OFFICIAL_EU_SURFACE_FAMILY_VA, BOTH_FAMILIES_BYTES)?;
    let first = decode_family(&encoded[..FAMILY_BYTES]);
    let second = decode_family(&encoded[FAMILY_BYTES..]);
    Ok(SurfaceCoefficientFamilies { first, second })
}

/// Extract the sixteen per-effect contiguous capability tables in the reviewed PE span.
///
/// Table and entry order are preserved exactly. The caller must establish the
/// supported artifact identity before calling this bounded parser.
pub fn extract_official_eu_surface_capabilities(
    input: &[u8],
) -> Result<SurfaceCapabilityTables, PeSurfaceError> {
    let image = Pe32Image::parse(input)?;
    let encoded =
        image.read_va_range(OFFICIAL_EU_SURFACE_CAPABILITY_VA, SURFACE_CAPABILITY_BYTES)?;
    let mut values = [[0; SURFACE_CAPABILITY_ENTRIES]; SURFACE_CAPABILITY_COUNT];
    for (table_index, table) in values.iter_mut().enumerate() {
        let offset = table_index * SURFACE_CAPABILITY_ENTRIES;
        table.copy_from_slice(&encoded[offset..offset + SURFACE_CAPABILITY_ENTRIES]);
    }
    Ok(SurfaceCapabilityTables { values })
}

fn decode_family(input: &[u8]) -> SurfaceCoefficientFamily {
    debug_assert_eq!(input.len(), FAMILY_BYTES);
    let mut values = [0; SURFACE_COEFFICIENT_COUNT];
    for (index, value) in values.iter_mut().enumerate() {
        let offset = index * 4;
        *value = i32::from_le_bytes(
            input[offset..offset + 4]
                .try_into()
                .expect("mapped family contains complete words"),
        );
    }
    SurfaceCoefficientFamily { values }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOS_LFANEW_OFFSET: usize = 0x3c;
    const COFF_HEADER_SIZE: usize = 20;
    const SECTION_HEADER_SIZE: usize = 40;
    const PE32_MAGIC: u16 = 0x10b;
    const I386_MACHINE: u16 = 0x14c;
    const PE_OFFSET: usize = 0x80;
    const OPTIONAL_SIZE: usize = 0xe0;
    const SECTION_TABLE: usize = PE_OFFSET + 4 + COFF_HEADER_SIZE + OPTIONAL_SIZE;
    const RAW_OFFSET: usize = 0x200;
    const SECTION_RVA: u32 = 0x000f_7000;
    const IMAGE_BASE: u32 = 0x0040_0000;
    const TARGET_DELTA: usize = 0x2cc;

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn synthetic_pe() -> Vec<u8> {
        let raw_size = 0x800usize;
        let capability_raw = RAW_OFFSET + raw_size;
        let capability_raw_size = 0x900usize;
        let mut bytes = vec![0; capability_raw + capability_raw_size];
        bytes[0..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, DOS_LFANEW_OFFSET, PE_OFFSET as u32);
        bytes[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        let coff = PE_OFFSET + 4;
        put_u16(&mut bytes, coff, I386_MACHINE);
        put_u16(&mut bytes, coff + 2, 2);
        put_u16(&mut bytes, coff + 16, OPTIONAL_SIZE as u16);
        let optional = coff + COFF_HEADER_SIZE;
        put_u16(&mut bytes, optional, PE32_MAGIC);
        put_u32(&mut bytes, optional + 28, IMAGE_BASE);

        bytes[SECTION_TABLE..SECTION_TABLE + 8].copy_from_slice(b".rdata\0\0");
        put_u32(&mut bytes, SECTION_TABLE + 8, raw_size as u32);
        put_u32(&mut bytes, SECTION_TABLE + 12, SECTION_RVA);
        put_u32(&mut bytes, SECTION_TABLE + 16, raw_size as u32);
        put_u32(&mut bytes, SECTION_TABLE + 20, RAW_OFFSET as u32);
        let capability_section = SECTION_TABLE + SECTION_HEADER_SIZE;
        bytes[capability_section..capability_section + 8].copy_from_slice(b".data\0\0\0");
        put_u32(
            &mut bytes,
            capability_section + 8,
            capability_raw_size as u32,
        );
        put_u32(&mut bytes, capability_section + 12, 0x000e_c000);
        put_u32(
            &mut bytes,
            capability_section + 16,
            capability_raw_size as u32,
        );
        put_u32(&mut bytes, capability_section + 20, capability_raw as u32);

        let target = RAW_OFFSET + TARGET_DELTA;
        for index in 0..SURFACE_COEFFICIENT_COUNT {
            put_u32(&mut bytes, target + index * 4, (index as i32 - 60) as u32);
            put_u32(
                &mut bytes,
                target + FAMILY_BYTES + index * 4,
                (i32::MIN + index as i32) as u32,
            );
        }
        put_u32(
            &mut bytes,
            target + FAMILY_BYTES + (SURFACE_COEFFICIENT_COUNT - 1) * 4,
            i32::MAX as u32,
        );
        let capability_target = capability_raw + 0xf8;
        for table in 0..SURFACE_CAPABILITY_COUNT {
            for entry in 0..SURFACE_CAPABILITY_ENTRIES {
                bytes[capability_target + table * SURFACE_CAPABILITY_ENTRIES + entry] =
                    (table * 17 + entry) as u8;
            }
        }
        bytes
    }

    #[test]
    fn extracts_two_adjacent_signed_families_from_a_synthetic_pe() {
        let parsed = extract_official_eu_surface_coefficients(&synthetic_pe()).unwrap();
        assert_eq!(parsed.first.values[0], -60);
        assert_eq!(parsed.first.values[60], 0);
        assert_eq!(parsed.first.values[119], 59);
        assert_eq!(parsed.second.values[0], i32::MIN);
        assert_eq!(parsed.second.values[119], i32::MAX);
    }

    #[test]
    fn extracts_sixteen_capability_tables_in_address_order() {
        let parsed = extract_official_eu_surface_capabilities(&synthetic_pe()).unwrap();
        for table in 0..SURFACE_CAPABILITY_COUNT {
            for entry in 0..SURFACE_CAPABILITY_ENTRIES {
                assert_eq!(parsed.values[table][entry], (table * 17 + entry) as u8);
            }
        }
        assert_eq!(parsed.values[15][119], 118);
    }

    #[test]
    fn capability_extraction_rejects_a_short_mapped_span() {
        let mut pe = synthetic_pe();
        let capability_section = SECTION_TABLE + SECTION_HEADER_SIZE;
        put_u32(
            &mut pe,
            capability_section + 16,
            0xf8 + SURFACE_CAPABILITY_BYTES as u32 - 1,
        );
        assert_eq!(
            extract_official_eu_surface_capabilities(&pe),
            Err(PeSurfaceError::UnmappedRange {
                virtual_address: OFFICIAL_EU_SURFACE_CAPABILITY_VA,
                byte_len: SURFACE_CAPABILITY_BYTES,
            })
        );
    }

    #[test]
    fn rejects_non_pe32_and_non_i386_images() {
        let mut pe64 = synthetic_pe();
        put_u16(&mut pe64, PE_OFFSET + 4 + COFF_HEADER_SIZE, 0x20b);
        assert_eq!(
            extract_official_eu_surface_coefficients(&pe64),
            Err(PeSurfaceError::NotPe32 {
                offset: PE_OFFSET + 4 + COFF_HEADER_SIZE,
                magic: 0x20b,
            })
        );

        let mut amd64 = synthetic_pe();
        put_u16(&mut amd64, PE_OFFSET + 4, 0x8664);
        assert!(matches!(
            extract_official_eu_surface_coefficients(&amd64),
            Err(PeSurfaceError::UnsupportedMachine {
                machine: 0x8664,
                ..
            })
        ));
    }

    #[test]
    fn rejects_bad_signatures_with_their_file_offsets() {
        let mut bad_dos = synthetic_pe();
        bad_dos[0] = 0;
        assert_eq!(
            extract_official_eu_surface_coefficients(&bad_dos),
            Err(PeSurfaceError::InvalidDosMagic { offset: 0 })
        );

        let mut bad_pe = synthetic_pe();
        bad_pe[PE_OFFSET] = 0;
        assert_eq!(
            extract_official_eu_surface_coefficients(&bad_pe),
            Err(PeSurfaceError::InvalidPeSignature { offset: PE_OFFSET })
        );
    }

    #[test]
    fn rejects_truncated_headers_and_section_tables() {
        let dos_stub = &synthetic_pe()[..DOS_LFANEW_OFFSET + 2];
        assert!(matches!(
            extract_official_eu_surface_coefficients(dos_stub),
            Err(PeSurfaceError::Truncated {
                offset: DOS_LFANEW_OFFSET,
                ..
            })
        ));

        let truncated = &synthetic_pe()[..SECTION_TABLE + SECTION_HEADER_SIZE - 1];
        assert!(matches!(
            extract_official_eu_surface_coefficients(truncated),
            Err(PeSurfaceError::Truncated {
                offset: SECTION_TABLE,
                ..
            })
        ));
    }

    #[test]
    fn rejects_optional_headers_too_short_for_the_image_base() {
        let mut pe = synthetic_pe();
        put_u16(&mut pe, PE_OFFSET + 4 + 16, 31);
        assert_eq!(
            extract_official_eu_surface_coefficients(&pe),
            Err(PeSurfaceError::OptionalHeaderTooShort {
                offset: PE_OFFSET + 4 + COFF_HEADER_SIZE,
                size: 31,
            })
        );
    }

    #[test]
    fn rejects_addresses_before_the_image_base() {
        let mut pe = synthetic_pe();
        put_u32(
            &mut pe,
            PE_OFFSET + 4 + COFF_HEADER_SIZE + 28,
            OFFICIAL_EU_SURFACE_FAMILY_VA + 1,
        );
        assert!(matches!(
            extract_official_eu_surface_coefficients(&pe),
            Err(PeSurfaceError::AddressBeforeImage { .. })
        ));
    }

    #[test]
    fn rejects_ranges_that_cross_the_section_raw_boundary() {
        let mut pe = synthetic_pe();
        put_u32(
            &mut pe,
            SECTION_TABLE + 16,
            (TARGET_DELTA + BOTH_FAMILIES_BYTES - 1) as u32,
        );
        assert_eq!(
            extract_official_eu_surface_coefficients(&pe),
            Err(PeSurfaceError::UnmappedRange {
                virtual_address: OFFICIAL_EU_SURFACE_FAMILY_VA,
                byte_len: BOTH_FAMILIES_BYTES,
            })
        );
    }

    #[test]
    fn rejects_raw_backing_that_is_not_fully_within_virtual_size() {
        let mut pe = synthetic_pe();
        put_u32(
            &mut pe,
            SECTION_TABLE + 8,
            (TARGET_DELTA + BOTH_FAMILIES_BYTES - 1) as u32,
        );
        assert_eq!(
            extract_official_eu_surface_coefficients(&pe),
            Err(PeSurfaceError::UnmappedRange {
                virtual_address: OFFICIAL_EU_SURFACE_FAMILY_VA,
                byte_len: BOTH_FAMILIES_BYTES,
            })
        );
    }

    #[test]
    fn zero_virtual_size_explicitly_falls_back_to_raw_size() {
        let mut pe = synthetic_pe();
        put_u32(&mut pe, SECTION_TABLE + 8, 0);

        let parsed = extract_official_eu_surface_coefficients(&pe).unwrap();
        assert_eq!(parsed.first.values[0], -60);
        assert_eq!(parsed.second.values[119], i32::MAX);
    }

    #[test]
    fn reports_a_truncated_mapped_raw_span_at_the_resolved_offset() {
        let mut pe = synthetic_pe();
        pe.truncate(RAW_OFFSET + TARGET_DELTA + BOTH_FAMILIES_BYTES - 1);
        assert_eq!(
            extract_official_eu_surface_coefficients(&pe),
            Err(PeSurfaceError::Truncated {
                offset: RAW_OFFSET + TARGET_DELTA,
                needed: BOTH_FAMILIES_BYTES,
                input_len: RAW_OFFSET + TARGET_DELTA + BOTH_FAMILIES_BYTES - 1,
            })
        );
    }

    #[test]
    fn rejects_overlapping_sections_instead_of_choosing_one() {
        let mut pe = synthetic_pe();
        put_u16(&mut pe, PE_OFFSET + 4 + 2, 2);
        let second = SECTION_TABLE + SECTION_HEADER_SIZE;
        pe.copy_within(SECTION_TABLE..SECTION_TABLE + SECTION_HEADER_SIZE, second);

        assert_eq!(
            extract_official_eu_surface_coefficients(&pe),
            Err(PeSurfaceError::AmbiguousRange {
                virtual_address: OFFICIAL_EU_SURFACE_FAMILY_VA,
                first_section_offset: SECTION_TABLE,
                second_section_offset: second,
            })
        );
    }
}
