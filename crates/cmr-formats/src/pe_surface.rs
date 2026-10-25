//! Bounded extraction of the two surface-coefficient families embedded in a
//! positively identified game executable.
//!
//! This module parses only the PE structure needed to translate the reviewed
//! official-EU virtual address into a file span. Artifact hashing, edition
//! selection, filesystem access, and persistence belong to higher layers. No
//! coefficient payload is compiled into this crate.

use std::{error::Error, fmt};

/// Number of signed little-endian dwords in each recovered family.
pub const SURFACE_COEFFICIENT_COUNT: usize = 120;

/// Virtual address of the first family in the reviewed official-EU profile.
pub const OFFICIAL_EU_SURFACE_FAMILY_VA: u32 = 0x004f_72cc;

const FAMILY_BYTES: usize = SURFACE_COEFFICIENT_COUNT * size_of::<i32>();
const BOTH_FAMILIES_BYTES: usize = FAMILY_BYTES * 2;
const DOS_LFANEW_OFFSET: usize = 0x3c;
const COFF_HEADER_SIZE: usize = 20;
const SECTION_HEADER_SIZE: usize = 40;
const PE32_MAGIC: u16 = 0x10b;
const I386_MACHINE: u16 = 0x14c;

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

/// Extract the two reviewed surface-coefficient families from caller-owned PE
/// bytes.
///
/// The caller must first establish that `input` is the supported artifact.
/// This function intentionally performs no hash/profile selection.
pub fn extract_official_eu_surface_coefficients(
    input: &[u8],
) -> Result<SurfaceCoefficientFamilies, PeSurfaceError> {
    require(input, 0, 2)?;
    if &input[..2] != b"MZ" {
        return Err(PeSurfaceError::InvalidDosMagic { offset: 0 });
    }

    let pe_offset = read_u32(input, DOS_LFANEW_OFFSET)? as usize;
    require(input, pe_offset, 4 + COFF_HEADER_SIZE)?;
    if &input[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return Err(PeSurfaceError::InvalidPeSignature { offset: pe_offset });
    }

    let coff_offset = pe_offset + 4;
    let machine = read_u16(input, coff_offset)?;
    if machine != I386_MACHINE {
        return Err(PeSurfaceError::UnsupportedMachine {
            offset: coff_offset,
            machine,
        });
    }
    let section_count = read_u16(input, coff_offset + 2)? as usize;
    let optional_size = read_u16(input, coff_offset + 16)? as usize;
    let optional_offset = coff_offset + COFF_HEADER_SIZE;
    require(input, optional_offset, optional_size)?;
    if optional_size < 32 {
        return Err(PeSurfaceError::OptionalHeaderTooShort {
            offset: optional_offset,
            size: optional_size,
        });
    }
    let optional_magic = read_u16(input, optional_offset)?;
    if optional_magic != PE32_MAGIC {
        return Err(PeSurfaceError::NotPe32 {
            offset: optional_offset,
            magic: optional_magic,
        });
    }
    let image_base = read_u32(input, optional_offset + 28)?;
    let target_rva = OFFICIAL_EU_SURFACE_FAMILY_VA
        .checked_sub(image_base)
        .ok_or(PeSurfaceError::AddressBeforeImage {
            virtual_address: OFFICIAL_EU_SURFACE_FAMILY_VA,
            image_base,
        })?;

    let section_table =
        optional_offset
            .checked_add(optional_size)
            .ok_or(PeSurfaceError::ArithmeticOverflow {
                offset: optional_offset,
            })?;
    let section_bytes = section_count.checked_mul(SECTION_HEADER_SIZE).ok_or(
        PeSurfaceError::ArithmeticOverflow {
            offset: section_table,
        },
    )?;
    require(input, section_table, section_bytes)?;

    let file_offset = map_raw_range(
        input,
        section_table,
        section_count,
        target_rva,
        BOTH_FAMILIES_BYTES,
    )?
    .ok_or(PeSurfaceError::UnmappedRange {
        virtual_address: OFFICIAL_EU_SURFACE_FAMILY_VA,
        byte_len: BOTH_FAMILIES_BYTES,
    })?;

    let first = decode_family(input, file_offset)?;
    let second_offset =
        file_offset
            .checked_add(FAMILY_BYTES)
            .ok_or(PeSurfaceError::ArithmeticOverflow {
                offset: file_offset,
            })?;
    let second = decode_family(input, second_offset)?;
    Ok(SurfaceCoefficientFamilies { first, second })
}

fn map_raw_range(
    input: &[u8],
    section_table: usize,
    section_count: usize,
    target_rva: u32,
    byte_len: usize,
) -> Result<Option<usize>, PeSurfaceError> {
    let byte_len_u32 = u32::try_from(byte_len).map_err(|_| PeSurfaceError::ArithmeticOverflow {
        offset: section_table,
    })?;
    let mut matched: Option<(usize, usize)> = None;
    for index in 0..section_count {
        let header = section_table + index * SECTION_HEADER_SIZE;
        let virtual_size = read_u32(input, header + 8)?;
        let section_rva = read_u32(input, header + 12)?;
        let raw_size = read_u32(input, header + 16)?;
        let raw_offset = read_u32(input, header + 20)?;
        let Some(delta) = target_rva.checked_sub(section_rva) else {
            continue;
        };
        let Some(range_end) = delta.checked_add(byte_len_u32) else {
            continue;
        };
        // PE images occasionally encode a zero VirtualSize. Treat that shape
        // as mapping SizeOfRawData; otherwise require the requested range to
        // fit both the virtual extent and the file-backed raw extent.
        let mapped_size = if virtual_size == 0 {
            raw_size
        } else {
            virtual_size
        };
        if range_end > mapped_size || range_end > raw_size {
            continue;
        }
        let file_offset = usize::try_from(raw_offset)
            .ok()
            .and_then(|offset| offset.checked_add(delta as usize))
            .ok_or(PeSurfaceError::ArithmeticOverflow {
                offset: header + 20,
            })?;
        if let Some((_, first_header)) = matched {
            return Err(PeSurfaceError::AmbiguousRange {
                virtual_address: OFFICIAL_EU_SURFACE_FAMILY_VA,
                first_section_offset: first_header,
                second_section_offset: header,
            });
        }
        matched = Some((file_offset, header));
    }
    if let Some((file_offset, _)) = matched {
        require(input, file_offset, byte_len)?;
        Ok(Some(file_offset))
    } else {
        Ok(None)
    }
}

fn decode_family(input: &[u8], offset: usize) -> Result<SurfaceCoefficientFamily, PeSurfaceError> {
    require(input, offset, FAMILY_BYTES)?;
    let mut values = [0; SURFACE_COEFFICIENT_COUNT];
    for (index, value) in values.iter_mut().enumerate() {
        *value = read_u32(input, offset + index * 4)? as i32;
    }
    Ok(SurfaceCoefficientFamily { values })
}

fn require(input: &[u8], offset: usize, needed: usize) -> Result<(), PeSurfaceError> {
    let end = offset
        .checked_add(needed)
        .ok_or(PeSurfaceError::ArithmeticOverflow { offset })?;
    if end > input.len() {
        return Err(PeSurfaceError::Truncated {
            offset,
            needed,
            input_len: input.len(),
        });
    }
    Ok(())
}

fn read_u16(input: &[u8], offset: usize) -> Result<u16, PeSurfaceError> {
    require(input, offset, 2)?;
    Ok(u16::from_le_bytes([input[offset], input[offset + 1]]))
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, PeSurfaceError> {
    require(input, offset, 4)?;
    Ok(u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut bytes = vec![0; RAW_OFFSET + raw_size];
        bytes[0..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, DOS_LFANEW_OFFSET, PE_OFFSET as u32);
        bytes[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        let coff = PE_OFFSET + 4;
        put_u16(&mut bytes, coff, I386_MACHINE);
        put_u16(&mut bytes, coff + 2, 1);
        put_u16(&mut bytes, coff + 16, OPTIONAL_SIZE as u16);
        let optional = coff + COFF_HEADER_SIZE;
        put_u16(&mut bytes, optional, PE32_MAGIC);
        put_u32(&mut bytes, optional + 28, IMAGE_BASE);

        bytes[SECTION_TABLE..SECTION_TABLE + 8].copy_from_slice(b".rdata\0\0");
        put_u32(&mut bytes, SECTION_TABLE + 8, raw_size as u32);
        put_u32(&mut bytes, SECTION_TABLE + 12, SECTION_RVA);
        put_u32(&mut bytes, SECTION_TABLE + 16, raw_size as u32);
        put_u32(&mut bytes, SECTION_TABLE + 20, RAW_OFFSET as u32);

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
